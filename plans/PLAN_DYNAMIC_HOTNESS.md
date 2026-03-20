# Plan: Dynamic Sliding Window Hotness Detection

## Problem

The current hotness detection precomputes stats at 12 fixed age points (0, 5, 10, 15, ..., 360 min) when a stream goes live, then snaps to the nearest bucket each poll. This causes streamers to lose their "hot" status when they cross into a later bucket with insufficient distinct streams — even though neighboring buckets clearly show they're hot.

Example: CohhCarnage at 8.43σ with 146 observations at the 3h bucket, but only 5 distinct streams (needs 7), so hotness drops despite being clearly hot at the 2h bucket.

## Solution

Replace the precomputed profile with per-poll dynamic SQL queries at the exact current stream age. Also cache the hotness result so the display path doesn't duplicate queries.

## Detection gates (unchanged)

A stream can only be marked hot when ALL of:
1. `stats.count >= min_observations` (default 5) — enough data points in the window
2. `stats.distinct_streams >= min_streams` (default 7) — baseline built from multiple independent streams
3. `stats.stddev > 0.0` — not all identical historical values
4. All observations within 30-day retention window (`OBSERVATION_RETENTION_SECS`)
5. Current stream excluded from baseline (via `stream_started_at != current_stream`)

When gates aren't met (`compute_hotness` returns `None`), `was_hot` is preserved — if we previously knew they were hot, we don't forget that just because we moved into a data-sparse region.

## Files to modify

| File | Change |
|------|--------|
| `crates/twitch-backend/src/backend.rs` | Rework `CachedHotnessProfile`, `record_and_evaluate_hotness`, `evaluate_hotness` |
| `crates/twitch-backend/src/db.rs` | Add `get_viewer_observations_excluding_stream` method |
| `features/streamer-hotness.md` | Update to reflect dynamic sliding window, remove precomputed profile docs, document `was_hot` preservation |

Files NOT changing:
- `hotness_detection.rs` — pure functions unchanged. `compute_hotness_profile` and `find_nearest_bucket` stay for the debug profiles view.
- `app_services.rs` — DTOs unchanged
- Debug profiles view (`get_debug_hotness_profiles`) — already computes from scratch, no change needed
- Frontend JS — no change (still shows 12 fixed-point profile table + live hotness table)

## Step-by-step

### 1. Add `get_viewer_observations_excluding_stream` to db.rs

New method on `Database`:

```rust
pub fn get_viewer_observations_excluding_stream(
    &self,
    broadcaster_id: i64,
    age_min: i64,
    age_max: i64,
    since: i64,
    exclude_stream_started_at: i64,
) -> anyhow::Result<Vec<ViewerObservation>>
```

SQL:
```sql
SELECT broadcaster_id, observed_at, stream_age_min, viewer_count, stream_started_at
FROM viewer_observations
WHERE broadcaster_id = ?1
  AND stream_age_min BETWEEN ?2 AND ?3
  AND observed_at > ?4
  AND stream_started_at != ?5
```

This filters by `stream_started_at != current_stream` instead of `observed_at < until`. More precise — excludes exactly the current stream's observations regardless of timing, and uses the existing `idx_vo_broadcaster_age` index.

Add a unit test for this method.

### 2. Rework `CachedHotnessProfile`

From:
```rust
struct CachedHotnessProfile {
    profile: Vec<(i64, BucketStats)>,
    was_hot: bool,
}
```

To:
```rust
struct CachedHotnessProfile {
    stream_started_at: i64,       // for excluding current stream from baseline
    was_hot: bool,                // for edge-triggered notifications
    last_hotness: Option<HotnessInfo>,  // cached result for display path
}
```

The `profile` field (precomputed bucket stats) is removed. Instead, `stream_started_at` is stored so each poll can query the DB excluding the current stream. `last_hotness` caches the most recent evaluation result so `evaluate_hotness` (display path) doesn't duplicate DB queries.

### 3. Rewrite `record_and_evaluate_hotness`

**On newly_live** (simplified — no more bulk observation fetch + profile computation):
```rust
for stream in &event.newly_live {
    cache.insert(stream.user_id.clone(), CachedHotnessProfile {
        stream_started_at: stream.started_at.timestamp(),
        was_hot: false,
        last_hotness: None,
    });
}
```

**Each poll** (dynamic query instead of cached profile lookup):
```rust
for stream in &event.streams {
    let Some(cached) = cache.get_mut(&stream.user_id) else { continue };

    let age = (now - stream.started_at).num_minutes().max(0);
    let (age_lo, age_hi) = compute_age_window(age);

    let obs = self.db.get_viewer_observations_excluding_stream(
        broadcaster_id, age_lo, age_hi, since, cached.stream_started_at,
    );

    let stats = compute_bucket_stats(&obs);
    let result = compute_hotness(&stream.user_id, stream.viewer_count, &stats, &hotness_cfg);

    // Cache for display path
    cached.last_hotness = result.clone();

    match result {
        Some(info) => {
            let was_hot = cached.was_hot;
            cached.was_hot = info.is_hot;
            if info.is_hot && !was_hot && cfg.notify_on_hot {
                // fire notification
            }
        }
        None => {
            // KEY CHANGE: preserve was_hot instead of resetting to false.
            // If we knew they were hot but now lack data, don't forget that.
        }
    }
}
```

The `was_hot` preservation on `None` is the behavioral fix for the original problem. Even without the dynamic window, this alone would prevent the symptom. But the dynamic window fixes the root cause (data-sparse fixed buckets).

### 4. Simplify `evaluate_hotness`

This method is called by `push_display_state` (for `hot_stream_ids`) and `get_debug_hotness_data` (debug view). Instead of redoing DB queries, read from `last_hotness` cached by `record_and_evaluate_hotness`:

```rust
fn evaluate_hotness(&self, streams: &[Stream]) -> Vec<HotnessInfo> {
    let cache = self.hotness_cache.lock().unwrap();
    streams.iter()
        .filter_map(|s| cache.get(&s.user_id)?.last_hotness.clone())
        .collect()
}
```

This is much simpler and avoids duplicate DB queries between the poll path and display path.

### 5. Update imports in backend.rs

- Add: `compute_age_window`, `compute_bucket_stats` (now called directly)
- Remove: `BucketStats` (no longer stored on cache struct, only used transiently)
- Keep: `compute_hotness_profile`, `find_nearest_bucket` (still used by `get_debug_hotness_profiles`)

`HOTNESS_AGE_POINTS` stays — used by `get_debug_hotness_profiles` for the debug table.

### 6. Update feature documentation

Update `features/streamer-hotness.md` to reflect:
- **Caching strategy** section: replace precomputed-profile-at-12-age-points with dynamic per-poll sliding window query. Document that `CachedHotnessProfile` now stores `stream_started_at` + `was_hot` + `last_hotness` instead of the full profile.
- **Design decisions**: update "Precomputed profile at 12 age points" section — explain the move to dynamic queries and why (data sparsity at later buckets). Note that the debug profiles view still uses the 12 fixed age points for visualization.
- **Edge-triggered notifications**: document `was_hot` preservation on insufficient data (previously reset to false, which caused false cool-offs).
- **Implementation history**: add a Phase 9 entry for this change.

## Verification

1. `make lint` — no clippy warnings
2. `make test-all` — all Rust + QML tests pass
3. Manual test with debug view: watch a streamer cross bucket boundaries and confirm hotness persists when data is sparse in later buckets

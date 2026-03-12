# Feature: Streamer Hotness Detection

**Status:** Implemented

## What it does

Detects when a followed streamer's current viewer count is significantly above their historical norm *for that point in the stream*, and surfaces this through notifications and visual indicators.

A 5,000-viewer spike means nothing for xQc but is extraordinary for a 200-viewer streamer. Hotness detection automatically scales per-streamer — no manual thresholds needed.

### User-facing behavior

- **Desktop notification** on hot detection: `🔥🔥🔥 (2.3σ) StreamerName on GameName IS HOT`
- **Tray menu**: 🔥 prefix on hot stream labels
- **KDE plasmoid**: animated swirling fire-colored `ConicalGradient` ring on the streamer's avatar (2s rotation cycle, overrides the favourite border)
- **Debug view** (debug builds only): live table showing mean, stddev, z-score, observation count, and is_hot for every live stream
- **Edge-triggered**: notification fires only on the not-hot → hot transition. If they cool off and spike again, that's a new edge, new notification.

## How it works

### Data collection

Every 60-second poll, one `viewer_observations` row is recorded per live stream:

| Column | Source |
|---|---|
| `broadcaster_id` | Stream data |
| `observed_at` | UTC timestamp |
| `stream_age_min` | `(now - stream.started_at)` in minutes |
| `viewer_count` | Current viewer count |

Stored in SQLite (`data.db`). Indexed on `(broadcaster_id, stream_age_min)` for age-window queries and `(observed_at)` for retention pruning. 30-day retention window. At ~500k rows/month (1440 polls/day × 12 streamers × 30 days) this is trivial for SQLite.

### Detection algorithm: sliding-window z-score

The core insight is that viewer counts vary by stream age — the first 10 minutes look different from hour 3. So the baseline must be age-aware.

**Z-score**: `z = (observed - mean) / stddev`. Measures how many standard deviations the current viewer count is from the historical mean at this point in a stream. A z-score of 2.0 means the streamer has 2 standard deviations more viewers than usual.

**Sliding window over stream age**: rather than fixed buckets (0–15 min, 15–30 min, etc.), the window scales with stream age:

```
half_width = max(stream_age_min / 3, 5)
window = [age - half_width, age + half_width]   (lower clamped to 0)
```

Examples:
- Minute 3 → window [0, 8] (narrow, early-stream)
- Minute 60 → window [40, 80] (wider, more data)
- Minute 180 → window [120, 240] (widest)

This naturally handles the viewer ramp-up without modeling the curve shape.

**Thresholds**: a stream is "hot" when `z >= hotness_z_threshold` (default 2.0) and there are at least `hotness_min_observations` (default 5) observations in the bucket. Zero stddev (all identical historical values) returns no result rather than dividing by zero.

### Caching strategy

To avoid per-poll DB queries, hotness profiles are cached in memory:

1. **On stream go-live**: query all historical observations for this broadcaster (excluding the current stream via `until = stream.started_at`), precompute bucket stats at 12 fixed age points: `[0, 5, 10, 15, 30, 45, 60, 90, 120, 180, 240, 360]`
2. **Each poll**: look up the nearest precomputed age point via `find_nearest_bucket`, compute z-score against those stats
3. **On stream offline**: evict from cache

The cache (`CachedHotnessProfile`) also tracks `was_hot: bool` for edge detection.

### Configuration

Global config (`~/.config/twitch-tray/config.json`):
- `hotness_z_threshold: f64` (default 2.0)
- `hotness_min_observations: usize` (default 5)
- `notify_on_hot: bool` (default true)

Per-streamer override:
- `hotness_z_threshold_override: Option<f64>` in `StreamerSettings`

## Code locations

| Layer | File | What |
|---|---|---|
| Pure detection math | `crates/twitch-backend/src/hotness_detection.rs` | `compute_age_window`, `compute_bucket_stats`, `compute_hotness`, `compute_hotness_profile`, `find_nearest_bucket` — zero side effects |
| DB persistence | `crates/twitch-backend/src/db.rs` | `viewer_observations` table, `record_viewer_observations`, `get_viewer_observations` |
| Cache + orchestration | `crates/twitch-backend/src/backend.rs` | `CachedHotnessProfile`, `record_and_evaluate_hotness`, `evaluate_hotness`, `HOTNESS_AGE_POINTS` |
| Config | `crates/twitch-backend/src/config.rs` | `hotness_z_threshold`, `hotness_min_observations`, `notify_on_hot`, `hotness_z_threshold_override` |
| Notifications | `crates/twitch-backend/src/notify.rs` | `Notifier::stream_hot()`, `DesktopNotifier` impl, `STREAM_HOT` category |
| Display data | `crates/twitch-backend/src/handle.rs` | `hot_stream_ids: HashSet<String>` on `RawDisplayData` |
| Tray menu | `crates/twitch-menu-tauri/src/display_state.rs` | `is_hot: bool` on `StreamEntry`, 🔥 prefix |
| KDE plasmoid | `crates/twitch-kde/src/dto.rs`, `plasmoid_state.rs` | `is_hot` on `LiveStreamDto` |
| QML visuals | `crates/twitch-kde/plasmoid/contents/ui/StreamerAvatar.qml` | Animated `ConicalGradient` ring |
| Debug view | `crates/twitch-backend/src/app_services.rs` | `DebugHotnessEntry`, `get_debug_hotness_data()` |
| Debug commands | `crates/twitch-settings-tauri/src/commands.rs` | Tauri command `get_debug_hotness_data` |

## Design decisions

### Z-score over percentile-based detection

Z-score is simpler to compute, configure, and explain. The threshold is a single number (2.0σ) rather than needing to maintain sorted distributions. Downside: z-score assumes roughly normal distributions, and viewer counts are skewed right. The debug view was added specifically to evaluate whether percentile-based detection would work better in practice.

### Raw rows over rolling stats

Considered using Welford's online algorithm to maintain rolling mean/variance per bucket, avoiding storing raw observations. Chose raw rows because:
- ~500k rows/month is nothing for SQLite
- Raw data allows retroactive algorithm changes (different window sizes, percentile calculations) without re-collecting
- Debug view can show full distribution, not just summary stats
- Simpler code — no incremental stats bookkeeping

### Sliding window over fixed buckets

Fixed buckets (e.g., 0–15 min, 15–30 min) create cliff edges at boundaries and waste data (a 14-minute observation can't inform the 15-minute bucket). The sliding window centered on the current stream age uses all nearby data, with width proportional to stream age so early-stream windows stay narrow.

### Precomputed profile at 12 age points (not on-the-fly)

The implementation precomputes stats at 12 fixed age points and snaps to the nearest one, rather than computing the sliding window at the exact current stream age each poll. This was a pragmatic choice to avoid per-poll iteration over the full observation set. The tradeoff is coarseness at the high end (120-minute gap between the 240 and 360 age points). This could be refined with more age points or on-the-fly computation if needed.

### Current stream excluded from baseline

An early bug: observations from the *current* stream were included in the historical baseline, causing false positives after just a few minutes of data. Fixed by adding an `until` parameter to `get_viewer_observations` and passing `stream.started_at.timestamp()` when building the profile cache. Only data from prior streams forms the baseline.

### No category distinction

The baseline includes all streams regardless of category. A streamer who does a special event in a popular category might appear "hot" relative to their usual category's audience. This is arguably the correct behavior — they *are* getting more viewers than normal, regardless of why.

### No time-of-day bucketing (yet)

Morning streams and evening streams may have different audience sizes. Discussed and deferred — splitting observations by time-of-day would fragment an already-sparse dataset. UTC timestamps are stored, so this can be added later if the debug view reveals time-dependent patterns.

### Edge-triggered notifications

One notification per not-hot → hot transition. If the streamer cools off and spikes again, that's a new transition and fires a new notification. This avoids notification spam while still catching multiple hot periods within a single stream.

## Implementation history

Built in 8 phases, following the project's crate-boundary architecture:

1. **DB layer** — new `viewer_observations` table with `record_viewer_observations` and `get_viewer_observations` methods in `db.rs`. Indexes on `(broadcaster_id, stream_age_min)` and `(observed_at)`.

2. **Observation recording** — the history recording listener in `backend.rs` writes one `ViewerObservation` row per live stream per `StreamsUpdated` event, computing `stream_age_min` from `started_at`.

3. **Detection module** — `hotness_detection.rs`, a pure-function module with zero side effects. Five functions (`compute_age_window`, `compute_bucket_stats`, `compute_hotness`, `compute_hotness_profile`, `find_nearest_bucket`) and 16 unit tests.

4. **Config** — `hotness_z_threshold`, `hotness_min_observations`, `notify_on_hot` on global `Config`; `hotness_z_threshold_override` on `StreamerSettings`.

5. **Cache + edge detection + notifications** — `CachedHotnessProfile` in `backend.rs` with precomputed profile and `was_hot` flag. Populates on newly-live, evicts on offline, evaluates each poll. `stream_hot()` added to `Notifier` trait with `DesktopNotifier` and `RecordingNotifier` implementations. `hot_stream_ids: HashSet<String>` added to `RawDisplayData`.

6. **Display integration** — `is_hot: bool` on `StreamEntry` in tray menu (`display_state.rs`), fire emoji prefix on labels. `is_hot` on `LiveStreamDto` for KDE plasmoid, mapped from `hot_stream_ids`.

7. **Debug view** — `DebugHotnessEntry` type in `app_services.rs` exposing broadcaster name, current viewers, mean, stddev, z-score, observation count, and is_hot. Tauri command `get_debug_hotness_data` gated behind `is_debug_build()`.

8. **QML plasmoid visuals** — `isHot` property on `StreamerAvatar` and `StreamRow`. Animated swirling `ConicalGradient` ring (fire colors, 2s rotation) that overrides the favourite border. QML tests for hot ring visibility, border behavior, and hot+favourite interaction.

### Dependency graph

```
Phase 1 (DB) ───┬──→ Phase 2 (Recording)
                 │
Phase 3 (Detect) ┤──→ Phase 5 (Cache + Notify) ──→ Phase 6 (Display) ──→ Phase 8 (QML)
                 │                                  │
Phase 4 (Config) ┘                                  └──→ Phase 7 (Debug)
```

## Future considerations

- **Time-of-day bucketing**: data is already collected (UTC timestamps), add bucketing if evaluation shows time-dependent baselines matter
- **Percentile-based detection**: alternative to z-score for non-normal distributions — debug view helps evaluate
- **Observation pruning**: not yet implemented, will be handled by a generic pruner for all tables
- **More granular age points**: the 12 fixed age points get coarse past 4 hours — could add more points or switch to on-the-fly computation from cached raw observations

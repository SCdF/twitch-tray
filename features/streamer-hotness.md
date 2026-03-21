# Feature: Streamer Hotness Detection

**Status:** Implemented

## What it does

Detects when a followed streamer's current viewer count is significantly above their historical norm *for that point in the stream*, and surfaces this through notifications and visual indicators.

A 5,000-viewer spike means nothing for xQc but is extraordinary for a 200-viewer streamer. Hotness detection automatically scales per-streamer — no manual thresholds needed.

### User-facing behavior

- **Desktop notification** on hot detection: `🔥🔥🔥 (2.3σ) StreamerName on GameName IS HOT`
- **Tray menu**: 🔥 prefix on hot stream labels
- **KDE plasmoid**: animated swirling fire-colored `ConicalGradient` ring on the streamer's avatar (2s rotation cycle, overrides the favourite border)
- **Debug view** (debug builds only): live table showing mean, stddev, z-score, observation count, distinct streams, and is_hot for every live stream
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
| `stream_started_at` | `stream.started_at` as Unix timestamp (identifies which stream) |

Stored in SQLite (`data.db`). Indexed on `(broadcaster_id, stream_age_min)` for age-window queries and `(observed_at)` for retention pruning. 30-day retention window. At ~500k rows/month (1440 polls/day × 12 streamers × 30 days) this is trivial for SQLite.

### Detection algorithm: sliding-window z-score

The core insight is that viewer counts vary by stream age — the first 10 minutes look different from hour 3. So the baseline must be age-aware.

**Anscombe-transformed z-score**: Raw viewer counts are integers, and for small streamers the minimum possible fluctuation (+1 viewer) represents a large fraction of their total. This means small streamers are far more likely to trigger hot detection than large ones — a +6 viewer spike on a 10-viewer streamer is unremarkable but produces a huge z-score. The **Anscombe variance-stabilizing transform** fixes this by mapping each viewer count `x` to `sqrt(x + 3/8)` before computing statistics. In this transformed space, the variance is approximately constant (~1/4) regardless of the mean, so z-scores become comparable across streamers of very different sizes.

The z-score is computed in transformed space:
```
y = sqrt(x + 3/8)                         # Anscombe transform
z = (y_observed - mean(y_historical)) / stddev(y_historical)
```

A z-score of 2.0 means the transformed viewer count is 2 standard deviations above the transformed historical mean. The raw (untransformed) mean and stddev are preserved alongside the transformed values for display in the debug view and notifications.

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

**Thresholds with hysteresis (Schmitt trigger)**: dual z-score thresholds prevent oscillation when a streamer hovers near the boundary:
- A stream **becomes hot** when `z >= hotness_z_threshold` (default 2.0)
- A hot stream **cools off** only when `z < hotness_z_cool_threshold` (default 1.0)
- Between the two thresholds, the previous state is preserved (dead zone)
- At least `hotness_min_observations` (default 5) data points in the bucket
- At least `hotness_min_streams` (default 7) distinct streams contributed to the bucket

The dead zone (default: 1.0 to 2.0) means a streamer who becomes hot at z=2.0 won't lose their hot status until they drop below z=1.0. This is the standard approach in signal processing — analogous to warning vs action limits in statistical process control charts.

The `min_streams` gate ensures the baseline is built from multiple independent streams rather than a single session. This prevents false positives when the app has only observed one or two streams for a streamer. Zero transformed stddev (all identical historical values after transform) returns no result rather than dividing by zero.

### Caching strategy

Hotness is evaluated dynamically each poll with a sliding window query at the exact current stream age:

1. **On stream go-live**: initialise `CachedHotnessProfile` with `stream_started_at` (for excluding the current stream from the baseline), `was_hot: false`, and `last_hotness: None`
2. **Each poll**: compute the age window via `compute_age_window(age)`, query DB for historical observations in that window excluding the current stream (`stream_started_at != current`), compute bucket stats, evaluate z-score. The result is cached as `last_hotness` so the display path (`evaluate_hotness`) reads from cache without duplicating DB queries.
3. **On stream offline**: evict from cache

The `was_hot` flag is preserved when `compute_hotness` returns `None` (insufficient data). If we previously knew a streamer was hot but moved into a data-sparse age region, we don't forget that — preventing false cool-off notifications.

The debug profiles view (`get_debug_hotness_profiles`) still computes the 12 fixed age-point profile for visualization, but this is independent of the live detection path.

### Configuration

Global config (`~/.config/twitch-tray/config.json`):
- `hotness_z_threshold: f64` (default 2.0) — z-score to enter hot state
- `hotness_z_cool_threshold: f64` (default 1.0) — z-score below which a hot stream cools off
- `hotness_min_observations: usize` (default 5)
- `hotness_min_streams: usize` (default 7) — minimum distinct streams observed before detection activates
- `notify_on_hot: bool` (default true)

Per-streamer override:
- `hotness_z_threshold_override: Option<f64>` in `StreamerSettings`

## Code locations

| Layer | File | What |
|---|---|---|
| Pure detection math | `crates/twitch-backend/src/hotness_detection.rs` | `compute_age_window`, `compute_bucket_stats`, `compute_hotness`, `compute_hotness_profile`, `find_nearest_bucket` — zero side effects |
| DB persistence | `crates/twitch-backend/src/db.rs` | `viewer_observations` table, `record_viewer_observations`, `get_viewer_observations`, `get_viewer_observations_excluding_stream` |
| Cache + orchestration | `crates/twitch-backend/src/backend.rs` | `CachedHotnessProfile`, `record_and_evaluate_hotness`, `evaluate_hotness`, `HOTNESS_AGE_POINTS` |
| Config | `crates/twitch-backend/src/config.rs` | `hotness_z_threshold`, `hotness_min_observations`, `hotness_min_streams`, `notify_on_hot`, `hotness_z_threshold_override` |
| Notifications | `crates/twitch-backend/src/notify.rs` | `Notifier::stream_hot()`, `DesktopNotifier` impl, `STREAM_HOT` category |
| Display data | `crates/twitch-backend/src/handle.rs` | `hot_stream_ids: HashSet<String>` on `RawDisplayData` |
| Tray menu | `crates/twitch-menu-tauri/src/display_state.rs` | `is_hot: bool` on `StreamEntry`, 🔥 prefix |
| KDE plasmoid | `crates/twitch-kde/src/dto.rs`, `plasmoid_state.rs` | `is_hot` on `LiveStreamDto` |
| QML visuals | `crates/twitch-kde/plasmoid/contents/ui/StreamerAvatar.qml` | Animated `ConicalGradient` ring |
| Debug view | `crates/twitch-backend/src/app_services.rs` | `DebugHotnessEntry` (includes `distinct_streams`), `get_debug_hotness_data()` |
| Debug commands | `crates/twitch-settings-tauri/src/commands.rs` | Tauri command `get_debug_hotness_data` |

## Design decisions

### Anscombe transform for scale-invariant detection

Viewer counts are integers, and for small streamers the minimum possible fluctuation (+1 viewer) is a large fraction of their total. Without correction, a 10-viewer streamer going to 16 viewers (+60%) produces a much larger z-score than a 10,000-viewer streamer going to 16,000 (+60%), because the raw stddev is proportionally smaller for low counts (variance scales with the mean in Poisson-like count data).

The **Anscombe transform** (`y = sqrt(x + 3/8)`) is a variance-stabilizing transform derived analytically for Poisson data (Anscombe, 1948). After transformation, the variance is approximately 1/4 regardless of the mean, so z-scores become comparable across streamers of all sizes. The `3/8` constant is not a tuning parameter — it's the analytically optimal value.

**Alternatives considered:**

- **Minimum stddev floor** (`max(stddev, mean * k)` for some constant `k`): Simple but requires tuning an arbitrary constant. Different values of `k` would suit different viewer count ranges, and it breaks the statistical interpretation of the z-score threshold. The threshold would need re-tuning whenever the floor constant changes.

- **Log transform** (`y = log(x + 1)`): Common for right-skewed data, but over-corrects for small counts. A streamer going from 2→4 viewers and from 200→400 viewers would produce identical z-scores, which doesn't match intuition — doubling from 2 is much noisier than doubling from 200. The `+1` to handle zero is also ad-hoc.

- **Bayesian shrinkage / empirical Bayes**: The most principled approach — regularize variance estimates with a prior so small-sample streamers are pulled toward a global baseline. However, it requires choosing a prior distribution, maintaining cross-streamer aggregate stats, and significantly complicates the detection path. The z-score threshold would need reinterpretation. Better suited if Anscombe proves insufficient in practice.

- **Coefficient of variation / relative thresholds** (require viewers to be some % above mean in addition to z-score): Adds a second threshold to tune and explain. The Anscombe transform achieves the same goal (scale-invariance) without a second parameter.

### Z-score over percentile-based detection

Z-score is simpler to compute, configure, and explain. The threshold is a single number (2.0σ) rather than needing to maintain sorted distributions. Downside: z-score assumes roughly normal distributions, and viewer counts are skewed right. The Anscombe transform partially addresses the skewness issue by compressing the right tail. The debug view was added specifically to evaluate whether percentile-based detection would work better in practice.

### Raw rows over rolling stats

Considered using Welford's online algorithm to maintain rolling mean/variance per bucket, avoiding storing raw observations. Chose raw rows because:
- ~500k rows/month is nothing for SQLite
- Raw data allows retroactive algorithm changes (different window sizes, percentile calculations) without re-collecting
- Debug view can show full distribution, not just summary stats
- Simpler code — no incremental stats bookkeeping

### Sliding window over fixed buckets

Fixed buckets (e.g., 0–15 min, 15–30 min) create cliff edges at boundaries and waste data (a 14-minute observation can't inform the 15-minute bucket). The sliding window centered on the current stream age uses all nearby data, with width proportional to stream age so early-stream windows stay narrow.

### Dynamic per-poll sliding window (not precomputed)

Originally, stats were precomputed at 12 fixed age points when a stream went live, and each poll snapped to the nearest bucket. This caused streamers to lose their "hot" status when they crossed into a later bucket that happened to have insufficient distinct streams — even though neighboring buckets clearly showed they were hot.

The fix replaces the precomputed profile with a per-poll DB query at the exact current stream age. The `get_viewer_observations_excluding_stream` method filters by `stream_started_at != current_stream` (more precise than the previous `observed_at < until` approach) and uses the existing `idx_vo_broadcaster_age` index. The debug profiles view still uses the 12 fixed age points for visualization.

### Current stream excluded from baseline

An early bug: observations from the *current* stream were included in the historical baseline, causing false positives after just a few minutes of data. Fixed by filtering on `stream_started_at != current_stream` via `get_viewer_observations_excluding_stream`. Only data from prior streams forms the baseline.

### No category distinction

The baseline includes all streams regardless of category. A streamer who does a special event in a popular category might appear "hot" relative to their usual category's audience. This is arguably the correct behavior — they *are* getting more viewers than normal, regardless of why.

### No time-of-day bucketing (yet)

Morning streams and evening streams may have different audience sizes. Discussed and deferred — splitting observations by time-of-day would fragment an already-sparse dataset. UTC timestamps are stored, so this can be added later if the debug view reveals time-dependent patterns.

### Minimum distinct streams over minimum observations alone

The original implementation only gated on `min_observations` (data point count), but a single 5-hour stream generates ~300 observations — easily exceeding the threshold. This meant a "baseline" could be built from one stream, making the first unusual stream look hot simply because it differed from one prior session.

The fix adds `stream_started_at` to each observation row and counts distinct `stream_started_at` values per bucket (`BucketStats.distinct_streams`). Detection requires data from `min_streams` (default 7) distinct streams, ensuring the baseline reflects the streamer's typical viewership across multiple independent sessions. A week of casual use (~1 stream/day) naturally satisfies this.

Distinct streams was chosen over distinct calendar days because streams can span midnight, and a streamer who does two streams in one day provides more independent signal than one.

### Hysteresis (dual thresholds) over single threshold

A single z-score threshold causes oscillation ("flickering") when a streamer hovers near the boundary — every 60-second poll might flip between hot and not-hot. The standard signal processing solution is a **Schmitt trigger**: separate entry and exit thresholds with a dead zone between them.

**Alternatives considered:**

- **Percentage buffer** (cool off when z < threshold * 0.75): Just an indirect way to express the same dual-threshold idea, but the percentage has no statistical meaning and is harder to reason about.

- **Time-based cooldown** (require N consecutive not-hot polls): Ignores the actual signal. A streamer who drops to z=-1.0 would still show as hot for N minutes. Also adds temporal state that interacts awkwardly with the existing `was_hot` preservation on insufficient data.

- **Exponential moving average of z-scores**: Smoothing naturally adds hysteresis but changes the statistical interpretation, adds hidden state, and introduces a new parameter (decay rate) that's harder to reason about than a second z-score threshold.

The dual-threshold approach stays entirely within the z-score framework, adds exactly one parameter (`hotness_z_cool_threshold`), and is the most transparent to users.

### Edge-triggered notifications

One notification per not-hot → hot transition. If the streamer cools off and spikes again, that's a new transition and fires a new notification. This avoids notification spam while still catching multiple hot periods within a single stream.

When `compute_hotness` returns `None` (insufficient data in the current window), `was_hot` is preserved rather than reset to `false`. This prevents false cool-off edges when a streamer moves into a data-sparse age region — if we knew they were hot, we don't forget that just because the current bucket lacks data.

## Implementation history

Built in 8 phases, following the project's crate-boundary architecture:

1. **DB layer** — `viewer_observations` table with `record_viewer_observations` and `get_viewer_observations` methods in `db.rs`. Columns: `broadcaster_id`, `observed_at`, `stream_age_min`, `viewer_count`, `stream_started_at`. Indexes on `(broadcaster_id, stream_age_min)` and `(observed_at)`.

2. **Observation recording** — the history recording listener in `backend.rs` writes one `ViewerObservation` row per live stream per `StreamsUpdated` event, computing `stream_age_min` from `started_at`.

3. **Detection module** — `hotness_detection.rs`, a pure-function module with zero side effects. Five functions (`compute_age_window`, `compute_bucket_stats`, `compute_hotness`, `compute_hotness_profile`, `find_nearest_bucket`). `BucketStats` tracks `distinct_streams` alongside `count`, and `HotnessConfig` includes `min_streams` to gate on multi-stream baselines.

4. **Config** — `hotness_z_threshold`, `hotness_min_observations`, `hotness_min_streams`, `notify_on_hot` on global `Config`; `hotness_z_threshold_override` on `StreamerSettings`.

5. **Cache + edge detection + notifications** — `CachedHotnessProfile` in `backend.rs` with precomputed profile and `was_hot` flag. Populates on newly-live, evicts on offline, evaluates each poll. `stream_hot()` added to `Notifier` trait with `DesktopNotifier` and `RecordingNotifier` implementations. `hot_stream_ids: HashSet<String>` added to `RawDisplayData`.

6. **Display integration** — `is_hot: bool` on `StreamEntry` in tray menu (`display_state.rs`), fire emoji prefix on labels. `is_hot` on `LiveStreamDto` for KDE plasmoid, mapped from `hot_stream_ids`.

7. **Debug view** — `DebugHotnessEntry` type in `app_services.rs` exposing broadcaster name, current viewers, mean, stddev, z-score, observation count, distinct streams, and is_hot. Tauri command `get_debug_hotness_data` gated behind `is_debug_build()`.

8. **QML plasmoid visuals** — `isHot` property on `StreamerAvatar` and `StreamRow`. Animated swirling `ConicalGradient` ring (fire colors, 2s rotation) that overrides the favourite border. QML tests for hot ring visibility, border behavior, and hot+favourite interaction.

9. **Dynamic sliding window** — replaced precomputed 12-age-point profiles with per-poll dynamic DB queries at the exact current stream age. Added `get_viewer_observations_excluding_stream` to `db.rs` (filters by `stream_started_at != current`). `CachedHotnessProfile` now stores `stream_started_at` + `was_hot` + `last_hotness` instead of the full profile. `was_hot` is preserved on insufficient data to prevent false cool-off notifications. `evaluate_hotness` reads from `last_hotness` cache instead of re-querying.

10. **Anscombe variance-stabilizing transform** — applied `sqrt(x + 3/8)` transform to viewer counts before computing z-scores, making detection scale-invariant across streamers of different sizes. `BucketStats` now carries `transformed_mean` and `transformed_stddev` alongside raw values. `compute_hotness` computes z-score in transformed space; raw mean/stddev preserved for display. No DB, config, or display changes needed — the transform is internal to the detection math.

11. **Hysteresis (dual z-score thresholds)** — added `z_cool_threshold` (default 1.0) to prevent oscillation when a streamer's z-score hovers near the entry threshold. `compute_hotness` now takes `was_hot: bool` and uses the entry threshold (`z_threshold`) when cold, or the exit threshold (`z_cool_threshold`) when already hot. Between the two thresholds, the previous state is preserved. Added `hotness_z_cool_threshold` to `Config` and the settings UI.

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
- ~~**More granular age points**: the 12 fixed age points get coarse past 4 hours~~ — resolved by Phase 9 dynamic sliding window

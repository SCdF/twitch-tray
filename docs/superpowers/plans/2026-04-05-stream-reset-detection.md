# Stream Reset Detection Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Detect when a Twitch stream resets (OBS crash, internet blip) and preserve the original `started_at` so downstream systems treat it as one continuous stream.

**Architecture:** A `normalize_stream_resets` method on `Backend` runs in `refresh_followed_streams` after fetching from the API but before calling `state.set_followed_streams()`. It mutates `Stream.started_at` in-place to the original value when a reset is detected. A `recent_streams` grace-period map (`HashMap<String, (DateTime<Utc>, Instant)>`) tracks recently-offline streams so resets that span 1+ poll intervals are also caught. A `resumed_user_ids` set is passed through to `set_followed_streams` so that grace-period returns are not treated as `newly_live`.

**Tech Stack:** Rust, tokio, chrono

---

## File Structure

| Action | File | Responsibility |
|--------|------|----------------|
| Modify | `crates/twitch-backend/src/config.rs` | Add `stream_reset_grace_min` config field (default 5) |
| Modify | `crates/twitch-backend/src/backend.rs` | Add `recent_streams` map to `Backend`, `normalize_stream_resets` method, call it in `refresh_followed_streams` |
| Modify | `crates/twitch-backend/src/state.rs` | Add `resumed_user_ids` parameter to `set_followed_streams`, exclude from `newly_live` |

---

### Task 1: Add `stream_reset_grace_min` config field

**Files:**
- Modify: `crates/twitch-backend/src/config.rs`

- [ ] **Step 1: Add the default constant and serde helper**

In `config.rs`, after the existing `DEFAULT_NOTIFY_ON_HOT` constant (line 30), add:

```rust
pub const DEFAULT_STREAM_RESET_GRACE_MIN: u64 = 5;
```

After the existing `default_notify_on_hot()` function (find it near the other `fn default_*()` functions), add:

```rust
fn default_stream_reset_grace_min() -> u64 {
    DEFAULT_STREAM_RESET_GRACE_MIN
}
```

- [ ] **Step 2: Add the field to `Config`**

In the `Config` struct, after the `notify_on_hot` field (line 134), add:

```rust
    /// Grace period (in minutes) for treating a stream restart as the same stream.
    /// If a streamer goes offline and comes back within this window, the original
    /// start time is preserved — preventing false hotness detections and phantom
    /// stream history entries from brief disconnects.
    #[serde(default = "default_stream_reset_grace_min")]
    pub stream_reset_grace_min: u64,
```

- [ ] **Step 3: Run tests to verify nothing breaks**

Run: `cargo test -p twitch-backend`
Expected: All existing tests PASS. The new field has a serde default, so existing configs and test `Config::default()` values pick it up automatically.

- [ ] **Step 4: Commit**

```bash
git add crates/twitch-backend/src/config.rs
git commit -m "feat: add stream_reset_grace_min config field (default 5 min)"
```

---

### Task 2: Add `resumed_user_ids` parameter to `set_followed_streams`

This lets the caller tell `set_followed_streams` that certain user IDs are returning from a grace-period absence and should not be treated as `newly_live`.

**Files:**
- Modify: `crates/twitch-backend/src/state.rs`
- Test: `crates/twitch-backend/src/state.rs` (inline `#[cfg(test)]` module)

- [ ] **Step 1: Write the failing test**

In the `#[cfg(test)] mod tests` block in `state.rs`, add:

```rust
    #[tokio::test]
    async fn resumed_stream_not_treated_as_newly_live() {
        let state = AppState::new();
        let mut rx = state.subscribe_streams();

        let stream_a = make_stream("a", "StreamerA");
        state
            .set_followed_streams(vec![stream_a.clone()], HashSet::new())
            .await;
        let _ = rx.recv().await;

        // Stream A goes offline
        state.set_followed_streams(vec![], HashSet::new()).await;
        let _ = rx.recv().await;

        // Stream A returns — but marked as resumed
        let resumed: HashSet<String> = ["a".to_string()].into();
        state
            .set_followed_streams(vec![stream_a], resumed)
            .await;
        let event = rx.recv().await.unwrap();

        assert!(
            event.newly_live.is_empty(),
            "resumed stream should not appear in newly_live"
        );
    }
```

Also add `use std::collections::HashSet;` to the test module's imports if not already present.

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p twitch-backend state::tests::resumed_stream_not_treated_as_newly_live`
Expected: Compilation error — `set_followed_streams` doesn't accept a second argument yet.

- [ ] **Step 3: Update `set_followed_streams` signature and implementation**

Change the `set_followed_streams` method signature from:

```rust
    pub async fn set_followed_streams(&self, streams: Vec<Stream>) {
```

to:

```rust
    pub async fn set_followed_streams(&self, streams: Vec<Stream>, resumed_user_ids: HashSet<String>) {
```

Add `use std::collections::HashSet;` to the top of `state.rs` if not already present.

Then change the `newly_live` computation from:

```rust
        let newly_live: Vec<_> = streams
            .iter()
            .filter(|s| !old_by_id.contains(&s.user_id))
            .cloned()
            .collect();
```

to:

```rust
        let newly_live: Vec<_> = streams
            .iter()
            .filter(|s| !old_by_id.contains(&s.user_id) && !resumed_user_ids.contains(&s.user_id))
            .cloned()
            .collect();
```

- [ ] **Step 4: Fix all existing callers**

Every existing call to `set_followed_streams` needs the new parameter. Search for all call sites:

In `crates/twitch-backend/src/backend.rs` (`refresh_followed_streams`, line ~752):
```rust
        self.state.set_followed_streams(streams, HashSet::new()).await;
```
Add `use std::collections::HashSet;` to the top of `backend.rs` if not already present.

In `crates/twitch-backend/src/state.rs` test module — update all existing test calls to pass `HashSet::new()`:

Every existing test that calls `state.set_followed_streams(vec![...]).await` becomes `state.set_followed_streams(vec![...], HashSet::new()).await`.

In `crates/twitch-backend/src/session.rs` — search for `set_followed_streams` and update similarly.

In `crates/twitch-app-tauri/tests/` — search for `set_followed_streams` and update similarly.

In any other crate that calls it (search workspace-wide with `grep -r "set_followed_streams"`).

- [ ] **Step 5: Run test to verify it passes**

Run: `cargo test -p twitch-backend state::tests::resumed_stream_not_treated_as_newly_live`
Expected: PASS

- [ ] **Step 6: Write test for non-resumed stream still being newly_live**

```rust
    #[tokio::test]
    async fn non_resumed_stream_still_treated_as_newly_live() {
        let state = AppState::new();
        let mut rx = state.subscribe_streams();

        state.set_followed_streams(vec![], HashSet::new()).await;
        let _ = rx.recv().await;

        // Stream appears without being in resumed set
        let stream_a = make_stream("a", "StreamerA");
        state
            .set_followed_streams(vec![stream_a], HashSet::new())
            .await;
        let event = rx.recv().await.unwrap();

        assert_eq!(event.newly_live.len(), 1);
        assert_eq!(event.newly_live[0].user_id, "a");
    }
```

- [ ] **Step 7: Run all state tests**

Run: `cargo test -p twitch-backend state::tests`
Expected: All PASS

- [ ] **Step 8: Run full workspace tests to catch any missed callers**

Run: `cargo test --workspace`
Expected: All PASS (any missed caller will fail to compile)

- [ ] **Step 9: Commit**

```bash
git add crates/twitch-backend/src/state.rs crates/twitch-backend/src/backend.rs crates/twitch-backend/src/session.rs crates/twitch-app-tauri/tests/
git commit -m "feat: add resumed_user_ids to set_followed_streams to suppress false newly_live"
```

---

### Task 3: Add `recent_streams` map and `normalize_stream_resets` to Backend

This is the core logic. The `recent_streams` map tracks streams that recently went offline, so resets spanning 1+ poll intervals can be detected.

**Files:**
- Modify: `crates/twitch-backend/src/backend.rs`
- Test: `crates/twitch-backend/src/backend.rs` (inline `#[cfg(test)]` module)

- [ ] **Step 1: Add `recent_streams` field to `Backend`**

In the `Backend` struct (after `hotness_cache`, line ~139), add:

```rust
    /// Recently-offline streams for detecting resets that span poll boundaries.
    /// Maps user_id -> (original_started_at, went_offline_at).
    /// Entries older than `stream_reset_grace_min` are pruned each poll.
    recent_streams: Arc<std::sync::Mutex<HashMap<String, (DateTime<Utc>, Instant)>>>,
```

In `Backend::new()` (after `hotness_cache` initialization, line ~199), add:

```rust
            recent_streams: Arc::new(std::sync::Mutex::new(HashMap::new())),
```

In `Backend::with_test_deps()` (after `hotness_cache` initialization, line ~1303), add:

```rust
            recent_streams: Arc::new(std::sync::Mutex::new(HashMap::new())),
```

- [ ] **Step 2: Write failing test for instant reset (same poll)**

```rust
    #[tokio::test]
    async fn started_at_preserved_when_stream_resets_within_same_poll() {
        let (backend, _tmp) = make_test_backend(60);

        let original_start = Utc::now() - Duration::hours(2);
        let mut stream = make_stream("123", "TestStreamer");
        stream.started_at = original_start;

        // Simulate stream being live in state
        backend
            .state
            .set_followed_streams(vec![stream.clone()], HashSet::new())
            .await;

        // API returns same user_id with new started_at (reset)
        let mut reset_stream = stream.clone();
        reset_stream.started_at = Utc::now();

        let mut streams = vec![reset_stream];
        let resumed = backend.normalize_stream_resets(&mut streams).await;

        assert_eq!(streams[0].started_at, original_start);
        assert!(resumed.is_empty(), "instant reset should not produce resumed IDs");
    }
```

- [ ] **Step 3: Run test to verify it fails**

Run: `cargo test -p twitch-backend backend::tests::started_at_preserved_when_stream_resets_within_same_poll`
Expected: Compilation error — `normalize_stream_resets` doesn't exist yet.

- [ ] **Step 4: Implement `normalize_stream_resets`**

Add this method to the `impl Backend` block (after `refresh_followed_streams`):

```rust
    /// Detects stream resets and preserves original `started_at`.
    ///
    /// Two cases:
    /// 1. **Instant reset**: stream was live last poll with a different `started_at`.
    ///    The `started_at` is overwritten in-place.
    /// 2. **Grace-period reset**: stream went offline for 1+ polls but returned within
    ///    `stream_reset_grace_min`. The `started_at` is overwritten and the `user_id`
    ///    is returned in the `resumed` set to suppress false `newly_live` detection.
    ///
    /// Also maintains the `recent_streams` map: stashes newly-offline streams and
    /// prunes entries older than the grace period.
    async fn normalize_stream_resets(
        &self,
        streams: &mut [crate::twitch::Stream],
    ) -> HashSet<String> {
        let grace = std::time::Duration::from_secs(
            self.config.get().stream_reset_grace_min * 60,
        );

        let old_streams = self.state.get_followed_streams().await;
        let old_by_id: HashMap<&str, &crate::twitch::Stream> = old_streams
            .iter()
            .map(|s| (s.user_id.as_str(), s))
            .collect();

        let new_by_id: std::collections::HashSet<&str> =
            streams.iter().map(|s| s.user_id.as_str()).collect();

        let mut resumed = HashSet::new();

        // Case 1: instant reset — same user_id still live, different started_at
        for stream in streams.iter_mut() {
            if let Some(old) = old_by_id.get(stream.user_id.as_str()) {
                if stream.started_at != old.started_at {
                    tracing::info!(
                        "Stream reset detected for {} (instant) — preserving original started_at",
                        stream.user_name,
                    );
                    stream.started_at = old.started_at;
                }
            }
        }

        // Stash streams that just went offline into the grace-period map
        {
            let mut recent = self.recent_streams.lock().unwrap();
            for old in &old_streams {
                if !new_by_id.contains(old.user_id.as_str()) {
                    recent.insert(
                        old.user_id.clone(),
                        (old.started_at, Instant::now()),
                    );
                }
            }
        }

        // Case 2: grace-period reset — stream was offline, now back within grace window
        {
            let mut recent = self.recent_streams.lock().unwrap();
            for stream in streams.iter_mut() {
                if old_by_id.contains_key(stream.user_id.as_str()) {
                    continue; // handled in case 1
                }
                if let Some((original_started_at, went_offline)) =
                    recent.remove(&stream.user_id)
                {
                    if went_offline.elapsed() < grace {
                        tracing::info!(
                            "Stream reset detected for {} (grace period) — preserving original started_at",
                            stream.user_name,
                        );
                        stream.started_at = original_started_at;
                        resumed.insert(stream.user_id.clone());
                    }
                }
            }

            // Prune stale entries
            recent.retain(|_, (_, t)| t.elapsed() < grace);
        }

        resumed
    }
```

- [ ] **Step 5: Run test to verify it passes**

Run: `cargo test -p twitch-backend backend::tests::started_at_preserved_when_stream_resets_within_same_poll`
Expected: PASS

- [ ] **Step 6: Write failing test for grace-period reset**

```rust
    #[tokio::test]
    async fn started_at_preserved_when_stream_returns_within_grace_period() {
        let (backend, _tmp) = make_test_backend(60);

        let original_start = Utc::now() - Duration::hours(2);
        let mut stream = make_stream("123", "TestStreamer");
        stream.started_at = original_start;

        // Stream is live
        backend
            .state
            .set_followed_streams(vec![stream.clone()], HashSet::new())
            .await;

        // Stream goes offline — normalize sees it disappear and stashes it
        let mut empty: Vec<crate::twitch::Stream> = vec![];
        let _ = backend.normalize_stream_resets(&mut empty).await;
        backend
            .state
            .set_followed_streams(vec![], HashSet::new())
            .await;

        // Stream returns with new started_at (within grace period)
        let mut reset_stream = stream.clone();
        reset_stream.started_at = Utc::now();
        let mut streams = vec![reset_stream];
        let resumed = backend.normalize_stream_resets(&mut streams).await;

        assert_eq!(streams[0].started_at, original_start);
        assert!(
            resumed.contains("123"),
            "grace-period return should be in resumed set"
        );
    }
```

- [ ] **Step 7: Run test to verify it passes**

Run: `cargo test -p twitch-backend backend::tests::started_at_preserved_when_stream_returns_within_grace_period`
Expected: PASS (the implementation already handles this)

- [ ] **Step 8: Write failing test — stream NOT resumed after grace period expires**

```rust
    #[tokio::test]
    async fn new_started_at_used_when_stream_returns_after_grace_period() {
        // Use 0-minute grace period so everything expires immediately
        let (backend, _tmp) = make_test_backend_with_grace(60, 0);

        let original_start = Utc::now() - Duration::hours(2);
        let mut stream = make_stream("123", "TestStreamer");
        stream.started_at = original_start;

        // Stream is live
        backend
            .state
            .set_followed_streams(vec![stream.clone()], HashSet::new())
            .await;

        // Stream goes offline
        let mut empty: Vec<crate::twitch::Stream> = vec![];
        let _ = backend.normalize_stream_resets(&mut empty).await;
        backend
            .state
            .set_followed_streams(vec![], HashSet::new())
            .await;

        // Stream returns — but grace period is 0 so it's already expired
        let new_start = Utc::now();
        let mut reset_stream = stream.clone();
        reset_stream.started_at = new_start;
        let mut streams = vec![reset_stream];
        let resumed = backend.normalize_stream_resets(&mut streams).await;

        assert_eq!(streams[0].started_at, new_start, "should use new started_at after grace expires");
        assert!(resumed.is_empty(), "should not be in resumed set");
    }
```

This requires a helper that sets the grace period config:

```rust
    fn make_test_backend_with_grace(poll_interval_sec: u64, grace_min: u64) -> (Backend, TempDir) {
        let tmp = TempDir::new().expect("tempdir");
        let db_path = tmp.path().join("data.db");
        let token_path = tmp.path().join("token.json");

        let config = Config {
            poll_interval_sec,
            stream_reset_grace_min: grace_min,
            ..Config::default()
        };

        let backend = Backend::with_test_deps(config, &db_path, &token_path);
        (backend, tmp)
    }
```

- [ ] **Step 9: Run test to verify it passes**

Run: `cargo test -p twitch-backend backend::tests::new_started_at_used_when_stream_returns_after_grace_period`
Expected: PASS

- [ ] **Step 10: Write test — stale entries pruned from grace period map**

```rust
    #[test]
    fn stale_entries_pruned_from_grace_period_map() {
        let (backend, _tmp) = make_test_backend_with_grace(60, 0);

        // Manually insert a stale entry
        {
            let mut recent = backend.recent_streams.lock().unwrap();
            recent.insert(
                "old_user".to_string(),
                (Utc::now() - Duration::hours(1), Instant::now()),
            );
        }

        // Run normalize with empty streams — should prune
        let rt = tokio::runtime::Runtime::new().unwrap();
        let mut streams: Vec<crate::twitch::Stream> = vec![];
        rt.block_on(backend.normalize_stream_resets(&mut streams));

        let recent = backend.recent_streams.lock().unwrap();
        assert!(recent.is_empty(), "stale entry should be pruned");
    }
```

- [ ] **Step 11: Run test to verify it passes**

Run: `cargo test -p twitch-backend backend::tests::stale_entries_pruned_from_grace_period_map`
Expected: PASS

- [ ] **Step 12: Write test — multiple resets preserve original started_at**

```rust
    #[tokio::test]
    async fn started_at_preserved_across_multiple_resets() {
        let (backend, _tmp) = make_test_backend(60);

        let original_start = Utc::now() - Duration::hours(3);
        let mut stream = make_stream("123", "TestStreamer");
        stream.started_at = original_start;

        // First poll — stream is live
        backend
            .state
            .set_followed_streams(vec![stream.clone()], HashSet::new())
            .await;

        // Second poll — first reset
        let mut reset1 = stream.clone();
        reset1.started_at = Utc::now() - Duration::hours(1);
        let mut streams = vec![reset1];
        let _ = backend.normalize_stream_resets(&mut streams).await;
        assert_eq!(streams[0].started_at, original_start);
        backend
            .state
            .set_followed_streams(streams, HashSet::new())
            .await;

        // Third poll — second reset
        let mut reset2 = stream.clone();
        reset2.started_at = Utc::now();
        let mut streams = vec![reset2];
        let _ = backend.normalize_stream_resets(&mut streams).await;
        assert_eq!(streams[0].started_at, original_start);
    }
```

- [ ] **Step 13: Run test to verify it passes**

Run: `cargo test -p twitch-backend backend::tests::started_at_preserved_across_multiple_resets`
Expected: PASS

- [ ] **Step 14: Write test — grace period return suppresses newly_live notification**

```rust
    #[tokio::test]
    async fn no_live_notification_on_grace_period_return() {
        let (backend, _tmp) = make_test_backend(60);

        let original_start = Utc::now() - Duration::hours(2);
        let mut stream = make_stream("123", "TestStreamer");
        stream.started_at = original_start;

        // Stream is live
        backend
            .state
            .set_followed_streams(vec![stream.clone()], HashSet::new())
            .await;

        // Stream goes offline — stash it
        let mut empty: Vec<crate::twitch::Stream> = vec![];
        let _ = backend.normalize_stream_resets(&mut empty).await;
        backend
            .state
            .set_followed_streams(vec![], HashSet::new())
            .await;

        // Subscribe to events
        let mut rx = backend.state.subscribe_streams();

        // Stream returns within grace period
        let mut reset_stream = stream.clone();
        reset_stream.started_at = Utc::now();
        let mut streams = vec![reset_stream];
        let resumed = backend.normalize_stream_resets(&mut streams).await;

        // Pass resumed set to set_followed_streams
        backend
            .state
            .set_followed_streams(streams, resumed)
            .await;
        let event = rx.recv().await.unwrap();

        assert!(
            event.newly_live.is_empty(),
            "grace-period return should not fire newly_live"
        );
    }
```

- [ ] **Step 15: Run test to verify it passes**

Run: `cargo test -p twitch-backend backend::tests::no_live_notification_on_grace_period_return`
Expected: PASS

- [ ] **Step 16: Write test — viewer observation uses original stream age after reset**

```rust
    #[test]
    fn viewer_count_recorded_at_original_stream_age_after_reset() {
        let (backend, _tmp) = make_test_backend(60);

        let original_start = Utc::now() - Duration::hours(2);
        let mut stream = make_stream("123", "TestStreamer");
        stream.started_at = original_start;

        // Stream goes live and records observations normally
        let event1 = StreamsUpdated {
            streams: vec![stream.clone()],
            newly_live: vec![stream.clone()],
            category_changes: vec![],
            title_changes: vec![],
        };
        backend.record_and_evaluate_hotness(&event1);

        // After a reset, started_at has already been normalized by normalize_stream_resets,
        // so the stream still has the original started_at.
        // Simulate a second poll with the same (normalized) started_at.
        let event2 = StreamsUpdated {
            streams: vec![stream.clone()],
            newly_live: vec![],
            category_changes: vec![],
            title_changes: vec![],
        };
        backend.record_and_evaluate_hotness(&event2);

        // Verify the recorded observations have the correct stream age (~120 min, not ~0)
        let now = Utc::now().timestamp();
        let obs = backend.db.get_all_recent_observations(now - 3600).unwrap();
        assert!(obs.len() >= 2);
        for o in &obs {
            assert!(
                o.stream_age_min >= 110,
                "stream_age_min should reflect original start, got {}",
                o.stream_age_min,
            );
        }
    }
```

- [ ] **Step 17: Run test to verify it passes**

Run: `cargo test -p twitch-backend backend::tests::viewer_count_recorded_at_original_stream_age_after_reset`
Expected: PASS

- [ ] **Step 18: Write test — no duplicate stream history entry on reset**

```rust
    #[test]
    fn no_duplicate_stream_history_on_reset() {
        let (backend, _tmp) = make_test_backend(60);

        let original_start = Utc::now() - Duration::hours(2);
        let mut stream = make_stream("123", "TestStreamer");
        stream.started_at = original_start;

        // Record the stream once
        backend.db.record_streams(&[stream.clone()]).unwrap();

        // After reset + normalization, started_at is still original_start,
        // so recording again is a no-op (INSERT OR IGNORE on UNIQUE(user_id, started_at))
        backend.db.record_streams(&[stream.clone()]).unwrap();

        let from = Utc::now() - Duration::hours(3);
        let to = Utc::now();
        let history = backend.db.get_streams_in_range(&[123], from, to).unwrap();
        let entries = history.get(&123).unwrap();
        assert_eq!(entries.len(), 1, "should have exactly one history entry");
    }
```

- [ ] **Step 19: Run test to verify it passes**

Run: `cargo test -p twitch-backend backend::tests::no_duplicate_stream_history_on_reset`
Expected: PASS

- [ ] **Step 20: Run all tests**

Run: `make test-all`
Expected: All PASS

- [ ] **Step 21: Commit**

```bash
git add crates/twitch-backend/src/backend.rs
git commit -m "feat: detect stream resets and preserve original started_at

Adds normalize_stream_resets to Backend with two detection paths:
- Instant: stream still live with different started_at
- Grace period: stream went offline briefly, returned within grace window

Fixes false hotness detection, phantom stream history entries, and
incorrect viewer observation age buckets caused by stream resets."
```

---

### Task 4: Wire `normalize_stream_resets` into `refresh_followed_streams`

**Files:**
- Modify: `crates/twitch-backend/src/backend.rs:735-753`

- [ ] **Step 1: Update `refresh_followed_streams`**

Change the method from:

```rust
    async fn refresh_followed_streams(&self) {
        if self.client.get_user_id().await.is_none() {
            return;
        }

        let mut streams = match self.with_retry(|| self.client.get_followed_streams()).await {
            Ok(streams) => streams,
            Err(e) => {
                tracing::error!("Failed to get followed streams: {}", e);
                return;
            }
        };

        // Enrich streams with profile image URLs from the Users API
        self.enrich_with_profile_images(&mut streams).await;

        self.session.record_live_refresh().await;
        self.state.set_followed_streams(streams).await;
    }
```

to:

```rust
    async fn refresh_followed_streams(&self) {
        if self.client.get_user_id().await.is_none() {
            return;
        }

        let mut streams = match self.with_retry(|| self.client.get_followed_streams()).await {
            Ok(streams) => streams,
            Err(e) => {
                tracing::error!("Failed to get followed streams: {}", e);
                return;
            }
        };

        // Enrich streams with profile image URLs from the Users API
        self.enrich_with_profile_images(&mut streams).await;

        // Normalize stream resets before state update — all downstream consumers
        // (hotness, history, display) see the original started_at.
        let resumed = self.normalize_stream_resets(&mut streams).await;

        self.session.record_live_refresh().await;
        self.state.set_followed_streams(streams, resumed).await;
    }
```

- [ ] **Step 2: Run all tests**

Run: `make test-all`
Expected: All PASS

- [ ] **Step 3: Run lint**

Run: `make lint`
Expected: No warnings

- [ ] **Step 4: Commit**

```bash
git add crates/twitch-backend/src/backend.rs
git commit -m "feat: wire stream reset normalization into refresh_followed_streams"
```

---

### Task 5: Update feature documentation

**Files:**
- Modify: `features/streamer-hotness.md`

- [ ] **Step 1: Add stream reset handling section**

After the "### No time-of-day bucketing (yet)" section (line ~165), add:

```markdown
### Stream reset detection

Twitch streams can reset briefly — OBS crashes, internet blips, etc. When this happens, Twitch assigns a new `stream.id` and `started_at`, but from the viewer's perspective the stream never ended. Without handling this:
- Viewer observations would be recorded at stream age ~0 with a mature audience, poisoning the historical baseline
- The reset stream would almost certainly trigger false hot detection
- A phantom entry in `stream_history` would inflate distinct stream counts

The fix normalizes `Stream.started_at` at the API boundary (`normalize_stream_resets` in `backend.rs`), before any downstream consumer sees the data. Two detection paths:
1. **Instant reset**: same `user_id` still live with a different `started_at` → overwrite in-place
2. **Grace-period reset**: stream went offline for 1+ polls but returned within `stream_reset_grace_min` (default 5 min) → overwrite and suppress `newly_live`

Because every downstream system reads `Stream.started_at` after normalization, a single fix handles hotness detection, viewer observations, stream history, schedule inference, and display duration.
```

- [ ] **Step 2: Add config entry**

In the "### Configuration" section, under the existing config entries, add:

```markdown
- `stream_reset_grace_min: u64` (default 5) — grace period for detecting stream resets that span poll boundaries. If a streamer goes offline and comes back within this window, the restart is treated as the same stream.
```

- [ ] **Step 3: Commit**

```bash
git add features/streamer-hotness.md
git commit -m "docs: document stream reset detection in hotness feature spec"
```

---

Plan complete and saved to `docs/superpowers/plans/2026-04-05-stream-reset-detection.md`. Two execution options:

**1. Subagent-Driven (recommended)** - I dispatch a fresh subagent per task, review between tasks, fast iteration

**2. Inline Execution** - Execute tasks in this session using executing-plans, batch execution with checkpoints

Which approach?
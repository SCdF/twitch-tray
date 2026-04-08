use chrono::{DateTime, Utc};
use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use tokio::sync::{broadcast, mpsc, watch, Mutex};
use tokio::time::{Duration, Instant};

use crate::app_services::AppServices;
use crate::auth::{TokenStore, CLIENT_ID};
use crate::config::ConfigManager;
use crate::db::Database;
use crate::events::BackendEvent;
use crate::handle::{AuthCommand, BackendHandle, HotnessDebugData, LoginProgress, RawDisplayData};
use crate::hotness_detection::{
    compute_age_window, compute_bucket_stats, compute_hotness, compute_hotness_profile,
    find_nearest_bucket, is_within_hotness_window, BucketStats, HotnessConfig, HotnessInfo,
    ViewerObservation,
};
use crate::notification_dispatcher::NotificationDispatcher;
use crate::notify::{DesktopNotifier, Notifier, SnoozeRequest, StreamerSettingsRequest};
use crate::schedule_walker::ScheduleWalker;
use crate::session::SessionManager;
use crate::state::AppState;
use crate::twitch::TwitchClient;
use tokio::task::JoinHandle;

/// Age points (in minutes) at which to precompute hotness bucket stats.
const HOTNESS_AGE_POINTS: &[i64] = &[0, 5, 10, 15, 30, 45, 60, 90, 120, 180, 240, 360];

/// Parameters for per-stream hotness evaluation (groups values to avoid too many arguments).
struct EvalHotnessParams<'a> {
    stats: &'a BucketStats,
    stream_age: i64,
    max_stream_age_min: u64,
    viewer_count: u32,
    broadcaster_id: &'a str,
    hotness_cfg: &'a HotnessConfig,
    notify_on_hot: bool,
}

/// Evaluates hotness for a single stream and updates the cache entry.
///
/// Returns `Some(info)` when the stream transitions from not-hot to hot (notification edge).
/// Returns `None` in all other cases (no transition, outside window, insufficient data).
///
/// Side effects on `cached`:
/// - `last_observation_count`, `last_distinct_streams` — always updated
/// - `last_hotness` — always updated with the evaluation (eligible or ineligible)
/// - `was_hot` — updated when inside the detection window
/// - When outside the window: hot status is preserved (streams don't lose 🔥 mid-stream)
fn evaluate_stream_hotness(
    cached: &mut CachedHotnessProfile,
    params: &EvalHotnessParams<'_>,
) -> Option<HotnessInfo> {
    // Always update raw debug stats
    cached.last_observation_count = params.stats.count;
    cached.last_distinct_streams = params.stats.distinct_streams;

    let age_within_window = is_within_hotness_window(params.stream_age, params.max_stream_age_min);

    // Evaluate hotness — compute_hotness returns Some for any non-empty bucket,
    // with eligible=false when age_within_window is false or gates fail.
    let result = compute_hotness(
        params.broadcaster_id,
        params.viewer_count,
        params.stats,
        params.hotness_cfg,
        cached.was_hot,
        age_within_window,
    );

    cached.last_hotness.clone_from(&result);

    // Past the detection window: a previously-hot stream stays hot only while
    // its current z-score is still above the cool threshold. This prevents a
    // stale 🔥 from clinging to a stream whose viewership has since dropped.
    // We never notify past the window (no new transitions out here).
    if !age_within_window {
        if cached.was_hot {
            let still_hot = result
                .as_ref()
                .is_some_and(|info| info.z_score >= params.hotness_cfg.z_cool_threshold);
            if let Some(ref mut info) = cached.last_hotness {
                info.is_hot = still_hot;
            }
            cached.was_hot = still_hot;
        }
        return None;
    }

    // Inside the window: drive was_hot from the (possibly ineligible) evaluation
    // and emit a notification on the not-hot → hot edge.
    if let Some(ref info) = result {
        let was_hot = cached.was_hot;
        cached.was_hot = info.is_hot;

        if info.is_hot && !was_hot && params.notify_on_hot {
            return result;
        }
    }

    None
}

/// Returns the observation retention period in seconds from config lookback days.
fn observation_retention_secs(lookback_days: u32) -> i64 {
    i64::from(lookback_days) * 24 * 3600
}

/// Cached hotness state for a single broadcaster.
struct CachedHotnessProfile {
    stream_started_at: i64,
    was_hot: bool,
    last_hotness: Option<HotnessInfo>,
    /// Raw stats from the last evaluation (always populated, even when gates block).
    last_age_window: Option<(i64, i64)>,
    last_observation_count: usize,
    last_distinct_streams: usize,
}

/// A stream that recently went offline, tracked for grace-period reset detection.
struct RecentStream {
    original_started_at: DateTime<Utc>,
    went_offline_at: Instant,
}

/// Internal backend orchestrator.
pub(crate) struct Backend {
    pub(crate) state: Arc<AppState>,
    pub(crate) config: Arc<ConfigManager>,
    pub(crate) client: TwitchClient,
    pub(crate) notifier: Arc<dyn Notifier>,
    pub(crate) db: Database,

    session: SessionManager,
    walker: Arc<ScheduleWalker>,
    dispatcher: Arc<NotificationDispatcher>,

    auth_cancel_tx: watch::Sender<bool>,
    auth_cancel_rx: watch::Receiver<bool>,

    login_progress_rx: watch::Receiver<Option<LoginProgress>>,

    snooze_tx: mpsc::UnboundedSender<SnoozeRequest>,
    snooze_rx: Arc<Mutex<Option<mpsc::UnboundedReceiver<SnoozeRequest>>>>,

    settings_tx: mpsc::UnboundedSender<StreamerSettingsRequest>,
    settings_rx: Arc<Mutex<Option<mpsc::UnboundedReceiver<StreamerSettingsRequest>>>>,

    /// In-memory cache for profile image URLs (user_id -> (url, fetched_at)).
    profile_image_cache: Arc<std::sync::Mutex<HashMap<String, (String, Instant)>>>,

    /// In-memory cache for box art URLs (game_id -> (url, fetched_at)).
    box_art_cache: Arc<std::sync::Mutex<HashMap<String, (String, Instant)>>>,

    /// In-memory cache for hotness profiles (broadcaster user_id -> profile).
    /// Populated when a stream goes live, evicted when it goes offline.
    hotness_cache: Arc<std::sync::Mutex<HashMap<String, CachedHotnessProfile>>>,

    /// Recently-offline streams for detecting resets that span poll boundaries.
    /// Maps user_id -> entry with original started_at and when it went offline.
    /// Entries older than `stream_reset_grace_min` are pruned each poll.
    recent_streams: Arc<std::sync::Mutex<HashMap<String, RecentStream>>>,

    /// Last raw `started_at` value seen from the API per live user_id (NOT the
    /// canonical/patched value stored in `state`). Used to make instant-reset
    /// detection idempotent: a "reset" is only flagged when the *API value*
    /// jumps forward, not when it persistently disagrees with a frozen
    /// canonical value. Entries are pruned when a stream goes offline.
    last_raw_started_at: Arc<std::sync::Mutex<HashMap<String, DateTime<Utc>>>>,
}

/// Minimum forward jump in `started_at` (raw API value to raw API value)
/// required to be considered a real stream reset rather than API jitter.
const STREAM_RESET_THRESHOLD: chrono::Duration = chrono::Duration::seconds(60);

impl Backend {
    fn new() -> anyhow::Result<Self> {
        use std::sync::atomic::AtomicBool;
        use tokio::sync::RwLock;

        let config = Arc::new(ConfigManager::new()?);
        let state = AppState::new();
        let (snooze_tx, snooze_rx) = mpsc::unbounded_channel();
        let (settings_tx, settings_rx) = mpsc::unbounded_channel();
        let notifier: Arc<dyn Notifier> =
            Arc::new(DesktopNotifier::new(snooze_tx.clone(), settings_tx.clone()));
        let client = TwitchClient::new(CLIENT_ID.to_string());
        let db = Database::new(&ConfigManager::config_dir()?.join("data.db"))?;
        let (auth_cancel_tx, auth_cancel_rx) = watch::channel(false);

        let (session, login_progress_rx) = SessionManager::new(
            TokenStore::new()?,
            client.clone(),
            state.clone(),
            db.clone(),
            Arc::new(AtomicBool::new(false)),
            Arc::new(RwLock::new(None)),
            Arc::new(Mutex::new(())),
        );

        let walker = Arc::new(ScheduleWalker::new(
            db.clone(),
            client.clone(),
            state.clone(),
            config.clone(),
            session.clone(),
        ));

        let dispatcher = Arc::new(NotificationDispatcher::new(
            notifier.clone(),
            config.clone(),
            session.initial_load_done.clone(),
        ));

        Ok(Self {
            state,
            config,
            client,
            notifier,
            db,
            session,
            walker,
            dispatcher,
            auth_cancel_tx,
            auth_cancel_rx,
            login_progress_rx,
            snooze_tx,
            snooze_rx: Arc::new(Mutex::new(Some(snooze_rx))),
            settings_tx,
            settings_rx: Arc::new(Mutex::new(Some(settings_rx))),
            profile_image_cache: Arc::new(std::sync::Mutex::new(HashMap::new())),
            box_art_cache: Arc::new(std::sync::Mutex::new(HashMap::new())),
            hotness_cache: Arc::new(std::sync::Mutex::new(HashMap::new())),
            recent_streams: Arc::new(std::sync::Mutex::new(HashMap::new())),
            last_raw_started_at: Arc::new(std::sync::Mutex::new(HashMap::new())),
        })
    }

    async fn with_retry<F, Fut, T>(&self, f: F) -> Result<T, crate::twitch::ApiError>
    where
        F: Fn() -> Fut,
        Fut: std::future::Future<Output = Result<T, crate::twitch::ApiError>>,
    {
        crate::twitch::with_retry(f, || self.session.try_refresh_token()).await
    }

    /// Starts all background tasks, wiring the display watch channel and event broadcast.
    fn start_tasks(
        self: &Arc<Self>,
        display_tx: &watch::Sender<RawDisplayData>,
        event_tx: &broadcast::Sender<BackendEvent>,
        auth_cmd_rx: mpsc::UnboundedReceiver<AuthCommand>,
    ) -> Vec<JoinHandle<()>> {
        let mut handles = Vec::new();

        // Session restore + initial data fetch
        let backend = self.clone();
        let display_tx_init = display_tx.clone();
        let event_tx_init = event_tx.clone();
        handles.push(tokio::spawn(async move {
            match backend.session.restore_session().await {
                Ok(()) => {
                    tracing::info!("Session restored");
                    let _ = event_tx_init.send(BackendEvent::AuthStateChanged {
                        is_authenticated: true,
                    });
                    backend.refresh_all_data().await;
                }
                Err(e) => {
                    tracing::info!("No stored session: {}", e);
                }
            }
            // Initial display push (unauthenticated or authenticated after restore)
            backend.push_display_state(&display_tx_init).await;
        }));

        // Auth command handler (login / logout)
        let backend = self.clone();
        let event_tx_auth = event_tx.clone();
        let display_tx_auth = display_tx.clone();
        handles.push(tokio::spawn(async move {
            let mut rx = auth_cmd_rx;
            while let Some(cmd) = rx.recv().await {
                match cmd {
                    AuthCommand::Login => {
                        backend.handle_login(&event_tx_auth, &display_tx_auth).await;
                    }
                    AuthCommand::Logout => {
                        backend
                            .handle_logout(&event_tx_auth, &display_tx_auth)
                            .await;
                    }
                }
            }
        }));

        // Stream polling task
        let backend = self.clone();
        handles.push(tokio::spawn(async move {
            let tick_duration = Duration::from_secs(1);
            loop {
                tokio::time::sleep(tick_duration).await;
                backend.tick_stream_poll(Utc::now()).await;
            }
        }));

        // Schedule queue walker
        handles.push(self.walker.clone().start());

        // Followed channels refresh task
        let backend = self.clone();
        handles.push(tokio::spawn(async move {
            let tick_duration = Duration::from_secs(1);
            let mut last_refresh: Option<DateTime<Utc>> = None;
            loop {
                tokio::time::sleep(tick_duration).await;
                let now = Utc::now();
                let interval_secs = backend.config.get().followed_refresh_min * 60;
                if backend
                    .tick_followed_channels(now, last_refresh, interval_secs)
                    .await
                {
                    last_refresh = Some(now);
                }
            }
        }));

        // Snooze notification task
        let backend = self.clone();
        handles.push(tokio::spawn(async move {
            let Some(mut rx) = backend.snooze_rx.lock().await.take() else {
                tracing::warn!("Snooze receiver already taken");
                return;
            };

            let mut snoozed: HashMap<String, (SnoozeRequest, crate::twitch::Stream)> =
                HashMap::new();

            loop {
                tokio::time::sleep(Duration::from_secs(1)).await;

                while let Ok(request) = rx.try_recv() {
                    tracing::info!(
                        "Snooze registered for {} (remind at {})",
                        request.user_name,
                        request.remind_at
                    );
                    let streams = backend.state.get_followed_streams().await;
                    if let Some(stream) = streams.iter().find(|s| s.user_id == request.user_id) {
                        snoozed.insert(request.user_id.clone(), (request, stream.clone()));
                    }
                }

                if snoozed.is_empty() {
                    continue;
                }

                let now = Utc::now();
                let live_streams = backend.state.get_followed_streams().await;

                let mut to_remove = Vec::new();
                for (user_id, (request, stream)) in &snoozed {
                    let still_live = live_streams.iter().any(|s| s.user_id == *user_id);
                    if !still_live {
                        tracing::debug!("Snooze cancelled for {} (stream offline)", user_id);
                        to_remove.push(user_id.clone());
                    } else if now >= request.remind_at {
                        let current_stream = live_streams
                            .iter()
                            .find(|s| s.user_id == *user_id)
                            .unwrap_or(stream);
                        if let Err(e) = backend.notifier.stream_reminder(current_stream) {
                            tracing::error!("Snooze reminder notification error: {}", e);
                        }
                        to_remove.push(user_id.clone());
                    }
                }

                for user_id in to_remove {
                    snoozed.remove(&user_id);
                }
            }
        }));

        // Settings request task — auto-adds streamer to config, then emits BackendEvent
        let backend = self.clone();
        let event_tx_settings = event_tx.clone();
        handles.push(tokio::spawn(async move {
            let Some(mut rx) = backend.settings_rx.lock().await.take() else {
                tracing::warn!("Settings receiver already taken");
                return;
            };

            while let Some(request) = rx.recv().await {
                tracing::info!(
                    "Settings requested for {} ({})",
                    request.display_name,
                    request.user_login,
                );

                // Auto-add streamer to config if not already present
                let mut cfg = backend.config.get();
                if !cfg.streamer_settings.contains_key(&request.user_login) {
                    cfg.streamer_settings.insert(
                        request.user_login.clone(),
                        crate::config::StreamerSettings {
                            display_name: request.display_name.clone(),
                            importance: crate::config::StreamerImportance::Normal,
                            hotness_z_threshold_override: None,
                        },
                    );
                    if let Err(e) = backend.config.save(cfg) {
                        tracing::error!("Failed to save config with new streamer: {}", e);
                    }
                }

                let _ = event_tx_settings.send(BackendEvent::OpenSettingsRequested {
                    user_login: request.user_login,
                    display_name: request.display_name,
                });
            }
        }));

        // State change listener task — pushes RawDisplayData on any state change
        let backend = self.clone();
        let display_tx_state = display_tx.clone();
        handles.push(tokio::spawn(async move {
            let mut rx = backend.state.subscribe();

            while rx.changed().await.is_ok() {
                if rx.borrow().is_none() {
                    continue;
                }

                // Debounce: coalesce rapid-fire state changes
                tokio::time::sleep(Duration::from_millis(500)).await;
                let _ = *rx.borrow_and_update();

                backend.push_display_state(&display_tx_state).await;
            }
        }));

        // Notification listener task
        handles.push(
            self.dispatcher
                .clone()
                .start(self.state.subscribe_streams()),
        );

        // History + viewer observation recording listener task
        let backend = self.clone();
        handles.push(tokio::spawn(async move {
            let mut rx = backend.state.subscribe_streams();

            loop {
                match rx.recv().await {
                    Ok(event) => {
                        if let Err(e) = backend.db.record_streams(&event.streams) {
                            tracing::error!("Failed to record stream history: {}", e);
                        }

                        // Record viewer observations for hotness detection
                        backend.record_and_evaluate_hotness(&event);
                    }
                    Err(tokio::sync::broadcast::error::RecvError::Lagged(n)) => {
                        tracing::warn!("History listener lagged by {} events", n);
                    }
                    Err(tokio::sync::broadcast::error::RecvError::Closed) => {
                        break;
                    }
                }
            }
        }));

        handles
    }

    /// Collects current state and sends a RawDisplayData snapshot.
    async fn push_display_state(&self, display_tx: &watch::Sender<RawDisplayData>) {
        let cfg = self.config.get();
        let scheduled_streams = self.state.get_scheduled_streams().await;

        // Ensure profile images are cached for scheduled broadcasters
        let sched_ids: Vec<String> = scheduled_streams
            .iter()
            .map(|s| s.broadcaster_id.clone())
            .collect();
        self.ensure_profile_images_cached(&sched_ids).await;

        let profile_image_urls = {
            let cache = self.profile_image_cache.lock().unwrap();
            cache
                .iter()
                .map(|(id, (url, _))| (id.clone(), url.clone()))
                .collect()
        };

        // Ensure box art is cached for followed categories
        let cat_ids: Vec<String> = cfg
            .followed_categories
            .iter()
            .map(|c| c.id.clone())
            .collect();
        self.ensure_box_art_cached(&cat_ids).await;

        let box_art_urls = {
            let cache = self.box_art_cache.lock().unwrap();
            cache
                .iter()
                .map(|(id, (url, _))| (id.clone(), url.clone()))
                .collect()
        };
        let live_streams = self.state.get_followed_streams().await;

        // Evaluate hotness for all live streams
        let hotness_results = self.evaluate_hotness(&live_streams);
        let hot_stream_ids: std::collections::HashSet<String> = hotness_results
            .iter()
            .filter(|h| h.is_hot)
            .map(|h| h.broadcaster_id.clone())
            .collect();

        let hotness_debug = if cfg!(debug_assertions) {
            self.collect_hotness_debug(&live_streams)
        } else {
            HashMap::new()
        };

        let raw = RawDisplayData {
            is_authenticated: self.state.is_authenticated().await,
            live_streams,
            scheduled_streams,
            schedules_loaded: self.state.schedules_loaded().await,
            followed_channels: self.state.get_followed_channels().await,
            followed_categories: cfg.followed_categories.clone(),
            category_streams: self.state.get_category_streams().await,
            config: cfg,
            profile_image_urls,
            box_art_urls,
            hot_stream_ids,
            hotness_debug,
        };
        let _ = display_tx.send(raw);
    }

    async fn tick_stream_poll(&self, now: DateTime<Utc>) -> bool {
        if !self.state.is_authenticated().await {
            return false;
        }

        let last_refresh = self.session.last_live_refresh().await;
        let poll_interval_secs = self.config.get().poll_interval_sec;

        let should_refresh = match last_refresh {
            None => true,
            Some(last) => (now - last).num_seconds() >= poll_interval_secs as i64,
        };

        if should_refresh {
            self.refresh_followed_streams().await;
            self.refresh_category_streams().await;
            self.refresh_schedules_from_db().await;
        }

        should_refresh
    }

    async fn tick_followed_channels(
        &self,
        now: DateTime<Utc>,
        last_refresh: Option<DateTime<Utc>>,
        interval_secs: u64,
    ) -> bool {
        if !self.state.is_authenticated().await {
            return false;
        }

        let should_refresh = match last_refresh {
            None => true,
            Some(last) => (now - last).num_seconds() >= interval_secs as i64,
        };

        if !should_refresh {
            return false;
        }

        if let Err(e) = self.session.load_followed_channels().await {
            tracing::warn!("Failed to refresh followed channels: {}", e);
            false
        } else {
            true
        }
    }

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
        let grace = std::time::Duration::from_secs(self.config.get().stream_reset_grace_min * 60);

        let old_streams = self.state.get_followed_streams().await;
        let old_by_id: HashMap<&str, &crate::twitch::Stream> = old_streams
            .iter()
            .map(|s| (s.user_id.as_str(), s))
            .collect();

        let new_by_id: std::collections::HashSet<String> =
            streams.iter().map(|s| s.user_id.clone()).collect();

        let mut resumed = HashSet::new();

        // Case 1: instant reset — same user_id still live, raw API `started_at`
        // jumped forward by more than the threshold from its previously-seen
        // raw value. Comparing raw-vs-raw (rather than raw-vs-canonical) keeps
        // detection idempotent: a persistent disagreement between the API
        // value and a frozen canonical value won't re-fire every poll.
        {
            let mut last_raw = self.last_raw_started_at.lock().unwrap();
            for stream in streams.iter_mut() {
                let new_raw = stream.started_at;

                // Reference: prefer the previously-seen raw API value. On the
                // very first encounter (e.g. fresh start), fall back to the
                // canonical value in state so we can still catch a reset that
                // happened between backend startup and the first poll-pair.
                let reference = last_raw
                    .get(&stream.user_id)
                    .copied()
                    .or_else(|| old_by_id.get(stream.user_id.as_str()).map(|s| s.started_at));

                let is_real_reset =
                    reference.is_some_and(|r| (new_raw - r) > STREAM_RESET_THRESHOLD);

                if is_real_reset {
                    if let Some(old) = old_by_id.get(stream.user_id.as_str()) {
                        tracing::info!(
                            "Stream reset detected for {} (instant) — preserving original started_at",
                            stream.user_name,
                        );
                        stream.started_at = old.started_at;
                    }
                } else if let Some(old) = old_by_id.get(stream.user_id.as_str()) {
                    // Sub-threshold jitter (or no change): silently normalise
                    // to the canonical value to avoid drift. No log.
                    stream.started_at = old.started_at;
                }

                last_raw.insert(stream.user_id.clone(), new_raw);
            }

            // Drop entries for streams that are no longer live this poll.
            last_raw.retain(|id, _| new_by_id.contains(id));
        }

        // Stash streams that just went offline into the grace-period map
        {
            let mut recent = self.recent_streams.lock().unwrap();
            for old in &old_streams {
                if !new_by_id.contains(&old.user_id) {
                    recent.insert(
                        old.user_id.clone(),
                        RecentStream {
                            original_started_at: old.started_at,
                            went_offline_at: Instant::now(),
                        },
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
                if let Some(entry) = recent.remove(&stream.user_id) {
                    if entry.went_offline_at.elapsed() < grace {
                        tracing::info!(
                            "Stream reset detected for {} (grace period) — preserving original started_at",
                            stream.user_name,
                        );
                        stream.started_at = entry.original_started_at;
                        resumed.insert(stream.user_id.clone());
                    }
                }
            }

            // Prune stale entries
            recent.retain(|_, entry| entry.went_offline_at.elapsed() < grace);
        }

        resumed
    }

    /// Records viewer observations and evaluates hotness for all live streams.
    ///
    /// For newly live streams, initialises the cache entry with `stream_started_at`.
    /// Each poll dynamically queries DB for the sliding window around the current age.
    /// For streams going offline, evicts them from the cache.
    fn record_and_evaluate_hotness(&self, event: &crate::state::StreamsUpdated) {
        let now = Utc::now();
        let now_ts = now.timestamp();
        let cfg = self.config.get();
        let since = now_ts - observation_retention_secs(cfg.hotness_lookback_days);

        // Build viewer observations from current live streams
        let observations: Vec<ViewerObservation> = event
            .streams
            .iter()
            .map(|s| {
                let age = (now - s.started_at).num_minutes().max(0);
                ViewerObservation {
                    broadcaster_id: s.user_id.parse().unwrap_or(0),
                    observed_at: now_ts,
                    stream_age_min: age,
                    viewer_count: s.viewer_count,
                    stream_started_at: s.started_at.timestamp(),
                }
            })
            .collect();

        if let Err(e) = self.db.record_viewer_observations(&observations) {
            tracing::error!("Failed to record viewer observations: {}", e);
        }

        // Initialise cache for newly live streams
        for stream in &event.newly_live {
            let mut cache = self.hotness_cache.lock().unwrap();
            cache.insert(
                stream.user_id.clone(),
                CachedHotnessProfile {
                    stream_started_at: stream.started_at.timestamp(),
                    was_hot: false,
                    last_hotness: None,
                    last_age_window: None,
                    last_observation_count: 0,
                    last_distinct_streams: 0,
                },
            );
        }

        // Evaluate hotness with dynamic sliding window queries
        {
            let mut cache = self.hotness_cache.lock().unwrap();
            for stream in &event.streams {
                let Some(cached) = cache.get_mut(&stream.user_id) else {
                    continue;
                };

                let broadcaster_id: i64 = match stream.user_id.parse() {
                    Ok(id) => id,
                    Err(_) => continue,
                };

                let age = (now - stream.started_at).num_minutes().max(0);
                let (age_lo, age_hi) = compute_age_window(age, cfg.hotness_age_window_divisor);

                // Release builds: skip the DB query (and the entire eval)
                // past the age cap, since the result would never be displayed.
                #[cfg(not(debug_assertions))]
                if !is_within_hotness_window(age, cfg.hotness_max_stream_age_min) {
                    cached.last_age_window = Some((age_lo, age_hi));
                    // Preserve sticky hot state without re-evaluating.
                    if cached.was_hot {
                        if let Some(ref mut info) = cached.last_hotness {
                            info.is_hot = true;
                        }
                    }
                    continue;
                }

                let obs = match self.db.get_viewer_observations_excluding_stream(
                    broadcaster_id,
                    age_lo,
                    age_hi,
                    since,
                    cached.stream_started_at,
                ) {
                    Ok(obs) => obs,
                    Err(e) => {
                        tracing::warn!(
                            "Failed to query hotness observations for {}: {}",
                            stream.user_name,
                            e
                        );
                        continue;
                    }
                };

                let stats = compute_bucket_stats(&obs);

                cached.last_age_window = Some((age_lo, age_hi));

                let z_threshold = cfg
                    .streamer_settings
                    .get(&stream.user_login)
                    .and_then(|s| s.hotness_z_threshold_override)
                    .unwrap_or(cfg.hotness_z_threshold);

                let hotness_cfg = HotnessConfig {
                    z_threshold,
                    z_cool_threshold: cfg.hotness_z_cool_threshold,
                    min_observations: cfg.hotness_min_observations,
                    min_streams: cfg.hotness_min_streams,
                };

                let params = EvalHotnessParams {
                    stats: &stats,
                    stream_age: age,
                    max_stream_age_min: cfg.hotness_max_stream_age_min,
                    viewer_count: stream.viewer_count,
                    broadcaster_id: &stream.user_id,
                    hotness_cfg: &hotness_cfg,
                    notify_on_hot: cfg.notify_on_hot,
                };

                if let Some(info) = evaluate_stream_hotness(cached, &params) {
                    tracing::info!(
                        "🔥 {} is HOT (z={:.1}σ, {} viewers, avg {:.0})",
                        stream.user_name,
                        info.z_score,
                        info.current_viewers,
                        info.mean_viewers,
                    );
                    if let Err(e) = self.notifier.stream_hot(stream, &info) {
                        tracing::error!("Hot notification error: {}", e);
                    }
                }
            }
        }

        // Evict streams that went offline
        let live_ids: std::collections::HashSet<&str> =
            event.streams.iter().map(|s| s.user_id.as_str()).collect();
        {
            let mut cache = self.hotness_cache.lock().unwrap();
            cache.retain(|id, _| live_ids.contains(id.as_str()));
        }
    }

    /// Returns cached hotness info for live streams.
    /// Results are populated by `record_and_evaluate_hotness` each poll.
    fn evaluate_hotness(&self, streams: &[crate::twitch::Stream]) -> Vec<HotnessInfo> {
        let cache = self.hotness_cache.lock().unwrap();
        streams
            .iter()
            .filter_map(|s| cache.get(&s.user_id)?.last_hotness.clone())
            .collect()
    }

    /// Collects debug hotness data from the cache for all live streams.
    fn collect_hotness_debug(
        &self,
        streams: &[crate::twitch::Stream],
    ) -> HashMap<String, HotnessDebugData> {
        let cache = self.hotness_cache.lock().unwrap();
        streams
            .iter()
            .filter_map(|s| {
                let cached = cache.get(&s.user_id)?;
                let (age_lo, age_hi) = cached.last_age_window?;
                let hotness = cached.last_hotness.as_ref()?;
                Some((
                    s.user_id.clone(),
                    HotnessDebugData {
                        mean_viewers: hotness.mean_viewers,
                        z_score: hotness.z_score,
                        age_window_lo: age_lo,
                        age_window_hi: age_hi,
                        observation_count: cached.last_observation_count,
                        eligible: hotness.eligible,
                    },
                ))
            })
            .collect()
    }

    pub(crate) async fn refresh_all_data(&self) {
        self.refresh_followed_streams().await;
        self.refresh_schedules_from_db().await;
        self.refresh_category_streams().await;
        self.session.mark_initial_load_done();
        self.session.record_live_refresh().await;
    }

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

        // Detect stream resets and preserve original started_at
        let resumed = self.normalize_stream_resets(&mut streams).await;

        self.session.record_live_refresh().await;
        self.state.set_followed_streams(streams, resumed).await;
    }

    /// Ensures all given user IDs have profile images in the cache.
    /// Fetches any missing ones from the Twitch Users API.
    async fn ensure_profile_images_cached(&self, user_ids: &[String]) {
        const CACHE_TTL: Duration = Duration::from_secs(3600);

        if user_ids.is_empty() {
            return;
        }

        let now = Instant::now();

        let uncached_ids: Vec<String> = {
            let cache = self.profile_image_cache.lock().unwrap();
            user_ids
                .iter()
                .filter(|id| {
                    cache
                        .get(id.as_str())
                        .is_none_or(|(_, fetched_at)| now.duration_since(*fetched_at) >= CACHE_TTL)
                })
                .cloned()
                .collect()
        };

        if uncached_ids.is_empty() {
            return;
        }

        // Fetch in batches of 100 (Twitch API limit)
        let uncached_refs: Vec<&str> = uncached_ids
            .iter()
            .map(std::string::String::as_str)
            .collect();
        let mut fetched = HashMap::new();
        for chunk in uncached_refs.chunks(100) {
            match self
                .with_retry(|| self.client.get_users_by_ids(chunk))
                .await
            {
                Ok(users) => {
                    for user in users {
                        fetched.insert(user.id, user.profile_image_url);
                    }
                }
                Err(e) => {
                    tracing::warn!("Failed to fetch user profiles: {}", e);
                }
            }
        }

        if !fetched.is_empty() {
            let mut cache = self.profile_image_cache.lock().unwrap();
            for (id, url) in fetched {
                cache.insert(id, (url, now));
            }
        }
    }

    /// Ensures all given game/category IDs have box art URLs in the cache.
    /// Fetches any missing ones from the Twitch Games API.
    async fn ensure_box_art_cached(&self, game_ids: &[String]) {
        const CACHE_TTL: Duration = Duration::from_secs(3600);

        if game_ids.is_empty() {
            return;
        }

        let now = Instant::now();

        let uncached_ids: Vec<String> = {
            let cache = self.box_art_cache.lock().unwrap();
            game_ids
                .iter()
                .filter(|id| {
                    cache
                        .get(id.as_str())
                        .is_none_or(|(_, fetched_at)| now.duration_since(*fetched_at) >= CACHE_TTL)
                })
                .cloned()
                .collect()
        };

        if uncached_ids.is_empty() {
            return;
        }

        let uncached_refs: Vec<&str> = uncached_ids
            .iter()
            .map(std::string::String::as_str)
            .collect();
        let mut fetched = HashMap::new();
        for chunk in uncached_refs.chunks(100) {
            match self
                .with_retry(|| self.client.get_games_by_ids(chunk))
                .await
            {
                Ok(games) => {
                    for game in games {
                        // Replace template placeholders with fixed dimensions (144x192)
                        let url = game
                            .box_art_url
                            .replace("{width}", "144")
                            .replace("{height}", "192");
                        fetched.insert(game.id, url);
                    }
                }
                Err(e) => {
                    tracing::warn!("Failed to fetch game box art: {}", e);
                }
            }
        }

        if !fetched.is_empty() {
            let mut cache = self.box_art_cache.lock().unwrap();
            for (id, url) in fetched {
                cache.insert(id, (url, now));
            }
        }
    }

    async fn enrich_with_profile_images(&self, streams: &mut [crate::twitch::Stream]) {
        let user_ids: Vec<String> = streams.iter().map(|s| s.user_id.clone()).collect();
        self.ensure_profile_images_cached(&user_ids).await;

        let cache = self.profile_image_cache.lock().unwrap();
        for stream in streams.iter_mut() {
            if let Some((url, _)) = cache.get(&stream.user_id) {
                stream.profile_image_url = url.clone();
            }
        }
    }

    pub(crate) async fn refresh_schedules_from_db(&self) {
        self.walker.refresh_schedules_from_db().await;
    }

    pub(crate) async fn refresh_category_streams(&self) {
        let categories = self.config.get().followed_categories;
        if categories.is_empty() {
            return;
        }

        let language = crate::twitch::system_language();
        let lang_ref = language.as_deref();

        for category in &categories {
            let cat_id = category.id.clone();
            let mut streams = match self
                .with_retry(|| self.client.get_streams_by_category(&cat_id, lang_ref))
                .await
            {
                Ok(streams) => streams,
                Err(e) => {
                    tracing::error!(
                        "Failed to get category streams for {}: {}",
                        category.name,
                        e
                    );
                    continue;
                }
            };

            self.enrich_with_profile_images(&mut streams).await;

            self.state
                .set_category_streams(category.id.clone(), streams)
                .await;
        }
    }

    async fn handle_login(
        &self,
        event_tx: &broadcast::Sender<BackendEvent>,
        display_tx: &watch::Sender<RawDisplayData>,
    ) {
        let _ = self.auth_cancel_tx.send(false);
        let cancel_rx = self.auth_cancel_rx.clone();

        match self.session.handle_login(cancel_rx).await {
            Ok(()) => {
                let _ = event_tx.send(BackendEvent::AuthStateChanged {
                    is_authenticated: true,
                });
                self.refresh_all_data().await;
                self.push_display_state(display_tx).await;
            }
            Err(e) => {
                tracing::error!("Authentication failed: {}", e);
                let _ = self.notifier.error(&format!("Authentication failed: {e}"));
            }
        }
    }

    async fn handle_logout(
        &self,
        event_tx: &broadcast::Sender<BackendEvent>,
        display_tx: &watch::Sender<RawDisplayData>,
    ) {
        self.session.handle_logout().await;
        let _ = event_tx.send(BackendEvent::AuthStateChanged {
            is_authenticated: false,
        });
        self.push_display_state(display_tx).await;
    }

    pub(crate) async fn get_debug_hotness_data(
        &self,
    ) -> Vec<crate::app_services::DebugHotnessEntry> {
        use crate::app_services::DebugHotnessEntry;

        let streams = self.state.get_followed_streams().await;
        let cache = self.hotness_cache.lock().unwrap();

        streams
            .iter()
            .map(|stream| {
                let cached = cache.get(&stream.user_id);
                let hotness = cached.and_then(|c| c.last_hotness.as_ref());

                DebugHotnessEntry {
                    broadcaster_name: stream.user_name.clone(),
                    broadcaster_login: stream.user_login.clone(),
                    current_viewers: stream.viewer_count,
                    mean: hotness.map(|h| h.mean_viewers),
                    stddev: hotness.map(|h| h.stddev),
                    z_score: hotness.map(|h| h.z_score),
                    observation_count: hotness.map_or(0, |h| h.observation_count),
                    distinct_streams: hotness.map_or(0, |h| h.distinct_streams),
                    is_hot: hotness.is_some_and(|h| h.is_hot),
                    age_min: cached.and_then(|c| c.last_age_window.map(|(lo, _)| lo)),
                    age_max: cached.and_then(|c| c.last_age_window.map(|(_, hi)| hi)),
                    window_observations: cached.map_or(0, |c| c.last_observation_count),
                    window_distinct_streams: cached.map_or(0, |c| c.last_distinct_streams),
                }
            })
            .collect()
    }

    pub(crate) async fn get_debug_hotness_profiles(
        &self,
    ) -> Vec<crate::app_services::DebugHotnessProfileEntry> {
        use crate::app_services::{DebugBucketEntry, DebugHotnessProfileEntry};

        let channels = self.state.get_followed_channels().await;
        let streams = self.state.get_followed_streams().await;
        let now = Utc::now();
        let cfg = self.config.get();
        let since = now.timestamp() - observation_retention_secs(cfg.hotness_lookback_days);

        // Build a map of live streams by broadcaster_id
        let live_map: HashMap<&str, &crate::twitch::Stream> =
            streams.iter().map(|s| (s.user_id.as_str(), s)).collect();

        // Load all recent observations and group by broadcaster_id
        let all_obs = match self.db.get_all_recent_observations(since) {
            Ok(obs) => obs,
            Err(e) => {
                tracing::error!("Failed to load observations for debug profiles: {}", e);
                return Vec::new();
            }
        };

        let mut obs_by_broadcaster: HashMap<i64, Vec<_>> = HashMap::new();
        for obs in all_obs {
            obs_by_broadcaster
                .entry(obs.broadcaster_id)
                .or_default()
                .push(obs);
        }

        channels
            .iter()
            .filter_map(|ch| {
                let broadcaster_id: i64 = ch.broadcaster_id.parse().ok()?;
                let obs = obs_by_broadcaster.get(&broadcaster_id);

                // Skip channels with no observation history
                let obs = obs?;

                // Exclude current stream's observations so the profile reflects
                // only prior streams — matching the baseline used for live hotness.
                let filtered_obs: Vec<_> =
                    if let Some(stream) = live_map.get(ch.broadcaster_id.as_str()) {
                        let current_started = stream.started_at.timestamp();
                        obs.iter()
                            .filter(|o| o.stream_started_at != current_started)
                            .cloned()
                            .collect()
                    } else {
                        obs.clone()
                    };

                if filtered_obs.is_empty() {
                    return None;
                }

                let profile = compute_hotness_profile(
                    &filtered_obs,
                    HOTNESS_AGE_POINTS,
                    cfg.hotness_age_window_divisor,
                );
                let is_live = live_map.contains_key(ch.broadcaster_id.as_str());

                let (current_bucket_age, current_viewers) =
                    if let Some(stream) = live_map.get(ch.broadcaster_id.as_str()) {
                        let age = (now - stream.started_at).num_minutes().max(0);
                        let nearest = find_nearest_bucket(&profile, age).map(|_| {
                            // Find the actual age point of the nearest bucket
                            profile
                                .iter()
                                .min_by_key(|(a, _)| (a - age).unsigned_abs())
                                .map_or(0, |(a, _)| *a)
                        });
                        (nearest, Some(stream.viewer_count))
                    } else {
                        (None, None)
                    };

                let buckets = profile
                    .iter()
                    .map(|(age_point, stats)| DebugBucketEntry {
                        age_point: *age_point,
                        mean: stats.mean,
                        stddev: stats.stddev,
                        count: stats.count,
                        distinct_streams: stats.distinct_streams,
                    })
                    .collect();

                Some(DebugHotnessProfileEntry {
                    broadcaster_name: ch.broadcaster_name.clone(),
                    broadcaster_login: ch.broadcaster_login.clone(),
                    broadcaster_id: ch.broadcaster_id.clone(),
                    is_live,
                    current_bucket_age,
                    current_viewers,
                    buckets,
                })
            })
            .collect()
    }

    pub(crate) async fn get_debug_schedule_data(
        &self,
        start: i64,
        end: i64,
    ) -> Vec<crate::app_services::DebugStreamEntry> {
        use crate::app_services::DebugStreamEntry;

        let start_dt = DateTime::from_timestamp(start, 0).unwrap_or_default();
        let end_dt = DateTime::from_timestamp(end, 0).unwrap_or_default();

        let mut entries: Vec<DebugStreamEntry> = self
            .db
            .get_raw_history_in_window(start, end)
            .unwrap_or_default()
            .into_iter()
            .map(|(name, login, ts)| DebugStreamEntry {
                is_inferred: false,
                broadcaster_name: name,
                broadcaster_login: login,
                started_at: ts,
            })
            .collect();

        if let Ok(channel_lookup) = self.db.get_followed_channel_lookup() {
            if let Ok(inferred) = self.db.infer_schedules(&channel_lookup, start_dt, end_dt) {
                for s in inferred {
                    entries.push(DebugStreamEntry {
                        is_inferred: true,
                        broadcaster_name: s.broadcaster_name,
                        broadcaster_login: s.broadcaster_login,
                        started_at: s.start_time.timestamp(),
                    });
                }
            }
        }

        entries.sort_by_key(|e| e.started_at);
        entries
    }
}

#[async_trait::async_trait]
impl AppServices for Backend {
    fn get_config(&self) -> crate::config::Config {
        self.config.get()
    }

    async fn save_config(&self, config: crate::config::Config) -> anyhow::Result<()> {
        self.config.save(config)?;
        AppServices::refresh_category_streams(self).await;
        AppServices::refresh_schedules_from_db(self).await;
        Ok(())
    }

    async fn search_categories(
        &self,
        query: &str,
    ) -> Result<Vec<crate::twitch::Category>, crate::twitch::ApiError> {
        self.client.search_categories(query).await
    }

    fn get_followed_categories(&self) -> Vec<crate::config::FollowedCategory> {
        self.config.get().followed_categories
    }

    async fn get_followed_channels(&self) -> Vec<crate::twitch::FollowedChannel> {
        self.state.get_followed_channels().await
    }

    async fn refresh_category_streams(&self) {
        Backend::refresh_category_streams(self).await;
    }

    async fn refresh_schedules_from_db(&self) {
        Backend::refresh_schedules_from_db(self).await;
    }

    async fn get_debug_schedule_data(
        &self,
        start: i64,
        end: i64,
    ) -> Vec<crate::app_services::DebugStreamEntry> {
        Backend::get_debug_schedule_data(self, start, end).await
    }

    async fn get_debug_hotness_data(&self) -> Vec<crate::app_services::DebugHotnessEntry> {
        Backend::get_debug_hotness_data(self).await
    }

    async fn get_debug_hotness_profiles(
        &self,
    ) -> Vec<crate::app_services::DebugHotnessProfileEntry> {
        Backend::get_debug_hotness_profiles(self).await
    }
}

impl Clone for Backend {
    fn clone(&self) -> Self {
        Self {
            state: self.state.clone(),
            config: self.config.clone(),
            client: self.client.clone(),
            notifier: self.notifier.clone(),
            db: self.db.clone(),
            session: self.session.clone(),
            walker: self.walker.clone(),
            dispatcher: self.dispatcher.clone(),
            auth_cancel_tx: self.auth_cancel_tx.clone(),
            auth_cancel_rx: self.auth_cancel_rx.clone(),
            login_progress_rx: self.login_progress_rx.clone(),
            snooze_tx: self.snooze_tx.clone(),
            snooze_rx: self.snooze_rx.clone(),
            settings_tx: self.settings_tx.clone(),
            settings_rx: self.settings_rx.clone(),
            profile_image_cache: self.profile_image_cache.clone(),
            box_art_cache: self.box_art_cache.clone(),
            hotness_cache: self.hotness_cache.clone(),
            recent_streams: self.recent_streams.clone(),
            last_raw_started_at: self.last_raw_started_at.clone(),
        }
    }
}

/// Creates and starts the backend, returning a handle for the app layer.
pub fn start() -> anyhow::Result<BackendHandle> {
    let backend = Arc::new(Backend::new()?);

    let (display_tx, display_rx) = watch::channel(RawDisplayData::default());
    let (event_tx, _) = broadcast::channel(64);
    let (auth_cmd_tx, auth_cmd_rx) = mpsc::unbounded_channel();

    let login_progress_rx = backend.login_progress_rx.clone();
    let tasks = backend.start_tasks(&display_tx, &event_tx, auth_cmd_rx);

    let services: Arc<dyn AppServices> = backend;

    Ok(BackendHandle {
        display_rx,
        event_tx,
        services,
        auth_cmd_tx,
        login_progress_rx,
        tasks,
    })
}

#[cfg(test)]
impl Backend {
    fn with_test_deps(
        config: crate::config::Config,
        db_path: &std::path::Path,
        token_path: &std::path::Path,
    ) -> Self {
        use std::sync::atomic::AtomicBool;
        use tokio::sync::RwLock;

        let config = Arc::new(ConfigManager::with_config(config));
        let state = AppState::new();
        let (snooze_tx, snooze_rx) = mpsc::unbounded_channel();
        let (settings_tx, settings_rx) = mpsc::unbounded_channel();
        let notifier: Arc<dyn Notifier> = Arc::new(crate::notify::mock::RecordingNotifier::new());
        let client = TwitchClient::new("test".into());
        let db = Database::new(db_path).expect("test db");
        let (auth_cancel_tx, auth_cancel_rx) = watch::channel(false);

        let store = crate::auth::TokenStore::with_path(token_path.to_path_buf());

        let (session, login_progress_rx) = SessionManager::new(
            store,
            client.clone(),
            state.clone(),
            db.clone(),
            Arc::new(AtomicBool::new(false)),
            Arc::new(RwLock::new(None)),
            Arc::new(Mutex::new(())),
        );

        let walker = Arc::new(ScheduleWalker::new(
            db.clone(),
            client.clone(),
            state.clone(),
            config.clone(),
            session.clone(),
        ));

        let dispatcher = Arc::new(NotificationDispatcher::new(
            notifier.clone(),
            config.clone(),
            session.initial_load_done.clone(),
        ));

        Self {
            state,
            config,
            client,
            notifier,
            db,
            session,
            walker,
            dispatcher,
            auth_cancel_tx,
            auth_cancel_rx,
            login_progress_rx,
            snooze_tx,
            snooze_rx: Arc::new(Mutex::new(Some(snooze_rx))),
            settings_tx,
            settings_rx: Arc::new(Mutex::new(Some(settings_rx))),
            profile_image_cache: Arc::new(std::sync::Mutex::new(HashMap::new())),
            box_art_cache: Arc::new(std::sync::Mutex::new(HashMap::new())),
            hotness_cache: Arc::new(std::sync::Mutex::new(HashMap::new())),
            recent_streams: Arc::new(std::sync::Mutex::new(HashMap::new())),
            last_raw_started_at: Arc::new(std::sync::Mutex::new(HashMap::new())),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::hotness_detection::BucketStats;

    fn make_cached(was_hot: bool) -> CachedHotnessProfile {
        CachedHotnessProfile {
            stream_started_at: 1_000_000,
            was_hot,
            last_hotness: if was_hot {
                Some(HotnessInfo {
                    broadcaster_id: "123".to_string(),
                    z_score: 2.5,
                    is_hot: true,
                    eligible: true,
                    mean_viewers: 100.0,
                    stddev: 20.0,
                    current_viewers: 200,
                    observation_count: 10,
                    distinct_streams: 7,
                })
            } else {
                None
            },
            last_age_window: None,
            last_observation_count: 0,
            last_distinct_streams: 0,
        }
    }

    fn make_stats(count: usize, distinct_streams: usize) -> BucketStats {
        // Stats that will produce a high z-score for 500 viewers against mean of 100
        BucketStats {
            mean: 100.0,
            stddev: 20.0,
            count,
            distinct_streams,
            transformed_mean: 10.0, // sqrt(100 + 0.375) ≈ 10.02
            transformed_stddev: 1.0,
        }
    }

    fn default_hotness_cfg() -> HotnessConfig {
        HotnessConfig {
            z_threshold: 2.0,
            z_cool_threshold: 1.0,
            min_observations: 5,
            min_streams: 7,
        }
    }

    fn make_params<'a>(
        stats: &'a BucketStats,
        stream_age: i64,
        max_stream_age_min: u64,
        viewer_count: u32,
        hotness_cfg: &'a HotnessConfig,
        notify_on_hot: bool,
    ) -> EvalHotnessParams<'a> {
        EvalHotnessParams {
            stats,
            stream_age,
            max_stream_age_min,
            viewer_count,
            broadcaster_id: "123",
            hotness_cfg,
            notify_on_hot,
        }
    }

    // === Debug cache always updated ===

    #[test]
    fn debug_stats_updated_when_inside_window() {
        let mut cached = make_cached(false);
        let stats = make_stats(10, 7);
        let cfg = default_hotness_cfg();
        let params = make_params(&stats, 60, 90, 500, &cfg, true);
        evaluate_stream_hotness(&mut cached, &params);
        assert_eq!(cached.last_observation_count, 10);
        assert_eq!(cached.last_distinct_streams, 7);
    }

    #[test]
    fn debug_stats_updated_when_outside_window() {
        let mut cached = make_cached(false);
        let stats = make_stats(10, 7);
        let cfg = default_hotness_cfg();
        let params = make_params(&stats, 91, 90, 500, &cfg, true);
        evaluate_stream_hotness(&mut cached, &params);
        assert_eq!(cached.last_observation_count, 10);
        assert_eq!(cached.last_distinct_streams, 7);
    }

    // === Window gate: no new hot detection after window ===

    #[test]
    fn cold_stream_stays_cold_after_window() {
        let mut cached = make_cached(false);
        let stats = make_stats(10, 7);
        let cfg = default_hotness_cfg();
        let params = make_params(&stats, 91, 90, 500, &cfg, true);
        let result = evaluate_stream_hotness(&mut cached, &params);
        assert!(result.is_none());
        assert!(!cached.was_hot);
    }

    // === Window gate: hot streams stay hot after window if still above cool threshold ===

    #[test]
    fn hot_stream_stays_hot_after_window_when_above_cool_threshold() {
        let mut cached = make_cached(true);
        let stats = make_stats(10, 7);
        let cfg = default_hotness_cfg();
        // viewers=500, transformed_mean=10, transformed_stddev=1 → z ≈ 12.4, well above cool=1.0
        let params = make_params(&stats, 91, 90, 500, &cfg, true);
        let result = evaluate_stream_hotness(&mut cached, &params);
        // No notification (not a new transition)
        assert!(result.is_none());
        // Stays hot — current z is still above cool threshold
        assert!(cached.was_hot);
        assert!(cached.last_hotness.as_ref().unwrap().is_hot);
    }

    #[test]
    fn hot_stream_cools_off_after_window_when_z_drops() {
        // Past the age cap, but the streamer is no longer drawing unusual numbers.
        // The 🔥 must turn off — sticky-hot was masking real cool-offs (the andersonjph bug).
        let mut cached = make_cached(true);
        let stats = make_stats(10, 7);
        let cfg = default_hotness_cfg();
        // viewers=100 → anscombe≈10.02, z≈0.02 → below cool threshold (1.0)
        let params = make_params(&stats, 91, 90, 100, &cfg, true);
        let result = evaluate_stream_hotness(&mut cached, &params);
        assert!(result.is_none());
        assert!(
            !cached.was_hot,
            "should cool off past window when z below cool threshold"
        );
        let info = cached.last_hotness.as_ref().unwrap();
        assert!(!info.is_hot);
        // And the real z must be reported, not 0.0
        assert!(info.z_score < cfg.z_cool_threshold);
        assert!(info.z_score.abs() > f64::EPSILON || info.z_score == 0.0);
    }

    // === Normal detection within window ===

    #[test]
    fn becomes_hot_within_window_returns_notification() {
        let mut cached = make_cached(false);
        let stats = make_stats(10, 7);
        let cfg = default_hotness_cfg();
        let params = make_params(&stats, 60, 90, 500, &cfg, true);
        let result = evaluate_stream_hotness(&mut cached, &params);
        assert!(result.is_some());
        let info = result.unwrap();
        assert!(info.is_hot);
        assert!(cached.was_hot);
    }

    #[test]
    fn becomes_hot_but_notify_disabled_returns_none() {
        let mut cached = make_cached(false);
        let stats = make_stats(10, 7);
        let cfg = default_hotness_cfg();
        let params = make_params(&stats, 60, 90, 500, &cfg, false);
        let result = evaluate_stream_hotness(&mut cached, &params);
        // No notification returned when notify_on_hot is false
        assert!(result.is_none());
        // But the stream IS hot
        assert!(cached.was_hot);
    }

    #[test]
    fn already_hot_within_window_no_notification() {
        let mut cached = make_cached(true);
        let stats = make_stats(10, 7);
        let cfg = default_hotness_cfg();
        let params = make_params(&stats, 60, 90, 500, &cfg, true);
        let result = evaluate_stream_hotness(&mut cached, &params);
        // No new transition → no notification
        assert!(result.is_none());
        assert!(cached.was_hot);
    }

    // === Zero max means infinite window ===

    #[test]
    fn zero_max_age_means_infinite_detection_window() {
        let mut cached = make_cached(false);
        let stats = make_stats(10, 7);
        let cfg = default_hotness_cfg();
        let params = make_params(&stats, 999, 0, 500, &cfg, true);
        let result = evaluate_stream_hotness(&mut cached, &params);
        // Should still evaluate (window is infinite)
        assert!(result.is_some());
        assert!(cached.was_hot);
    }

    // === Insufficient data ===

    #[test]
    fn insufficient_observations_returns_none() {
        let mut cached = make_cached(false);
        let stats = make_stats(3, 7); // below min_observations
        let cfg = default_hotness_cfg();
        let params = make_params(&stats, 60, 90, 500, &cfg, true);
        let result = evaluate_stream_hotness(&mut cached, &params);
        assert!(result.is_none());
        assert!(!cached.was_hot);
    }

    // === Record ineligible evaluations past age cap ===

    #[test]
    fn evaluate_stream_hotness_records_ineligible_past_age_cap() {
        let stats = BucketStats {
            mean: 2000.0,
            stddev: 500.0,
            count: 50,
            distinct_streams: 10,
            transformed_mean: 44.0,
            transformed_stddev: 5.0,
        };
        let cfg = HotnessConfig {
            z_threshold: 2.0,
            z_cool_threshold: 1.0,
            min_observations: 5,
            min_streams: 7,
        };
        let mut cached = make_cached(false);
        let params = EvalHotnessParams {
            stats: &stats,
            stream_age: 200, // past the cap
            max_stream_age_min: 90,
            viewer_count: 5000,
            broadcaster_id: "123",
            hotness_cfg: &cfg,
            notify_on_hot: false,
        };

        let notify = evaluate_stream_hotness(&mut cached, &params);

        assert!(notify.is_none(), "no notification past age cap");
        let info = cached
            .last_hotness
            .as_ref()
            .expect("evaluation should be recorded even past age cap");
        assert!(!info.eligible);
        assert!(!info.is_hot);
        assert_eq!(info.observation_count, 50);
    }

    // === tick_stream_poll / tick_followed_channels tests ===

    use crate::config::Config;
    use chrono::Duration;
    use tempfile::TempDir;

    fn make_test_backend(poll_interval_sec: u64) -> (Backend, TempDir) {
        let tmp = TempDir::new().expect("tempdir");
        let db_path = tmp.path().join("data.db");
        let token_path = tmp.path().join("token.json");

        let config = Config {
            poll_interval_sec,
            ..Config::default()
        };

        let backend = Backend::with_test_deps(config, &db_path, &token_path);
        (backend, tmp)
    }

    #[tokio::test]
    async fn tick_stream_poll_skips_when_unauthenticated() {
        let (backend, _tmp) = make_test_backend(60);
        let now = Utc::now();
        assert!(!backend.tick_stream_poll(now).await);
    }

    #[tokio::test]
    async fn tick_stream_poll_refreshes_on_first_call() {
        let (backend, _tmp) = make_test_backend(60);
        // Simulate authenticated state
        backend
            .state
            .set_authenticated(true, "user123".into(), "testuser".into())
            .await;

        let now = Utc::now();
        let result = backend.tick_stream_poll(now).await;
        // last_live_refresh is None on first call, so should_refresh = true
        assert!(result);
    }

    #[tokio::test]
    async fn tick_stream_poll_skips_within_interval() {
        let (backend, _tmp) = make_test_backend(60);
        backend
            .state
            .set_authenticated(true, "user123".into(), "testuser".into())
            .await;

        // Set last_live_refresh to 30 seconds ago (within 60s interval)
        let now = Utc::now();
        *backend.session.last_live_refresh.write().await = Some(now - Duration::seconds(30));

        assert!(!backend.tick_stream_poll(now).await);
    }

    #[tokio::test]
    async fn tick_stream_poll_refreshes_after_interval() {
        let (backend, _tmp) = make_test_backend(60);
        backend
            .state
            .set_authenticated(true, "user123".into(), "testuser".into())
            .await;

        // Set last_live_refresh to 61 seconds ago (past 60s interval)
        let now = Utc::now();
        *backend.session.last_live_refresh.write().await = Some(now - Duration::seconds(61));

        assert!(backend.tick_stream_poll(now).await);
    }

    #[tokio::test]
    async fn tick_followed_channels_skips_when_unauthenticated() {
        let (backend, _tmp) = make_test_backend(60);
        let now = Utc::now();
        assert!(!backend.tick_followed_channels(now, None, 900).await);
    }

    #[tokio::test]
    async fn tick_followed_channels_skips_within_interval() {
        let (backend, _tmp) = make_test_backend(60);
        backend
            .state
            .set_authenticated(true, "user123".into(), "testuser".into())
            .await;

        let now = Utc::now();
        let last_refresh = Some(now - Duration::seconds(100));
        // interval is 900s, last refresh was 100s ago — should skip
        assert!(!backend.tick_followed_channels(now, last_refresh, 900).await);
    }

    // === record_and_evaluate_hotness cache lifecycle tests ===

    use crate::state::StreamsUpdated;
    use crate::test_helpers::make_stream;

    #[test]
    fn hotness_cache_initialised_for_newly_live_stream() {
        let (backend, _tmp) = make_test_backend(60);
        let stream = make_stream("123", "TestStreamer");

        let event = StreamsUpdated {
            streams: vec![stream.clone()],
            newly_live: vec![stream],
            category_changes: vec![],
            title_changes: vec![],
        };

        backend.record_and_evaluate_hotness(&event);

        let cache = backend.hotness_cache.lock().unwrap();
        assert!(
            cache.contains_key("123"),
            "newly live stream should be in cache"
        );
        assert!(!cache["123"].was_hot, "new stream should not start hot");
    }

    #[test]
    fn hotness_cache_evicts_offline_streams() {
        let (backend, _tmp) = make_test_backend(60);
        let stream = make_stream("123", "TestStreamer");

        // Stream goes live
        let event1 = StreamsUpdated {
            streams: vec![stream.clone()],
            newly_live: vec![stream],
            category_changes: vec![],
            title_changes: vec![],
        };
        backend.record_and_evaluate_hotness(&event1);
        assert!(backend.hotness_cache.lock().unwrap().contains_key("123"));

        // Stream goes offline
        let event2 = StreamsUpdated {
            streams: vec![],
            newly_live: vec![],
            category_changes: vec![],
            title_changes: vec![],
        };
        backend.record_and_evaluate_hotness(&event2);
        assert!(
            !backend.hotness_cache.lock().unwrap().contains_key("123"),
            "offline stream should be evicted from cache"
        );
    }

    #[test]
    fn hotness_records_viewer_observations_to_db() {
        let (backend, _tmp) = make_test_backend(60);
        let stream = make_stream("123", "TestStreamer");

        let event = StreamsUpdated {
            streams: vec![stream.clone()],
            newly_live: vec![stream],
            category_changes: vec![],
            title_changes: vec![],
        };
        backend.record_and_evaluate_hotness(&event);

        // Verify observations were recorded in the DB
        let now = Utc::now().timestamp();
        let obs = backend.db.get_all_recent_observations(now - 3600).unwrap();
        assert!(!obs.is_empty(), "observations should be recorded in DB");
        assert_eq!(obs[0].broadcaster_id, 123);
        assert_eq!(obs[0].viewer_count, 1000); // default from make_stream
    }

    #[test]
    fn collect_hotness_debug_includes_eligible_flag() {
        let (backend, _tmp) = make_test_backend(60);
        let stream = make_stream("123", "TestStreamer");
        {
            let mut cache = backend.hotness_cache.lock().unwrap();
            cache.insert(
                "123".to_string(),
                CachedHotnessProfile {
                    stream_started_at: 0,
                    was_hot: false,
                    last_hotness: Some(HotnessInfo {
                        broadcaster_id: "123".to_string(),
                        z_score: 0.0,
                        is_hot: false,
                        eligible: false,
                        mean_viewers: 1234.0,
                        stddev: 0.0,
                        current_viewers: 5000,
                        observation_count: 3,
                        distinct_streams: 2,
                    }),
                    last_age_window: Some((10, 20)),
                    last_observation_count: 3,
                    last_distinct_streams: 2,
                },
            );
        }

        let debug = backend.collect_hotness_debug(&[stream]);
        let entry = debug.get("123").expect("entry present");
        assert!(!entry.eligible);
        assert_eq!(entry.observation_count, 3);
    }

    // === ensure_*_cached tests ===

    #[tokio::test]
    async fn ensure_profile_images_cached_noop_on_empty() {
        let (backend, _tmp) = make_test_backend(60);
        backend.ensure_profile_images_cached(&[]).await;
        assert!(backend.profile_image_cache.lock().unwrap().is_empty());
    }

    #[tokio::test]
    async fn ensure_box_art_cached_noop_on_empty() {
        let (backend, _tmp) = make_test_backend(60);
        backend.ensure_box_art_cached(&[]).await;
        assert!(backend.box_art_cache.lock().unwrap().is_empty());
    }

    #[tokio::test]
    async fn ensure_profile_images_cached_skips_fresh_entries() {
        let (backend, _tmp) = make_test_backend(60);
        // Pre-populate cache with a fresh entry
        {
            let mut cache = backend.profile_image_cache.lock().unwrap();
            cache.insert(
                "123".to_string(),
                ("https://img.test/1.jpg".to_string(), Instant::now()),
            );
        }

        // Request the same ID — should not attempt fetch (would fail without real token)
        backend
            .ensure_profile_images_cached(&["123".to_string()])
            .await;

        let cache = backend.profile_image_cache.lock().unwrap();
        assert_eq!(cache.get("123").unwrap().0, "https://img.test/1.jpg");
    }

    #[tokio::test]
    async fn ensure_box_art_cached_skips_fresh_entries() {
        let (backend, _tmp) = make_test_backend(60);
        {
            let mut cache = backend.box_art_cache.lock().unwrap();
            cache.insert(
                "game1".to_string(),
                ("https://img.test/art.jpg".to_string(), Instant::now()),
            );
        }

        backend.ensure_box_art_cached(&["game1".to_string()]).await;

        let cache = backend.box_art_cache.lock().unwrap();
        assert_eq!(cache.get("game1").unwrap().0, "https://img.test/art.jpg");
    }

    // === refresh_category_streams tests ===

    #[tokio::test]
    async fn refresh_category_streams_noop_when_no_categories() {
        let (backend, _tmp) = make_test_backend(60);
        // Default config has no followed_categories
        backend.refresh_category_streams().await;
        let streams = backend.state.get_category_streams().await;
        assert!(streams.is_empty());
    }

    // === normalize_stream_resets tests ===

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

    #[tokio::test]
    async fn started_at_preserved_when_stream_resets_within_same_poll() {
        let (backend, _tmp) = make_test_backend(60);

        let original_start = Utc::now() - Duration::hours(2);
        let mut stream = make_stream("123", "TestStreamer");
        stream.started_at = original_start;

        backend
            .state
            .set_followed_streams(vec![stream.clone()], HashSet::new())
            .await;

        let mut reset_stream = stream.clone();
        reset_stream.started_at = Utc::now();

        let mut streams = vec![reset_stream];
        let resumed = backend.normalize_stream_resets(&mut streams).await;

        assert_eq!(streams[0].started_at, original_start);
        assert!(
            resumed.is_empty(),
            "instant reset should not produce resumed IDs"
        );
    }

    #[tokio::test]
    async fn started_at_preserved_when_stream_returns_within_grace_period() {
        let (backend, _tmp) = make_test_backend(60);

        let original_start = Utc::now() - Duration::hours(2);
        let mut stream = make_stream("123", "TestStreamer");
        stream.started_at = original_start;

        backend
            .state
            .set_followed_streams(vec![stream.clone()], HashSet::new())
            .await;

        let mut empty: Vec<crate::twitch::Stream> = vec![];
        let _ = backend.normalize_stream_resets(&mut empty).await;
        backend
            .state
            .set_followed_streams(vec![], HashSet::new())
            .await;

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

    #[tokio::test]
    async fn new_started_at_used_when_stream_returns_after_grace_period() {
        let (backend, _tmp) = make_test_backend_with_grace(60, 0);

        let original_start = Utc::now() - Duration::hours(2);
        let mut stream = make_stream("123", "TestStreamer");
        stream.started_at = original_start;

        backend
            .state
            .set_followed_streams(vec![stream.clone()], HashSet::new())
            .await;

        let mut empty: Vec<crate::twitch::Stream> = vec![];
        let _ = backend.normalize_stream_resets(&mut empty).await;
        backend
            .state
            .set_followed_streams(vec![], HashSet::new())
            .await;

        let new_start = Utc::now();
        let mut reset_stream = stream.clone();
        reset_stream.started_at = new_start;
        let mut streams = vec![reset_stream];
        let resumed = backend.normalize_stream_resets(&mut streams).await;

        assert_eq!(
            streams[0].started_at, new_start,
            "should use new started_at after grace expires"
        );
        assert!(resumed.is_empty(), "should not be in resumed set");
    }

    #[tokio::test]
    async fn stale_entries_pruned_from_grace_period_map() {
        let (backend, _tmp) = make_test_backend_with_grace(60, 0);

        {
            let mut recent = backend.recent_streams.lock().unwrap();
            recent.insert(
                "old_user".to_string(),
                RecentStream {
                    original_started_at: Utc::now() - Duration::hours(1),
                    went_offline_at: Instant::now(),
                },
            );
        }

        let mut streams: Vec<crate::twitch::Stream> = vec![];
        backend.normalize_stream_resets(&mut streams).await;

        let recent = backend.recent_streams.lock().unwrap();
        assert!(recent.is_empty(), "stale entry should be pruned");
    }

    #[tokio::test]
    async fn started_at_preserved_across_multiple_resets() {
        let (backend, _tmp) = make_test_backend(60);

        let original_start = Utc::now() - Duration::hours(3);
        let mut stream = make_stream("123", "TestStreamer");
        stream.started_at = original_start;

        backend
            .state
            .set_followed_streams(vec![stream.clone()], HashSet::new())
            .await;

        let mut reset1 = stream.clone();
        reset1.started_at = Utc::now() - Duration::hours(1);
        let mut streams = vec![reset1];
        let _ = backend.normalize_stream_resets(&mut streams).await;
        assert_eq!(streams[0].started_at, original_start);
        backend
            .state
            .set_followed_streams(streams, HashSet::new())
            .await;

        let mut reset2 = stream.clone();
        reset2.started_at = Utc::now();
        let mut streams = vec![reset2];
        let _ = backend.normalize_stream_resets(&mut streams).await;
        assert_eq!(streams[0].started_at, original_start);
    }

    #[tokio::test]
    async fn instant_reset_not_re_flagged_on_subsequent_polls() {
        // Regression: persistent disagreement between API and the frozen
        // canonical value used to re-fire reset detection every poll.
        let (backend, _tmp) = make_test_backend(60);

        let original = Utc::now() - Duration::hours(2);
        let post_reset_raw = Utc::now() - Duration::minutes(30);

        let mut s = make_stream("123", "TestStreamer");
        s.started_at = original;
        backend
            .state
            .set_followed_streams(vec![s.clone()], HashSet::new())
            .await;

        // Poll 1: API reports a forward jump → real reset, preserved.
        let mut first = s.clone();
        first.started_at = post_reset_raw;
        let mut v = vec![first];
        backend.normalize_stream_resets(&mut v).await;
        assert_eq!(v[0].started_at, original);
        backend.state.set_followed_streams(v, HashSet::new()).await;

        // Poll 2: API reports the SAME post-reset raw value. last_raw now
        // matches → must NOT be treated as a new reset, but the canonical
        // value is still preserved.
        let mut second = s.clone();
        second.started_at = post_reset_raw;
        let mut v2 = vec![second];
        backend.normalize_stream_resets(&mut v2).await;
        assert_eq!(v2[0].started_at, original);

        let last_raw = backend.last_raw_started_at.lock().unwrap();
        assert_eq!(last_raw.get("123").copied(), Some(post_reset_raw));
    }

    #[tokio::test]
    async fn sub_threshold_jitter_does_not_trigger_reset() {
        let (backend, _tmp) = make_test_backend(60);

        let original = Utc::now() - Duration::hours(2);
        let mut s = make_stream("123", "TestStreamer");
        s.started_at = original;
        backend
            .state
            .set_followed_streams(vec![s.clone()], HashSet::new())
            .await;

        // Seed last_raw with the canonical value.
        let mut seed = vec![s.clone()];
        backend.normalize_stream_resets(&mut seed).await;
        backend
            .state
            .set_followed_streams(seed, HashSet::new())
            .await;

        // API reports a 30-second jitter — well under the 60s threshold.
        let mut jittered = s.clone();
        jittered.started_at = original + Duration::seconds(30);
        let mut v = vec![jittered];
        backend.normalize_stream_resets(&mut v).await;
        // Normalised back to canonical without being flagged as a reset.
        assert_eq!(v[0].started_at, original);
    }

    #[tokio::test]
    async fn no_live_notification_on_grace_period_return() {
        let (backend, _tmp) = make_test_backend(60);

        let original_start = Utc::now() - Duration::hours(2);
        let mut stream = make_stream("123", "TestStreamer");
        stream.started_at = original_start;

        backend
            .state
            .set_followed_streams(vec![stream.clone()], HashSet::new())
            .await;

        let mut empty: Vec<crate::twitch::Stream> = vec![];
        let _ = backend.normalize_stream_resets(&mut empty).await;
        backend
            .state
            .set_followed_streams(vec![], HashSet::new())
            .await;

        let mut rx = backend.state.subscribe_streams();

        let mut reset_stream = stream.clone();
        reset_stream.started_at = Utc::now();
        let mut streams = vec![reset_stream];
        let resumed = backend.normalize_stream_resets(&mut streams).await;

        backend.state.set_followed_streams(streams, resumed).await;
        let event = rx.recv().await.unwrap();

        assert!(
            event.newly_live.is_empty(),
            "grace-period return should not fire newly_live"
        );
    }

    #[test]
    fn viewer_count_recorded_at_original_stream_age_after_reset() {
        let (backend, _tmp) = make_test_backend(60);

        let original_start = Utc::now() - Duration::hours(2);
        let mut stream = make_stream("123", "TestStreamer");
        stream.started_at = original_start;

        let event1 = StreamsUpdated {
            streams: vec![stream.clone()],
            newly_live: vec![stream.clone()],
            category_changes: vec![],
            title_changes: vec![],
        };
        backend.record_and_evaluate_hotness(&event1);

        let event2 = StreamsUpdated {
            streams: vec![stream.clone()],
            newly_live: vec![],
            category_changes: vec![],
            title_changes: vec![],
        };
        backend.record_and_evaluate_hotness(&event2);

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

    #[test]
    fn no_duplicate_stream_history_on_reset() {
        let (backend, _tmp) = make_test_backend(60);

        let original_start = Utc::now() - Duration::hours(2);
        let mut stream = make_stream("123", "TestStreamer");
        stream.started_at = original_start;

        backend.db.record_streams(&[stream.clone()]).unwrap();
        backend.db.record_streams(&[stream.clone()]).unwrap();

        let from = Utc::now() - Duration::hours(3);
        let to = Utc::now();
        let history = backend.db.get_streams_in_range(&[123], from, to).unwrap();
        let entries = history.get(&123).unwrap();
        assert_eq!(entries.len(), 1, "should have exactly one history entry");
    }
}

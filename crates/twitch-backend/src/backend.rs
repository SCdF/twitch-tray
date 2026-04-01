use chrono::{DateTime, Utc};
use std::collections::HashMap;
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
/// - `last_hotness`, `was_hot` — updated when inside the detection window
/// - When outside the window: hot status is preserved (streams don't lose 🔥 mid-stream)
fn evaluate_stream_hotness(
    cached: &mut CachedHotnessProfile,
    params: &EvalHotnessParams<'_>,
) -> Option<HotnessInfo> {
    // Always update debug stats
    cached.last_observation_count = params.stats.count;
    cached.last_distinct_streams = params.stats.distinct_streams;

    // Past the detection window: preserve existing hot status, no new evaluation
    if !is_within_hotness_window(params.stream_age, params.max_stream_age_min) {
        if cached.was_hot {
            if let Some(ref mut info) = cached.last_hotness {
                info.is_hot = true;
            }
        }
        return None;
    }

    // Evaluate hotness
    let result = compute_hotness(
        params.broadcaster_id,
        params.viewer_count,
        params.stats,
        params.hotness_cfg,
        cached.was_hot,
    );

    cached.last_hotness.clone_from(&result);

    // Preserve was_hot on None — don't forget hot status in data-sparse regions
    if let Some(ref info) = result {
        let was_hot = cached.was_hot;
        cached.was_hot = info.is_hot;

        // Edge detection: notify only on not-hot → hot transition
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
}

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

        self.session.record_live_refresh().await;
        self.state.set_followed_streams(streams).await;
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

    // === Window gate: hot streams stay hot after window ===

    #[test]
    fn hot_stream_stays_hot_after_window() {
        let mut cached = make_cached(true);
        let stats = make_stats(10, 7);
        let cfg = default_hotness_cfg();
        let params = make_params(&stats, 91, 90, 500, &cfg, true);
        let result = evaluate_stream_hotness(&mut cached, &params);
        // No notification (not a new transition)
        assert!(result.is_none());
        // But stays hot
        assert!(cached.was_hot);
        assert!(cached.last_hotness.as_ref().unwrap().is_hot);
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
}

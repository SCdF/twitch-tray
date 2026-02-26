use chrono::{DateTime, Utc};
use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use tauri::AppHandle;
use tokio::sync::{mpsc, watch, Mutex, RwLock};
use tokio::time::Duration;

/// Within this many seconds, an inferred schedule is considered a duplicate of an API schedule
const SCHEDULE_DEDUP_WINDOW_SECS: i64 = 3600;

use crate::auth::{DeviceFlow, Token, TokenStore, CLIENT_ID};
use crate::config::ConfigManager;
use crate::db::Database;
use crate::notify::{DesktopNotifier, Notifier, SnoozeRequest, StreamerSettingsRequest};
use crate::state::AppState;
use crate::tray::TrayManager;
use crate::twitch::{ApiError, TwitchClient};

/// Main application orchestrator
pub struct App {
    pub state: Arc<AppState>,
    pub config: ConfigManager,
    pub store: TokenStore,
    pub client: TwitchClient,
    pub notifier: DesktopNotifier,
    pub tray_manager: TrayManager,
    pub db: Database,

    // Tracks if initial load is complete (don't notify until then)
    initial_load_done: Arc<AtomicBool>,

    // Cancellation for auth flow
    auth_cancel_tx: watch::Sender<bool>,
    auth_cancel_rx: watch::Receiver<bool>,

    // Last SUCCESSFUL refresh time for sleep-aware polling
    // Only updated when API calls succeed, used to determine notification suppression
    last_live_refresh: Arc<RwLock<Option<DateTime<Utc>>>>,

    // Serializes token refresh attempts so only one task refreshes at a time.
    // Twitch refresh tokens are single-use: concurrent refreshes cause 400 errors.
    refresh_mutex: Arc<Mutex<()>>,

    // Snooze notification channel
    snooze_tx: mpsc::UnboundedSender<SnoozeRequest>,
    snooze_rx: Arc<Mutex<Option<mpsc::UnboundedReceiver<SnoozeRequest>>>>,

    // Streamer settings notification channel
    settings_tx: mpsc::UnboundedSender<StreamerSettingsRequest>,
    settings_rx: Arc<Mutex<Option<mpsc::UnboundedReceiver<StreamerSettingsRequest>>>>,
}

impl App {
    /// Creates a new application instance
    pub fn new() -> anyhow::Result<Self> {
        let config = ConfigManager::new()?;
        let store = TokenStore::new()?;
        let state = AppState::new();
        let cfg = config.get();
        let (snooze_tx, snooze_rx) = mpsc::unbounded_channel();
        let (settings_tx, settings_rx) = mpsc::unbounded_channel();
        let notifier = DesktopNotifier::new(
            cfg.notify_on_live,
            cfg.notify_on_category,
            snooze_tx.clone(),
            settings_tx.clone(),
        );
        let client = TwitchClient::new(CLIENT_ID.to_string());
        let tray_manager = TrayManager::new(state.clone());
        let db = Database::new(ConfigManager::config_dir()?.join("data.db"))?;
        let (auth_cancel_tx, auth_cancel_rx) = watch::channel(false);

        Ok(Self {
            state,
            config,
            store,
            client,
            notifier,
            tray_manager,
            db,
            initial_load_done: Arc::new(AtomicBool::new(false)),
            auth_cancel_tx,
            auth_cancel_rx,
            last_live_refresh: Arc::new(RwLock::new(None)),
            refresh_mutex: Arc::new(Mutex::new(())),
            snooze_tx,
            snooze_rx: Arc::new(Mutex::new(Some(snooze_rx))),
            settings_tx,
            settings_rx: Arc::new(Mutex::new(Some(settings_rx))),
        })
    }

    /// Tries to restore a session from stored token
    pub async fn restore_session(&self) -> anyhow::Result<()> {
        let mut token = self.store.load_token()?;
        let flow = DeviceFlow::new(CLIENT_ID.to_string());

        // If token is expired locally, refresh immediately
        let needs_refresh = if token.is_expired() {
            tracing::info!("Token expired, attempting refresh...");
            true
        } else {
            // Token looks valid locally — validate with Twitch in case it was
            // revoked server-side (e.g., after sleep or token rotation)
            match flow.validate_token(&token.access_token).await {
                Ok(_) => false,
                Err(_) => {
                    tracing::info!("Token rejected by Twitch, attempting refresh...");
                    true
                }
            }
        };

        if needs_refresh {
            token = flow.refresh_token(&token.refresh_token).await?;
            self.store.save_token(&token)?;
            tracing::info!("Token refreshed successfully");
        }

        if !token.is_valid() {
            anyhow::bail!("Stored token is invalid");
        }

        self.initialize_session(&token).await?;
        Ok(())
    }

    /// Initializes a session with the given token
    pub async fn initialize_session(&self, token: &Token) -> anyhow::Result<()> {
        self.client
            .set_access_token(token.access_token.clone())
            .await;
        self.client.set_user_id(token.user_id.clone()).await;

        self.state
            .set_authenticated(true, token.user_id.clone(), token.user_login.clone())
            .await;

        // Load followed channels
        if let Err(e) = self.load_followed_channels().await {
            tracing::warn!("Failed to load followed channels: {}", e);
        }

        Ok(())
    }

    async fn load_followed_channels(&self) -> anyhow::Result<()> {
        let follows = self
            .with_retry(|| self.client.get_all_followed_channels())
            .await?;

        // Persist to DB and seed the schedule queue
        self.db.sync_followed(&follows)?;
        let ids = self.db.get_followed_ids()?;
        self.db.ensure_schedule_queue_entries(&ids)?;

        self.state.set_followed_channels(follows).await;
        Ok(())
    }

    /// Calls `f()`, and on `ApiError::Unauthorized` refreshes the token and retries once.
    async fn with_retry<F, Fut, T>(&self, f: F) -> Result<T, ApiError>
    where
        F: Fn() -> Fut,
        Fut: std::future::Future<Output = Result<T, ApiError>>,
    {
        match f().await {
            Ok(val) => Ok(val),
            Err(ApiError::Unauthorized) => {
                self.try_refresh_token().await.map_err(ApiError::Other)?;
                f().await
            }
            Err(e) => Err(e),
        }
    }

    /// Attempts to refresh the OAuth token
    ///
    /// Called when API calls return 401 Unauthorized, indicating the token
    /// has expired (e.g., after laptop sleep). Serialized via mutex because
    /// Twitch refresh tokens are single-use — concurrent refreshes would
    /// invalidate each other.
    async fn try_refresh_token(&self) -> anyhow::Result<()> {
        // Snapshot the token that's currently failing, before waiting on the mutex
        let failing_token = self.client.get_access_token().await;

        let _guard = self.refresh_mutex.lock().await;

        // If the client's token changed while we waited, another task already refreshed
        if self.client.get_access_token().await != failing_token {
            tracing::debug!("Token already refreshed by another task");
            return Ok(());
        }

        tracing::info!("Token expired during API call, attempting refresh...");

        let token = self.store.load_token()?;
        let flow = DeviceFlow::new(CLIENT_ID.to_string());
        let new_token = flow.refresh_token(&token.refresh_token).await?;

        // Save the refreshed token
        self.store.save_token(&new_token)?;

        // Update the client with new credentials
        self.client
            .set_access_token(new_token.access_token.clone())
            .await;

        tracing::info!("Token refreshed successfully");
        Ok(())
    }

    /// Starts the polling tasks
    pub fn start_polling(self: &Arc<Self>, app_handle: AppHandle) {
        let cfg = self.config.get();

        // Clone self for the async tasks
        let app = self.clone();

        let poll_interval_secs = cfg.poll_interval_sec;

        // Stream polling task - uses wall-clock time to handle sleep correctly
        tokio::spawn(async move {
            // Use a short tick interval to detect wake-from-sleep quickly
            let tick_duration = Duration::from_secs(1);

            loop {
                tokio::time::sleep(tick_duration).await;

                if !app.state.is_authenticated().await {
                    continue;
                }

                let now = Utc::now();
                let last_refresh = *app.last_live_refresh.read().await;

                let should_refresh = match last_refresh {
                    None => true, // Never refreshed, do it now
                    Some(last) => {
                        let elapsed = (now - last).num_seconds();
                        elapsed >= poll_interval_secs as i64
                    }
                };

                if should_refresh {
                    app.refresh_followed_streams().await;
                    app.refresh_category_streams().await;
                    app.refresh_schedules_from_db().await;
                }
            }
        });

        // Schedule queue walker — checks one broadcaster at a time
        let app = self.clone();
        let schedule_check_interval = cfg.schedule_check_interval_sec;
        let schedule_stale_hours = cfg.schedule_stale_hours;

        tokio::spawn(async move {
            let tick_duration = Duration::from_secs(schedule_check_interval);

            loop {
                tokio::time::sleep(tick_duration).await;

                if !app.state.is_authenticated().await {
                    continue;
                }

                let stale_threshold = (schedule_stale_hours * 3600) as i64;
                let broadcaster = match app.db.get_next_stale_broadcaster(stale_threshold) {
                    Ok(Some(b)) => b,
                    Ok(None) => continue, // All are fresh
                    Err(e) => {
                        tracing::error!("Failed to query schedule queue: {}", e);
                        continue;
                    }
                };

                let (bid, blogin, bname) = broadcaster;
                let bid_str = bid.to_string();
                tracing::debug!("Checking schedule for {} ({})", bname, bid);

                match app.with_retry(|| app.client.get_schedule(&bid_str)).await {
                    Ok(Some(data)) => {
                        let segments = convert_schedule_segments(&data);
                        if let Err(e) = app.db.replace_future_schedules(bid, &segments) {
                            tracing::error!("Failed to store schedules for {}: {}", blogin, e);
                        }
                        if let Err(e) = app.db.update_last_checked(bid) {
                            tracing::error!("Failed to update last_checked for {}: {}", blogin, e);
                        }
                        app.refresh_schedules_from_db().await;
                    }
                    Ok(None) => {
                        // No schedule (404) — clear future entries for this broadcaster
                        if let Err(e) = app.db.replace_future_schedules(bid, &[]) {
                            tracing::error!("Failed to clear schedules for {}: {}", blogin, e);
                        }
                        if let Err(e) = app.db.update_last_checked(bid) {
                            tracing::error!("Failed to update last_checked for {}: {}", blogin, e);
                        }
                        app.refresh_schedules_from_db().await;
                    }
                    Err(e) => {
                        // Don't update last_checked — will retry next cycle
                        tracing::warn!("Failed to fetch schedule for {}: {}", blogin, e);
                    }
                }
            }
        });

        // Followed channels refresh task
        let app = self.clone();
        let followed_refresh_secs = cfg.followed_refresh_min * 60;
        let last_followed_refresh: Arc<RwLock<Option<DateTime<Utc>>>> = Arc::new(RwLock::new(None));

        tokio::spawn(async move {
            let tick_duration = Duration::from_secs(1);

            loop {
                tokio::time::sleep(tick_duration).await;

                if !app.state.is_authenticated().await {
                    continue;
                }

                let now = Utc::now();
                let last_refresh = *last_followed_refresh.read().await;

                let should_refresh = match last_refresh {
                    None => true,
                    Some(last) => {
                        let elapsed = (now - last).num_seconds();
                        elapsed >= followed_refresh_secs as i64
                    }
                };

                if should_refresh {
                    if let Err(e) = app.load_followed_channels().await {
                        tracing::warn!("Failed to refresh followed channels: {}", e);
                    } else {
                        *last_followed_refresh.write().await = Some(Utc::now());
                    }
                }
            }
        });

        // Snooze notification task
        let app = self.clone();
        tokio::spawn(async move {
            // Take the receiver out of the Arc<Mutex<Option<...>>>
            let mut rx = match app.snooze_rx.lock().await.take() {
                Some(rx) => rx,
                None => {
                    tracing::warn!("Snooze receiver already taken");
                    return;
                }
            };

            let mut snoozed: HashMap<String, (SnoozeRequest, crate::twitch::Stream)> =
                HashMap::new();

            loop {
                tokio::time::sleep(Duration::from_secs(1)).await;

                // Drain new snooze requests
                while let Ok(request) = rx.try_recv() {
                    tracing::info!(
                        "Snooze registered for {} (remind at {})",
                        request.user_name,
                        request.remind_at
                    );
                    // Look up the current stream data to store with the snooze
                    let streams = app.state.get_followed_streams().await;
                    if let Some(stream) = streams.iter().find(|s| s.user_id == request.user_id) {
                        snoozed.insert(request.user_id.clone(), (request, stream.clone()));
                    }
                }

                if snoozed.is_empty() {
                    continue;
                }

                let now = Utc::now();
                let live_streams = app.state.get_followed_streams().await;

                let mut to_remove = Vec::new();
                for (user_id, (request, stream)) in &snoozed {
                    let still_live = live_streams.iter().any(|s| s.user_id == *user_id);
                    if !still_live {
                        tracing::debug!("Snooze cancelled for {} (stream offline)", user_id);
                        to_remove.push(user_id.clone());
                    } else if now >= request.remind_at {
                        // Update stream data with latest info
                        let current_stream = live_streams
                            .iter()
                            .find(|s| s.user_id == *user_id)
                            .unwrap_or(stream);
                        if let Err(e) = app.notifier.stream_reminder(current_stream) {
                            tracing::error!("Snooze reminder notification error: {}", e);
                        }
                        to_remove.push(user_id.clone());
                    }
                }

                for user_id in to_remove {
                    snoozed.remove(&user_id);
                }
            }
        });

        // Streamer settings task
        let app = self.clone();
        let settings_app_handle = app_handle.clone();
        tokio::spawn(async move {
            let mut rx = match app.settings_rx.lock().await.take() {
                Some(rx) => rx,
                None => {
                    tracing::warn!("Settings receiver already taken");
                    return;
                }
            };

            while let Some(request) = rx.recv().await {
                tracing::info!(
                    "Settings requested for {} ({})",
                    request.display_name,
                    request.user_login,
                );

                // Auto-add streamer to config if not already present
                let mut cfg = app.config.get();
                if !cfg.streamer_settings.contains_key(&request.user_login) {
                    cfg.streamer_settings.insert(
                        request.user_login.clone(),
                        crate::config::StreamerSettings {
                            display_name: request.display_name.clone(),
                            importance: crate::config::StreamerImportance::Normal,
                        },
                    );
                    if let Err(e) = app.config.save(cfg) {
                        tracing::error!("Failed to save config with new streamer: {}", e);
                    }
                }

                crate::tray::open_streamer_settings_window(
                    &settings_app_handle,
                    &request.user_login,
                    &request.display_name,
                );
            }
        });

        // State change listener task — rebuilds menu on any state change
        let app = self.clone();
        let handle = app_handle.clone();

        tokio::spawn(async move {
            let mut rx = app.state.subscribe();

            while rx.changed().await.is_ok() {
                if rx.borrow().is_none() {
                    continue;
                }

                // Debounce: wait for rapid-fire state changes to settle.
                // A single poll cycle triggers multiple state updates (streams,
                // categories, schedules) — coalesce them into one menu rebuild
                // to avoid visible flicker on Linux where set_menu() destroys
                // and recreates the GTK popup.
                tokio::time::sleep(Duration::from_millis(500)).await;

                // Mark the latest value as seen so changes during the debounce
                // window don't trigger another immediate rebuild.
                let _ = *rx.borrow_and_update();

                let cfg = app.config.get();
                let category_streams = app.state.get_category_streams().await;
                if let Err(e) = app
                    .tray_manager
                    .rebuild_menu_with_categories(
                        &handle,
                        cfg.followed_categories,
                        category_streams,
                        cfg.streamer_settings,
                        cfg.schedule_lookahead_hours,
                    )
                    .await
                {
                    tracing::error!("Failed to rebuild menu on state change: {}", e);
                }
            }
        });

        // Notification listener task — reacts to stream update events
        let app = self.clone();
        tokio::spawn(async move {
            let mut rx = app.state.subscribe_streams();
            let mut last_event_time: Option<DateTime<Utc>> = None;

            loop {
                match rx.recv().await {
                    Ok(event) => {
                        let now = Utc::now();
                        let cfg = app.config.get();
                        let decision = crate::notification_filter::filter_notifications(
                            &event,
                            last_event_time,
                            now,
                            cfg.notify_max_gap_min * 60,
                            app.initial_load_done.load(Ordering::SeqCst),
                            &cfg.streamer_settings,
                        );
                        last_event_time = Some(now);

                        for stream in decision.streams_to_notify {
                            if let Err(e) = app.notifier.stream_live(&stream) {
                                tracing::error!("Notification error: {}", e);
                            }
                        }
                        for change in decision.categories_to_notify {
                            if let Err(e) = app
                                .notifier
                                .category_changed(&change.stream, &change.old_category)
                            {
                                tracing::error!("Notification error: {}", e);
                            }
                        }
                    }
                    Err(tokio::sync::broadcast::error::RecvError::Lagged(n)) => {
                        tracing::warn!("Notification listener lagged by {} events", n);
                    }
                    Err(tokio::sync::broadcast::error::RecvError::Closed) => {
                        break;
                    }
                }
            }
        });

        // History recording listener task — records stream data on every update
        let app = self.clone();
        tokio::spawn(async move {
            let mut rx = app.state.subscribe_streams();

            loop {
                match rx.recv().await {
                    Ok(event) => {
                        if let Err(e) = app.db.record_streams(&event.streams) {
                            tracing::error!("Failed to record stream history: {}", e);
                        }
                    }
                    Err(tokio::sync::broadcast::error::RecvError::Lagged(n)) => {
                        tracing::warn!("History listener lagged by {} events", n);
                    }
                    Err(tokio::sync::broadcast::error::RecvError::Closed) => {
                        break;
                    }
                }
            }
        });
    }

    /// Performs initial data refresh
    pub async fn refresh_all_data(&self) {
        self.refresh_followed_streams().await;
        self.refresh_schedules_from_db().await;
        self.refresh_category_streams().await;
        self.initial_load_done.store(true, Ordering::SeqCst);

        // Set initial refresh time
        *self.last_live_refresh.write().await = Some(Utc::now());
    }

    async fn refresh_followed_streams(&self) {
        if self.client.get_user_id().await.is_none() {
            return;
        }

        let streams = match self.with_retry(|| self.client.get_followed_streams()).await {
            Ok(streams) => streams,
            Err(e) => {
                tracing::error!("Failed to get followed streams: {}", e);
                return;
            }
        };

        // Update last successful refresh time
        *self.last_live_refresh.write().await = Some(Utc::now());

        // Write to state — this broadcasts the StreamsUpdated event
        self.state.set_followed_streams(streams).await;
    }

    /// Reads upcoming schedules from DB, merges with inferred schedules, and updates state.
    ///
    /// Both API and inferred schedules use the same display window:
    /// `[now - schedule_before_now_min, now + schedule_lookahead_hours]`.
    /// Deduplication removes inferred entries that overlap with an API schedule
    /// for the same broadcaster within 60 minutes.
    pub async fn refresh_schedules_from_db(&self) {
        let cfg = self.config.get();
        let now = Utc::now();
        let start = now - chrono::Duration::minutes(cfg.schedule_before_now_min as i64);
        let end = now + chrono::Duration::hours(cfg.schedule_lookahead_hours as i64);

        let db_schedules = match self.db.get_upcoming_schedules(start, end) {
            Ok(s) => s,
            Err(e) => {
                tracing::error!("Failed to read schedules from DB: {}", e);
                return;
            }
        };

        // Infer schedules from stream history using the same window
        let channels = self.state.get_followed_channels().await;
        let channel_lookup: HashMap<String, _> = channels
            .into_iter()
            .map(|c| (c.broadcaster_id.clone(), c))
            .collect();

        let mut combined = db_schedules;
        match self.db.infer_schedules(&channel_lookup, start, end) {
            Ok(inferred) => {
                if !inferred.is_empty() {
                    // Deduplicate: skip inferred schedules that overlap with an
                    // API schedule for the same broadcaster within 60 minutes
                    let deduped: Vec<_> = inferred
                        .into_iter()
                        .filter(|inf| {
                            !combined.iter().any(|api| {
                                api.broadcaster_id == inf.broadcaster_id
                                    && (api.start_time - inf.start_time).num_seconds().abs()
                                        <= SCHEDULE_DEDUP_WINDOW_SECS
                            })
                        })
                        .collect();
                    if !deduped.is_empty() {
                        tracing::debug!("Inferred {} schedule(s) from history", deduped.len());
                        combined.extend(deduped);
                        combined.sort_by_key(|s| s.start_time);
                    }
                }
            }
            Err(e) => {
                tracing::error!("Failed to infer schedules: {}", e);
            }
        }

        self.state.set_scheduled_streams(combined).await;
    }

    /// Refreshes streams for all followed categories
    pub async fn refresh_category_streams(&self) {
        let categories = self.config.get().followed_categories;
        if categories.is_empty() {
            return;
        }

        for category in &categories {
            let cat_id = category.id.clone();
            let streams = match self
                .with_retry(|| self.client.get_streams_by_category(&cat_id))
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

            self.state
                .set_category_streams(category.id.clone(), streams)
                .await;
        }
    }

    /// Handles login request
    pub async fn handle_login(&self) {
        let flow = DeviceFlow::new(CLIENT_ID.to_string());

        // Reset cancellation
        let _ = self.auth_cancel_tx.send(false);

        let store = TokenStore::new().expect("Failed to create token store");
        let client = self.client.clone();
        let state = self.state.clone();
        let db = self.db.clone();
        let cfg = self.config.get();
        let notifier_enabled = cfg.notify_on_live;
        let schedule_before_now_min = cfg.schedule_before_now_min;
        let schedule_lookahead_hours = cfg.schedule_lookahead_hours;
        let cancel_rx = self.auth_cancel_rx.clone();
        let initial_load_done = self.initial_load_done.clone();
        let last_live_refresh = self.last_live_refresh.clone();

        tokio::spawn(async move {
            match flow
                .authenticate(
                    |_user_code, verification_uri| {
                        // Open browser to verification URL
                        if let Err(e) = open::that(verification_uri) {
                            tracing::error!("Failed to open browser: {}", e);
                        }
                    },
                    cancel_rx,
                )
                .await
            {
                Ok(token) => {
                    // Save token
                    if let Err(e) = store.save_token(&token) {
                        tracing::error!("Failed to save token: {}", e);
                    }

                    // Initialize session
                    client.set_access_token(token.access_token.clone()).await;
                    client.set_user_id(token.user_id.clone()).await;

                    state
                        .set_authenticated(true, token.user_id.clone(), token.user_login.clone())
                        .await;

                    // Load followed channels and persist to DB
                    if let Ok(follows) = client.get_all_followed_channels().await {
                        if let Err(e) = db.sync_followed(&follows) {
                            tracing::error!("Failed to sync followed to DB: {}", e);
                        }
                        if let Ok(ids) = db.get_followed_ids() {
                            if let Err(e) = db.ensure_schedule_queue_entries(&ids) {
                                tracing::error!("Failed to seed schedule queue: {}", e);
                            }
                        }
                        state.set_followed_channels(follows).await;
                    }

                    // Initial data fetch
                    if let Ok(streams) = client.get_followed_streams().await {
                        state.set_followed_streams(streams).await;
                    }

                    // Show cached schedules from DB immediately
                    let sched_now = Utc::now();
                    let sched_start =
                        sched_now - chrono::Duration::minutes(schedule_before_now_min as i64);
                    let sched_end =
                        sched_now + chrono::Duration::hours(schedule_lookahead_hours as i64);
                    if let Ok(db_schedules) = db.get_upcoming_schedules(sched_start, sched_end) {
                        state.set_scheduled_streams(db_schedules).await;
                    }

                    // Mark initial load done so notification listener starts firing
                    initial_load_done.store(true, Ordering::SeqCst);

                    // Set initial refresh time so polling knows when to next refresh
                    *last_live_refresh.write().await = Some(Utc::now());

                    tracing::info!("Logged in as {}", token.user_login);
                }
                Err(e) => {
                    tracing::error!("Authentication failed: {}", e);
                    let (err_tx, _) = mpsc::unbounded_channel();
                    let (err_settings_tx, _) = mpsc::unbounded_channel();
                    let notifier =
                        DesktopNotifier::new(notifier_enabled, false, err_tx, err_settings_tx);
                    let _ = notifier.error(&format!("Authentication failed: {}", e));
                }
            }
        });
    }

    /// Handles logout request
    pub async fn handle_logout(&self) {
        // Clear stored token
        if let Err(e) = self.store.delete_token() {
            tracing::error!("Failed to delete token: {}", e);
        }

        // Clear state — triggers menu rebuild via state change listener
        self.state.clear().await;
        self.client.clear_auth().await;
        self.initial_load_done.store(false, Ordering::SeqCst);
    }
}

/// Converts raw API schedule segments into `ScheduledStream` structs.
/// Skips canceled segments. Does NOT filter by time horizon (stores all future segments).
fn convert_schedule_segments(
    data: &crate::twitch::ScheduleData,
) -> Vec<crate::twitch::ScheduledStream> {
    let Some(segments) = &data.segments else {
        return Vec::new();
    };

    segments
        .iter()
        .filter(|seg| seg.canceled_until.is_none())
        .map(|seg| crate::twitch::ScheduledStream {
            id: seg.id.clone(),
            broadcaster_id: data.broadcaster_id.clone(),
            broadcaster_name: data.broadcaster_name.clone(),
            broadcaster_login: data.broadcaster_login.clone(),
            title: seg.title.clone(),
            start_time: seg.start_time,
            end_time: seg.end_time,
            category: seg.category.as_ref().map(|c| c.name.clone()),
            category_id: seg.category.as_ref().map(|c| c.id.clone()),
            is_recurring: seg.is_recurring,
            is_inferred: false,
        })
        .collect()
}

impl Clone for App {
    fn clone(&self) -> Self {
        let cfg = self.config.get();
        Self {
            state: self.state.clone(),
            config: ConfigManager::new().expect("Failed to create config manager"),
            store: TokenStore::new().expect("Failed to create token store"),
            client: self.client.clone(),
            notifier: DesktopNotifier::new(
                cfg.notify_on_live,
                cfg.notify_on_category,
                self.snooze_tx.clone(),
                self.settings_tx.clone(),
            ),
            tray_manager: self.tray_manager.clone(),
            db: self.db.clone(),
            initial_load_done: self.initial_load_done.clone(),
            auth_cancel_tx: self.auth_cancel_tx.clone(),
            auth_cancel_rx: self.auth_cancel_rx.clone(),
            last_live_refresh: self.last_live_refresh.clone(),
            refresh_mutex: self.refresh_mutex.clone(),
            snooze_tx: self.snooze_tx.clone(),
            snooze_rx: self.snooze_rx.clone(),
            settings_tx: self.settings_tx.clone(),
            settings_rx: self.settings_rx.clone(),
        }
    }
}

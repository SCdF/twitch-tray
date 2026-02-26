use chrono::{DateTime, Utc};
use std::collections::HashMap;
use std::sync::atomic::Ordering;
use std::sync::Arc;
use tauri::AppHandle;
use tokio::sync::{mpsc, watch, Mutex};
use tokio::time::Duration;

use crate::app_services::AppServices;
use crate::auth::{TokenStore, CLIENT_ID};
use crate::config::ConfigManager;
use crate::db::Database;
use crate::display::DisplayBackend;
use crate::display_state::{
    compute_display_state, DisplayConfig, DEFAULT_LIVE_MENU_LIMIT, DEFAULT_SCHEDULE_MENU_LIMIT,
};
use crate::notify::{DesktopNotifier, Notifier, SnoozeRequest, StreamerSettingsRequest};
use crate::schedule_walker::ScheduleWalker;
use crate::session::SessionManager;
use crate::state::AppState;
use crate::twitch::TwitchClient;

/// Main application orchestrator
pub struct App {
    pub state: Arc<AppState>,
    pub config: ConfigManager,
    pub client: TwitchClient,
    pub notifier: DesktopNotifier,
    pub db: Database,

    // Auth lifecycle (session restore, login, logout, token refresh)
    session: SessionManager,

    // Schedule queue walker
    walker: Arc<ScheduleWalker>,

    // Cancellation for auth flow
    auth_cancel_tx: watch::Sender<bool>,
    auth_cancel_rx: watch::Receiver<bool>,

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
        use std::sync::atomic::AtomicBool;
        use tokio::sync::RwLock;

        let config = ConfigManager::new()?;
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
        let db = Database::new(ConfigManager::config_dir()?.join("data.db"))?;
        let (auth_cancel_tx, auth_cancel_rx) = watch::channel(false);

        let session = SessionManager::new(
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
            ConfigManager::new()?,
            session.clone(),
        ));

        Ok(Self {
            state,
            config,
            client,
            notifier,
            db,
            session,
            walker,
            auth_cancel_tx,
            auth_cancel_rx,
            snooze_tx,
            snooze_rx: Arc::new(Mutex::new(Some(snooze_rx))),
            settings_tx,
            settings_rx: Arc::new(Mutex::new(Some(settings_rx))),
        })
    }

    /// Tries to restore a session from stored token.
    pub async fn restore_session(&self) -> anyhow::Result<()> {
        self.session.restore_session().await
    }

    /// Calls `f()`, and on `ApiError::Unauthorized` refreshes the token and retries once.
    async fn with_retry<F, Fut, T>(&self, f: F) -> Result<T, crate::twitch::ApiError>
    where
        F: Fn() -> Fut,
        Fut: std::future::Future<Output = Result<T, crate::twitch::ApiError>>,
    {
        crate::twitch::with_retry(f, || self.session.try_refresh_token()).await
    }

    /// Starts the polling tasks
    pub fn start_polling(
        self: &Arc<Self>,
        app_handle: AppHandle,
        display: Arc<dyn DisplayBackend>,
    ) {
        // Stream polling task - uses wall-clock time to handle sleep correctly
        let app = self.clone();
        tokio::spawn(async move {
            // Use a short tick interval to detect wake-from-sleep quickly
            let tick_duration = Duration::from_secs(1);
            loop {
                tokio::time::sleep(tick_duration).await;
                app.tick_stream_poll(Utc::now()).await;
            }
        });

        // Schedule queue walker — checks one broadcaster at a time
        self.walker.clone().start();

        // Followed channels refresh task
        let app = self.clone();
        tokio::spawn(async move {
            let tick_duration = Duration::from_secs(1);
            let mut last_refresh: Option<DateTime<Utc>> = None;
            loop {
                tokio::time::sleep(tick_duration).await;
                let now = Utc::now();
                let interval_secs = app.config.get().followed_refresh_min * 60;
                if app
                    .tick_followed_channels(now, last_refresh, interval_secs)
                    .await
                {
                    last_refresh = Some(now);
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
        let disp = display.clone();

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

                let authenticated = app.state.is_authenticated().await;
                let display_state = if authenticated {
                    let streams = app.state.get_followed_streams().await;
                    let scheduled = app.state.get_scheduled_streams().await;
                    let schedules_loaded = app.state.schedules_loaded().await;
                    let cfg = app.config.get();
                    let category_streams = app.state.get_category_streams().await;
                    compute_display_state(
                        streams,
                        scheduled,
                        schedules_loaded,
                        cfg.followed_categories,
                        category_streams,
                        &DisplayConfig {
                            streamer_settings: cfg.streamer_settings,
                            schedule_lookahead_hours: cfg.schedule_lookahead_hours,
                            live_limit: DEFAULT_LIVE_MENU_LIMIT,
                            schedule_limit: DEFAULT_SCHEDULE_MENU_LIMIT,
                        },
                        Utc::now(),
                    )
                } else {
                    crate::display_state::DisplayState::unauthenticated()
                };

                if let Err(e) = disp.update(display_state) {
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
                            app.session.initial_load_done.load(Ordering::SeqCst),
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

    /// Checks if a streams refresh is due and runs it.
    /// Returns true if a refresh was triggered.
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

    /// Checks if a followed-channels refresh is due and runs it.
    /// Returns true if the refresh completed successfully.
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

    /// Performs initial data refresh
    pub async fn refresh_all_data(&self) {
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

        let streams = match self.with_retry(|| self.client.get_followed_streams()).await {
            Ok(streams) => streams,
            Err(e) => {
                tracing::error!("Failed to get followed streams: {}", e);
                return;
            }
        };

        // Update last successful refresh time
        self.session.record_live_refresh().await;

        // Write to state — this broadcasts the StreamsUpdated event
        self.state.set_followed_streams(streams).await;
    }

    /// Reads upcoming schedules from DB, merges with inferred schedules, and updates state.
    pub async fn refresh_schedules_from_db(&self) {
        self.walker.refresh_schedules_from_db().await;
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

    /// Handles a login request.
    ///
    /// Spawns the OAuth device flow in the background. On success the session
    /// is initialized and `refresh_all_data` is called. On failure an error
    /// notification is sent.
    pub async fn handle_login(&self) {
        let _ = self.auth_cancel_tx.send(false);
        let cancel_rx = self.auth_cancel_rx.clone();
        let session = self.session.clone();
        let app = self.clone();

        tokio::spawn(async move {
            match session.handle_login(cancel_rx).await {
                Ok(()) => {
                    app.refresh_all_data().await;
                }
                Err(e) => {
                    tracing::error!("Authentication failed: {}", e);
                    let _ = app.notifier.error(&format!("Authentication failed: {}", e));
                }
            }
        });
    }

    /// Handles a logout request.
    pub async fn handle_logout(&self) {
        self.session.handle_logout().await;
    }
}

#[async_trait::async_trait]
impl AppServices for App {
    fn get_config(&self) -> crate::config::Config {
        self.config.get()
    }

    async fn save_config(&self, config: crate::config::Config) -> anyhow::Result<()> {
        self.config.save(config)?;
        // Use trait dispatch so the trait methods are used and testable via mock
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
        App::refresh_category_streams(self).await
    }

    async fn refresh_schedules_from_db(&self) {
        App::refresh_schedules_from_db(self).await
    }
}

impl Clone for App {
    fn clone(&self) -> Self {
        let cfg = self.config.get();
        Self {
            state: self.state.clone(),
            config: ConfigManager::new().expect("Failed to create config manager"),
            client: self.client.clone(),
            notifier: DesktopNotifier::new(
                cfg.notify_on_live,
                cfg.notify_on_category,
                self.snooze_tx.clone(),
                self.settings_tx.clone(),
            ),
            db: self.db.clone(),
            session: self.session.clone(),
            walker: self.walker.clone(),
            auth_cancel_tx: self.auth_cancel_tx.clone(),
            auth_cancel_rx: self.auth_cancel_rx.clone(),
            snooze_tx: self.snooze_tx.clone(),
            snooze_rx: self.snooze_rx.clone(),
            settings_tx: self.settings_tx.clone(),
            settings_rx: self.settings_rx.clone(),
        }
    }
}

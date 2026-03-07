use chrono::{DateTime, Utc};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::{broadcast, mpsc, watch, Mutex};
use tokio::time::Duration;

use crate::app_services::AppServices;
use crate::auth::{TokenStore, CLIENT_ID};
use crate::config::ConfigManager;
use crate::db::Database;
use crate::events::BackendEvent;
use crate::handle::{AuthCommand, BackendHandle, LoginProgress, RawDisplayData};
use crate::notification_dispatcher::NotificationDispatcher;
use crate::notify::{DesktopNotifier, Notifier, SnoozeRequest, StreamerSettingsRequest};
use crate::schedule_walker::ScheduleWalker;
use crate::session::SessionManager;
use crate::state::AppState;
use crate::twitch::TwitchClient;
use tokio::task::JoinHandle;

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
        let db = Database::new(ConfigManager::config_dir()?.join("data.db"))?;
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
        display_tx: watch::Sender<RawDisplayData>,
        event_tx: broadcast::Sender<BackendEvent>,
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
            let mut rx = match backend.snooze_rx.lock().await.take() {
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
            let mut rx = match backend.settings_rx.lock().await.take() {
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
                let mut cfg = backend.config.get();
                if !cfg.streamer_settings.contains_key(&request.user_login) {
                    cfg.streamer_settings.insert(
                        request.user_login.clone(),
                        crate::config::StreamerSettings {
                            display_name: request.display_name.clone(),
                            importance: crate::config::StreamerImportance::Normal,
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

        // History recording listener task
        let backend = self.clone();
        handles.push(tokio::spawn(async move {
            let mut rx = backend.state.subscribe_streams();

            loop {
                match rx.recv().await {
                    Ok(event) => {
                        if let Err(e) = backend.db.record_streams(&event.streams) {
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
        }));

        handles
    }

    /// Collects current state and sends a RawDisplayData snapshot.
    async fn push_display_state(&self, display_tx: &watch::Sender<RawDisplayData>) {
        let cfg = self.config.get();
        let raw = RawDisplayData {
            is_authenticated: self.state.is_authenticated().await,
            live_streams: self.state.get_followed_streams().await,
            scheduled_streams: self.state.get_scheduled_streams().await,
            schedules_loaded: self.state.schedules_loaded().await,
            followed_channels: self.state.get_followed_channels().await,
            followed_categories: cfg.followed_categories.clone(),
            category_streams: self.state.get_category_streams().await,
            config: cfg,
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

        let streams = match self.with_retry(|| self.client.get_followed_streams()).await {
            Ok(streams) => streams,
            Err(e) => {
                tracing::error!("Failed to get followed streams: {}", e);
                return;
            }
        };

        self.session.record_live_refresh().await;
        self.state.set_followed_streams(streams).await;
    }

    pub(crate) async fn refresh_schedules_from_db(&self) {
        self.walker.refresh_schedules_from_db().await;
    }

    pub(crate) async fn refresh_category_streams(&self) {
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
                let _ = self
                    .notifier
                    .error(&format!("Authentication failed: {}", e));
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
        Backend::refresh_category_streams(self).await
    }

    async fn refresh_schedules_from_db(&self) {
        Backend::refresh_schedules_from_db(self).await
    }

    async fn get_debug_schedule_data(
        &self,
        start: i64,
        end: i64,
    ) -> Vec<crate::app_services::DebugStreamEntry> {
        Backend::get_debug_schedule_data(self, start, end).await
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
    let tasks = backend.start_tasks(display_tx, event_tx.clone(), auth_cmd_rx);

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

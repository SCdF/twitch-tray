use chrono::{DateTime, Utc};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use tauri::AppHandle;
use tokio::sync::{watch, RwLock};
use tokio::time::Duration;

use crate::auth::{DeviceFlow, Token, TokenStore, CLIENT_ID};
use crate::config::ConfigManager;
use crate::notify::{DesktopNotifier, Notifier};
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

    // Tracks if initial load is complete (don't notify until then)
    initial_load_done: AtomicBool,

    // Cancellation for auth flow
    auth_cancel_tx: watch::Sender<bool>,
    auth_cancel_rx: watch::Receiver<bool>,

    // Last refresh times for sleep-aware polling
    last_live_refresh: Arc<RwLock<Option<DateTime<Utc>>>>,
    last_schedule_refresh: Arc<RwLock<Option<DateTime<Utc>>>>,
}

impl App {
    /// Creates a new application instance
    pub fn new() -> anyhow::Result<Self> {
        let config = ConfigManager::new()?;
        let store = TokenStore::new()?;
        let state = AppState::new();
        let cfg = config.get();
        let notifier = DesktopNotifier::new(cfg.notify_on_live);
        let client = TwitchClient::new(CLIENT_ID.to_string());
        let tray_manager = TrayManager::new(state.clone());
        let (auth_cancel_tx, auth_cancel_rx) = watch::channel(false);

        Ok(Self {
            state,
            config,
            store,
            client,
            notifier,
            tray_manager,
            initial_load_done: AtomicBool::new(false),
            auth_cancel_tx,
            auth_cancel_rx,
            last_live_refresh: Arc::new(RwLock::new(None)),
            last_schedule_refresh: Arc::new(RwLock::new(None)),
        })
    }

    /// Tries to restore a session from stored token
    pub async fn restore_session(&self) -> anyhow::Result<()> {
        let mut token = self.store.load_token()?;

        // If token is expired, try to refresh it
        if token.is_expired() {
            tracing::info!("Token expired, attempting refresh...");
            let flow = DeviceFlow::new(CLIENT_ID.to_string());
            token = flow.refresh_token(&token.refresh_token).await?;

            // Save the refreshed token
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
        let follows = self.client.get_all_followed_channels().await?;
        let ids: Vec<String> = follows.into_iter().map(|f| f.broadcaster_id).collect();
        self.state.set_followed_channel_ids(ids).await;
        Ok(())
    }

    /// Attempts to refresh the OAuth token
    ///
    /// Called when API calls return 401 Unauthorized, indicating the token
    /// has expired (e.g., after laptop sleep).
    async fn try_refresh_token(&self) -> anyhow::Result<()> {
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
        let handle = app_handle.clone();

        let poll_interval_secs = cfg.poll_interval_sec;
        let notify_max_gap_secs = cfg.notify_max_gap_min * 60;

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
                    // Calculate if we should suppress notifications (gap too large)
                    let suppress_notifications = match last_refresh {
                        None => false, // First refresh, don't suppress
                        Some(last) => {
                            let elapsed = (now - last).num_seconds();
                            elapsed > notify_max_gap_secs as i64
                        }
                    };

                    if suppress_notifications {
                        tracing::info!(
                            "Suppressing notifications: refresh gap exceeded {} minutes",
                            cfg.notify_max_gap_min
                        );
                    }

                    app.refresh_followed_streams_with_options(suppress_notifications)
                        .await;

                    // Update last refresh time
                    *app.last_live_refresh.write().await = Some(Utc::now());

                    if let Err(e) = app.tray_manager.rebuild_menu(&handle).await {
                        tracing::error!("Failed to rebuild menu: {}", e);
                    }
                }
            }
        });

        // Schedule polling task - uses wall-clock time to handle sleep correctly
        let app = self.clone();
        let handle = app_handle.clone();
        let schedule_poll_secs = cfg.schedule_poll_min * 60;

        tokio::spawn(async move {
            // Use a short tick interval to detect wake-from-sleep quickly
            let tick_duration = Duration::from_secs(1);

            loop {
                tokio::time::sleep(tick_duration).await;

                if !app.state.is_authenticated().await {
                    continue;
                }

                let now = Utc::now();
                let last_refresh = *app.last_schedule_refresh.read().await;

                let should_refresh = match last_refresh {
                    None => true, // Never refreshed, do it now
                    Some(last) => {
                        let elapsed = (now - last).num_seconds();
                        elapsed >= schedule_poll_secs as i64
                    }
                };

                if should_refresh {
                    app.refresh_scheduled_streams().await;

                    // Update last refresh time
                    *app.last_schedule_refresh.write().await = Some(Utc::now());

                    if let Err(e) = app.tray_manager.rebuild_menu(&handle).await {
                        tracing::error!("Failed to rebuild menu: {}", e);
                    }
                }
            }
        });

        // State change listener task
        let app = self.clone();
        let handle = app_handle;

        tokio::spawn(async move {
            let mut rx = app.state.subscribe();

            while rx.changed().await.is_ok() {
                // Copy the value before awaiting to avoid holding the borrow across await
                let has_change = rx.borrow().is_some();
                if has_change {
                    if let Err(e) = app.tray_manager.rebuild_menu(&handle).await {
                        tracing::error!("Failed to rebuild menu on state change: {}", e);
                    }
                }
            }
        });
    }

    /// Performs initial data refresh
    pub async fn refresh_all_data(&self) {
        self.refresh_followed_streams().await;
        self.refresh_scheduled_streams().await;
        self.initial_load_done.store(true, Ordering::SeqCst);

        // Set initial refresh times
        let now = Utc::now();
        *self.last_live_refresh.write().await = Some(now);
        *self.last_schedule_refresh.write().await = Some(now);
    }

    async fn refresh_followed_streams(&self) {
        self.refresh_followed_streams_with_options(false).await;
    }

    async fn refresh_followed_streams_with_options(&self, suppress_notifications: bool) {
        if self.client.get_user_id().await.is_none() {
            return;
        }

        let streams = match self.client.get_followed_streams().await {
            Ok(streams) => streams,
            Err(ApiError::Unauthorized) => {
                // Token expired - try to refresh and retry
                if let Err(e) = self.try_refresh_token().await {
                    tracing::error!("Failed to refresh token: {}", e);
                    return;
                }
                // Retry the request with new token
                match self.client.get_followed_streams().await {
                    Ok(streams) => streams,
                    Err(e) => {
                        tracing::error!(
                            "Failed to get followed streams after token refresh: {}",
                            e
                        );
                        return;
                    }
                }
            }
            Err(e) => {
                tracing::error!("Failed to get followed streams: {}", e);
                return;
            }
        };

        let result = self.state.set_followed_streams(streams).await;

        // Notify for newly live streams (only after initial load, and if not suppressed)
        if self.initial_load_done.load(Ordering::SeqCst) && !suppress_notifications {
            for stream in &result.newly_live {
                if let Err(e) = self.notifier.stream_live(stream) {
                    tracing::error!("Notification error: {}", e);
                }
            }
        }
    }

    async fn refresh_scheduled_streams(&self) {
        if self.client.get_user_id().await.is_none() {
            return;
        }

        tracing::debug!("Fetching scheduled streams...");

        let scheduled = match self.client.get_scheduled_streams_for_followed().await {
            Ok(scheduled) => scheduled,
            Err(ApiError::Unauthorized) => {
                // Token expired - try to refresh and retry
                if let Err(e) = self.try_refresh_token().await {
                    tracing::error!("Failed to refresh token: {}", e);
                    return;
                }
                // Retry the request with new token
                match self.client.get_scheduled_streams_for_followed().await {
                    Ok(scheduled) => scheduled,
                    Err(e) => {
                        tracing::error!(
                            "Failed to get scheduled streams after token refresh: {}",
                            e
                        );
                        return;
                    }
                }
            }
            Err(e) => {
                tracing::error!("Failed to get scheduled streams: {}", e);
                return;
            }
        };

        self.state.set_scheduled_streams(scheduled).await;
    }

    /// Handles login request
    pub async fn handle_login(&self, app_handle: &AppHandle) {
        let flow = DeviceFlow::new(CLIENT_ID.to_string());

        // Reset cancellation
        let _ = self.auth_cancel_tx.send(false);

        let app_handle = app_handle.clone();
        let store = TokenStore::new().expect("Failed to create token store");
        let client = self.client.clone();
        let state = self.state.clone();
        let notifier_enabled = self.config.get().notify_on_live;
        let cancel_rx = self.auth_cancel_rx.clone();
        let tray_manager = TrayManager::new(state.clone());

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

                    // Load followed channels
                    if let Ok(follows) = client.get_all_followed_channels().await {
                        let ids: Vec<String> =
                            follows.into_iter().map(|f| f.broadcaster_id).collect();
                        state.set_followed_channel_ids(ids).await;
                    }

                    // Initial data fetch
                    if let Ok(streams) = client.get_followed_streams().await {
                        state.set_followed_streams(streams).await;
                    }

                    if let Ok(scheduled) = client.get_scheduled_streams_for_followed().await {
                        state.set_scheduled_streams(scheduled).await;
                    }

                    // Rebuild menu
                    if let Err(e) = tray_manager.rebuild_menu(&app_handle).await {
                        tracing::error!("Failed to rebuild menu: {}", e);
                    }

                    tracing::info!("Logged in as {}", token.user_login);
                }
                Err(e) => {
                    tracing::error!("Authentication failed: {}", e);
                    let notifier = DesktopNotifier::new(notifier_enabled);
                    let _ = notifier.error(&format!("Authentication failed: {}", e));
                }
            }
        });
    }

    /// Handles logout request
    pub async fn handle_logout(&self, app_handle: &AppHandle) {
        // Clear stored token
        if let Err(e) = self.store.delete_token() {
            tracing::error!("Failed to delete token: {}", e);
        }

        // Clear state
        self.state.clear().await;
        self.client.clear_auth().await;
        self.initial_load_done.store(false, Ordering::SeqCst);

        // Rebuild menu
        if let Err(e) = self.tray_manager.rebuild_menu(app_handle).await {
            tracing::error!("Failed to rebuild menu: {}", e);
        }
    }
}

impl Clone for App {
    fn clone(&self) -> Self {
        Self {
            state: self.state.clone(),
            config: ConfigManager::new().expect("Failed to create config manager"),
            store: TokenStore::new().expect("Failed to create token store"),
            client: self.client.clone(),
            notifier: DesktopNotifier::new(self.config.get().notify_on_live),
            tray_manager: TrayManager::new(self.state.clone()),
            initial_load_done: AtomicBool::new(self.initial_load_done.load(Ordering::SeqCst)),
            auth_cancel_tx: self.auth_cancel_tx.clone(),
            auth_cancel_rx: self.auth_cancel_rx.clone(),
            last_live_refresh: self.last_live_refresh.clone(),
            last_schedule_refresh: self.last_schedule_refresh.clone(),
        }
    }
}

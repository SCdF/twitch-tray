use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use tauri::AppHandle;
use tokio::sync::watch;
use tokio::time::{interval, Duration};

use crate::auth::{DeviceFlow, Token, TokenStore, CLIENT_ID};
use crate::config::ConfigManager;
use crate::notify::Notifier;
use crate::state::AppState;
use crate::tray::TrayManager;
use crate::twitch::TwitchClient;

/// Main application orchestrator
pub struct App {
    pub state: Arc<AppState>,
    pub config: ConfigManager,
    pub store: TokenStore,
    pub client: TwitchClient,
    pub notifier: Notifier,
    pub tray_manager: TrayManager,

    // Tracks if initial load is complete (don't notify until then)
    initial_load_done: AtomicBool,

    // Cancellation for auth flow
    auth_cancel_tx: watch::Sender<bool>,
    auth_cancel_rx: watch::Receiver<bool>,
}

impl App {
    /// Creates a new application instance
    pub fn new() -> anyhow::Result<Self> {
        let config = ConfigManager::new()?;
        let store = TokenStore::new()?;
        let state = AppState::new();
        let cfg = config.get();
        let notifier = Notifier::new(cfg.notify_on_live);
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
        })
    }

    /// Tries to restore a session from stored token
    pub async fn restore_session(&self) -> anyhow::Result<()> {
        let token = self.store.load_token()?;

        if !token.is_valid() {
            anyhow::bail!("Stored token is invalid or expired");
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

    /// Starts the polling tasks
    pub fn start_polling(self: &Arc<Self>, app_handle: AppHandle) {
        let cfg = self.config.get();

        // Clone self for the async tasks
        let app = self.clone();
        let handle = app_handle.clone();

        // Stream polling task
        tokio::spawn(async move {
            let mut ticker = interval(Duration::from_secs(cfg.poll_interval_sec));

            loop {
                ticker.tick().await;

                if !app.state.is_authenticated().await {
                    continue;
                }

                app.refresh_followed_streams().await;
                if let Err(e) = app.tray_manager.rebuild_menu(&handle).await {
                    tracing::error!("Failed to rebuild menu: {}", e);
                }
            }
        });

        // Schedule polling task
        let app = self.clone();
        let handle = app_handle.clone();

        tokio::spawn(async move {
            let mut ticker = interval(Duration::from_secs(cfg.schedule_poll_min * 60));

            loop {
                ticker.tick().await;

                if !app.state.is_authenticated().await {
                    continue;
                }

                app.refresh_scheduled_streams().await;
                if let Err(e) = app.tray_manager.rebuild_menu(&handle).await {
                    tracing::error!("Failed to rebuild menu: {}", e);
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
    }

    async fn refresh_followed_streams(&self) {
        if self.client.get_user_id().await.is_none() {
            return;
        }

        match self.client.get_followed_streams().await {
            Ok(streams) => {
                let result = self.state.set_followed_streams(streams).await;

                // Notify for newly live streams (only after initial load)
                if self.initial_load_done.load(Ordering::SeqCst) {
                    for stream in &result.newly_live {
                        if let Err(e) = self.notifier.stream_live(stream) {
                            tracing::error!("Notification error: {}", e);
                        }
                    }
                }
            }
            Err(e) => {
                tracing::error!("Failed to get followed streams: {}", e);
            }
        }
    }

    async fn refresh_scheduled_streams(&self) {
        if self.client.get_user_id().await.is_none() {
            return;
        }

        tracing::debug!("Fetching scheduled streams...");

        match self.client.get_scheduled_streams_for_followed().await {
            Ok(scheduled) => {
                self.state.set_scheduled_streams(scheduled).await;
            }
            Err(e) => {
                tracing::error!("Failed to get scheduled streams: {}", e);
            }
        }
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
                    let notifier = Notifier::new(notifier_enabled);
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
            notifier: Notifier::new(self.config.get().notify_on_live),
            tray_manager: TrayManager::new(self.state.clone()),
            initial_load_done: AtomicBool::new(self.initial_load_done.load(Ordering::SeqCst)),
            auth_cancel_tx: self.auth_cancel_tx.clone(),
            auth_cancel_rx: self.auth_cancel_rx.clone(),
        }
    }
}

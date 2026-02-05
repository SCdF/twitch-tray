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

    // Last SUCCESSFUL refresh times for sleep-aware polling
    // Only updated when API calls succeed, used to determine notification suppression
    last_live_refresh: Arc<RwLock<Option<DateTime<Utc>>>>,
    last_schedule_refresh: Arc<RwLock<Option<DateTime<Utc>>>>,

    // Maximum gap between successful refreshes before suppressing notifications (in seconds)
    notify_max_gap_secs: u64,
}

impl App {
    /// Creates a new application instance
    pub fn new() -> anyhow::Result<Self> {
        let config = ConfigManager::new()?;
        let store = TokenStore::new()?;
        let state = AppState::new();
        let cfg = config.get();
        let notifier = DesktopNotifier::new(cfg.notify_on_live, cfg.notify_on_category);
        let client = TwitchClient::new(CLIENT_ID.to_string());
        let tray_manager = TrayManager::new(state.clone());
        let (auth_cancel_tx, auth_cancel_rx) = watch::channel(false);
        let notify_max_gap_secs = cfg.notify_max_gap_min * 60;

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
            notify_max_gap_secs,
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
                    // Notification suppression is handled inside refresh_followed_streams
                    // based on last *successful* refresh time
                    app.refresh_followed_streams().await;

                    // Also refresh category streams on the same schedule
                    app.refresh_category_streams().await;

                    // Rebuild menu with category data
                    let cfg = app.config.get();
                    let category_streams = app.state.get_category_streams().await;
                    if let Err(e) = app
                        .tray_manager
                        .rebuild_menu_with_categories(
                            &handle,
                            cfg.followed_categories,
                            category_streams,
                        )
                        .await
                    {
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

                    // Rebuild menu with category data
                    let cfg = app.config.get();
                    let category_streams = app.state.get_category_streams().await;
                    if let Err(e) = app
                        .tray_manager
                        .rebuild_menu_with_categories(
                            &handle,
                            cfg.followed_categories,
                            category_streams,
                        )
                        .await
                    {
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
                    // Rebuild menu with category data
                    let cfg = app.config.get();
                    let category_streams = app.state.get_category_streams().await;
                    if let Err(e) = app
                        .tray_manager
                        .rebuild_menu_with_categories(
                            &handle,
                            cfg.followed_categories,
                            category_streams,
                        )
                        .await
                    {
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
        self.refresh_category_streams().await;
        self.initial_load_done.store(true, Ordering::SeqCst);

        // Set initial refresh times
        let now = Utc::now();
        *self.last_live_refresh.write().await = Some(now);
        *self.last_schedule_refresh.write().await = Some(now);
    }

    async fn refresh_followed_streams(&self) {
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

        // SUCCESS: We have stream data. Now determine if we should send notifications
        // based on the gap since the last *successful* refresh.
        let now = Utc::now();
        let last_successful = *self.last_live_refresh.read().await;

        let suppress_notifications = match last_successful {
            None => false, // First successful refresh, don't suppress
            Some(last) => {
                let elapsed = (now - last).num_seconds();
                let should_suppress = elapsed > self.notify_max_gap_secs as i64;
                if should_suppress {
                    tracing::info!(
                        "Suppressing notifications: gap of {}s exceeds max of {}s",
                        elapsed,
                        self.notify_max_gap_secs
                    );
                }
                should_suppress
            }
        };

        // Update last successful refresh time BEFORE processing notifications
        // This ensures the timestamp reflects when we got valid data
        *self.last_live_refresh.write().await = Some(now);

        let result = self.state.set_followed_streams(streams).await;

        // Notify for newly live streams (only after initial load, and if not suppressed)
        if self.initial_load_done.load(Ordering::SeqCst) && !suppress_notifications {
            for stream in &result.newly_live {
                if let Err(e) = self.notifier.stream_live(stream) {
                    tracing::error!("Notification error: {}", e);
                }
            }
            for change in &result.category_changes {
                if let Err(e) = self
                    .notifier
                    .category_changed(&change.stream, &change.old_category)
                {
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

    /// Refreshes streams for all followed categories
    pub async fn refresh_category_streams(&self) {
        let categories = self.config.get().followed_categories;
        if categories.is_empty() {
            return;
        }

        for category in &categories {
            let streams = match self.client.get_streams_by_category(&category.id).await {
                Ok(streams) => streams,
                Err(ApiError::Unauthorized) => {
                    // Token expired - try to refresh and retry
                    if let Err(e) = self.try_refresh_token().await {
                        tracing::error!("Failed to refresh token: {}", e);
                        continue;
                    }
                    // Retry the request with new token
                    match self.client.get_streams_by_category(&category.id).await {
                        Ok(streams) => streams,
                        Err(e) => {
                            tracing::error!(
                                "Failed to get category streams after token refresh: {}",
                                e
                            );
                            continue;
                        }
                    }
                }
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
        let tray_manager = self.tray_manager.clone();

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
                    let notifier = DesktopNotifier::new(notifier_enabled, false);
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
        let cfg = self.config.get();
        Self {
            state: self.state.clone(),
            config: ConfigManager::new().expect("Failed to create config manager"),
            store: TokenStore::new().expect("Failed to create token store"),
            client: self.client.clone(),
            notifier: DesktopNotifier::new(cfg.notify_on_live, cfg.notify_on_category),
            tray_manager: self.tray_manager.clone(),
            initial_load_done: AtomicBool::new(self.initial_load_done.load(Ordering::SeqCst)),
            auth_cancel_tx: self.auth_cancel_tx.clone(),
            auth_cancel_rx: self.auth_cancel_rx.clone(),
            last_live_refresh: self.last_live_refresh.clone(),
            last_schedule_refresh: self.last_schedule_refresh.clone(),
            notify_max_gap_secs: self.notify_max_gap_secs,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Helper function that replicates the notification suppression logic
    /// from `refresh_followed_streams`. This allows us to unit test the
    /// decision-making without needing the full App infrastructure.
    fn should_suppress_notifications(
        last_successful_refresh: Option<DateTime<Utc>>,
        now: DateTime<Utc>,
        notify_max_gap_secs: u64,
    ) -> bool {
        match last_successful_refresh {
            None => false, // First successful refresh, don't suppress
            Some(last) => {
                let elapsed = (now - last).num_seconds();
                elapsed > notify_max_gap_secs as i64
            }
        }
    }

    // === Notification suppression decision tests ===

    #[test]
    fn first_refresh_does_not_suppress_notifications() {
        let now = Utc::now();
        let max_gap_secs = 600; // 10 minutes

        // No previous successful refresh
        let suppress = should_suppress_notifications(None, now, max_gap_secs);

        assert!(
            !suppress,
            "First refresh should never suppress notifications"
        );
    }

    #[test]
    fn recent_refresh_does_not_suppress_notifications() {
        let now = Utc::now();
        let max_gap_secs = 600; // 10 minutes

        // Last successful refresh was 60 seconds ago (within the 10 minute window)
        let last_refresh = now - chrono::Duration::seconds(60);
        let suppress = should_suppress_notifications(Some(last_refresh), now, max_gap_secs);

        assert!(
            !suppress,
            "Refresh within max gap should not suppress notifications"
        );
    }

    #[test]
    fn old_refresh_suppresses_notifications() {
        let now = Utc::now();
        let max_gap_secs = 600; // 10 minutes

        // Last successful refresh was 15 minutes ago (exceeds 10 minute window)
        let last_refresh = now - chrono::Duration::seconds(900);
        let suppress = should_suppress_notifications(Some(last_refresh), now, max_gap_secs);

        assert!(
            suppress,
            "Refresh exceeding max gap should suppress notifications"
        );
    }

    #[test]
    fn exactly_at_boundary_does_not_suppress() {
        let now = Utc::now();
        let max_gap_secs = 600; // 10 minutes

        // Exactly at the boundary (not exceeding, so should not suppress)
        let last_refresh = now - chrono::Duration::seconds(600);
        let suppress = should_suppress_notifications(Some(last_refresh), now, max_gap_secs);

        assert!(
            !suppress,
            "Refresh exactly at max gap boundary should not suppress"
        );
    }

    #[test]
    fn one_second_over_boundary_suppresses() {
        let now = Utc::now();
        let max_gap_secs = 600; // 10 minutes

        // One second over the boundary
        let last_refresh = now - chrono::Duration::seconds(601);
        let suppress = should_suppress_notifications(Some(last_refresh), now, max_gap_secs);

        assert!(suppress, "Refresh one second over max gap should suppress");
    }

    #[test]
    fn very_long_gap_suppresses() {
        let now = Utc::now();
        let max_gap_secs = 600; // 10 minutes

        // Hours of sleep/suspension
        let last_refresh = now - chrono::Duration::hours(8);
        let suppress = should_suppress_notifications(Some(last_refresh), now, max_gap_secs);

        assert!(
            suppress,
            "Very long gap (hours) should suppress notifications"
        );
    }

    #[test]
    fn custom_max_gap_respected() {
        let now = Utc::now();
        let max_gap_secs = 120; // 2 minutes

        // 3 minutes ago - exceeds 2 minute custom threshold
        let last_refresh = now - chrono::Duration::seconds(180);
        let suppress = should_suppress_notifications(Some(last_refresh), now, max_gap_secs);

        assert!(suppress, "Custom max gap of 2 minutes should be respected");
    }

    // === Integration-style tests for timestamp management ===

    #[tokio::test]
    async fn timestamp_starts_as_none() {
        let timestamp: Arc<RwLock<Option<DateTime<Utc>>>> = Arc::new(RwLock::new(None));
        let value = *timestamp.read().await;
        assert!(value.is_none(), "Timestamp should start as None");
    }

    #[tokio::test]
    async fn timestamp_updated_after_success() {
        let timestamp: Arc<RwLock<Option<DateTime<Utc>>>> = Arc::new(RwLock::new(None));

        // Simulate successful refresh updating the timestamp
        let now = Utc::now();
        *timestamp.write().await = Some(now);

        let value = *timestamp.read().await;
        assert!(value.is_some(), "Timestamp should be Some after update");
        assert_eq!(value.unwrap(), now, "Timestamp should match the set value");
    }

    /// This test documents the fix for the wake-from-sleep bug:
    /// Failed refreshes should NOT update the timestamp, so that subsequent
    /// successful refreshes correctly calculate the gap from the last success.
    #[tokio::test]
    async fn failed_refresh_scenario() {
        let last_successful: Arc<RwLock<Option<DateTime<Utc>>>> = Arc::new(RwLock::new(None));
        let max_gap_secs = 600; // 10 minutes

        // T=0: First successful refresh
        let t0 = Utc::now();
        *last_successful.write().await = Some(t0);

        // T=8h: Computer wakes from sleep, API call FAILS
        // In the buggy code, timestamp would be updated here despite failure
        // In the fixed code, we DON'T update the timestamp on failure
        // (simulated by not updating last_successful)
        let _t1 = t0 + chrono::Duration::hours(8);
        // NO update to last_successful - simulates failed refresh

        // T=8h+1m: Retry succeeds
        let t2 = t0 + chrono::Duration::hours(8) + chrono::Duration::minutes(1);
        let last = *last_successful.read().await;

        // The gap should be calculated from t0 (last SUCCESS), not from t1
        let suppress = should_suppress_notifications(last, t2, max_gap_secs);

        assert!(
            suppress,
            "After 8+ hours, notifications should be suppressed even if a \
             failed refresh occurred in between"
        );
    }

    /// This test documents the correct behavior for normal polling
    #[tokio::test]
    async fn normal_polling_scenario() {
        let max_gap_secs = 600; // 10 minutes

        // Simulating normal 60-second polling intervals
        let t0 = Utc::now();
        let t1 = t0 + chrono::Duration::seconds(60);
        let t2 = t0 + chrono::Duration::seconds(120);

        // After first poll (60s gap), should not suppress
        assert!(
            !should_suppress_notifications(Some(t0), t1, max_gap_secs),
            "60 second gap should not suppress"
        );

        // After second poll (60s gap from t1), should not suppress
        assert!(
            !should_suppress_notifications(Some(t1), t2, max_gap_secs),
            "60 second gap should not suppress"
        );
    }
}

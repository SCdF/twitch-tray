//! Session management: auth lifecycle, token refresh, login/logout.
//!
//! `SessionManager` owns everything related to "who is logged in and whether
//! the token is still valid". It knows nothing about the display layer or
//! polling schedules.

use chrono::{DateTime, Utc};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use tokio::sync::{watch, Mutex, RwLock};

use crate::auth::{DeviceFlow, Token, TokenStore, CLIENT_ID};
use crate::db::Database;
use crate::state::AppState;
use crate::twitch::TwitchClient;

/// Manages the auth lifecycle: session restore, login, logout, and token refresh.
pub struct SessionManager {
    pub(crate) store: TokenStore,
    pub(crate) client: TwitchClient,
    pub(crate) state: Arc<AppState>,
    pub(crate) db: Database,
    /// Serializes token refresh so only one task refreshes at a time.
    /// Twitch refresh tokens are single-use: concurrent refreshes cause 400 errors.
    refresh_mutex: Arc<Mutex<()>>,
    /// True once the first data load after login completes (suppresses startup notifications).
    pub(crate) initial_load_done: Arc<AtomicBool>,
    /// Timestamp of the last successful live-stream API call (for sleep-aware polling).
    pub(crate) last_live_refresh: Arc<RwLock<Option<DateTime<Utc>>>>,
}

impl SessionManager {
    pub fn new(
        store: TokenStore,
        client: TwitchClient,
        state: Arc<AppState>,
        db: Database,
        initial_load_done: Arc<AtomicBool>,
        last_live_refresh: Arc<RwLock<Option<DateTime<Utc>>>>,
        refresh_mutex: Arc<Mutex<()>>,
    ) -> Self {
        Self {
            store,
            client,
            state,
            db,
            initial_load_done,
            last_live_refresh,
            refresh_mutex,
        }
    }

    /// Tries to restore a session from a stored token.
    ///
    /// If the token is expired or rejected by Twitch it is refreshed first.
    /// Returns `Err` if no valid token can be obtained.
    pub async fn restore_session(&self) -> anyhow::Result<()> {
        let mut token = self.store.load_token()?;
        let flow = DeviceFlow::new(CLIENT_ID.to_string());

        let needs_refresh = if token.is_expired() {
            tracing::info!("Token expired, attempting refresh...");
            true
        } else {
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

    /// Sets up the client and state for an authenticated session, then loads
    /// followed channels.
    pub async fn initialize_session(&self, token: &Token) -> anyhow::Result<()> {
        self.client
            .set_access_token(token.access_token.clone())
            .await;
        self.client.set_user_id(token.user_id.clone()).await;

        self.state
            .set_authenticated(true, token.user_id.clone(), token.user_login.clone())
            .await;

        if let Err(e) = self.load_followed_channels().await {
            tracing::warn!("Failed to load followed channels: {}", e);
        }

        Ok(())
    }

    /// Fetches all followed channels from the API and syncs them to the DB.
    pub async fn load_followed_channels(&self) -> anyhow::Result<()> {
        let follows = crate::twitch::with_retry(
            || self.client.get_all_followed_channels(),
            || self.try_refresh_token(),
        )
        .await
        .map_err(anyhow::Error::from)?;

        self.db.sync_followed(&follows)?;
        let ids = self.db.get_followed_ids()?;
        self.db.ensure_schedule_queue_entries(&ids)?;

        self.state.set_followed_channels(follows).await;
        Ok(())
    }

    /// Attempts to refresh the OAuth token.
    ///
    /// Serialized via mutex because Twitch refresh tokens are single-use —
    /// concurrent refreshes would invalidate each other.
    pub async fn try_refresh_token(&self) -> anyhow::Result<()> {
        let failing_token = self.client.get_access_token().await;

        let _guard = self.refresh_mutex.lock().await;

        if self.client.get_access_token().await != failing_token {
            tracing::debug!("Token already refreshed by another task");
            return Ok(());
        }

        tracing::info!("Token expired during API call, attempting refresh...");

        let token = self.store.load_token()?;
        let flow = DeviceFlow::new(CLIENT_ID.to_string());
        let new_token = flow.refresh_token(&token.refresh_token).await?;

        self.store.save_token(&new_token)?;
        self.client
            .set_access_token(new_token.access_token.clone())
            .await;

        tracing::info!("Token refreshed successfully");
        Ok(())
    }

    /// Runs the OAuth device flow, then initializes the session on success.
    ///
    /// Returns `Ok(())` when the user has authenticated. The caller is
    /// responsible for spawning this in a background task and for performing
    /// the initial data refresh (`App::refresh_all_data`) on success.
    pub async fn handle_login(&self, cancel: watch::Receiver<bool>) -> anyhow::Result<()> {
        let flow = DeviceFlow::new(CLIENT_ID.to_string());

        let token = flow
            .authenticate(
                |_user_code, verification_uri| {
                    if let Err(e) = open::that(verification_uri) {
                        tracing::error!("Failed to open browser: {}", e);
                    }
                },
                cancel,
            )
            .await?;

        self.store.save_token(&token)?;
        self.initialize_session(&token).await?;

        tracing::info!("Logged in as {}", token.user_login);
        Ok(())
    }

    /// Clears the stored token, client credentials, and app state.
    pub async fn handle_logout(&self) {
        if let Err(e) = self.store.delete_token() {
            tracing::error!("Failed to delete token: {}", e);
        }

        self.state.clear().await;
        self.client.clear_auth().await;
        self.initial_load_done.store(false, Ordering::SeqCst);
    }

    /// Marks that the initial data load is complete (notifications may now fire).
    pub fn mark_initial_load_done(&self) {
        self.initial_load_done.store(true, Ordering::SeqCst);
    }

    /// Records the current time as the last successful live-stream refresh.
    pub async fn record_live_refresh(&self) {
        *self.last_live_refresh.write().await = Some(Utc::now());
    }

    /// Returns the time of the last successful live-stream refresh.
    pub async fn last_live_refresh(&self) -> Option<DateTime<Utc>> {
        *self.last_live_refresh.read().await
    }
}

impl Clone for SessionManager {
    fn clone(&self) -> Self {
        Self {
            store: TokenStore::new().expect("Failed to create token store"),
            client: self.client.clone(),
            state: self.state.clone(),
            db: self.db.clone(),
            refresh_mutex: self.refresh_mutex.clone(),
            initial_load_done: self.initial_load_done.clone(),
            last_live_refresh: self.last_live_refresh.clone(),
        }
    }
}

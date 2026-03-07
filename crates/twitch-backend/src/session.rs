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
use crate::handle::LoginProgress;
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
    /// Publishes device code flow progress so the KDE plasmoid (and other consumers) can
    /// show the pending code to the user.
    pub(crate) login_progress_tx: watch::Sender<Option<LoginProgress>>,
}

impl SessionManager {
    /// Creates a new `SessionManager` and returns it together with the receiver
    /// end of the login-progress watch channel (for callers that need to observe
    /// the device code flow state).
    pub fn new(
        store: TokenStore,
        client: TwitchClient,
        state: Arc<AppState>,
        db: Database,
        initial_load_done: Arc<AtomicBool>,
        last_live_refresh: Arc<RwLock<Option<DateTime<Utc>>>>,
        refresh_mutex: Arc<Mutex<()>>,
    ) -> (Self, watch::Receiver<Option<LoginProgress>>) {
        let (login_progress_tx, login_progress_rx) = watch::channel(None);
        (
            Self {
                store,
                client,
                state,
                db,
                initial_load_done,
                last_live_refresh,
                refresh_mutex,
                login_progress_tx,
            },
            login_progress_rx,
        )
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

        let token = run_device_flow(
            flow,
            cancel,
            self.login_progress_tx.clone(),
            |verification_uri| {
                if let Err(e) = open::that(verification_uri) {
                    tracing::error!("Failed to open browser: {}", e);
                }
            },
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

/// Runs the device code flow, emitting `LoginProgress` updates on `progress_tx`.
///
/// Calls `on_browser` with the `verification_uri` once the device code is obtained
/// (e.g. to open the URL in the system browser).
/// Sends `PendingCode` with both the user code and URI, `Confirmed` on success,
/// `Failed` on error. Returns the token on success.
async fn run_device_flow<H, F>(
    flow: DeviceFlow<H>,
    cancel: watch::Receiver<bool>,
    progress_tx: watch::Sender<Option<LoginProgress>>,
    on_browser: F,
) -> anyhow::Result<Token>
where
    H: crate::twitch::http::HttpClient,
    F: FnOnce(&str),
{
    let tx_for_callback = progress_tx.clone();

    let result = flow
        .authenticate(
            |user_code, verification_uri| {
                let _ = tx_for_callback.send(Some(LoginProgress::PendingCode {
                    user_code: user_code.to_string(),
                    verification_uri: verification_uri.to_string(),
                }));
                on_browser(verification_uri);
            },
            cancel,
        )
        .await;

    match result {
        Ok(token) => {
            let _ = progress_tx.send(Some(LoginProgress::Confirmed));
            Ok(token)
        }
        Err(e) => {
            let _ = progress_tx.send(Some(LoginProgress::Failed(e.to_string())));
            Err(e)
        }
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
            login_progress_tx: self.login_progress_tx.clone(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::auth::DeviceFlow;
    use crate::twitch::http::mock::MockHttpClient;
    use serde::Serialize;
    use tokio::sync::watch;

    const DEVICE_CODE_URL: &str = "https://id.twitch.tv/oauth2/device";
    const TOKEN_URL: &str = "https://id.twitch.tv/oauth2/token";
    const VALIDATE_URL: &str = "https://id.twitch.tv/oauth2/validate";

    #[derive(Serialize)]
    struct DeviceCodeBody {
        device_code: String,
        user_code: String,
        verification_uri: String,
        expires_in: i64,
        interval: i64,
    }

    #[derive(Serialize)]
    struct TokenBody {
        access_token: String,
        refresh_token: String,
        expires_in: i64,
        scope: Vec<String>,
    }

    #[derive(Serialize)]
    struct ValidateBody {
        login: String,
        user_id: String,
    }

    fn device_code_body(user_code: &str) -> DeviceCodeBody {
        DeviceCodeBody {
            device_code: "dev_code".into(),
            user_code: user_code.into(),
            verification_uri: "https://twitch.tv/activate".into(),
            expires_in: 600,
            interval: 1,
        }
    }

    fn token_body() -> TokenBody {
        TokenBody {
            access_token: "tok_abc".into(),
            refresh_token: "ref_def".into(),
            expires_in: 3600,
            scope: vec!["user:read:follows".into()],
        }
    }

    fn validate_body() -> ValidateBody {
        ValidateBody {
            login: "testuser".into(),
            user_id: "99999".into(),
        }
    }

    #[tokio::test]
    async fn login_progress_sends_pending_code_with_user_code() {
        // Mock: device code succeeds, token poll returns access_denied (non-retryable → fast fail)
        let mock = MockHttpClient::new()
            .on_post_json(DEVICE_CODE_URL, &device_code_body("ABC-123"))
            .on_post(TOKEN_URL, 400, r#"{"message":"access_denied"}"#);
        let flow = DeviceFlow::with_http_client("client_id".into(), mock);

        let (progress_tx, mut progress_rx) = watch::channel(None::<LoginProgress>);
        let (_cancel_tx, cancel_rx) = watch::channel(false);

        // Spawn the flow so we can observe intermediate channel states
        let task = tokio::spawn(run_device_flow(flow, cancel_rx, progress_tx, |_| {}));

        // Wait for the first progress update (PendingCode)
        progress_rx.changed().await.unwrap();
        let value = progress_rx.borrow().clone();

        let _ = task.await;

        assert!(
            matches!(value, Some(LoginProgress::PendingCode { ref user_code, .. }) if user_code == "ABC-123"),
            "expected PendingCode with user_code ABC-123, got {:?}",
            value
        );
    }

    #[tokio::test]
    async fn login_progress_sends_confirmed_on_success() {
        let mock = MockHttpClient::new()
            .on_post_json(DEVICE_CODE_URL, &device_code_body("XYZ"))
            .on_post_json(TOKEN_URL, &token_body())
            .on_get(
                VALIDATE_URL,
                200,
                serde_json::to_string(&validate_body()).unwrap(),
            );
        let flow = DeviceFlow::with_http_client("client_id".into(), mock);

        let (progress_tx, progress_rx) = watch::channel(None::<LoginProgress>);
        let (_cancel_tx, cancel_rx) = watch::channel(false);

        let result = run_device_flow(flow, cancel_rx, progress_tx, |_| {}).await;

        assert!(result.is_ok(), "expected Ok, got {:?}", result);
        assert_eq!(
            *progress_rx.borrow(),
            Some(LoginProgress::Confirmed),
            "expected Confirmed after successful flow"
        );
    }

    #[tokio::test]
    async fn login_progress_sends_failed_on_error() {
        // access_denied is returned immediately after device code
        let mock = MockHttpClient::new()
            .on_post_json(DEVICE_CODE_URL, &device_code_body("XYZ"))
            .on_post(TOKEN_URL, 400, r#"{"message":"access_denied"}"#);
        let flow = DeviceFlow::with_http_client("client_id".into(), mock);

        let (progress_tx, progress_rx) = watch::channel(None::<LoginProgress>);
        let (_cancel_tx, cancel_rx) = watch::channel(false);

        let result = run_device_flow(flow, cancel_rx, progress_tx, |_| {}).await;

        assert!(result.is_err(), "expected Err");
        assert!(
            matches!(*progress_rx.borrow(), Some(LoginProgress::Failed(_))),
            "expected Failed, got {:?}",
            *progress_rx.borrow()
        );
    }
}

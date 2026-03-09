use super::store::Token;
use crate::twitch::http::{HttpClient, ReqwestClient};
use anyhow::{Context, Result};
use chrono::{Duration, Utc};
use reqwest::header::{HeaderMap, HeaderValue, AUTHORIZATION};
use serde::Deserialize;

const DEVICE_CODE_URL: &str = "https://id.twitch.tv/oauth2/device";
const TOKEN_URL: &str = "https://id.twitch.tv/oauth2/token";
const VALIDATE_URL: &str = "https://id.twitch.tv/oauth2/validate";

/// Required OAuth scopes
const REQUIRED_SCOPES: &str = "user:read:follows";

/// Device flow errors
#[derive(Debug, thiserror::Error)]
pub enum DeviceFlowError {
    #[error("Authorization pending")]
    AuthorizationPending,
    #[error("Slow down")]
    SlowDown,
    #[error("Access denied by user")]
    AccessDenied,
    #[error("Device code expired")]
    ExpiredToken,
    #[error("Network error: {0}")]
    Network(String),
    #[error("API error: {0}")]
    Api(String),
}

/// Response from the device code request
#[derive(Debug, Clone, Deserialize)]
pub struct DeviceCodeResponse {
    pub device_code: String,
    pub user_code: String,
    pub verification_uri: String,
    pub expires_in: i64,
    pub interval: i64,
}

/// Response from the token request
#[derive(Debug, Clone, Deserialize)]
struct TokenResponse {
    access_token: String,
    refresh_token: String,
    expires_in: i64,
    scope: Vec<String>,
}

/// Error response from Twitch
#[derive(Debug, Clone, Deserialize)]
struct ErrorResponse {
    message: String,
}

/// Response from token validation
#[derive(Debug, Clone, Deserialize)]
pub struct ValidateResponse {
    pub login: String,
    pub user_id: String,
}

/// OAuth Device Code Flow handler
pub struct DeviceFlow<H: HttpClient = ReqwestClient> {
    client_id: String,
    http: H,
}

impl DeviceFlow<ReqwestClient> {
    /// Creates a new device flow handler
    pub fn new(client_id: String) -> Self {
        Self {
            client_id,
            http: ReqwestClient::new(),
        }
    }
}

impl<H: HttpClient> DeviceFlow<H> {
    /// Creates a device flow handler with a custom HTTP client (for testing)
    #[cfg(test)]
    pub fn with_http_client(client_id: String, http: H) -> Self {
        Self { client_id, http }
    }

    /// Initiates the device code flow
    pub async fn request_device_code(&self) -> Result<DeviceCodeResponse> {
        let params = vec![
            ("client_id".to_string(), self.client_id.clone()),
            ("scopes".to_string(), REQUIRED_SCOPES.to_string()),
        ];

        let response = self
            .http
            .post_form_response(DEVICE_CODE_URL, params)
            .await
            .context("Failed to request device code")?;

        if !response.is_success() {
            anyhow::bail!(
                "Device code request failed: {} - {}",
                response.status,
                response.body
            );
        }

        response
            .json()
            .context("Failed to parse device code response")
    }

    /// Polls for the access token once
    async fn poll_for_token(&self, device_code: &str) -> Result<TokenResponse, DeviceFlowError> {
        let params = vec![
            ("client_id".to_string(), self.client_id.clone()),
            ("device_code".to_string(), device_code.to_string()),
            (
                "grant_type".to_string(),
                "urn:ietf:params:oauth:grant-type:device_code".to_string(),
            ),
        ];

        let response = self
            .http
            .post_form_response(TOKEN_URL, params)
            .await
            .map_err(|e| DeviceFlowError::Network(e.to_string()))?;

        if response.status == 400 {
            let err_resp: ErrorResponse = response
                .json()
                .map_err(|e| DeviceFlowError::Network(e.to_string()))?;

            return match err_resp.message.as_str() {
                "authorization_pending" => Err(DeviceFlowError::AuthorizationPending),
                "slow_down" => Err(DeviceFlowError::SlowDown),
                "access_denied" => Err(DeviceFlowError::AccessDenied),
                "expired_token" => Err(DeviceFlowError::ExpiredToken),
                _ => Err(DeviceFlowError::Api(err_resp.message)),
            };
        }

        if !response.is_success() {
            return Err(DeviceFlowError::Api(format!(
                "Token request failed: {}",
                response.status
            )));
        }

        response
            .json()
            .map_err(|e| DeviceFlowError::Network(e.to_string()))
    }

    /// Polls until the user authorizes or the code expires
    async fn wait_for_token(
        &self,
        dcr: &DeviceCodeResponse,
        cancel: tokio::sync::watch::Receiver<bool>,
    ) -> Result<TokenResponse, DeviceFlowError> {
        let mut interval = std::time::Duration::from_secs(dcr.interval as u64);
        if interval.is_zero() {
            interval = std::time::Duration::from_secs(5);
        }

        let deadline = Utc::now() + Duration::seconds(dcr.expires_in);

        tracing::info!(
            "Polling for token every {:?} (expires in {}s)",
            interval,
            dcr.expires_in
        );

        loop {
            // Check for cancellation
            if *cancel.borrow() {
                return Err(DeviceFlowError::Api("Authentication cancelled".into()));
            }

            tokio::time::sleep(interval).await;

            if Utc::now() > deadline {
                tracing::warn!("Device code expired");
                return Err(DeviceFlowError::ExpiredToken);
            }

            match self.poll_for_token(&dcr.device_code).await {
                Ok(token) => {
                    tracing::info!("Token received successfully");
                    return Ok(token);
                }
                Err(DeviceFlowError::AuthorizationPending) => {
                    tracing::debug!("Authorization pending, continuing to poll...");
                }
                Err(DeviceFlowError::SlowDown) => {
                    interval += std::time::Duration::from_secs(5);
                    tracing::info!("Slowing down, new interval: {:?}", interval);
                }
                Err(e) => {
                    tracing::error!("Poll error: {}", e);
                    return Err(e);
                }
            }
        }
    }

    /// Validates an access token and returns user info
    pub async fn validate_token(&self, access_token: &str) -> Result<ValidateResponse> {
        let mut headers = HeaderMap::new();
        let auth_value = HeaderValue::from_str(&format!("OAuth {access_token}"))
            .context("Invalid access token value")?;
        headers.insert(AUTHORIZATION, auth_value);

        let response = self
            .http
            .get_response(VALIDATE_URL, &headers)
            .await
            .context("Failed to validate token")?;

        if response.status == 401 {
            anyhow::bail!("Token expired or invalid");
        }

        if !response.is_success() {
            anyhow::bail!("Validation failed: {}", response.status);
        }

        response
            .json()
            .context("Failed to parse validation response")
    }

    /// Refreshes an access token using a refresh token
    pub async fn refresh_token(&self, refresh_token: &str) -> Result<Token> {
        let params = vec![
            ("client_id".to_string(), self.client_id.clone()),
            ("refresh_token".to_string(), refresh_token.to_string()),
            ("grant_type".to_string(), "refresh_token".to_string()),
        ];

        let response = self
            .http
            .post_form_response(TOKEN_URL, params)
            .await
            .context("Failed to refresh token")?;

        if !response.is_success() {
            anyhow::bail!(
                "Token refresh failed: {} - {}",
                response.status,
                response.body
            );
        }

        let tr: TokenResponse = response
            .json()
            .context("Failed to parse refresh response")?;

        // Validate the new token to get user info
        let vr = self.validate_token(&tr.access_token).await?;

        Ok(Token {
            access_token: tr.access_token,
            refresh_token: tr.refresh_token,
            expires_at: Utc::now() + Duration::seconds(tr.expires_in),
            scopes: tr.scope,
            user_id: vr.user_id,
            user_login: vr.login,
        })
    }

    /// Performs the full device code flow
    ///
    /// The `on_code` callback is called with the user code and verification URI
    /// when the device code is obtained, so the caller can display them to the user.
    pub async fn authenticate<F>(
        &self,
        on_code: F,
        cancel: tokio::sync::watch::Receiver<bool>,
    ) -> Result<Token>
    where
        F: FnOnce(&str, &str),
    {
        let dcr = self.request_device_code().await?;

        // Notify the caller of the user code
        on_code(&dcr.user_code, &dcr.verification_uri);

        let tr = self.wait_for_token(&dcr, cancel).await?;

        // Validate the token to get user info
        let vr = self.validate_token(&tr.access_token).await?;

        Ok(Token {
            access_token: tr.access_token,
            refresh_token: tr.refresh_token,
            expires_at: Utc::now() + Duration::seconds(tr.expires_in),
            scopes: tr.scope,
            user_id: vr.user_id,
            user_login: vr.login,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::twitch::http::mock::MockHttpClient;
    use serde::Serialize;
    use tokio::sync::watch;

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
    async fn poll_for_token_returns_success() {
        let mock = MockHttpClient::new().on_post_json(TOKEN_URL, &token_body());
        let flow = DeviceFlow::with_http_client("client_id".into(), mock);

        let result = flow.poll_for_token("device_code_123").await;
        assert!(result.is_ok(), "expected Ok, got {:?}", result);
        let token = result.unwrap();
        assert_eq!(token.access_token, "tok_abc");
        assert_eq!(token.refresh_token, "ref_def");
    }

    #[tokio::test]
    async fn poll_for_token_returns_authorization_pending() {
        let mock =
            MockHttpClient::new().on_post(TOKEN_URL, 400, r#"{"message":"authorization_pending"}"#);
        let flow = DeviceFlow::with_http_client("client_id".into(), mock);

        let result = flow.poll_for_token("device_code_123").await;
        assert!(
            matches!(result, Err(DeviceFlowError::AuthorizationPending)),
            "expected AuthorizationPending, got {:?}",
            result
        );
    }

    #[tokio::test]
    async fn poll_for_token_returns_slow_down() {
        let mock = MockHttpClient::new().on_post(TOKEN_URL, 400, r#"{"message":"slow_down"}"#);
        let flow = DeviceFlow::with_http_client("client_id".into(), mock);

        let result = flow.poll_for_token("device_code_123").await;
        assert!(
            matches!(result, Err(DeviceFlowError::SlowDown)),
            "expected SlowDown, got {:?}",
            result
        );
    }

    #[tokio::test]
    async fn poll_for_token_returns_expired_token() {
        let mock = MockHttpClient::new().on_post(TOKEN_URL, 400, r#"{"message":"expired_token"}"#);
        let flow = DeviceFlow::with_http_client("client_id".into(), mock);

        let result = flow.poll_for_token("device_code_123").await;
        assert!(
            matches!(result, Err(DeviceFlowError::ExpiredToken)),
            "expected ExpiredToken, got {:?}",
            result
        );
    }

    #[tokio::test]
    async fn poll_for_token_returns_access_denied() {
        let mock = MockHttpClient::new().on_post(TOKEN_URL, 400, r#"{"message":"access_denied"}"#);
        let flow = DeviceFlow::with_http_client("client_id".into(), mock);

        let result = flow.poll_for_token("device_code_123").await;
        assert!(
            matches!(result, Err(DeviceFlowError::AccessDenied)),
            "expected AccessDenied, got {:?}",
            result
        );
    }

    #[tokio::test]
    async fn wait_for_token_cancelled_immediately() {
        let mock = MockHttpClient::new();
        let flow = DeviceFlow::with_http_client("client_id".into(), mock);

        let (_tx, cancel) = watch::channel(true); // already cancelled
        let dcr = DeviceCodeResponse {
            device_code: "code".into(),
            user_code: "USER-CODE".into(),
            verification_uri: "https://twitch.tv/activate".into(),
            expires_in: 600,
            interval: 5,
        };

        let result = flow.wait_for_token(&dcr, cancel).await;
        assert!(
            matches!(result, Err(DeviceFlowError::Api(ref msg)) if msg.contains("cancelled")),
            "expected cancellation error, got {:?}",
            result
        );
    }

    #[tokio::test]
    async fn validate_token_returns_user_info() {
        let mock = MockHttpClient::new().on_get(VALIDATE_URL, 200, {
            serde_json::to_string(&validate_body()).unwrap()
        });
        let flow = DeviceFlow::with_http_client("client_id".into(), mock);

        let result = flow.validate_token("my_access_token").await;
        assert!(result.is_ok(), "expected Ok, got {:?}", result);
        let vr = result.unwrap();
        assert_eq!(vr.login, "testuser");
        assert_eq!(vr.user_id, "99999");
    }

    #[tokio::test]
    async fn validate_token_returns_error_on_401() {
        let mock = MockHttpClient::new().on_get(VALIDATE_URL, 401, "Unauthorized");
        let flow = DeviceFlow::with_http_client("client_id".into(), mock);

        let result = flow.validate_token("bad_token").await;
        assert!(result.is_err());
        assert!(result
            .unwrap_err()
            .to_string()
            .contains("expired or invalid"));
    }

    #[tokio::test]
    async fn refresh_token_success() {
        let mock = MockHttpClient::new()
            .on_post_json(TOKEN_URL, &token_body())
            .on_get(VALIDATE_URL, 200, {
                serde_json::to_string(&validate_body()).unwrap()
            });
        let flow = DeviceFlow::with_http_client("client_id".into(), mock);

        let result = flow.refresh_token("my_refresh_token").await;
        assert!(result.is_ok(), "expected Ok, got {:?}", result);
        let token = result.unwrap();
        assert_eq!(token.access_token, "tok_abc");
        assert_eq!(token.user_login, "testuser");
        assert_eq!(token.user_id, "99999");
    }

    #[tokio::test]
    async fn refresh_token_fails_on_non_success_status() {
        let mock = MockHttpClient::new().on_post(TOKEN_URL, 401, "Unauthorized");
        let flow = DeviceFlow::with_http_client("client_id".into(), mock);

        let result = flow.refresh_token("bad_refresh_token").await;
        assert!(result.is_err());
        assert!(result
            .unwrap_err()
            .to_string()
            .contains("Token refresh failed"));
    }

    #[tokio::test]
    async fn request_device_code_success() {
        #[derive(Serialize)]
        struct DeviceCodeBody {
            device_code: String,
            user_code: String,
            verification_uri: String,
            expires_in: i64,
            interval: i64,
        }

        let body = DeviceCodeBody {
            device_code: "dev_code_xyz".into(),
            user_code: "HELLO-WORLD".into(),
            verification_uri: "https://twitch.tv/activate".into(),
            expires_in: 600,
            interval: 5,
        };

        let mock = MockHttpClient::new().on_post_json(DEVICE_CODE_URL, &body);
        let flow = DeviceFlow::with_http_client("client_id".into(), mock);

        let result = flow.request_device_code().await;
        assert!(result.is_ok(), "expected Ok, got {:?}", result);
        let dcr = result.unwrap();
        assert_eq!(dcr.device_code, "dev_code_xyz");
        assert_eq!(dcr.user_code, "HELLO-WORLD");
    }
}

use anyhow::{Context, Result};
use chrono::{Duration, Utc};
use reqwest::Client;
use serde::Deserialize;
use std::collections::HashMap;

use super::store::Token;

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
    Network(#[from] reqwest::Error),
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
#[allow(dead_code)]
struct TokenResponse {
    access_token: String,
    refresh_token: String,
    expires_in: i64,
    scope: Vec<String>,
    token_type: String,
}

/// Error response from Twitch
#[derive(Debug, Clone, Deserialize)]
#[allow(dead_code)]
struct ErrorResponse {
    status: Option<i64>,
    message: String,
}

/// Response from token validation
#[derive(Debug, Clone, Deserialize)]
#[allow(dead_code)]
pub struct ValidateResponse {
    pub client_id: String,
    pub login: String,
    pub scopes: Vec<String>,
    pub user_id: String,
    pub expires_in: i64,
}

/// OAuth Device Code Flow handler
pub struct DeviceFlow {
    client_id: String,
    http: Client,
}

impl DeviceFlow {
    /// Creates a new device flow handler
    pub fn new(client_id: String) -> Self {
        Self {
            client_id,
            http: Client::builder()
                .timeout(std::time::Duration::from_secs(10))
                .build()
                .expect("Failed to create HTTP client"),
        }
    }

    /// Initiates the device code flow
    pub async fn request_device_code(&self) -> Result<DeviceCodeResponse> {
        let mut params = HashMap::new();
        params.insert("client_id", self.client_id.as_str());
        params.insert("scopes", REQUIRED_SCOPES);

        let response = self
            .http
            .post(DEVICE_CODE_URL)
            .form(&params)
            .send()
            .await
            .context("Failed to request device code")?;

        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
            anyhow::bail!("Device code request failed: {} - {}", status, body);
        }

        response
            .json()
            .await
            .context("Failed to parse device code response")
    }

    /// Polls for the access token once
    async fn poll_for_token(&self, device_code: &str) -> Result<TokenResponse, DeviceFlowError> {
        let mut params = HashMap::new();
        params.insert("client_id", self.client_id.as_str());
        params.insert("device_code", device_code);
        params.insert("grant_type", "urn:ietf:params:oauth:grant-type:device_code");

        let response = self.http.post(TOKEN_URL).form(&params).send().await?;

        if response.status() == reqwest::StatusCode::BAD_REQUEST {
            let err_resp: ErrorResponse = response.json().await?;

            return match err_resp.message.as_str() {
                "authorization_pending" => Err(DeviceFlowError::AuthorizationPending),
                "slow_down" => Err(DeviceFlowError::SlowDown),
                "access_denied" => Err(DeviceFlowError::AccessDenied),
                "expired_token" => Err(DeviceFlowError::ExpiredToken),
                _ => Err(DeviceFlowError::Api(err_resp.message)),
            };
        }

        if !response.status().is_success() {
            return Err(DeviceFlowError::Api(format!(
                "Token request failed: {}",
                response.status()
            )));
        }

        Ok(response.json().await?)
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
                    continue;
                }
                Err(DeviceFlowError::SlowDown) => {
                    interval += std::time::Duration::from_secs(5);
                    tracing::info!("Slowing down, new interval: {:?}", interval);
                    continue;
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
        let response = self
            .http
            .get(VALIDATE_URL)
            .header("Authorization", format!("OAuth {}", access_token))
            .send()
            .await
            .context("Failed to validate token")?;

        if response.status() == reqwest::StatusCode::UNAUTHORIZED {
            anyhow::bail!("Token expired or invalid");
        }

        if !response.status().is_success() {
            anyhow::bail!("Validation failed: {}", response.status());
        }

        response
            .json()
            .await
            .context("Failed to parse validation response")
    }

    /// Refreshes an access token using a refresh token
    pub async fn refresh_token(&self, refresh_token: &str) -> Result<Token> {
        let mut params = HashMap::new();
        params.insert("client_id", self.client_id.as_str());
        params.insert("refresh_token", refresh_token);
        params.insert("grant_type", "refresh_token");

        let response = self
            .http
            .post(TOKEN_URL)
            .form(&params)
            .send()
            .await
            .context("Failed to refresh token")?;

        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
            anyhow::bail!("Token refresh failed: {} - {}", status, body);
        }

        let tr: TokenResponse = response
            .json()
            .await
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

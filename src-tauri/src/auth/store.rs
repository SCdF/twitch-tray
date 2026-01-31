use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

const SERVICE_NAME: &str = "twitch-tray";
const TOKEN_FILE: &str = "token.json";

/// OAuth token data
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Token {
    pub access_token: String,
    pub refresh_token: String,
    pub expires_at: DateTime<Utc>,
    pub scopes: Vec<String>,
    pub user_id: String,
    pub user_login: String,
}

impl Token {
    /// Checks if the token has expired
    pub fn is_expired(&self) -> bool {
        Utc::now() > self.expires_at
    }

    /// Checks if the token exists and is not expired
    pub fn is_valid(&self) -> bool {
        !self.access_token.is_empty() && !self.is_expired()
    }
}

/// Token store errors
#[derive(Debug, thiserror::Error)]
#[allow(dead_code)]
pub enum StoreError {
    #[error("No token stored")]
    NoToken,
    #[error("Token expired")]
    TokenExpired,
    #[error("Storage error: {0}")]
    Storage(#[from] anyhow::Error),
}

/// Secure token storage
///
/// Uses a file-based encrypted storage via the keyring crate.
/// Falls back to plain JSON file if keyring is unavailable.
pub struct TokenStore {
    keyring_entry: Option<keyring::Entry>,
    fallback_path: PathBuf,
}

impl TokenStore {
    /// Creates a new token store
    pub fn new() -> Result<Self> {
        let config_dir = crate::config::ConfigManager::config_dir()?;
        std::fs::create_dir_all(&config_dir)?;
        let fallback_path = config_dir.join(TOKEN_FILE);

        // Try to create a keyring entry
        let keyring_entry = keyring::Entry::new(SERVICE_NAME, "oauth_token").ok();

        Ok(Self {
            keyring_entry,
            fallback_path,
        })
    }

    /// Saves the OAuth token
    pub fn save_token(&self, token: &Token) -> Result<()> {
        let data = serde_json::to_string(token).context("Failed to serialize token")?;

        // Try keyring first
        if let Some(ref entry) = self.keyring_entry {
            if entry.set_password(&data).is_ok() {
                // Also remove fallback file if it exists
                let _ = std::fs::remove_file(&self.fallback_path);
                return Ok(());
            }
        }

        // Fall back to file storage
        std::fs::write(&self.fallback_path, &data).context("Failed to write token file")?;

        Ok(())
    }

    /// Loads the stored OAuth token
    pub fn load_token(&self) -> Result<Token, StoreError> {
        // Try keyring first
        if let Some(ref entry) = self.keyring_entry {
            if let Ok(data) = entry.get_password() {
                let token: Token =
                    serde_json::from_str(&data).map_err(|e| StoreError::Storage(e.into()))?;
                return Ok(token);
            }
        }

        // Fall back to file storage
        if self.fallback_path.exists() {
            let data = std::fs::read_to_string(&self.fallback_path)
                .map_err(|e| StoreError::Storage(e.into()))?;
            let token: Token =
                serde_json::from_str(&data).map_err(|e| StoreError::Storage(e.into()))?;
            return Ok(token);
        }

        Err(StoreError::NoToken)
    }

    /// Deletes the stored token
    pub fn delete_token(&self) -> Result<()> {
        // Try keyring first
        if let Some(ref entry) = self.keyring_entry {
            let _ = entry.delete_credential();
        }

        // Also remove fallback file
        if self.fallback_path.exists() {
            std::fs::remove_file(&self.fallback_path).context("Failed to delete token file")?;
        }

        Ok(())
    }

    /// Checks if a token is stored
    pub fn has_token(&self) -> bool {
        // Check keyring
        if let Some(ref entry) = self.keyring_entry {
            if entry.get_password().is_ok() {
                return true;
            }
        }

        // Check fallback file
        self.fallback_path.exists()
    }
}

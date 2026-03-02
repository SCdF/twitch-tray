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
pub enum StoreError {
    #[error("No token stored")]
    NoToken,
    #[error("Storage error: {0}")]
    Storage(#[from] anyhow::Error),
}

/// Secure token storage using file system with optional keyring backup
///
/// Uses a file-based storage via the keyring crate.
/// Falls back to plain JSON file if keyring is unavailable.
pub struct FileTokenStore {
    keyring_entry: Option<keyring::Entry>,
    fallback_path: PathBuf,
}

impl FileTokenStore {
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

    /// Creates a token store with a custom path (for testing)
    #[cfg(test)]
    pub fn with_path(path: PathBuf) -> Self {
        Self {
            keyring_entry: None,
            fallback_path: path,
        }
    }
}

/// Test-only async methods for FileTokenStore
#[cfg(test)]
impl FileTokenStore {
    /// Saves the OAuth token
    pub async fn save(&self, token: &Token) -> Result<()> {
        let data = serde_json::to_string(token).context("Failed to serialize token")?;
        std::fs::write(&self.fallback_path, &data).context("Failed to write token file")?;
        Ok(())
    }

    /// Loads the stored OAuth token
    pub async fn load(&self) -> Result<Token, StoreError> {
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
    pub async fn delete(&self) -> Result<()> {
        if self.fallback_path.exists() {
            std::fs::remove_file(&self.fallback_path).context("Failed to delete token file")?;
        }
        Ok(())
    }

    /// Checks if a token is stored
    pub async fn has_token(&self) -> bool {
        self.fallback_path.exists()
    }
}

/// Legacy synchronous interface for TokenStore
///
/// This wrapper maintains backward compatibility with existing code
/// that uses synchronous token store operations.
pub struct TokenStore {
    inner: FileTokenStore,
}

impl TokenStore {
    /// Creates a new token store
    pub fn new() -> Result<Self> {
        Ok(Self {
            inner: FileTokenStore::new()?,
        })
    }

    /// Saves the OAuth token
    pub fn save_token(&self, token: &Token) -> Result<()> {
        let data = serde_json::to_string(token).context("Failed to serialize token")?;

        // Always save to file storage for reliability
        std::fs::write(&self.inner.fallback_path, &data).context("Failed to write token file")?;

        // Also try keyring as a secondary store
        if let Some(ref entry) = self.inner.keyring_entry {
            let _ = entry.set_password(&data);
        }

        Ok(())
    }

    /// Loads the stored OAuth token
    pub fn load_token(&self) -> Result<Token, StoreError> {
        // Try file storage first (more reliable)
        if self.inner.fallback_path.exists() {
            let data = std::fs::read_to_string(&self.inner.fallback_path)
                .map_err(|e| StoreError::Storage(e.into()))?;
            let token: Token =
                serde_json::from_str(&data).map_err(|e| StoreError::Storage(e.into()))?;
            return Ok(token);
        }

        // Fall back to keyring (for migration from old storage)
        if let Some(ref entry) = self.inner.keyring_entry {
            if let Ok(data) = entry.get_password() {
                let token: Token =
                    serde_json::from_str(&data).map_err(|e| StoreError::Storage(e.into()))?;
                return Ok(token);
            }
        }

        Err(StoreError::NoToken)
    }

    /// Deletes the stored token
    pub fn delete_token(&self) -> Result<()> {
        // Delete file storage
        if self.inner.fallback_path.exists() {
            std::fs::remove_file(&self.inner.fallback_path)
                .context("Failed to delete token file")?;
        }

        // Also try to delete from keyring
        if let Some(ref entry) = self.inner.keyring_entry {
            let _ = entry.delete_credential();
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Duration;

    fn make_token(expires_in_hours: i64) -> Token {
        Token {
            access_token: "access_token_123".to_string(),
            refresh_token: "refresh_token_456".to_string(),
            expires_at: Utc::now() + Duration::hours(expires_in_hours),
            scopes: vec!["user:read:follows".to_string()],
            user_id: "user123".to_string(),
            user_login: "testuser".to_string(),
        }
    }

    // === Token tests ===

    #[test]
    fn token_is_expired_when_past_expiry() {
        let token = Token {
            access_token: "test".to_string(),
            refresh_token: "test".to_string(),
            expires_at: Utc::now() - Duration::hours(1),
            scopes: vec![],
            user_id: "123".to_string(),
            user_login: "test".to_string(),
        };

        assert!(token.is_expired());
    }

    #[test]
    fn token_is_not_expired_when_future_expiry() {
        let token = Token {
            access_token: "test".to_string(),
            refresh_token: "test".to_string(),
            expires_at: Utc::now() + Duration::hours(1),
            scopes: vec![],
            user_id: "123".to_string(),
            user_login: "test".to_string(),
        };

        assert!(!token.is_expired());
    }

    #[test]
    fn token_is_valid_when_not_empty_and_not_expired() {
        let token = make_token(1);
        assert!(token.is_valid());
    }

    #[test]
    fn token_is_invalid_when_empty_access_token() {
        let mut token = make_token(1);
        token.access_token = "".to_string();
        assert!(!token.is_valid());
    }

    #[test]
    fn token_is_invalid_when_expired() {
        let token = make_token(-1);
        assert!(!token.is_valid());
    }

    // === FileTokenStore tests (with temp files) ===

    #[tokio::test]
    async fn file_store_save_and_load() {
        let temp_dir = tempfile::tempdir().unwrap();
        let token_path = temp_dir.path().join("token.json");

        let store = FileTokenStore::with_path(token_path);
        let token = make_token(1);

        store.save(&token).await.unwrap();
        let loaded = store.load().await.unwrap();

        assert_eq!(loaded.access_token, token.access_token);
        assert_eq!(loaded.user_id, token.user_id);
    }

    #[tokio::test]
    async fn file_store_load_nonexistent_returns_error() {
        let temp_dir = tempfile::tempdir().unwrap();
        let token_path = temp_dir.path().join("nonexistent.json");

        let store = FileTokenStore::with_path(token_path);
        let result = store.load().await;

        assert!(matches!(result, Err(StoreError::NoToken)));
    }

    #[tokio::test]
    async fn file_store_delete_removes_file() {
        let temp_dir = tempfile::tempdir().unwrap();
        let token_path = temp_dir.path().join("token.json");

        let store = FileTokenStore::with_path(token_path.clone());
        store.save(&make_token(1)).await.unwrap();

        assert!(token_path.exists());

        store.delete().await.unwrap();

        assert!(!token_path.exists());
    }

    #[tokio::test]
    async fn file_store_has_token() {
        let temp_dir = tempfile::tempdir().unwrap();
        let token_path = temp_dir.path().join("token.json");

        let store = FileTokenStore::with_path(token_path);

        assert!(!store.has_token().await);

        store.save(&make_token(1)).await.unwrap();

        assert!(store.has_token().await);
    }

    // === Token serialization tests ===

    #[test]
    fn token_serialization_roundtrip() {
        let token = make_token(1);
        let json = serde_json::to_string(&token).unwrap();
        let deserialized: Token = serde_json::from_str(&json).unwrap();

        assert_eq!(deserialized.access_token, token.access_token);
        assert_eq!(deserialized.refresh_token, token.refresh_token);
        assert_eq!(deserialized.user_id, token.user_id);
        assert_eq!(deserialized.user_login, token.user_login);
        assert_eq!(deserialized.scopes, token.scopes);
    }
}

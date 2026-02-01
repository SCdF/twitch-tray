use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::sync::RwLock;

const APP_NAME: &str = "twitch-tray";
const CONFIG_FILE: &str = "config.json";

/// Application configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    #[serde(default = "default_poll_interval")]
    pub poll_interval_sec: u64,
    #[serde(default = "default_schedule_poll")]
    pub schedule_poll_min: u64,
    #[serde(default = "default_notify_on_live")]
    pub notify_on_live: bool,
    #[serde(default = "default_notify_on_category")]
    pub notify_on_category: bool,
}

fn default_poll_interval() -> u64 {
    60
}

fn default_schedule_poll() -> u64 {
    5
}

fn default_notify_on_live() -> bool {
    true
}

fn default_notify_on_category() -> bool {
    true
}

impl Default for Config {
    fn default() -> Self {
        Self {
            poll_interval_sec: default_poll_interval(),
            schedule_poll_min: default_schedule_poll(),
            notify_on_live: default_notify_on_live(),
            notify_on_category: default_notify_on_category(),
        }
    }
}

/// Configuration manager
pub struct ConfigManager {
    config: RwLock<Config>,
    file_path: PathBuf,
}

impl ConfigManager {
    /// Creates a new configuration manager
    pub fn new() -> Result<Self> {
        let config_dir = dirs::config_dir()
            .context("Could not determine config directory")?
            .join(APP_NAME);

        std::fs::create_dir_all(&config_dir).context("Failed to create config directory")?;

        let file_path = config_dir.join(CONFIG_FILE);

        let config = if file_path.exists() {
            let data = std::fs::read_to_string(&file_path).context("Failed to read config file")?;
            serde_json::from_str(&data).unwrap_or_default()
        } else {
            Config::default()
        };

        Ok(Self {
            config: RwLock::new(config),
            file_path,
        })
    }

    /// Gets a copy of the current configuration
    pub fn get(&self) -> Config {
        self.config.read().unwrap().clone()
    }

    /// Saves the configuration to disk
    pub fn save(&self) -> Result<()> {
        let config = self.config.read().unwrap();
        let data = serde_json::to_string_pretty(&*config).context("Failed to serialize config")?;
        std::fs::write(&self.file_path, data).context("Failed to write config file")?;
        Ok(())
    }

    /// Returns the path to the config file
    pub fn file_path(&self) -> &PathBuf {
        &self.file_path
    }

    /// Returns the config directory path
    pub fn config_dir() -> Result<PathBuf> {
        Ok(dirs::config_dir()
            .context("Could not determine config directory")?
            .join(APP_NAME))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // === Config default values tests ===

    #[test]
    fn default_poll_interval_is_60() {
        let config = Config::default();
        assert_eq!(config.poll_interval_sec, 60);
    }

    #[test]
    fn default_schedule_poll_is_5() {
        let config = Config::default();
        assert_eq!(config.schedule_poll_min, 5);
    }

    #[test]
    fn default_notify_on_live_is_true() {
        let config = Config::default();
        assert!(config.notify_on_live);
    }

    #[test]
    fn default_notify_on_category_is_true() {
        let config = Config::default();
        assert!(config.notify_on_category);
    }

    // === Partial deserialization tests ===

    #[test]
    fn deserialize_empty_uses_defaults() {
        let json = "{}";
        let config: Config = serde_json::from_str(json).unwrap();

        assert_eq!(config.poll_interval_sec, 60);
        assert_eq!(config.schedule_poll_min, 5);
        assert!(config.notify_on_live);
        assert!(config.notify_on_category);
    }

    #[test]
    fn deserialize_partial_uses_defaults_for_missing() {
        let json = r#"{"poll_interval_sec": 30}"#;
        let config: Config = serde_json::from_str(json).unwrap();

        assert_eq!(config.poll_interval_sec, 30); // Overridden
        assert_eq!(config.schedule_poll_min, 5); // Default
        assert!(config.notify_on_live); // Default
        assert!(config.notify_on_category); // Default
    }

    #[test]
    fn deserialize_full_config() {
        let json = r#"{
            "poll_interval_sec": 120,
            "schedule_poll_min": 10,
            "notify_on_live": false,
            "notify_on_category": false
        }"#;
        let config: Config = serde_json::from_str(json).unwrap();

        assert_eq!(config.poll_interval_sec, 120);
        assert_eq!(config.schedule_poll_min, 10);
        assert!(!config.notify_on_live);
        assert!(!config.notify_on_category);
    }

    #[test]
    fn serialize_roundtrip() {
        let original = Config {
            poll_interval_sec: 90,
            schedule_poll_min: 15,
            notify_on_live: true,
            notify_on_category: false,
        };

        let json = serde_json::to_string(&original).unwrap();
        let deserialized: Config = serde_json::from_str(&json).unwrap();

        assert_eq!(deserialized.poll_interval_sec, original.poll_interval_sec);
        assert_eq!(deserialized.schedule_poll_min, original.schedule_poll_min);
        assert_eq!(deserialized.notify_on_live, original.notify_on_live);
        assert_eq!(deserialized.notify_on_category, original.notify_on_category);
    }

    #[test]
    fn deserialize_ignores_unknown_fields() {
        let json = r#"{
            "poll_interval_sec": 30,
            "unknown_field": "should be ignored",
            "another_unknown": 123
        }"#;
        // This should not panic - unknown fields should be ignored
        let config: Config = serde_json::from_str(json).unwrap();
        assert_eq!(config.poll_interval_sec, 30);
    }
}

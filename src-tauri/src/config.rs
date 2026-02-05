use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::sync::RwLock;

const APP_NAME: &str = "twitch-tray";
const CONFIG_FILE: &str = "config.json";

/// A followed category for category stream tracking
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct FollowedCategory {
    pub id: String,
    pub name: String,
}

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
    /// Maximum gap (in minutes) between refreshes to still send notifications.
    /// If the app was asleep/suspended longer than this, notifications are suppressed
    /// to avoid a flood of alerts on wake.
    #[serde(default = "default_notify_max_gap")]
    pub notify_max_gap_min: u64,
    /// Categories to follow for category-based stream listings
    #[serde(default)]
    pub followed_categories: Vec<FollowedCategory>,
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

fn default_notify_max_gap() -> u64 {
    10
}

impl Default for Config {
    fn default() -> Self {
        Self {
            poll_interval_sec: default_poll_interval(),
            schedule_poll_min: default_schedule_poll(),
            notify_on_live: default_notify_on_live(),
            notify_on_category: default_notify_on_category(),
            notify_max_gap_min: default_notify_max_gap(),
            followed_categories: Vec::new(),
        }
    }
}

/// Configuration manager
pub struct ConfigManager {
    config: RwLock<Config>,
}

impl ConfigManager {
    /// Creates a new configuration manager
    pub fn new() -> Result<Self> {
        let config_dir = dirs::config_dir()
            .context("Could not determine config directory")?
            .join(APP_NAME);

        std::fs::create_dir_all(&config_dir).context("Failed to create config directory")?;

        let config_file = config_dir.join(CONFIG_FILE);

        let config = if config_file.exists() {
            let data =
                std::fs::read_to_string(&config_file).context("Failed to read config file")?;
            serde_json::from_str(&data).unwrap_or_default()
        } else {
            Config::default()
        };

        Ok(Self {
            config: RwLock::new(config),
        })
    }

    /// Gets a copy of the current configuration
    pub fn get(&self) -> Config {
        self.config.read().unwrap().clone()
    }

    /// Updates and saves the configuration
    pub fn save(&self, config: Config) -> Result<()> {
        let config_dir = Self::config_dir()?;
        let config_file = config_dir.join(CONFIG_FILE);

        let json = serde_json::to_string_pretty(&config).context("Failed to serialize config")?;
        std::fs::write(&config_file, json).context("Failed to write config file")?;

        // Update in-memory config
        *self.config.write().unwrap() = config;

        Ok(())
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

    #[test]
    fn default_notify_max_gap_is_10() {
        let config = Config::default();
        assert_eq!(config.notify_max_gap_min, 10);
    }

    #[test]
    fn default_followed_categories_is_empty() {
        let config = Config::default();
        assert!(config.followed_categories.is_empty());
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
        assert_eq!(config.notify_max_gap_min, 10);
        assert!(config.followed_categories.is_empty());
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
            notify_max_gap_min: 15,
            followed_categories: vec![FollowedCategory {
                id: "12345".to_string(),
                name: "Just Chatting".to_string(),
            }],
        };

        let json = serde_json::to_string(&original).unwrap();
        let deserialized: Config = serde_json::from_str(&json).unwrap();

        assert_eq!(deserialized.poll_interval_sec, original.poll_interval_sec);
        assert_eq!(deserialized.schedule_poll_min, original.schedule_poll_min);
        assert_eq!(deserialized.notify_on_live, original.notify_on_live);
        assert_eq!(deserialized.notify_on_category, original.notify_on_category);
        assert_eq!(deserialized.notify_max_gap_min, original.notify_max_gap_min);
        assert_eq!(
            deserialized.followed_categories,
            original.followed_categories
        );
    }

    #[test]
    fn deserialize_with_categories() {
        let json = r#"{
            "followed_categories": [
                {"id": "509658", "name": "Just Chatting"},
                {"id": "27471", "name": "Minecraft"}
            ]
        }"#;
        let config: Config = serde_json::from_str(json).unwrap();

        assert_eq!(config.followed_categories.len(), 2);
        assert_eq!(config.followed_categories[0].id, "509658");
        assert_eq!(config.followed_categories[0].name, "Just Chatting");
        assert_eq!(config.followed_categories[1].id, "27471");
        assert_eq!(config.followed_categories[1].name, "Minecraft");
    }

    #[test]
    fn followed_category_equality() {
        let cat1 = FollowedCategory {
            id: "123".to_string(),
            name: "Test".to_string(),
        };
        let cat2 = FollowedCategory {
            id: "123".to_string(),
            name: "Test".to_string(),
        };
        let cat3 = FollowedCategory {
            id: "456".to_string(),
            name: "Test".to_string(),
        };

        assert_eq!(cat1, cat2);
        assert_ne!(cat1, cat3);
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

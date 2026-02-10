use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::RwLock;

const APP_NAME: &str = "twitch-tray";
const CONFIG_FILE: &str = "config.json";

/// Importance level for a streamer, affecting display and notifications
#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum StreamerImportance {
    Favourite,
    #[default]
    Normal,
    Silent,
    Ignore,
}

/// Per-streamer settings
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct StreamerSettings {
    pub display_name: String,
    #[serde(default)]
    pub importance: StreamerImportance,
}

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
    #[serde(default = "default_notify_on_live")]
    pub notify_on_live: bool,
    #[serde(default = "default_notify_on_category")]
    pub notify_on_category: bool,
    /// Maximum gap (in minutes) between refreshes to still send notifications.
    /// If the app was asleep/suspended longer than this, notifications are suppressed
    /// to avoid a flood of alerts on wake.
    #[serde(default = "default_notify_max_gap")]
    pub notify_max_gap_min: u64,
    /// How many hours before a schedule entry is considered stale and re-fetched
    #[serde(default = "default_schedule_stale_hours")]
    pub schedule_stale_hours: u64,
    /// How often (in seconds) the schedule queue walker checks the next broadcaster
    #[serde(default = "default_schedule_check_interval")]
    pub schedule_check_interval_sec: u64,
    /// How often (in minutes) to refresh the followed channels list from the API
    #[serde(default = "default_followed_refresh")]
    pub followed_refresh_min: u64,
    /// Categories to follow for category-based stream listings
    #[serde(default)]
    pub followed_categories: Vec<FollowedCategory>,
    /// Per-streamer settings (keyed by user_login)
    #[serde(default)]
    pub streamer_settings: HashMap<String, StreamerSettings>,
}

fn default_poll_interval() -> u64 {
    60
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

fn default_schedule_stale_hours() -> u64 {
    24
}

fn default_schedule_check_interval() -> u64 {
    10
}

fn default_followed_refresh() -> u64 {
    15
}

impl Default for Config {
    fn default() -> Self {
        Self {
            poll_interval_sec: default_poll_interval(),
            notify_on_live: default_notify_on_live(),
            notify_on_category: default_notify_on_category(),
            notify_max_gap_min: default_notify_max_gap(),
            schedule_stale_hours: default_schedule_stale_hours(),
            schedule_check_interval_sec: default_schedule_check_interval(),
            followed_refresh_min: default_followed_refresh(),
            followed_categories: Vec::new(),
            streamer_settings: HashMap::new(),
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
    fn default_schedule_stale_hours_is_24() {
        let config = Config::default();
        assert_eq!(config.schedule_stale_hours, 24);
    }

    #[test]
    fn default_schedule_check_interval_is_10() {
        let config = Config::default();
        assert_eq!(config.schedule_check_interval_sec, 10);
    }

    #[test]
    fn default_followed_refresh_is_15() {
        let config = Config::default();
        assert_eq!(config.followed_refresh_min, 15);
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

    #[test]
    fn default_streamer_settings_is_empty() {
        let config = Config::default();
        assert!(config.streamer_settings.is_empty());
    }

    #[test]
    fn default_streamer_importance_is_normal() {
        assert_eq!(StreamerImportance::default(), StreamerImportance::Normal);
    }

    // === Partial deserialization tests ===

    #[test]
    fn deserialize_empty_uses_defaults() {
        let json = "{}";
        let config: Config = serde_json::from_str(json).unwrap();

        assert_eq!(config.poll_interval_sec, 60);
        assert!(config.notify_on_live);
        assert!(config.notify_on_category);
        assert_eq!(config.notify_max_gap_min, 10);
        assert_eq!(config.schedule_stale_hours, 24);
        assert_eq!(config.schedule_check_interval_sec, 10);
        assert_eq!(config.followed_refresh_min, 15);
        assert!(config.followed_categories.is_empty());
        assert!(config.streamer_settings.is_empty());
    }

    #[test]
    fn deserialize_partial_uses_defaults_for_missing() {
        let json = r#"{"poll_interval_sec": 30}"#;
        let config: Config = serde_json::from_str(json).unwrap();

        assert_eq!(config.poll_interval_sec, 30); // Overridden
        assert!(config.notify_on_live); // Default
        assert!(config.notify_on_category); // Default
    }

    #[test]
    fn deserialize_full_config() {
        let json = r#"{
            "poll_interval_sec": 120,
            "notify_on_live": false,
            "notify_on_category": false,
            "schedule_stale_hours": 48,
            "schedule_check_interval_sec": 30,
            "followed_refresh_min": 30
        }"#;
        let config: Config = serde_json::from_str(json).unwrap();

        assert_eq!(config.poll_interval_sec, 120);
        assert!(!config.notify_on_live);
        assert!(!config.notify_on_category);
        assert_eq!(config.schedule_stale_hours, 48);
        assert_eq!(config.schedule_check_interval_sec, 30);
        assert_eq!(config.followed_refresh_min, 30);
    }

    #[test]
    fn serialize_roundtrip() {
        let mut streamer_settings = HashMap::new();
        streamer_settings.insert(
            "teststreamer".to_string(),
            StreamerSettings {
                display_name: "TestStreamer".to_string(),
                importance: StreamerImportance::Favourite,
            },
        );

        let original = Config {
            poll_interval_sec: 90,
            notify_on_live: true,
            notify_on_category: false,
            notify_max_gap_min: 15,
            schedule_stale_hours: 48,
            schedule_check_interval_sec: 20,
            followed_refresh_min: 30,
            followed_categories: vec![FollowedCategory {
                id: "12345".to_string(),
                name: "Just Chatting".to_string(),
            }],
            streamer_settings,
        };

        let json = serde_json::to_string(&original).unwrap();
        let deserialized: Config = serde_json::from_str(&json).unwrap();

        assert_eq!(deserialized.poll_interval_sec, original.poll_interval_sec);
        assert_eq!(deserialized.notify_on_live, original.notify_on_live);
        assert_eq!(deserialized.notify_on_category, original.notify_on_category);
        assert_eq!(deserialized.notify_max_gap_min, original.notify_max_gap_min);
        assert_eq!(
            deserialized.schedule_stale_hours,
            original.schedule_stale_hours
        );
        assert_eq!(
            deserialized.schedule_check_interval_sec,
            original.schedule_check_interval_sec
        );
        assert_eq!(
            deserialized.followed_refresh_min,
            original.followed_refresh_min
        );
        assert_eq!(
            deserialized.followed_categories,
            original.followed_categories
        );
        assert_eq!(deserialized.streamer_settings, original.streamer_settings);
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
    fn deserialize_with_streamer_settings() {
        let json = r#"{
            "streamer_settings": {
                "ninja": {"display_name": "Ninja", "importance": "favourite"},
                "shroud": {"display_name": "Shroud", "importance": "silent"}
            }
        }"#;
        let config: Config = serde_json::from_str(json).unwrap();

        assert_eq!(config.streamer_settings.len(), 2);
        assert_eq!(
            config.streamer_settings.get("ninja").unwrap().importance,
            StreamerImportance::Favourite
        );
        assert_eq!(
            config.streamer_settings.get("shroud").unwrap().importance,
            StreamerImportance::Silent
        );
    }

    #[test]
    fn streamer_settings_default_importance() {
        let json = r#"{
            "streamer_settings": {
                "ninja": {"display_name": "Ninja"}
            }
        }"#;
        let config: Config = serde_json::from_str(json).unwrap();

        assert_eq!(
            config.streamer_settings.get("ninja").unwrap().importance,
            StreamerImportance::Normal
        );
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

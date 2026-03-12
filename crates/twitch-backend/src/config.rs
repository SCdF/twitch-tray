use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::RwLock;

const APP_NAME: &str = "twitch-tray";
const CONFIG_FILE: &str = "config.json";

// Default values as named constants — referenceable from tests and other modules
pub const DEFAULT_POLL_INTERVAL_SEC: u64 = 60;
pub const DEFAULT_NOTIFY_ON_LIVE: bool = true;
pub const DEFAULT_NOTIFY_ON_CATEGORY: bool = true;
pub const DEFAULT_NOTIFY_MAX_GAP_MIN: u64 = 10;
pub const DEFAULT_SCHEDULE_STALE_HOURS: u64 = 24;
pub const DEFAULT_SCHEDULE_CHECK_INTERVAL_SEC: u64 = 10;
pub const DEFAULT_FOLLOWED_REFRESH_MIN: u64 = 15;
pub const DEFAULT_SCHEDULE_LOOKAHEAD_HOURS: u64 = 6;
pub const DEFAULT_SCHEDULE_BEFORE_NOW_MIN: u64 = 30;
pub const DEFAULT_LIVE_MENU_LIMIT: usize = 10;
pub const DEFAULT_SCHEDULE_MENU_LIMIT: usize = 5;
pub const DEFAULT_HOTNESS_Z_THRESHOLD: f64 = 2.0;
pub const DEFAULT_HOTNESS_MIN_OBSERVATIONS: usize = 5;
pub const DEFAULT_NOTIFY_ON_HOT: bool = true;

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
    #[serde(default)]
    pub hotness_z_threshold_override: Option<f64>,
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
    /// How many hours ahead to show in the schedule section
    #[serde(default = "default_schedule_lookahead")]
    pub schedule_lookahead_hours: u64,
    /// Minutes before now to include in the schedule window.
    /// Grace period so recently-started schedules still show if the streamer hasn't gone live yet.
    #[serde(default = "default_schedule_before_now")]
    pub schedule_before_now_min: u64,
    /// Maximum live streams shown directly in the main menu before the overflow submenu.
    #[serde(default = "default_live_menu_limit")]
    pub live_menu_limit: usize,
    /// Maximum scheduled streams shown directly in the main menu before the overflow submenu.
    #[serde(default = "default_schedule_menu_limit")]
    pub schedule_menu_limit: usize,
    /// Z-score threshold for detecting "hot" streams (default: 2.0).
    /// A stream is hot when its current viewers exceed the historical mean by this many
    /// standard deviations.
    #[serde(default = "default_hotness_z_threshold")]
    pub hotness_z_threshold: f64,
    /// Minimum historical observations needed before hotness detection activates (default: 5)
    #[serde(default = "default_hotness_min_observations")]
    pub hotness_min_observations: usize,
    /// Send desktop notifications when a stream is detected as hot (default: true)
    #[serde(default = "default_notify_on_hot")]
    pub notify_on_hot: bool,
    /// Categories to follow for category-based stream listings
    #[serde(default)]
    pub followed_categories: Vec<FollowedCategory>,
    /// Per-streamer settings (keyed by user_login)
    #[serde(default)]
    pub streamer_settings: HashMap<String, StreamerSettings>,
}

fn default_poll_interval() -> u64 {
    DEFAULT_POLL_INTERVAL_SEC
}

fn default_notify_on_live() -> bool {
    DEFAULT_NOTIFY_ON_LIVE
}

fn default_notify_on_category() -> bool {
    DEFAULT_NOTIFY_ON_CATEGORY
}

fn default_notify_max_gap() -> u64 {
    DEFAULT_NOTIFY_MAX_GAP_MIN
}

fn default_schedule_stale_hours() -> u64 {
    DEFAULT_SCHEDULE_STALE_HOURS
}

fn default_schedule_check_interval() -> u64 {
    DEFAULT_SCHEDULE_CHECK_INTERVAL_SEC
}

fn default_followed_refresh() -> u64 {
    DEFAULT_FOLLOWED_REFRESH_MIN
}

fn default_schedule_lookahead() -> u64 {
    DEFAULT_SCHEDULE_LOOKAHEAD_HOURS
}

fn default_schedule_before_now() -> u64 {
    DEFAULT_SCHEDULE_BEFORE_NOW_MIN
}

fn default_live_menu_limit() -> usize {
    DEFAULT_LIVE_MENU_LIMIT
}

fn default_schedule_menu_limit() -> usize {
    DEFAULT_SCHEDULE_MENU_LIMIT
}

fn default_hotness_z_threshold() -> f64 {
    DEFAULT_HOTNESS_Z_THRESHOLD
}

fn default_hotness_min_observations() -> usize {
    DEFAULT_HOTNESS_MIN_OBSERVATIONS
}

fn default_notify_on_hot() -> bool {
    DEFAULT_NOTIFY_ON_HOT
}

impl Default for Config {
    fn default() -> Self {
        Self {
            poll_interval_sec: DEFAULT_POLL_INTERVAL_SEC,
            notify_on_live: DEFAULT_NOTIFY_ON_LIVE,
            notify_on_category: DEFAULT_NOTIFY_ON_CATEGORY,
            notify_max_gap_min: DEFAULT_NOTIFY_MAX_GAP_MIN,
            schedule_stale_hours: DEFAULT_SCHEDULE_STALE_HOURS,
            schedule_check_interval_sec: DEFAULT_SCHEDULE_CHECK_INTERVAL_SEC,
            followed_refresh_min: DEFAULT_FOLLOWED_REFRESH_MIN,
            schedule_lookahead_hours: DEFAULT_SCHEDULE_LOOKAHEAD_HOURS,
            schedule_before_now_min: DEFAULT_SCHEDULE_BEFORE_NOW_MIN,
            live_menu_limit: DEFAULT_LIVE_MENU_LIMIT,
            schedule_menu_limit: DEFAULT_SCHEDULE_MENU_LIMIT,
            hotness_z_threshold: DEFAULT_HOTNESS_Z_THRESHOLD,
            hotness_min_observations: DEFAULT_HOTNESS_MIN_OBSERVATIONS,
            notify_on_hot: DEFAULT_NOTIFY_ON_HOT,
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
        self.config
            .read()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone()
    }

    /// Updates and saves the configuration
    pub fn save(&self, config: Config) -> Result<()> {
        let config_dir = Self::config_dir()?;
        let config_file = config_dir.join(CONFIG_FILE);

        let json = serde_json::to_string_pretty(&config).context("Failed to serialize config")?;
        std::fs::write(&config_file, json).context("Failed to write config file")?;

        // Update in-memory config
        *self
            .config
            .write()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = config;

        Ok(())
    }

    /// Returns the config directory path
    pub fn config_dir() -> Result<PathBuf> {
        Ok(dirs::config_dir()
            .context("Could not determine config directory")?
            .join(APP_NAME))
    }

    /// Creates a `ConfigManager` pre-loaded with the given config (no disk I/O).
    /// Only available in tests.
    #[cfg(test)]
    pub fn with_config(config: Config) -> Self {
        Self {
            config: RwLock::new(config),
        }
    }

    /// Overwrites the in-memory config without writing to disk.
    /// Only available in tests.
    #[cfg(test)]
    pub fn set(&self, config: Config) {
        *self.config.write().unwrap_or_else(|e| e.into_inner()) = config;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // === Config default values tests ===

    #[test]
    fn default_poll_interval_is_60() {
        let config = Config::default();
        assert_eq!(config.poll_interval_sec, DEFAULT_POLL_INTERVAL_SEC);
    }

    #[test]
    fn default_schedule_stale_hours_is_24() {
        let config = Config::default();
        assert_eq!(config.schedule_stale_hours, DEFAULT_SCHEDULE_STALE_HOURS);
    }

    #[test]
    fn default_schedule_check_interval_is_10() {
        let config = Config::default();
        assert_eq!(
            config.schedule_check_interval_sec,
            DEFAULT_SCHEDULE_CHECK_INTERVAL_SEC
        );
    }

    #[test]
    fn default_followed_refresh_is_15() {
        let config = Config::default();
        assert_eq!(config.followed_refresh_min, DEFAULT_FOLLOWED_REFRESH_MIN);
    }

    #[test]
    fn default_schedule_lookahead_is_6() {
        let config = Config::default();
        assert_eq!(
            config.schedule_lookahead_hours,
            DEFAULT_SCHEDULE_LOOKAHEAD_HOURS
        );
    }

    #[test]
    fn default_schedule_before_now_is_30() {
        let config = Config::default();
        assert_eq!(
            config.schedule_before_now_min,
            DEFAULT_SCHEDULE_BEFORE_NOW_MIN
        );
    }

    #[test]
    fn default_notify_on_live_is_true() {
        let config = Config::default();
        assert_eq!(config.notify_on_live, DEFAULT_NOTIFY_ON_LIVE);
    }

    #[test]
    fn default_notify_on_category_is_true() {
        let config = Config::default();
        assert_eq!(config.notify_on_category, DEFAULT_NOTIFY_ON_CATEGORY);
    }

    #[test]
    fn default_notify_max_gap_is_10() {
        let config = Config::default();
        assert_eq!(config.notify_max_gap_min, DEFAULT_NOTIFY_MAX_GAP_MIN);
    }

    #[test]
    fn default_live_menu_limit_is_10() {
        let config = Config::default();
        assert_eq!(config.live_menu_limit, DEFAULT_LIVE_MENU_LIMIT);
    }

    #[test]
    fn default_schedule_menu_limit_is_5() {
        let config = Config::default();
        assert_eq!(config.schedule_menu_limit, DEFAULT_SCHEDULE_MENU_LIMIT);
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

        assert_eq!(config.poll_interval_sec, DEFAULT_POLL_INTERVAL_SEC);
        assert_eq!(config.notify_on_live, DEFAULT_NOTIFY_ON_LIVE);
        assert_eq!(config.notify_on_category, DEFAULT_NOTIFY_ON_CATEGORY);
        assert_eq!(config.notify_max_gap_min, DEFAULT_NOTIFY_MAX_GAP_MIN);
        assert_eq!(config.schedule_stale_hours, DEFAULT_SCHEDULE_STALE_HOURS);
        assert_eq!(
            config.schedule_check_interval_sec,
            DEFAULT_SCHEDULE_CHECK_INTERVAL_SEC
        );
        assert_eq!(config.followed_refresh_min, DEFAULT_FOLLOWED_REFRESH_MIN);
        assert_eq!(
            config.schedule_lookahead_hours,
            DEFAULT_SCHEDULE_LOOKAHEAD_HOURS
        );
        assert_eq!(
            config.schedule_before_now_min,
            DEFAULT_SCHEDULE_BEFORE_NOW_MIN
        );
        assert_eq!(config.live_menu_limit, DEFAULT_LIVE_MENU_LIMIT);
        assert_eq!(config.schedule_menu_limit, DEFAULT_SCHEDULE_MENU_LIMIT);
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
                hotness_z_threshold_override: None,
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
            schedule_lookahead_hours: 12,
            schedule_before_now_min: 20,
            live_menu_limit: 7,
            schedule_menu_limit: 3,
            hotness_z_threshold: 3.0,
            hotness_min_observations: 10,
            notify_on_hot: false,
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
        assert_eq!(
            deserialized.schedule_lookahead_hours,
            original.schedule_lookahead_hours
        );
        assert_eq!(
            deserialized.schedule_before_now_min,
            original.schedule_before_now_min
        );
        assert_eq!(deserialized.live_menu_limit, original.live_menu_limit);
        assert_eq!(
            deserialized.schedule_menu_limit,
            original.schedule_menu_limit
        );
        assert!(
            (deserialized.hotness_z_threshold - original.hotness_z_threshold).abs() < f64::EPSILON
        );
        assert_eq!(
            deserialized.hotness_min_observations,
            original.hotness_min_observations
        );
        assert_eq!(deserialized.notify_on_hot, original.notify_on_hot);
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

    // === Hotness config tests ===

    #[test]
    fn default_hotness_z_threshold_is_2() {
        let config = Config::default();
        assert!((config.hotness_z_threshold - DEFAULT_HOTNESS_Z_THRESHOLD).abs() < f64::EPSILON);
    }

    #[test]
    fn default_hotness_min_observations_is_5() {
        let config = Config::default();
        assert_eq!(
            config.hotness_min_observations,
            DEFAULT_HOTNESS_MIN_OBSERVATIONS
        );
    }

    #[test]
    fn default_notify_on_hot_is_true() {
        let config = Config::default();
        assert_eq!(config.notify_on_hot, DEFAULT_NOTIFY_ON_HOT);
    }

    #[test]
    fn deserialize_empty_uses_hotness_defaults() {
        let json = "{}";
        let config: Config = serde_json::from_str(json).unwrap();
        assert!((config.hotness_z_threshold - DEFAULT_HOTNESS_Z_THRESHOLD).abs() < f64::EPSILON);
        assert_eq!(
            config.hotness_min_observations,
            DEFAULT_HOTNESS_MIN_OBSERVATIONS
        );
        assert_eq!(config.notify_on_hot, DEFAULT_NOTIFY_ON_HOT);
    }

    #[test]
    fn deserialize_with_hotness_settings() {
        let json = r#"{
            "hotness_z_threshold": 3.5,
            "hotness_min_observations": 10,
            "notify_on_hot": false
        }"#;
        let config: Config = serde_json::from_str(json).unwrap();
        assert!((config.hotness_z_threshold - 3.5).abs() < f64::EPSILON);
        assert_eq!(config.hotness_min_observations, 10);
        assert!(!config.notify_on_hot);
    }

    #[test]
    fn streamer_hotness_threshold_override_deserialized() {
        let json = r#"{
            "streamer_settings": {
                "ninja": {"display_name": "Ninja", "hotness_z_threshold_override": 3.0}
            }
        }"#;
        let config: Config = serde_json::from_str(json).unwrap();
        let settings = config.streamer_settings.get("ninja").unwrap();
        assert_eq!(settings.hotness_z_threshold_override, Some(3.0));
    }

    #[test]
    fn streamer_hotness_threshold_override_defaults_to_none() {
        let json = r#"{
            "streamer_settings": {
                "ninja": {"display_name": "Ninja"}
            }
        }"#;
        let config: Config = serde_json::from_str(json).unwrap();
        let settings = config.streamer_settings.get("ninja").unwrap();
        assert_eq!(settings.hotness_z_threshold_override, None);
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

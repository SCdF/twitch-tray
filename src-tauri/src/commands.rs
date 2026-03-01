use std::sync::Arc;

use tauri::State;

use crate::app_services::{AppServices, DebugStreamEntry};
use crate::config::{Config, FollowedCategory};
use crate::twitch::{Category, FollowedChannel};

/// Gets the current configuration.
#[tauri::command]
pub fn get_config(app: State<'_, Arc<dyn AppServices>>) -> Config {
    app.get_config()
}

/// Saves the configuration and triggers an immediate data refresh.
#[tauri::command]
pub async fn save_config(
    app: State<'_, Arc<dyn AppServices>>,
    config: Config,
) -> Result<(), String> {
    app.save_config(config).await.map_err(|e| e.to_string())
}

/// Searches for categories by name.
#[tauri::command]
pub async fn search_categories(
    app: State<'_, Arc<dyn AppServices>>,
    query: String,
) -> Result<Vec<Category>, String> {
    app.search_categories(&query)
        .await
        .map_err(|e| e.to_string())
}

/// Gets the followed categories from config.
#[tauri::command]
pub fn get_followed_categories(app: State<'_, Arc<dyn AppServices>>) -> Vec<FollowedCategory> {
    app.get_followed_categories()
}

/// Gets the list of followed channels from state.
#[tauri::command]
pub async fn get_followed_channels_list(
    app: State<'_, Arc<dyn AppServices>>,
) -> Result<Vec<FollowedChannel>, String> {
    Ok(app.get_followed_channels().await)
}

/// Returns true when the binary was compiled with debug assertions enabled.
///
/// The frontend uses this to decide whether to show the Debug tab.
#[tauri::command]
pub fn is_debug_build() -> bool {
    cfg!(debug_assertions)
}

/// Returns raw history and inferred schedule entries for the given Unix timestamp window.
#[tauri::command]
pub async fn get_debug_schedule_data(
    app: State<'_, Arc<dyn AppServices>>,
    start: i64,
    end: i64,
) -> Result<Vec<DebugStreamEntry>, String> {
    Ok(app.get_debug_schedule_data(start, end).await)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app_services::mock::MockAppServices;
    use crate::config::DEFAULT_POLL_INTERVAL_SEC;
    use crate::twitch::Category;

    // =========================================================
    // get_config
    // =========================================================

    #[test]
    fn get_config_returns_default_config() {
        let services = MockAppServices::new();
        let config = services.get_config();
        assert_eq!(config.poll_interval_sec, DEFAULT_POLL_INTERVAL_SEC);
    }

    // =========================================================
    // save_config
    // =========================================================

    #[tokio::test]
    async fn save_config_persists_new_values() {
        let services = MockAppServices::new();
        let mut config = Config::default();
        config.poll_interval_sec = 120;
        services.save_config(config).await.unwrap();
        assert_eq!(services.get_config().poll_interval_sec, 120);
    }

    #[tokio::test]
    async fn save_config_triggers_both_refreshes() {
        let services = MockAppServices::new();
        services.save_config(Config::default()).await.unwrap();
        assert_eq!(
            services.refresh_category_count(),
            1,
            "save_config must trigger refresh_category_streams"
        );
        assert_eq!(
            services.refresh_schedules_count(),
            1,
            "save_config must trigger refresh_schedules_from_db"
        );
    }

    #[tokio::test]
    async fn save_config_increments_call_counter() {
        let services = MockAppServices::new();
        services.save_config(Config::default()).await.unwrap();
        services.save_config(Config::default()).await.unwrap();
        assert_eq!(services.save_config_count(), 2);
    }

    // =========================================================
    // search_categories
    // =========================================================

    #[tokio::test]
    async fn search_categories_returns_configured_results() {
        let services = MockAppServices::new();
        let cat = Category {
            id: "123".to_string(),
            name: "Fortnite".to_string(),
            box_art_url: String::new(),
        };
        services.set_search_results(vec![cat.clone()]);
        let results = services.search_categories("fort").await.unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].name, "Fortnite");
    }

    #[tokio::test]
    async fn search_categories_returns_empty_by_default() {
        let services = MockAppServices::new();
        let results = services.search_categories("anything").await.unwrap();
        assert!(results.is_empty());
    }

    // =========================================================
    // get_followed_categories
    // =========================================================

    #[test]
    fn get_followed_categories_returns_config_value() {
        let services = MockAppServices::new();
        // Default config has no followed categories
        let cats = services.get_followed_categories();
        assert!(cats.is_empty());
    }

    // =========================================================
    // get_followed_channels
    // =========================================================

    #[tokio::test]
    async fn get_followed_channels_returns_configured_channels() {
        let services = MockAppServices::new();
        let channel = FollowedChannel {
            broadcaster_id: "1".to_string(),
            broadcaster_login: "streamer".to_string(),
            broadcaster_name: "Streamer".to_string(),
            followed_at: chrono::Utc::now(),
        };
        services.set_channels(vec![channel]);
        let channels = services.get_followed_channels().await;
        assert_eq!(channels.len(), 1);
        assert_eq!(channels[0].broadcaster_login, "streamer");
    }

    // =========================================================
    // get_debug_schedule_data
    // =========================================================

    #[tokio::test]
    async fn debug_schedule_data_delegates_to_services() {
        use crate::app_services::DebugStreamEntry;

        let services = MockAppServices::new();
        let entry = DebugStreamEntry {
            is_inferred: false,
            broadcaster_name: "TestStreamer".to_string(),
            broadcaster_login: "teststreamer".to_string(),
            started_at: 1_000_000,
        };
        services.set_debug_entries(vec![entry]);
        let result = services.get_debug_schedule_data(0, 2_000_000).await;
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].broadcaster_login, "teststreamer");
        assert!(!result[0].is_inferred);
    }

    #[tokio::test]
    async fn debug_schedule_data_increments_call_count() {
        let services = MockAppServices::new();
        services.get_debug_schedule_data(100, 200).await;
        services.get_debug_schedule_data(200, 300).await;
        assert_eq!(services.debug_call_count(), 2);
    }
}

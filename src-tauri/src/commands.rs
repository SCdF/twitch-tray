use std::sync::Arc;

use tauri::State;

use crate::app::App;
use crate::config::{Config, FollowedCategory};
use crate::twitch::{Category, FollowedChannel};

/// Gets the current configuration
#[tauri::command]
pub fn get_config(app: State<'_, Arc<App>>) -> Config {
    app.config.get()
}

/// Saves the configuration and triggers an immediate refresh
#[tauri::command]
pub async fn save_config(app: State<'_, Arc<App>>, config: Config) -> Result<(), String> {
    // Save the config
    app.config.save(config).map_err(|e| e.to_string())?;

    // Refresh data — state changes trigger menu rebuild via listener
    app.refresh_category_streams().await;
    app.refresh_schedules_from_db().await;

    Ok(())
}

/// Searches for categories by name
#[tauri::command]
pub async fn search_categories(
    app: State<'_, Arc<App>>,
    query: String,
) -> Result<Vec<Category>, String> {
    app.client
        .search_categories(&query)
        .await
        .map_err(|e| e.to_string())
}

/// Gets the followed categories from config
#[tauri::command]
pub fn get_followed_categories(app: State<'_, Arc<App>>) -> Vec<FollowedCategory> {
    app.config.get().followed_categories
}

/// Gets the list of followed channels from state
#[tauri::command]
pub async fn get_followed_channels_list(
    app: State<'_, Arc<App>>,
) -> Result<Vec<FollowedChannel>, String> {
    Ok(app.state.get_followed_channels().await)
}

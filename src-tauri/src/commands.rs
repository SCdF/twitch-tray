use std::sync::Arc;

use tauri::{AppHandle, State};

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
pub async fn save_config(
    app_handle: AppHandle,
    app: State<'_, Arc<App>>,
    config: Config,
) -> Result<(), String> {
    // Save the config
    app.config.save(config.clone()).map_err(|e| e.to_string())?;

    // Refresh category streams with new config
    app.refresh_category_streams().await;

    // Rebuild menu with updated data
    let category_streams = app.state.get_category_streams().await;
    app.tray_manager
        .rebuild_menu_with_categories(
            &app_handle,
            config.followed_categories,
            category_streams,
            config.streamer_settings,
        )
        .await
        .map_err(|e| e.to_string())?;

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

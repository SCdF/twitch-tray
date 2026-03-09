use tauri::{AppHandle, Manager, WebviewWindowBuilder};

/// Width of the settings window in logical pixels
const SETTINGS_WINDOW_SIZE: f64 = 975.0;

/// Opens the settings window
pub fn open_settings_window(app: &AppHandle) {
    // Check if window already exists
    if let Some(window) = app.get_webview_window("settings") {
        let _ = window.set_focus();
        return;
    }

    // Create new settings window
    match WebviewWindowBuilder::new(app, "settings", tauri::WebviewUrl::App("index.html".into()))
        .title("Twitch Tray Settings")
        .inner_size(SETTINGS_WINDOW_SIZE, SETTINGS_WINDOW_SIZE)
        .resizable(true)
        .center()
        .build()
    {
        Ok(_) => tracing::info!("Settings window opened"),
        Err(e) => tracing::error!("Failed to open settings window: {}", e),
    }
}

/// Opens a small settings window for a specific streamer
pub fn open_streamer_settings_window(app: &AppHandle, user_login: &str, display_name: &str) {
    let window_id = format!("streamer-settings-{user_login}");

    // Focus existing window if already open
    if let Some(window) = app.get_webview_window(&window_id) {
        let _ = window.set_focus();
        return;
    }

    let url = format!("index.html?streamer={user_login}");
    let title = format!("{display_name} - Settings");

    match WebviewWindowBuilder::new(app, &window_id, tauri::WebviewUrl::App(url.into()))
        .title(&title)
        .inner_size(SETTINGS_WINDOW_SIZE, SETTINGS_WINDOW_SIZE)
        .resizable(true)
        .center()
        .build()
    {
        Ok(_) => tracing::info!("Streamer settings window opened for {}", user_login),
        Err(e) => tracing::error!("Failed to open streamer settings window: {}", e),
    }
}

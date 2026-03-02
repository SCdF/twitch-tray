// Re-export backend modules for integration tests
pub use twitch_backend::state;
pub use twitch_backend::twitch;

// Re-export menu modules for integration test helpers
pub use twitch_menu_tauri::display;
pub use twitch_menu_tauri::display_state;
pub use twitch_menu_tauri::tray;

#[cfg(test)]
mod test_helpers;

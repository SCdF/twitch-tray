// Re-export backend modules for integration tests
pub use twitch_backend::state;
pub use twitch_backend::twitch;

// Re-export menu modules so integration test helpers can access them
pub use twitch_menu_tauri::display;
pub use twitch_menu_tauri::display_state;
pub use twitch_menu_tauri::tray;

// Local wrapper for AppServices mock (cfg(test) can't cross crate boundaries)
pub mod app_services;

#[cfg(test)]
mod test_helpers;

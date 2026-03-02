// Re-export backend modules for integration tests
pub use twitch_backend::state;
pub use twitch_backend::twitch;

// Local modules that stay in this crate (move to twitch-menu-tauri in Phase 3)
pub mod app_services;
pub mod display;
pub mod display_state;
pub mod tray;

#[cfg(test)]
mod test_helpers;

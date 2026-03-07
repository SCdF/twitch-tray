// twitch-backend: pure Rust backend crate — no Tauri or GTK dependencies.

pub mod app_services;
pub mod auth;
pub mod config;
pub mod db;
pub mod events;
pub mod handle;
pub mod notification_dispatcher;
pub mod notification_filter;
pub mod notify;
pub mod schedule_inference;
pub mod schedule_walker;
pub mod session;
pub mod state;
pub mod twitch;

pub(crate) mod backend;

#[cfg(test)]
pub(crate) mod test_helpers;

// Primary public API
pub use backend::start;
pub use events::BackendEvent;
pub use handle::{AuthCommand, BackendHandle, LoginProgress, RawDisplayData};

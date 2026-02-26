// Library entry point for integration tests
// This exposes modules for testing while keeping main.rs as the binary entry point

pub mod app_services;
pub mod auth;
pub mod config;
pub mod display;
pub mod display_state;
pub mod notification_dispatcher;
pub mod notification_filter;
pub mod notify;
pub mod state;
pub mod tray;
pub mod twitch;

#[cfg(test)]
mod test_helpers;

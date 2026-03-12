// twitch-menu-tauri: Tauri system tray menu implementation.
// Depends on twitch-backend for domain types; provides the display layer.

pub mod display;
pub mod display_state;
pub mod tray;

#[cfg(test)]
mod test_helpers;

use chrono::Utc;
use std::sync::Arc;
use tokio::sync::watch;
use twitch_backend::handle::RawDisplayData;

use crate::display::DisplayBackend;
use crate::display_state::{compute_display_state, DisplayConfig, DisplayState};
use crate::tray::TrayBackend;

/// Starts the display listener task.
///
/// Subscribes to `display_rx` (a watch channel of `RawDisplayData`), converts
/// each snapshot into a `DisplayState`, and calls `tray_backend.update()`.
/// Returns a `JoinHandle` so the caller can manage the task lifetime.
pub fn start_listener(
    mut display_rx: watch::Receiver<RawDisplayData>,
    tray_backend: Arc<TrayBackend>,
) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        while display_rx.changed().await.is_ok() {
            let raw = display_rx.borrow().clone();
            let display_config = DisplayConfig {
                streamer_settings: raw.config.streamer_settings.clone(),
                schedule_lookahead_hours: raw.config.schedule_lookahead_hours,
                live_limit: raw.config.live_menu_limit,
                schedule_limit: raw.config.schedule_menu_limit,
                hot_stream_ids: raw.hot_stream_ids.clone(),
            };
            let state = if raw.is_authenticated {
                compute_display_state(
                    raw.live_streams,
                    raw.scheduled_streams,
                    raw.schedules_loaded,
                    &raw.followed_categories,
                    &raw.category_streams,
                    &display_config,
                    Utc::now(),
                )
            } else {
                DisplayState::unauthenticated()
            };
            if let Err(e) = tray_backend.update(state) {
                tracing::error!("Failed to update tray: {}", e);
            }
        }
    })
}

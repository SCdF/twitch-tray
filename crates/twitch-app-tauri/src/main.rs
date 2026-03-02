// Prevents additional console window on Windows in release
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod app_services;
mod commands;

#[cfg(test)]
mod test_helpers;

use std::sync::Arc;
use tauri::{Listener, Manager};
use tokio::sync::mpsc;
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt, EnvFilter};

use twitch_backend::{AuthCommand, BackendEvent};
use twitch_menu_tauri::display::DisplayBackend;
use twitch_menu_tauri::display_state::DisplayState;
use twitch_menu_tauri::tray::{handle_menu_event, open_streamer_settings_window, TrayBackend};

fn main() {
    // Initialize logging
    tracing_subscriber::registry()
        .with(EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")))
        .with(tracing_subscriber::fmt::layer())
        .init();

    tracing::info!("Starting Twitch Tray");

    // Build the Tauri application
    tauri::Builder::default()
        .invoke_handler(tauri::generate_handler![
            commands::get_config,
            commands::save_config,
            commands::search_categories,
            commands::get_followed_categories,
            commands::get_followed_channels_list,
            commands::is_debug_build,
            commands::get_debug_schedule_data,
        ])
        .setup(|app| {
            // Start the backend (spawns all polling/notification tasks)
            let handle = twitch_backend::start().expect("Failed to start backend");

            // Store services for Tauri commands
            app.manage(handle.services);

            // Store auth sender so the run() callback can route login/logout
            app.manage(handle.auth_cmd_tx);

            // Create the tray backend (holds AppHandle — only Tauri-coupled display type)
            let tray_backend = Arc::new(TrayBackend::new(app.handle().clone()));

            // Create the tray icon
            let tray = tray_backend
                .create_tray()
                .expect("Failed to create tray icon");

            // Set initial menu (unauthenticated state — no network needed)
            if let Err(e) = tray_backend.update(DisplayState::unauthenticated()) {
                tracing::error!("Failed to build initial menu: {}", e);
            }

            // Set up menu event handler
            tray.on_menu_event(|app, event| {
                handle_menu_event(app, event.id().as_ref());
            });

            // Start display listener: converts RawDisplayData → DisplayState → tray update
            twitch_menu_tauri::start_listener(handle.display_rx, tray_backend);

            // Event listener: open streamer settings window on request
            let mut event_rx = handle.event_tx.subscribe();
            let app_handle_for_events = app.handle().clone();
            tauri::async_runtime::spawn(async move {
                while let Ok(event) = event_rx.recv().await {
                    if let BackendEvent::OpenSettingsRequested {
                        user_login,
                        display_name,
                    } = event
                    {
                        open_streamer_settings_window(
                            &app_handle_for_events,
                            &user_login,
                            &display_name,
                        );
                    }
                }
            });

            Ok(())
        })
        .on_window_event(|_window, _event| {
            // Let windows close normally — ExitRequested handler prevents app exit
        })
        .build(tauri::generate_context!())
        .expect("Failed to build Tauri application")
        .run(|app, event| {
            // Prevent app from exiting when all windows are closed (tray app),
            // but allow programmatic exit (e.g. Quit button calls app.exit(0))
            if let tauri::RunEvent::ExitRequested { ref api, code, .. } = event {
                if code.is_none() {
                    api.prevent_exit();
                }
            }

            // Wire login / logout menu events to backend auth commands
            if let tauri::RunEvent::Ready = event {
                let app_handle = app.clone();
                app.listen("login-requested", move |_| {
                    if let Some(tx) = app_handle.try_state::<mpsc::UnboundedSender<AuthCommand>>() {
                        let _ = tx.send(AuthCommand::Login);
                    }
                });

                let app_handle2 = app.clone();
                app.listen("logout-requested", move |_| {
                    if let Some(tx) = app_handle2.try_state::<mpsc::UnboundedSender<AuthCommand>>()
                    {
                        let _ = tx.send(AuthCommand::Logout);
                    }
                });
            }
        });
}

// Prevents additional console window on Windows in release
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod app;
mod auth;
mod config;
mod notify;
mod state;
mod time;
mod tray;
mod twitch;

#[cfg(test)]
mod testutil;

use std::sync::Arc;
use tauri::{Listener, Manager};
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt, EnvFilter};

use crate::app::App;
use crate::tray::handle_menu_event;

fn main() {
    // Initialize logging
    tracing_subscriber::registry()
        .with(EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")))
        .with(tracing_subscriber::fmt::layer())
        .init();

    tracing::info!("Starting Twitch Tray");

    // Build the Tauri application
    tauri::Builder::default()
        .plugin(tauri_plugin_shell::init())
        .setup(|app| {
            // Create the application state
            let application = Arc::new(App::new().expect("Failed to initialize application"));

            // Store the app in Tauri state for event handlers
            app.manage(application.clone());

            // Create the tray icon
            let tray = application
                .tray_manager
                .create_tray(app.handle())
                .expect("Failed to create tray icon");

            // Set up menu event handler
            tray.on_menu_event(|app, event| {
                handle_menu_event(app, event.id().as_ref());
            });

            // Clone handles for async setup
            let app_handle = app.handle().clone();
            let app_clone = application.clone();

            // Spawn async initialization
            tauri::async_runtime::spawn(async move {
                // Set initial menu immediately (unauthenticated state - no network needed)
                if let Err(e) = app_clone.tray_manager.rebuild_menu(&app_handle).await {
                    tracing::error!("Failed to build initial menu: {}", e);
                }

                // Try to restore session (may involve token refresh over network)
                match app_clone.restore_session().await {
                    Ok(()) => {
                        tracing::info!("Session restored");
                        // Rebuild menu to show authenticated state
                        if let Err(e) = app_clone.tray_manager.rebuild_menu(&app_handle).await {
                            tracing::error!("Failed to rebuild menu after session restore: {}", e);
                        }
                    }
                    Err(e) => {
                        tracing::info!("No stored session: {}", e);
                    }
                }

                // Start polling tasks (includes state change listener for menu updates)
                app_clone.start_polling(app_handle.clone());

                // Fetch initial data in background - menu will update via state change listener
                if app_clone.state.is_authenticated().await {
                    app_clone.refresh_all_data().await;
                }
            });

            Ok(())
        })
        .on_window_event(|_window, _event| {
            // No windows in this app, but handler required
        })
        .build(tauri::generate_context!())
        .expect("Failed to build Tauri application")
        .run(|app, event| {
            // Handle application events
            if let tauri::RunEvent::ExitRequested { .. } = event {
                tracing::info!("Exit requested");
            }

            // Handle custom events
            if let tauri::RunEvent::Ready = event {
                let app_handle = app.clone();

                // Set up login event listener
                app.listen("login-requested", move |_| {
                    let handle = app_handle.clone();
                    tauri::async_runtime::spawn(async move {
                        let app = handle.state::<Arc<App>>();
                        app.handle_login(&handle).await;
                    });
                });
            }

            if let tauri::RunEvent::Ready = event {
                let app_handle = app.clone();

                // Set up logout event listener
                app.listen("logout-requested", move |_| {
                    let handle = app_handle.clone();
                    tauri::async_runtime::spawn(async move {
                        let app = handle.state::<Arc<App>>();
                        app.handle_logout(&handle).await;
                    });
                });
            }
        });
}

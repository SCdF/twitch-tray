// Prevents additional console window on Windows in release
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use std::sync::{Arc, Mutex};

use chrono::Utc;
use tauri::Manager;
use tokio::sync::mpsc;
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt, EnvFilter};

use twitch_backend::{handle::RawDisplayData, AuthCommand, BackendEvent};
use twitch_kde::{
    dbus_service::{spawn_state_watcher, DbusService, WindowRequest, OBJECT_PATH},
    plasmoid_state::compute_plasmoid_state,
};
use twitch_settings_tauri::window::{open_settings_window, open_streamer_settings_window};

fn main() {
    tracing_subscriber::registry()
        .with(EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")))
        .with(tracing_subscriber::fmt::layer())
        .init();

    tracing::info!("Starting Twitch KDE daemon");

    tauri::Builder::default()
        .invoke_handler(tauri::generate_handler![
            twitch_settings_tauri::commands::get_config,
            twitch_settings_tauri::commands::save_config,
            twitch_settings_tauri::commands::search_categories,
            twitch_settings_tauri::commands::get_followed_categories,
            twitch_settings_tauri::commands::get_followed_channels_list,
            twitch_settings_tauri::commands::is_debug_build,
            twitch_settings_tauri::commands::get_debug_schedule_data,
        ])
        .setup(|app| {
            let handle = twitch_backend::start().expect("Failed to start backend");

            // Store services for Tauri settings commands
            app.manage(handle.services);

            let (window_tx, mut window_rx) = mpsc::channel::<WindowRequest>(4);
            let (cancel_tx, mut cancel_rx) = mpsc::channel::<()>(1);

            // D-Bus service state — starts as unauthenticated
            let service_state = Arc::new(Mutex::new(compute_plasmoid_state(
                RawDisplayData::default(),
                None,
                Utc::now(),
            )));

            let service = DbusService {
                state: Arc::clone(&service_state),
                auth_cmd_tx: handle.auth_cmd_tx.clone(),
                window_tx: window_tx.clone(),
                open_url: Arc::new(|url| {
                    let _ = open::that(url);
                }),
                cancel_login_tx: cancel_tx,
            };

            // Connect to the session D-Bus and register the service
            let dbus_conn = tauri::async_runtime::block_on(async {
                zbus::connection::Builder::session()?
                    .name("org.twitch.TwitchTray1")?
                    .serve_at(OBJECT_PATH, service)?
                    .build()
                    .await
            })
            .expect("Failed to connect to D-Bus session bus");

            // Obtain signal context for emitting StateChanged signals
            let iface_ref = tauri::async_runtime::block_on(async {
                dbus_conn
                    .object_server()
                    .interface::<_, DbusService>(OBJECT_PATH)
                    .await
            })
            .expect("Failed to get D-Bus interface reference");

            let signal_ctxt = iface_ref.signal_context().to_owned();

            // Keep the D-Bus connection alive for the duration of the app
            app.manage(dbus_conn);

            // Watch display_rx + login_progress_rx → recompute state → emit StateChanged
            spawn_state_watcher(
                service_state,
                handle.display_rx,
                handle.login_progress_rx,
                signal_ctxt,
            );

            // Cancel login: route to Logout (aborts the in-progress device flow)
            let cancel_auth_tx = handle.auth_cmd_tx.clone();
            tauri::async_runtime::spawn(async move {
                while cancel_rx.recv().await.is_some() {
                    tracing::info!("Login cancelled by user");
                    let _ = cancel_auth_tx.send(AuthCommand::Logout);
                }
            });

            // Route WindowRequest → open_*_window (requires AppHandle)
            let app_handle = app.handle().clone();
            tauri::async_runtime::spawn(async move {
                while let Some(req) = window_rx.recv().await {
                    match req {
                        WindowRequest::OpenSettings => {
                            open_settings_window(&app_handle);
                        }
                        WindowRequest::OpenStreamerSettings {
                            user_login,
                            display_name,
                        } => {
                            open_streamer_settings_window(&app_handle, &user_login, &display_name);
                        }
                    }
                }
            });

            // Route OpenSettingsRequested backend events → window_tx
            let mut event_rx = handle.event_tx.subscribe();
            tauri::async_runtime::spawn(async move {
                while let Ok(event) = event_rx.recv().await {
                    if let BackendEvent::OpenSettingsRequested {
                        user_login,
                        display_name,
                    } = event
                    {
                        let _ = window_tx
                            .send(WindowRequest::OpenStreamerSettings {
                                user_login,
                                display_name,
                            })
                            .await;
                    }
                }
            });

            Ok(())
        })
        .build(tauri::generate_context!())
        .expect("Failed to build Tauri application")
        .run(|_app, event| {
            // Prevent exit when all windows close (daemon stays alive until killed)
            if let tauri::RunEvent::ExitRequested { ref api, code, .. } = event {
                if code.is_none() {
                    api.prevent_exit();
                }
            }
        });
}

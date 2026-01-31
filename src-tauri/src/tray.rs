use std::sync::Arc;
use tauri::{
    image::Image,
    menu::{Menu, MenuBuilder, MenuItemBuilder, SubmenuBuilder},
    tray::{TrayIcon, TrayIconBuilder},
    AppHandle, Emitter,
};

use crate::state::AppState;
use crate::twitch::{ScheduledStream, Stream};

const ICON_BYTES: &[u8] = include_bytes!("../icons/icon.png");
const ICON_GREY_BYTES: &[u8] = include_bytes!("../icons/icon_grey.png");

/// Menu item IDs
mod ids {
    pub const LOGIN: &str = "login";
    pub const LOGOUT: &str = "logout";
    pub const QUIT: &str = "quit";
    pub const STREAM_PREFIX: &str = "stream_";
    pub const SCHEDULED_PREFIX: &str = "scheduled_";
}

/// Loads an image from embedded PNG bytes
fn load_icon(bytes: &[u8]) -> tauri::Result<Image<'static>> {
    let decoder = png::Decoder::new(bytes);
    let mut reader = decoder
        .read_info()
        .map_err(|e| tauri::Error::Anyhow(e.into()))?;
    let mut buf = vec![0; reader.output_buffer_size()];
    let info = reader
        .next_frame(&mut buf)
        .map_err(|e| tauri::Error::Anyhow(e.into()))?;
    buf.truncate(info.buffer_size());

    Ok(Image::new_owned(buf, info.width, info.height))
}

/// Manages the system tray
pub struct TrayManager {
    state: Arc<AppState>,
}

impl TrayManager {
    /// Creates a new tray manager
    pub fn new(state: Arc<AppState>) -> Self {
        Self { state }
    }

    /// Creates the initial tray icon
    pub fn create_tray(&self, app: &AppHandle) -> tauri::Result<TrayIcon> {
        let icon = load_icon(ICON_GREY_BYTES)?;

        let tray = TrayIconBuilder::with_id("main")
            .icon(icon)
            .tooltip("Twitch Tray")
            .show_menu_on_left_click(true)
            .build(app)?;

        Ok(tray)
    }

    /// Rebuilds the menu based on current state
    pub async fn rebuild_menu(&self, app: &AppHandle) -> tauri::Result<()> {
        let authenticated = self.state.is_authenticated().await;
        let streams = self.state.get_followed_streams().await;
        let scheduled = self.state.get_scheduled_streams().await;
        let schedules_loaded = self.state.schedules_loaded().await;

        let menu = if authenticated {
            build_authenticated_menu(app, streams, scheduled, schedules_loaded)?
        } else {
            build_unauthenticated_menu(app)?
        };

        // Update the tray menu
        if let Some(tray) = app.tray_by_id("main") {
            tray.set_menu(Some(menu))?;

            // Update icon based on auth state
            let icon = if authenticated {
                load_icon(ICON_BYTES)?
            } else {
                load_icon(ICON_GREY_BYTES)?
            };
            tray.set_icon(Some(icon))?;
        }

        Ok(())
    }
}

fn build_unauthenticated_menu(app: &AppHandle) -> tauri::Result<Menu<tauri::Wry>> {
    let login = MenuItemBuilder::with_id(ids::LOGIN, "Login to Twitch").build(app)?;
    let quit = MenuItemBuilder::with_id(ids::QUIT, "Quit").build(app)?;

    MenuBuilder::new(app).items(&[&login, &quit]).build()
}

fn build_authenticated_menu(
    app: &AppHandle,
    mut streams: Vec<Stream>,
    scheduled: Vec<ScheduledStream>,
    schedules_loaded: bool,
) -> tauri::Result<Menu<tauri::Wry>> {
    let mut items: Vec<Box<dyn tauri::menu::IsMenuItem<tauri::Wry>>> = Vec::new();

    // === Following Live section ===
    let title = if streams.is_empty() {
        "Following Live".to_string()
    } else {
        format!("Following Live ({})", streams.len())
    };
    items.push(Box::new(
        MenuItemBuilder::new(title).enabled(false).build(app)?,
    ));

    if streams.is_empty() {
        items.push(Box::new(
            MenuItemBuilder::new("  No streams live")
                .enabled(false)
                .build(app)?,
        ));
    } else {
        // Sort by viewer count (highest first)
        streams.sort_by(|a, b| b.viewer_count.cmp(&a.viewer_count));

        // Show top 10 in main menu
        const MAIN_MENU_LIMIT: usize = 10;
        let (show_in_main, overflow) = if streams.len() > MAIN_MENU_LIMIT {
            let (main, over) = streams.split_at(MAIN_MENU_LIMIT);
            (main.to_vec(), over.to_vec())
        } else {
            (streams, Vec::new())
        };

        for stream in &show_in_main {
            let label = format_stream_label(stream);
            let id = format!("{}{}", ids::STREAM_PREFIX, stream.user_login);
            items.push(Box::new(MenuItemBuilder::with_id(id, label).build(app)?));
        }

        // Add "More" submenu if there are overflow streams
        if !overflow.is_empty() {
            let more_label = format!("More ({})...", overflow.len());
            let mut more_submenu = SubmenuBuilder::new(app, more_label);

            for stream in &overflow {
                let label = format_stream_label(stream);
                let id = format!("{}{}", ids::STREAM_PREFIX, stream.user_login);
                let item = MenuItemBuilder::with_id(id, label).build(app)?;
                more_submenu = more_submenu.item(&item);
            }

            items.push(Box::new(more_submenu.build()?));
        }
    }

    // === Scheduled section ===
    items.push(Box::new(
        MenuItemBuilder::new("Scheduled (Next 24h)")
            .enabled(false)
            .build(app)?,
    ));

    if scheduled.is_empty() {
        let label = if schedules_loaded {
            "  No scheduled streams"
        } else {
            "  Loading..."
        };
        items.push(Box::new(
            MenuItemBuilder::new(label).enabled(false).build(app)?,
        ));
    } else {
        // Show top 5 in main menu
        const MAIN_MENU_LIMIT: usize = 5;
        let (show_in_main, overflow) = if scheduled.len() > MAIN_MENU_LIMIT {
            let (main, over) = scheduled.split_at(MAIN_MENU_LIMIT);
            (main.to_vec(), over.to_vec())
        } else {
            (scheduled, Vec::new())
        };

        for sched in &show_in_main {
            let label = format_scheduled_label(sched);
            let id = format!("{}{}", ids::SCHEDULED_PREFIX, sched.broadcaster_login);
            items.push(Box::new(MenuItemBuilder::with_id(id, label).build(app)?));
        }

        // Add "More" submenu if there are overflow scheduled streams
        if !overflow.is_empty() {
            let more_label = format!("More ({})...", overflow.len());
            let mut more_submenu = SubmenuBuilder::new(app, more_label);

            for sched in &overflow {
                let label = format_scheduled_label(sched);
                let id = format!("{}{}", ids::SCHEDULED_PREFIX, sched.broadcaster_login);
                let item = MenuItemBuilder::with_id(id, label).build(app)?;
                more_submenu = more_submenu.item(&item);
            }

            items.push(Box::new(more_submenu.build()?));
        }
    }

    // === Logout and Quit ===
    let logout = MenuItemBuilder::with_id(ids::LOGOUT, "Logout").build(app)?;
    let quit = MenuItemBuilder::with_id(ids::QUIT, "Quit").build(app)?;

    // Build the menu with separators
    MenuBuilder::new(app)
        .items(&items.iter().map(|i| i.as_ref()).collect::<Vec<_>>())
        .separator()
        .items(&[&logout, &quit])
        .build()
}

/// Handles menu item clicks
pub fn handle_menu_event(app: &AppHandle, id: &str) {
    match id {
        ids::LOGIN => {
            app.emit("login-requested", ()).ok();
        }
        ids::LOGOUT => {
            app.emit("logout-requested", ()).ok();
        }
        ids::QUIT => {
            app.exit(0);
        }
        _ if id.starts_with(ids::STREAM_PREFIX) => {
            let user_login = &id[ids::STREAM_PREFIX.len()..];
            open_stream(user_login);
        }
        _ if id.starts_with(ids::SCHEDULED_PREFIX) => {
            let user_login = &id[ids::SCHEDULED_PREFIX.len()..];
            open_stream(user_login);
        }
        _ => {}
    }
}

/// Opens a Twitch stream in the default browser
fn open_stream(user_login: &str) {
    let url = format!("https://twitch.tv/{}", user_login);
    if let Err(e) = open::that(&url) {
        tracing::error!("Failed to open browser: {}", e);
    }
}

/// Formats a stream for the Following Live menu
/// Format: "StreamerName - GameName (1.2k, 2h 15m)"
fn format_stream_label(s: &Stream) -> String {
    format!(
        "{} - {} ({}, {})",
        s.user_name,
        truncate(&s.game_name, 20),
        s.format_viewer_count(),
        s.format_duration()
    )
}

/// Formats a scheduled stream
/// Format: "StreamerName - Tomorrow 3:00 PM"
fn format_scheduled_label(s: &ScheduledStream) -> String {
    format!("{} - {}", s.broadcaster_name, s.format_start_time())
}

/// Truncates a string to max length with ellipsis
fn truncate(s: &str, max: usize) -> String {
    if s.len() <= max {
        s.to_string()
    } else if max <= 3 {
        s[..max].to_string()
    } else {
        format!("{}...", &s[..max - 3])
    }
}

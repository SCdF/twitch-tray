use std::sync::{Arc, Mutex};

use tauri::{
    image::Image,
    menu::{Menu, MenuBuilder, MenuItemBuilder, SubmenuBuilder},
    tray::{TrayIcon, TrayIconBuilder},
    AppHandle, Emitter,
};

use crate::display::DisplayBackend;
use crate::display_state::DisplayState;

const ICON_BYTES: &[u8] = include_bytes!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../twitch-app-tauri/icons/icon.png"
));
const ICON_GREY_BYTES: &[u8] = include_bytes!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../twitch-app-tauri/icons/icon_grey.png"
));

/// Menu item IDs
mod ids {
    pub const LOGIN: &str = "login";
    pub const LOGOUT: &str = "logout";
    pub const QUIT: &str = "quit";
    pub const SETTINGS: &str = "settings";
    pub const STREAM_PREFIX: &str = "stream_";
    pub const SCHEDULED_PREFIX: &str = "scheduled_";
    pub const CATEGORY_STREAM_PREFIX: &str = "cat_stream_";
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

/// System tray adapter that implements [`DisplayBackend`].
///
/// This is the only type in the codebase that holds an `AppHandle`.
/// All business logic lives in the domain core; `TrayBackend` is a thin
/// adapter that converts a [`DisplayState`] into Tauri menu items.
#[derive(Clone)]
pub struct TrayBackend {
    app_handle: AppHandle,
    /// Serialises menu rebuilds to prevent concurrent GTK operations which
    /// can crash libayatana-appindicator on Linux.
    rebuild_lock: Arc<Mutex<()>>,
}

impl TrayBackend {
    /// Creates a new tray backend bound to the given app handle.
    pub fn new(app_handle: AppHandle) -> Self {
        Self {
            app_handle,
            rebuild_lock: Arc::new(Mutex::new(())),
        }
    }

    /// Creates the initial tray icon.
    pub fn create_tray(&self) -> tauri::Result<TrayIcon> {
        let icon = load_icon(ICON_GREY_BYTES)?;

        let tray = TrayIconBuilder::with_id("main")
            .icon(icon)
            .tooltip("Twitch Tray")
            .show_menu_on_left_click(true)
            .build(&self.app_handle)?;

        Ok(tray)
    }
}

impl DisplayBackend for TrayBackend {
    /// Renders the given display state as the tray menu.
    ///
    /// Serialises all menu rebuilds through a mutex to prevent concurrent
    /// GTK operations which can crash libayatana-appindicator on Linux.
    fn update(&self, state: DisplayState) -> anyhow::Result<()> {
        // Acquire lock to serialise menu rebuilds.
        // std::sync::Mutex is fine here: no `.await` is held while locked.
        let _guard = self.rebuild_lock.lock().unwrap_or_else(|e| e.into_inner());

        let app_handle = self.app_handle.clone();
        let authenticated = state.authenticated;

        // Build and set menu on the main thread to avoid GTK threading issues.
        // Clone the handle so the closure can own it while we call the method on the original.
        let app_handle_closure = app_handle.clone();
        app_handle
            .run_on_main_thread(move || {
                let app_handle = app_handle_closure;
                let menu_result = if authenticated {
                    render_display_state(&app_handle, state)
                } else {
                    build_unauthenticated_menu(&app_handle)
                };

                let menu = match menu_result {
                    Ok(m) => m,
                    Err(e) => {
                        tracing::error!("Failed to build menu: {}", e);
                        return;
                    }
                };

                if let Some(tray) = app_handle.tray_by_id("main") {
                    if let Err(e) = tray.set_menu(Some(menu)) {
                        tracing::error!("Failed to set tray menu: {}", e);
                        return;
                    }

                    let icon_result = if authenticated {
                        load_icon(ICON_BYTES)
                    } else {
                        load_icon(ICON_GREY_BYTES)
                    };

                    match icon_result {
                        Ok(icon) => {
                            if let Err(e) = tray.set_icon(Some(icon)) {
                                tracing::error!("Failed to set tray icon: {}", e);
                            }
                        }
                        Err(e) => {
                            tracing::error!("Failed to load icon: {}", e);
                        }
                    }
                }
            })
            .map_err(anyhow::Error::from)
    }
}

fn build_unauthenticated_menu(app: &AppHandle) -> tauri::Result<Menu<tauri::Wry>> {
    let login = MenuItemBuilder::with_id(ids::LOGIN, "Login to Twitch").build(app)?;
    let quit = MenuItemBuilder::with_id(ids::QUIT, "Quit").build(app)?;

    MenuBuilder::new(app).items(&[&login, &quit]).build()
}

/// Maps a `DisplayState` into Tauri menu items.
///
/// This is the only function in `tray/mod.rs` that knows about Tauri types.
/// All business logic (sorting, filtering, labelling) lives in `display_state.rs`.
fn render_display_state(app: &AppHandle, state: DisplayState) -> tauri::Result<Menu<tauri::Wry>> {
    let mut items: Vec<Box<dyn tauri::menu::IsMenuItem<tauri::Wry>>> = Vec::new();

    // === Following Live section ===
    let total_live = state.live_section.visible.len() + state.live_section.overflow.len();
    let live_title = if total_live == 0 {
        "Following Live".to_string()
    } else {
        format!("Following Live ({})", total_live)
    };
    items.push(Box::new(
        MenuItemBuilder::new(live_title).enabled(false).build(app)?,
    ));

    if total_live == 0 {
        items.push(Box::new(
            MenuItemBuilder::new("  No streams live")
                .enabled(false)
                .build(app)?,
        ));
    } else {
        for entry in &state.live_section.visible {
            let id = format!("{}{}", ids::STREAM_PREFIX, entry.stream.user_login);
            items.push(Box::new(
                MenuItemBuilder::with_id(id, &entry.label).build(app)?,
            ));
        }

        if !state.live_section.overflow.is_empty() {
            let more_label = format!("More ({})...", state.live_section.overflow.len());
            let mut more_submenu = SubmenuBuilder::new(app, more_label);

            for entry in &state.live_section.overflow {
                let id = format!("{}{}", ids::STREAM_PREFIX, entry.stream.user_login);
                let item = MenuItemBuilder::with_id(id, &entry.label).build(app)?;
                more_submenu = more_submenu.item(&item);
            }

            items.push(Box::new(more_submenu.build()?));
        }
    }

    // === Category sections ===
    if !state.category_sections.is_empty() {
        items.push(Box::new(
            MenuItemBuilder::new("Categories")
                .enabled(false)
                .build(app)?,
        ));

        for cat_section in &state.category_sections {
            let mut cat_submenu = SubmenuBuilder::new(app, &cat_section.header);

            for entry in &cat_section.entries {
                let id = format!("{}{}", ids::CATEGORY_STREAM_PREFIX, entry.stream.user_login);
                let item = MenuItemBuilder::with_id(id, &entry.label).build(app)?;
                cat_submenu = cat_submenu.item(&item);
            }

            items.push(Box::new(cat_submenu.build()?));
        }
    }

    // === Scheduled section ===
    items.push(Box::new(
        MenuItemBuilder::new(&state.schedule_section.header)
            .enabled(false)
            .build(app)?,
    ));

    let total_sched = state.schedule_section.visible.len() + state.schedule_section.overflow.len();
    if total_sched == 0 {
        let label = if state.schedule_section.schedules_loaded {
            "  No scheduled streams"
        } else {
            "  Loading..."
        };
        items.push(Box::new(
            MenuItemBuilder::new(label).enabled(false).build(app)?,
        ));
    } else {
        for entry in &state.schedule_section.visible {
            let id = format!(
                "{}{}",
                ids::SCHEDULED_PREFIX,
                entry.scheduled.broadcaster_login
            );
            items.push(Box::new(
                MenuItemBuilder::with_id(id, &entry.label).build(app)?,
            ));
        }

        if !state.schedule_section.overflow.is_empty() {
            let more_label = format!("More ({})...", state.schedule_section.overflow.len());
            let mut more_submenu = SubmenuBuilder::new(app, more_label);

            for entry in &state.schedule_section.overflow {
                let id = format!(
                    "{}{}",
                    ids::SCHEDULED_PREFIX,
                    entry.scheduled.broadcaster_login
                );
                let item = MenuItemBuilder::with_id(id, &entry.label).build(app)?;
                more_submenu = more_submenu.item(&item);
            }

            items.push(Box::new(more_submenu.build()?));
        }
    }

    // === Settings, Logout and Quit ===
    let settings = MenuItemBuilder::with_id(ids::SETTINGS, "Settings").build(app)?;
    let logout = MenuItemBuilder::with_id(ids::LOGOUT, "Logout").build(app)?;
    let quit = MenuItemBuilder::with_id(ids::QUIT, "Quit").build(app)?;

    MenuBuilder::new(app)
        .items(&items.iter().map(|i| i.as_ref()).collect::<Vec<_>>())
        .separator()
        .items(&[&settings, &logout, &quit])
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
        ids::SETTINGS => {
            twitch_settings_tauri::window::open_settings_window(app);
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
        _ if id.starts_with(ids::CATEGORY_STREAM_PREFIX) => {
            let user_login = &id[ids::CATEGORY_STREAM_PREFIX.len()..];
            open_stream(user_login);
        }
        _ => {}
    }
}

/// Opens a Twitch stream in the default browser
fn open_stream(user_login: &str) {
    let url = format!("https://twitch.tv/{user_login}");
    if let Err(e) = open::that(&url) {
        tracing::error!("Failed to open browser: {}", e);
    }
}

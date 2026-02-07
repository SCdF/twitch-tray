use std::collections::HashMap;
use std::sync::Arc;
use tauri::{
    image::Image,
    menu::{Menu, MenuBuilder, MenuItemBuilder, SubmenuBuilder},
    tray::{TrayIcon, TrayIconBuilder},
    AppHandle, Emitter, Manager, WebviewWindowBuilder,
};
use tokio::sync::Mutex;

use crate::config::{FollowedCategory, StreamerImportance, StreamerSettings};
use crate::state::AppState;
use crate::twitch::{ScheduledStream, Stream};

const ICON_BYTES: &[u8] = include_bytes!("../../icons/icon.png");
const ICON_GREY_BYTES: &[u8] = include_bytes!("../../icons/icon_grey.png");

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

/// Manages the system tray
#[derive(Clone)]
pub struct TrayManager {
    state: Arc<AppState>,
    /// Mutex to serialize menu rebuilds - prevents concurrent GTK operations
    /// which can crash libayatana-appindicator on Linux
    rebuild_lock: Arc<Mutex<()>>,
}

impl TrayManager {
    /// Creates a new tray manager
    pub fn new(state: Arc<AppState>) -> Self {
        Self {
            state,
            rebuild_lock: Arc::new(Mutex::new(())),
        }
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
        self.rebuild_menu_with_categories(app, Vec::new(), HashMap::new(), HashMap::new())
            .await
    }

    /// Rebuilds the menu with category data
    ///
    /// This method serializes all menu rebuilds through a mutex to prevent
    /// concurrent GTK operations which can crash libayatana-appindicator on Linux.
    pub async fn rebuild_menu_with_categories(
        &self,
        app: &AppHandle,
        followed_categories: Vec<FollowedCategory>,
        category_streams: HashMap<String, Vec<Stream>>,
        streamer_settings: HashMap<String, StreamerSettings>,
    ) -> tauri::Result<()> {
        // Acquire lock to serialize menu rebuilds - this prevents crashes from
        // concurrent GTK operations in libayatana-appindicator
        let _guard = self.rebuild_lock.lock().await;

        let authenticated = self.state.is_authenticated().await;
        let streams = self.state.get_followed_streams().await;
        let scheduled = self.state.get_scheduled_streams().await;
        let schedules_loaded = self.state.schedules_loaded().await;

        // Clone app handle for the closure
        let app_handle = app.clone();

        // Build and set menu on the main thread to avoid GTK threading issues
        app.run_on_main_thread(move || {
            let menu_result = if authenticated {
                build_authenticated_menu(
                    &app_handle,
                    streams,
                    scheduled,
                    schedules_loaded,
                    followed_categories,
                    category_streams,
                    streamer_settings,
                )
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

            // Update the tray menu
            if let Some(tray) = app_handle.tray_by_id("main") {
                if let Err(e) = tray.set_menu(Some(menu)) {
                    tracing::error!("Failed to set tray menu: {}", e);
                    return;
                }

                // Update icon based on auth state
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
        })?;

        Ok(())
    }
}

/// Opens the settings window
pub fn open_settings_window(app: &AppHandle) {
    // Check if window already exists
    if let Some(window) = app.get_webview_window("settings") {
        let _ = window.set_focus();
        return;
    }

    // Create new settings window
    match WebviewWindowBuilder::new(app, "settings", tauri::WebviewUrl::App("index.html".into()))
        .title("Twitch Tray Settings")
        .inner_size(975.0, 975.0)
        .resizable(true)
        .center()
        .build()
    {
        Ok(_) => tracing::info!("Settings window opened"),
        Err(e) => tracing::error!("Failed to open settings window: {}", e),
    }
}

fn build_unauthenticated_menu(app: &AppHandle) -> tauri::Result<Menu<tauri::Wry>> {
    let login = MenuItemBuilder::with_id(ids::LOGIN, "Login to Twitch").build(app)?;
    let quit = MenuItemBuilder::with_id(ids::QUIT, "Quit").build(app)?;

    MenuBuilder::new(app).items(&[&login, &quit]).build()
}

fn get_importance(
    user_login: &str,
    streamer_settings: &HashMap<String, StreamerSettings>,
) -> StreamerImportance {
    streamer_settings
        .get(user_login)
        .map(|s| s.importance)
        .unwrap_or_default()
}

fn build_authenticated_menu(
    app: &AppHandle,
    mut streams: Vec<Stream>,
    scheduled: Vec<ScheduledStream>,
    schedules_loaded: bool,
    followed_categories: Vec<FollowedCategory>,
    category_streams: HashMap<String, Vec<Stream>>,
    streamer_settings: HashMap<String, StreamerSettings>,
) -> tauri::Result<Menu<tauri::Wry>> {
    let mut items: Vec<Box<dyn tauri::menu::IsMenuItem<tauri::Wry>>> = Vec::new();

    // Filter out Ignore streamers from live streams
    streams.retain(|s| {
        get_importance(&s.user_login, &streamer_settings) != StreamerImportance::Ignore
    });

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
        // Sort: Favourites first (by viewers), then rest by viewers
        streams.sort_by(|a, b| {
            let a_fav =
                get_importance(&a.user_login, &streamer_settings) == StreamerImportance::Favourite;
            let b_fav =
                get_importance(&b.user_login, &streamer_settings) == StreamerImportance::Favourite;
            b_fav.cmp(&a_fav).then(b.viewer_count.cmp(&a.viewer_count))
        });

        // Show top 10 in main menu
        const MAIN_MENU_LIMIT: usize = 10;
        let (show_in_main, overflow) = if streams.len() > MAIN_MENU_LIMIT {
            let (main, over) = streams.split_at(MAIN_MENU_LIMIT);
            (main.to_vec(), over.to_vec())
        } else {
            (streams, Vec::new())
        };

        for stream in &show_in_main {
            let is_fav = get_importance(&stream.user_login, &streamer_settings)
                == StreamerImportance::Favourite;
            let label = format_stream_label_with_star(stream, is_fav);
            let id = format!("{}{}", ids::STREAM_PREFIX, stream.user_login);
            items.push(Box::new(MenuItemBuilder::with_id(id, label).build(app)?));
        }

        // Add "More" submenu if there are overflow streams
        if !overflow.is_empty() {
            let more_label = format!("More ({})...", overflow.len());
            let mut more_submenu = SubmenuBuilder::new(app, more_label);

            for stream in &overflow {
                let is_fav = get_importance(&stream.user_login, &streamer_settings)
                    == StreamerImportance::Favourite;
                let label = format_stream_label_with_star(stream, is_fav);
                let id = format!("{}{}", ids::STREAM_PREFIX, stream.user_login);
                let item = MenuItemBuilder::with_id(id, label).build(app)?;
                more_submenu = more_submenu.item(&item);
            }

            items.push(Box::new(more_submenu.build()?));
        }
    }

    // === Category sections ===
    // Only show header if there are categories with streams
    let has_category_streams = followed_categories
        .iter()
        .any(|cat| category_streams.get(&cat.id).is_some_and(|s| !s.is_empty()));

    if has_category_streams {
        items.push(Box::new(
            MenuItemBuilder::new("Categories")
                .enabled(false)
                .build(app)?,
        ));

        for category in &followed_categories {
            if let Some(cat_streams) = category_streams.get(&category.id) {
                if !cat_streams.is_empty() {
                    // Sort by viewer count and take top 10
                    let mut sorted_streams = cat_streams.clone();
                    sorted_streams.sort_by(|a, b| b.viewer_count.cmp(&a.viewer_count));
                    sorted_streams.truncate(10);

                    // Sum total viewers from the streams we have
                    let total_viewers: i64 = sorted_streams.iter().map(|s| s.viewer_count).sum();
                    let label =
                        format!("{} ({})", category.name, format_viewer_count(total_viewers));

                    let mut cat_submenu = SubmenuBuilder::new(app, &label);

                    for stream in &sorted_streams {
                        let label = format_category_stream_label(stream);
                        let id = format!("{}{}", ids::CATEGORY_STREAM_PREFIX, stream.user_login);
                        let item = MenuItemBuilder::with_id(id, label).build(app)?;
                        cat_submenu = cat_submenu.item(&item);
                    }

                    items.push(Box::new(cat_submenu.build()?));
                }
            }
        }
    }

    // === Scheduled section ===
    // Filter out Ignore streamers from scheduled
    let scheduled: Vec<_> = scheduled
        .into_iter()
        .filter(|s| {
            get_importance(&s.broadcaster_login, &streamer_settings) != StreamerImportance::Ignore
        })
        .collect();

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
            let is_fav = get_importance(&sched.broadcaster_login, &streamer_settings)
                == StreamerImportance::Favourite;
            let label = format_scheduled_label_with_star(sched, is_fav);
            let id = format!("{}{}", ids::SCHEDULED_PREFIX, sched.broadcaster_login);
            items.push(Box::new(MenuItemBuilder::with_id(id, label).build(app)?));
        }

        // Add "More" submenu if there are overflow scheduled streams
        if !overflow.is_empty() {
            let more_label = format!("More ({})...", overflow.len());
            let mut more_submenu = SubmenuBuilder::new(app, more_label);

            for sched in &overflow {
                let is_fav = get_importance(&sched.broadcaster_login, &streamer_settings)
                    == StreamerImportance::Favourite;
                let label = format_scheduled_label_with_star(sched, is_fav);
                let id = format!("{}{}", ids::SCHEDULED_PREFIX, sched.broadcaster_login);
                let item = MenuItemBuilder::with_id(id, label).build(app)?;
                more_submenu = more_submenu.item(&item);
            }

            items.push(Box::new(more_submenu.build()?));
        }
    }

    // === Settings, Logout and Quit ===
    let settings = MenuItemBuilder::with_id(ids::SETTINGS, "Settings").build(app)?;
    let logout = MenuItemBuilder::with_id(ids::LOGOUT, "Logout").build(app)?;
    let quit = MenuItemBuilder::with_id(ids::QUIT, "Quit").build(app)?;

    // Build the menu with separators
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
            open_settings_window(app);
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
    let url = format!("https://twitch.tv/{}", user_login);
    if let Err(e) = open::that(&url) {
        tracing::error!("Failed to open browser: {}", e);
    }
}

/// Formats a stream for the Following Live menu with optional star prefix
/// Format: "[star] StreamerName - GameName (1.2k, 2h 15m)"
pub(crate) fn format_stream_label_with_star(s: &Stream, star: bool) -> String {
    let prefix = if star { "\u{2605} " } else { "" };
    format!(
        "{}{} - {} ({}, {})",
        prefix,
        s.user_name,
        truncate(&s.game_name, 20),
        s.format_viewer_count(),
        s.format_duration()
    )
}

/// Formats a scheduled stream label with optional star prefix
/// Format: "[star] StreamerName - Tomorrow 3:00 PM"
pub(crate) fn format_scheduled_label_with_star(s: &ScheduledStream, star: bool) -> String {
    let prefix = if star { "\u{2605} " } else { "" };
    format!(
        "{}{} - {}",
        prefix,
        s.broadcaster_name,
        s.format_start_time()
    )
}

/// Formats a stream for category submenu (no game name since it's implied)
/// Format: "StreamerName (1.2k)"
pub(crate) fn format_category_stream_label(s: &Stream) -> String {
    format!("{} ({})", s.user_name, s.format_viewer_count())
}

/// Formats a viewer count with k suffix for thousands
fn format_viewer_count(count: i64) -> String {
    if count >= 1000 {
        let k = count as f64 / 1000.0;
        if k.fract() < 0.05 {
            format!("{}k", k as i64)
        } else {
            format!("{:.1}k", k)
        }
    } else {
        count.to_string()
    }
}

/// Truncates a string to max length with ellipsis
pub(crate) fn truncate(s: &str, max: usize) -> String {
    if s.len() <= max {
        s.to_string()
    } else if max <= 3 {
        s[..max].to_string()
    } else {
        format!("{}...", &s[..max - 3])
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::{Duration, Utc};

    /// Helper to create a test stream
    fn make_stream(user_name: &str, game_name: &str, viewer_count: i64, hours_ago: i64) -> Stream {
        Stream {
            id: "123".to_string(),
            user_id: "456".to_string(),
            user_login: user_name.to_lowercase(),
            user_name: user_name.to_string(),
            game_id: "789".to_string(),
            game_name: game_name.to_string(),
            title: "Test Stream".to_string(),
            viewer_count,
            started_at: Utc::now() - Duration::hours(hours_ago),
            thumbnail_url: "https://example.com/thumb.jpg".to_string(),
            tags: vec![],
        }
    }

    /// Helper to create a scheduled stream
    fn make_scheduled(broadcaster_name: &str, hours_until: i64) -> ScheduledStream {
        ScheduledStream {
            id: "sched123".to_string(),
            broadcaster_id: "456".to_string(),
            broadcaster_name: broadcaster_name.to_string(),
            broadcaster_login: broadcaster_name.to_lowercase(),
            title: "Scheduled Stream".to_string(),
            start_time: Utc::now() + Duration::hours(hours_until),
            end_time: None,
            category: Some("Gaming".to_string()),
            category_id: Some("123".to_string()),
            is_recurring: false,
        }
    }

    // === truncate tests ===

    #[test]
    fn truncate_short_string() {
        assert_eq!(truncate("Hello", 10), "Hello");
    }

    #[test]
    fn truncate_exact_length() {
        assert_eq!(truncate("Hello", 5), "Hello");
    }

    #[test]
    fn truncate_long_string() {
        assert_eq!(truncate("Hello World", 8), "Hello...");
    }

    #[test]
    fn truncate_max_3() {
        // When max <= 3, we just take first max chars without ellipsis
        assert_eq!(truncate("Hello", 3), "Hel");
    }

    #[test]
    fn truncate_max_4() {
        // max=4 means we show 1 char + "..."
        assert_eq!(truncate("Hello", 4), "H...");
    }

    #[test]
    fn truncate_empty_string() {
        assert_eq!(truncate("", 10), "");
    }

    #[test]
    fn truncate_game_name_realistic() {
        let long_game = "Counter-Strike: Global Offensive";
        assert_eq!(truncate(long_game, 20), "Counter-Strike: G...");
    }

    // === format_stream_label tests ===

    #[test]
    fn format_stream_label_basic() {
        let stream = make_stream("Ninja", "Fortnite", 5000, 2);
        let label = format_stream_label_with_star(&stream, false);

        assert!(
            label.contains("Ninja"),
            "Label should contain streamer name"
        );
        assert!(label.contains("Fortnite"), "Label should contain game name");
        assert!(label.contains("5k"), "Label should contain viewer count");
        assert!(label.contains("2h"), "Label should contain duration");
    }

    #[test]
    fn format_stream_label_long_game_name() {
        let stream = make_stream(
            "Streamer",
            "This Is A Very Long Game Name That Should Be Truncated",
            1000,
            1,
        );
        let label = format_stream_label_with_star(&stream, false);

        // Game name should be truncated to 20 chars
        assert!(
            label.len() < 100,
            "Label should be reasonable length: {}",
            label
        );
        assert!(
            label.contains("..."),
            "Long game name should be truncated: {}",
            label
        );
    }

    #[test]
    fn format_stream_label_small_viewers() {
        let stream = make_stream("SmallStreamer", "Minecraft", 42, 0);
        let label = format_stream_label_with_star(&stream, false);

        assert!(
            label.contains("42"),
            "Small viewer count should show exact number: {}",
            label
        );
    }

    // === format_scheduled_label tests ===

    #[test]
    fn format_scheduled_label_basic() {
        let scheduled = make_scheduled("StreamerName", 5);
        let label = format_scheduled_label_with_star(&scheduled, false);

        assert!(
            label.starts_with("StreamerName - "),
            "Label should start with broadcaster name: {}",
            label
        );
    }

    #[test]
    fn format_scheduled_label_contains_time() {
        let scheduled = make_scheduled("TestStreamer", 2);
        let label = format_scheduled_label_with_star(&scheduled, false);

        // Should contain time-related text (Today, Tomorrow, or day name)
        let has_time_info = label.contains("Today")
            || label.contains("Tomorrow")
            || label.contains("Mon")
            || label.contains("Tue")
            || label.contains("Wed")
            || label.contains("Thu")
            || label.contains("Fri")
            || label.contains("Sat")
            || label.contains("Sun");

        assert!(has_time_info, "Label should contain time info: {}", label);
    }
}

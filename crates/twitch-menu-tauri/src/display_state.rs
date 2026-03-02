use std::collections::{HashMap, HashSet};

use chrono::{DateTime, Duration, Utc};

use twitch_backend::config::{FollowedCategory, StreamerImportance, StreamerSettings};
use twitch_backend::notify::truncate;
use twitch_backend::twitch::{format_viewer_count, ScheduledStream, Stream};

/// Scheduled stream within this many minutes of a live broadcast is "covered" by the live stream
/// and hidden from the schedule section.
const LIVE_COVERS_SCHEDULE_WINDOW_MIN: i64 = 60;

/// A live stream entry ready to be rendered.
pub struct StreamEntry {
    pub stream: Stream,
    pub label: String,
}

/// The live-streams portion of the display.
pub struct LiveSection {
    pub visible: Vec<StreamEntry>,
    pub overflow: Vec<StreamEntry>,
}

/// A scheduled stream entry ready to be rendered.
pub struct ScheduledEntry {
    pub scheduled: ScheduledStream,
    pub label: String,
}

/// The scheduled-streams portion of the display.
pub struct ScheduleSection {
    pub header: String,
    pub visible: Vec<ScheduledEntry>,
    pub overflow: Vec<ScheduledEntry>,
    /// `true` once the initial schedule fetch has completed; used to pick the empty label.
    pub schedules_loaded: bool,
}

/// A single stream within a category section.
pub struct CategoryStreamEntry {
    pub stream: Stream,
    pub label: String,
}

/// A followed category and its top streams.
pub struct CategorySection {
    pub header: String,
    pub entries: Vec<CategoryStreamEntry>,
}

/// The full computed display state for the tray menu.
///
/// This is a pure data type — no Tauri or GTK types. The render layer
/// (`tray/mod.rs`) maps this into actual menu items.
///
/// When `authenticated` is false the render layer shows the login menu
/// and the other fields are ignored.
pub struct DisplayState {
    pub authenticated: bool,
    pub live_section: LiveSection,
    pub schedule_section: ScheduleSection,
    pub category_sections: Vec<CategorySection>,
}

impl DisplayState {
    /// A display state that renders as the "not logged in" menu.
    pub fn unauthenticated() -> Self {
        Self {
            authenticated: false,
            live_section: LiveSection {
                visible: Vec::new(),
                overflow: Vec::new(),
            },
            schedule_section: ScheduleSection {
                header: String::new(),
                visible: Vec::new(),
                overflow: Vec::new(),
                schedules_loaded: false,
            },
            category_sections: Vec::new(),
        }
    }
}

/// Configuration that governs how `compute_display_state` shapes the display.
pub struct DisplayConfig {
    pub streamer_settings: HashMap<String, StreamerSettings>,
    pub schedule_lookahead_hours: u64,
    /// Maximum live streams shown in the main menu before the overflow submenu.
    pub live_limit: usize,
    /// Maximum scheduled streams shown in the main menu before the overflow submenu.
    pub schedule_limit: usize,
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

/// Formats a stream label for the Following Live menu with optional star prefix.
///
/// Format: `"[★ ]StreamerName - GameName (1.2k, 2h 15m)"`
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

/// Formats a scheduled stream label with optional sparkle/star prefix.
///
/// Format: `"[✨ ][★ ]StreamerName - Tomorrow 3:00 PM"`
pub(crate) fn format_scheduled_label_with_star(s: &ScheduledStream, star: bool) -> String {
    let sparkle = if s.is_inferred { "\u{2728} " } else { "" };
    let star_str = if star { "\u{2605} " } else { "" };
    format!(
        "{}{}{} - {}",
        sparkle,
        star_str,
        s.broadcaster_name,
        s.format_start_time()
    )
}

/// Formats a stream for a category submenu (no game name since it's implied).
///
/// Format: `"StreamerName (1.2k)"`
pub(crate) fn format_category_stream_label(s: &Stream) -> String {
    format!("{} ({})", s.user_name, s.format_viewer_count())
}

/// Computes a fully resolved, render-ready display state from raw data.
///
/// This is a pure function — no Tauri, GTK, or async dependencies. All
/// business decisions (sorting, filtering, overflow splitting, label formatting)
/// are made here so the render layer is a thin data → menu-item mapper.
///
/// `now` is passed in rather than calling `Utc::now()` directly so the function
/// is deterministically testable.
pub fn compute_display_state(
    mut streams: Vec<Stream>,
    scheduled: Vec<ScheduledStream>,
    schedules_loaded: bool,
    followed_categories: Vec<FollowedCategory>,
    category_streams: HashMap<String, Vec<Stream>>,
    config: &DisplayConfig,
    now: DateTime<Utc>,
) -> DisplayState {
    let settings = &config.streamer_settings;

    // --- Live section ---

    // Filter out Ignore streamers
    streams.retain(|s| get_importance(&s.user_login, settings) != StreamerImportance::Ignore);

    // Remember which broadcasters are live (used for schedule filtering below)
    let live_logins: HashSet<String> = streams.iter().map(|s| s.user_login.clone()).collect();

    // Sort: Favourites first, then by viewer count descending
    streams.sort_by(|a, b| {
        let a_fav = get_importance(&a.user_login, settings) == StreamerImportance::Favourite;
        let b_fav = get_importance(&b.user_login, settings) == StreamerImportance::Favourite;
        b_fav.cmp(&a_fav).then(b.viewer_count.cmp(&a.viewer_count))
    });

    let (live_visible_raw, live_overflow_raw) = if streams.len() > config.live_limit {
        let (main, over) = streams.split_at(config.live_limit);
        (main.to_vec(), over.to_vec())
    } else {
        (streams, Vec::new())
    };

    let live_section = LiveSection {
        visible: live_visible_raw
            .into_iter()
            .map(|s| {
                let is_fav =
                    get_importance(&s.user_login, settings) == StreamerImportance::Favourite;
                let label = format_stream_label_with_star(&s, is_fav);
                StreamEntry { stream: s, label }
            })
            .collect(),
        overflow: live_overflow_raw
            .into_iter()
            .map(|s| {
                let is_fav =
                    get_importance(&s.user_login, settings) == StreamerImportance::Favourite;
                let label = format_stream_label_with_star(&s, is_fav);
                StreamEntry { stream: s, label }
            })
            .collect(),
    };

    // --- Category sections ---

    let mut category_sections = Vec::new();
    for category in &followed_categories {
        if let Some(cat_streams) = category_streams.get(&category.id) {
            if !cat_streams.is_empty() {
                let mut sorted = cat_streams.clone();
                sorted.sort_by(|a, b| b.viewer_count.cmp(&a.viewer_count));
                sorted.truncate(10);

                let total_viewers: u32 = sorted.iter().map(|s| s.viewer_count).sum();
                let header = format!("{} ({})", category.name, format_viewer_count(total_viewers));

                let entries = sorted
                    .into_iter()
                    .map(|s| {
                        let label = format_category_stream_label(&s);
                        CategoryStreamEntry { stream: s, label }
                    })
                    .collect();

                category_sections.push(CategorySection { header, entries });
            }
        }
    }

    // --- Schedule section ---

    // A scheduled stream is "covered" if the broadcaster is live and the
    // scheduled start is within the next 60 minutes — hide it from the schedule list.
    let soon_threshold = now + Duration::minutes(LIVE_COVERS_SCHEDULE_WINDOW_MIN);
    let schedule_header = format!("Scheduled (Next {}h)", config.schedule_lookahead_hours);

    let filtered_scheduled: Vec<_> = scheduled
        .into_iter()
        .filter(|s| get_importance(&s.broadcaster_login, settings) != StreamerImportance::Ignore)
        .filter(|s| !(live_logins.contains(&s.broadcaster_login) && s.start_time <= soon_threshold))
        .collect();

    let (sched_visible_raw, sched_overflow_raw) =
        if filtered_scheduled.len() > config.schedule_limit {
            let (main, over) = filtered_scheduled.split_at(config.schedule_limit);
            (main.to_vec(), over.to_vec())
        } else {
            (filtered_scheduled, Vec::new())
        };

    let schedule_section = ScheduleSection {
        header: schedule_header,
        visible: sched_visible_raw
            .into_iter()
            .map(|s| {
                let is_fav =
                    get_importance(&s.broadcaster_login, settings) == StreamerImportance::Favourite;
                let label = format_scheduled_label_with_star(&s, is_fav);
                ScheduledEntry {
                    scheduled: s,
                    label,
                }
            })
            .collect(),
        overflow: sched_overflow_raw
            .into_iter()
            .map(|s| {
                let is_fav =
                    get_importance(&s.broadcaster_login, settings) == StreamerImportance::Favourite;
                let label = format_scheduled_label_with_star(&s, is_fav);
                ScheduledEntry {
                    scheduled: s,
                    label,
                }
            })
            .collect(),
        schedules_loaded,
    };

    DisplayState {
        authenticated: true,
        live_section,
        schedule_section,
        category_sections,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_helpers::{make_scheduled, make_stream};
    use chrono::Duration;

    // =========================================================
    // Helpers
    // =========================================================

    /// Stream with a specific viewer count (user_login derived from user_name).
    fn stream_with_viewers(user_name: &str, viewer_count: u32) -> Stream {
        let mut s = make_stream(user_name, user_name);
        s.viewer_count = viewer_count;
        s
    }

    fn no_categories() -> (Vec<FollowedCategory>, HashMap<String, Vec<Stream>>) {
        (Vec::new(), HashMap::new())
    }

    fn no_scheduled() -> Vec<ScheduledStream> {
        Vec::new()
    }

    /// Default config: no settings, standard limits, 6h lookahead.
    fn default_config() -> DisplayConfig {
        DisplayConfig {
            streamer_settings: HashMap::new(),
            schedule_lookahead_hours: 6,
            live_limit: 10,
            schedule_limit: 5,
        }
    }

    /// Config with a single streamer set to the given importance.
    fn config_with_importance(user_login: &str, importance: StreamerImportance) -> DisplayConfig {
        let mut settings = HashMap::new();
        settings.insert(
            user_login.to_string(),
            StreamerSettings {
                display_name: user_login.to_string(),
                importance,
            },
        );
        DisplayConfig {
            streamer_settings: settings,
            schedule_lookahead_hours: 6,
            live_limit: 10,
            schedule_limit: 5,
        }
    }

    // =========================================================
    // format_stream_label_with_star
    // =========================================================

    #[test]
    fn format_stream_label_basic() {
        let mut s = make_stream("ninja", "Ninja");
        s.game_name = "Fortnite".to_string();
        s.viewer_count = 5000;
        s.started_at = Utc::now() - Duration::hours(2);
        let label = format_stream_label_with_star(&s, false);

        assert!(label.contains("Ninja"), "should contain streamer name");
        assert!(label.contains("Fortnite"), "should contain game name");
        assert!(label.contains("5k"), "should contain viewer count");
        assert!(label.contains("2h"), "should contain duration");
    }

    #[test]
    fn format_stream_label_long_game_name_truncated() {
        let mut s = make_stream("streamer", "Streamer");
        s.game_name = "This Is A Very Long Game Name That Should Be Truncated".to_string();
        s.viewer_count = 1000;
        let label = format_stream_label_with_star(&s, false);

        assert!(label.contains("..."), "long game name should be truncated");
    }

    #[test]
    fn format_stream_label_small_viewers_exact() {
        let mut s = make_stream("smallstreamer", "SmallStreamer");
        s.viewer_count = 42;
        let label = format_stream_label_with_star(&s, false);

        assert!(
            label.contains("42"),
            "small viewer count shows exact number"
        );
        assert!(!label.contains("k"), "small count should not have k suffix");
    }

    #[test]
    fn format_stream_label_star_prefix() {
        let s = make_stream("fav", "Fav");
        let with_star = format_stream_label_with_star(&s, true);
        let without_star = format_stream_label_with_star(&s, false);

        assert!(
            with_star.starts_with('\u{2605}'),
            "star label should start with ★"
        );
        assert!(
            !without_star.starts_with('\u{2605}'),
            "no-star label should not start with ★"
        );
    }

    // =========================================================
    // format_scheduled_label_with_star
    // =========================================================

    #[test]
    fn format_scheduled_label_basic() {
        let sched = make_scheduled("StreamerName", 5);
        let label = format_scheduled_label_with_star(&sched, false);

        assert!(
            label.starts_with("StreamerName - "),
            "label should start with broadcaster name: {}",
            label
        );
    }

    #[test]
    fn format_scheduled_label_contains_time() {
        let sched = make_scheduled("TestStreamer", 2);
        let label = format_scheduled_label_with_star(&sched, false);

        let has_time = label.contains("Today")
            || label.contains("Tomorrow")
            || ["Mon", "Tue", "Wed", "Thu", "Fri", "Sat", "Sun"]
                .iter()
                .any(|d| label.contains(d));

        assert!(has_time, "label should contain time info: {}", label);
    }

    #[test]
    fn format_scheduled_label_sparkle_for_inferred() {
        let mut sched = make_scheduled("Streamer", 3);
        sched.is_inferred = true;
        let label = format_scheduled_label_with_star(&sched, false);

        assert!(
            label.starts_with('\u{2728}'),
            "inferred should start with ✨"
        );
    }

    #[test]
    fn format_scheduled_label_star_for_favourite() {
        let sched = make_scheduled("Streamer", 3);
        let label = format_scheduled_label_with_star(&sched, true);

        assert!(label.contains('\u{2605}'), "favourite should contain ★");
    }

    #[test]
    fn format_scheduled_label_sparkle_and_star() {
        let mut sched = make_scheduled("Streamer", 3);
        sched.is_inferred = true;
        let label = format_scheduled_label_with_star(&sched, true);

        assert!(label.starts_with('\u{2728}'), "should start with ✨");
        assert!(label.contains('\u{2605}'), "should also contain ★");
    }

    // =========================================================
    // compute_display_state — live section
    // =========================================================

    #[test]
    fn ignore_streamers_filtered_from_live() {
        // make_stream uses user_name.to_lowercase() as user_login, so names must
        // match the settings keys exactly when lowercased.
        let streams = vec![
            make_stream("1", "ignoreuser"),
            make_stream("2", "normaluser"),
        ];
        let (cats, cat_streams) = no_categories();

        let state = compute_display_state(
            streams,
            no_scheduled(),
            true,
            cats,
            cat_streams,
            &config_with_importance("ignoreuser", StreamerImportance::Ignore),
            Utc::now(),
        );

        let all_live: Vec<_> = state
            .live_section
            .visible
            .iter()
            .chain(state.live_section.overflow.iter())
            .collect();

        assert_eq!(all_live.len(), 1);
        assert_eq!(all_live[0].stream.user_login, "normaluser");
    }

    #[test]
    fn live_streams_sorted_favourites_first() {
        // normal_high has more viewers, fav_low is a favourite — fav should appear first
        let mut normal_high = stream_with_viewers("normal_high", 50_000);
        normal_high.user_login = "normal_high".to_string();
        let mut fav_low = stream_with_viewers("fav_low", 100);
        fav_low.user_login = "fav_low".to_string();

        let (cats, cat_streams) = no_categories();

        let state = compute_display_state(
            vec![normal_high, fav_low],
            no_scheduled(),
            true,
            cats,
            cat_streams,
            &config_with_importance("fav_low", StreamerImportance::Favourite),
            Utc::now(),
        );

        assert_eq!(
            state.live_section.visible[0].stream.user_login, "fav_low",
            "favourite should appear first despite lower viewer count"
        );
        assert_eq!(
            state.live_section.visible[1].stream.user_login,
            "normal_high"
        );
    }

    #[test]
    fn live_streams_sorted_by_viewers_within_group() {
        let mut s1 = stream_with_viewers("low", 1_000);
        s1.user_login = "low".to_string();
        let mut s2 = stream_with_viewers("high", 10_000);
        s2.user_login = "high".to_string();
        let (cats, cat_streams) = no_categories();

        let state = compute_display_state(
            vec![s1, s2],
            no_scheduled(),
            true,
            cats,
            cat_streams,
            &default_config(),
            Utc::now(),
        );

        assert_eq!(state.live_section.visible[0].stream.user_login, "high");
        assert_eq!(state.live_section.visible[1].stream.user_login, "low");
    }

    #[test]
    fn live_overflow_split_at_limit() {
        let streams: Vec<Stream> = (0..12)
            .map(|i| {
                let name = format!("streamer{i}");
                make_stream(&name, &name)
            })
            .collect();
        let (cats, cat_streams) = no_categories();

        let state = compute_display_state(
            streams,
            no_scheduled(),
            true,
            cats,
            cat_streams,
            &default_config(),
            Utc::now(),
        );

        assert_eq!(
            state.live_section.visible.len(),
            10,
            "visible capped at limit"
        );
        assert_eq!(
            state.live_section.overflow.len(),
            2,
            "remainder in overflow"
        );
    }

    #[test]
    fn live_within_limit_no_overflow() {
        let streams: Vec<Stream> = (0..5)
            .map(|i| {
                let name = format!("s{i}");
                make_stream(&name, &name)
            })
            .collect();
        let (cats, cat_streams) = no_categories();

        let state = compute_display_state(
            streams,
            no_scheduled(),
            true,
            cats,
            cat_streams,
            &default_config(),
            Utc::now(),
        );

        assert_eq!(state.live_section.visible.len(), 5);
        assert!(state.live_section.overflow.is_empty());
    }

    #[test]
    fn favourite_live_stream_has_star_in_label() {
        let streams = vec![make_stream("favuser", "FavUser")];
        let (cats, cat_streams) = no_categories();

        let state = compute_display_state(
            streams,
            no_scheduled(),
            true,
            cats,
            cat_streams,
            &config_with_importance("favuser", StreamerImportance::Favourite),
            Utc::now(),
        );

        assert!(
            state.live_section.visible[0].label.contains('\u{2605}'),
            "favourite label should contain ★"
        );
    }

    // =========================================================
    // compute_display_state — schedule section
    // =========================================================

    #[test]
    fn ignore_streamers_filtered_from_schedule() {
        let scheduled = vec![make_scheduled("ignored_bc", 2)];
        let (cats, cat_streams) = no_categories();

        let state = compute_display_state(
            vec![],
            scheduled,
            true,
            cats,
            cat_streams,
            &config_with_importance("ignored_bc", StreamerImportance::Ignore),
            Utc::now(),
        );

        let total = state.schedule_section.visible.len() + state.schedule_section.overflow.len();
        assert_eq!(total, 0, "ignored streamer's schedule should be filtered");
    }

    #[test]
    fn schedule_hidden_when_broadcaster_live_within_window() {
        let now = Utc::now();
        // Broadcast starts 30 minutes from now — within the 60-min window
        let mut sched = make_scheduled("livebroadcaster", 0);
        sched.broadcaster_login = "livebroadcaster".to_string();
        sched.start_time = now + Duration::minutes(30);

        // The broadcaster is live
        let live_stream = make_stream("lid", "LiveBroadcaster");
        let mut live = live_stream;
        live.user_login = "livebroadcaster".to_string();

        let (cats, cat_streams) = no_categories();

        let state = compute_display_state(
            vec![live],
            vec![sched],
            true,
            cats,
            cat_streams,
            &default_config(),
            now,
        );

        let total = state.schedule_section.visible.len() + state.schedule_section.overflow.len();
        assert_eq!(
            total, 0,
            "schedule within 60-min window of live stream should be hidden"
        );
    }

    #[test]
    fn schedule_shown_when_broadcaster_live_but_far_in_future() {
        let now = Utc::now();
        // Broadcast starts 90 minutes from now — outside the 60-min window
        let mut sched = make_scheduled("livebc", 0);
        sched.broadcaster_login = "livebc".to_string();
        sched.start_time = now + Duration::minutes(90);

        let mut live = make_stream("lid", "LiveBc");
        live.user_login = "livebc".to_string();

        let (cats, cat_streams) = no_categories();

        let state = compute_display_state(
            vec![live],
            vec![sched],
            true,
            cats,
            cat_streams,
            &default_config(),
            now,
        );

        let total = state.schedule_section.visible.len() + state.schedule_section.overflow.len();
        assert_eq!(total, 1, "schedule outside window should still be shown");
    }

    #[test]
    fn schedule_shown_when_broadcaster_not_live() {
        let sched = make_scheduled("offlinebc", 2);
        let (cats, cat_streams) = no_categories();

        let state = compute_display_state(
            vec![], // no live streams
            vec![sched],
            true,
            cats,
            cat_streams,
            &default_config(),
            Utc::now(),
        );

        let total = state.schedule_section.visible.len() + state.schedule_section.overflow.len();
        assert_eq!(total, 1, "offline broadcaster's schedule should be shown");
    }

    #[test]
    fn schedule_overflow_split_at_limit() {
        let scheduled: Vec<_> = (0..7)
            .map(|i| make_scheduled(&format!("bc{i}"), i as i64 + 1))
            .collect();
        let (cats, cat_streams) = no_categories();

        let state = compute_display_state(
            vec![],
            scheduled,
            true,
            cats,
            cat_streams,
            &default_config(),
            Utc::now(),
        );

        assert_eq!(state.schedule_section.visible.len(), 5);
        assert_eq!(state.schedule_section.overflow.len(), 2);
    }

    #[test]
    fn schedules_loaded_false_propagated() {
        let (cats, cat_streams) = no_categories();
        let state = compute_display_state(
            vec![],
            vec![],
            false, // not yet loaded
            cats,
            cat_streams,
            &default_config(),
            Utc::now(),
        );

        assert!(!state.schedule_section.schedules_loaded);
    }

    #[test]
    fn schedule_header_includes_lookahead_hours() {
        let (cats, cat_streams) = no_categories();
        let state = compute_display_state(
            vec![],
            vec![],
            true,
            cats,
            cat_streams,
            &DisplayConfig {
                schedule_lookahead_hours: 12,
                ..default_config()
            },
            Utc::now(),
        );

        assert_eq!(state.schedule_section.header, "Scheduled (Next 12h)");
    }

    #[test]
    fn inferred_schedule_has_sparkle_in_label() {
        let mut sched = make_scheduled("inferredbc", 2);
        sched.is_inferred = true;
        let (cats, cat_streams) = no_categories();

        let state = compute_display_state(
            vec![],
            vec![sched],
            true,
            cats,
            cat_streams,
            &default_config(),
            Utc::now(),
        );

        assert!(
            state.schedule_section.visible[0]
                .label
                .starts_with('\u{2728}'),
            "inferred schedule label should start with ✨"
        );
    }

    #[test]
    fn favourite_schedule_has_star_in_label() {
        let mut sched = make_scheduled("favbc", 2);
        sched.broadcaster_login = "favbc".to_string();
        let (cats, cat_streams) = no_categories();

        let state = compute_display_state(
            vec![],
            vec![sched],
            true,
            cats,
            cat_streams,
            &config_with_importance("favbc", StreamerImportance::Favourite),
            Utc::now(),
        );

        assert!(
            state.schedule_section.visible[0].label.contains('\u{2605}'),
            "favourite schedule label should contain ★"
        );
    }

    // =========================================================
    // compute_display_state — category sections
    // =========================================================

    #[test]
    fn category_section_built_from_followed_categories() {
        let cats = vec![FollowedCategory {
            id: "cat1".to_string(),
            name: "Minecraft".to_string(),
        }];
        let mut cat_streams = HashMap::new();
        cat_streams.insert(
            "cat1".to_string(),
            vec![make_stream("mc_streamer", "McStreamer")],
        );

        let state = compute_display_state(
            vec![],
            no_scheduled(),
            true,
            cats,
            cat_streams,
            &default_config(),
            Utc::now(),
        );

        assert_eq!(state.category_sections.len(), 1);
        assert!(state.category_sections[0].header.contains("Minecraft"));
        assert_eq!(state.category_sections[0].entries.len(), 1);
    }

    #[test]
    fn category_section_empty_when_no_streams() {
        let cats = vec![FollowedCategory {
            id: "cat1".to_string(),
            name: "Minecraft".to_string(),
        }];
        let cat_streams = HashMap::new(); // no streams for cat1

        let state = compute_display_state(
            vec![],
            no_scheduled(),
            true,
            cats,
            cat_streams,
            &default_config(),
            Utc::now(),
        );

        assert!(
            state.category_sections.is_empty(),
            "no section created when category has no streams"
        );
    }
}

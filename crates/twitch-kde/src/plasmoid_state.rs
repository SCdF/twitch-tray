use std::collections::{HashMap, HashSet};

use chrono::{DateTime, Duration, Utc};
use twitch_backend::{
    config::{StreamerImportance, StreamerSettings},
    handle::{LoginProgress, RawDisplayData},
    twitch::{format_viewer_count, ScheduledStream, Stream},
};

use crate::dto::{
    CategorySectionDto, CategoryStreamDto, LiveSectionDto, LiveStreamDto, LoginStateDto,
    PlasmoidState, ScheduleSectionDto, ScheduledStreamDto,
};

/// A scheduled stream within this many minutes of a live broadcast is hidden from the schedule.
const LIVE_COVERS_SCHEDULE_WINDOW_MIN: i64 = 60;

fn get_importance(
    user_login: &str,
    settings: &HashMap<String, StreamerSettings>,
) -> StreamerImportance {
    settings
        .get(user_login)
        .map(|s| s.importance)
        .unwrap_or_default()
}

fn map_login_state(login_progress: Option<&LoginProgress>) -> LoginStateDto {
    match login_progress {
        None | Some(LoginProgress::Confirmed | LoginProgress::Failed(_)) => LoginStateDto::Idle,
        Some(LoginProgress::PendingCode {
            user_code,
            verification_uri,
        }) => LoginStateDto::PendingCode {
            user_code: user_code.clone(),
            verification_uri: verification_uri.clone(),
        },
    }
}

fn live_stream_to_dto(s: Stream, settings: &HashMap<String, StreamerSettings>) -> LiveStreamDto {
    let is_favourite = get_importance(&s.user_login, settings) == StreamerImportance::Favourite;
    let viewer_count_formatted = s.format_viewer_count();
    let duration_formatted = s.format_duration();
    LiveStreamDto {
        user_login: s.user_login,
        user_name: s.user_name,
        game_name: s.game_name,
        title: s.title,
        profile_image_url: s.profile_image_url,
        viewer_count_formatted,
        duration_formatted,
        is_favourite,
    }
}

fn scheduled_to_dto(
    s: ScheduledStream,
    settings: &HashMap<String, StreamerSettings>,
    profile_image_urls: &HashMap<String, String>,
) -> ScheduledStreamDto {
    let is_favourite =
        get_importance(&s.broadcaster_login, settings) == StreamerImportance::Favourite;
    let start_time_formatted = s.format_start_time();
    let title = if s.is_inferred {
        String::new()
    } else {
        s.title.clone()
    };
    let category = if s.is_inferred {
        String::new()
    } else {
        s.category.clone().unwrap_or_default()
    };
    let profile_image_url = profile_image_urls
        .get(&s.broadcaster_id)
        .cloned()
        .unwrap_or_default();
    ScheduledStreamDto {
        broadcaster_login: s.broadcaster_login,
        broadcaster_name: s.broadcaster_name,
        start_time_formatted,
        title,
        category,
        profile_image_url,
        is_inferred: s.is_inferred,
        is_favourite,
    }
}

/// Computes a fully resolved `PlasmoidState` DTO from raw backend data.
///
/// This is a pure function — no Tauri, D-Bus, or async dependencies.
/// `login_progress` comes from `BackendHandle.login_progress_rx` and is passed
/// separately because it is not part of `RawDisplayData`.
/// `now` is passed in rather than calling `Utc::now()` so the function is
/// deterministically testable.
pub fn compute_plasmoid_state(
    raw: RawDisplayData,
    login_progress: Option<&LoginProgress>,
    now: DateTime<Utc>,
) -> PlasmoidState {
    if !raw.is_authenticated {
        return PlasmoidState {
            authenticated: false,
            login_state: map_login_state(login_progress),
            live: LiveSectionDto {
                visible: vec![],
                overflow: vec![],
            },
            categories: vec![],
            schedule: ScheduleSectionDto {
                lookahead_hours: raw.config.schedule_lookahead_hours,
                loaded: false,
                visible: vec![],
                overflow: vec![],
            },
        };
    }

    let settings = &raw.config.streamer_settings;

    // --- Live section ---

    let mut streams = raw.live_streams;
    streams.retain(|s| get_importance(&s.user_login, settings) != StreamerImportance::Ignore);

    let live_logins: HashSet<String> = streams.iter().map(|s| s.user_login.clone()).collect();

    streams.sort_by(|a, b| {
        let a_fav = get_importance(&a.user_login, settings) == StreamerImportance::Favourite;
        let b_fav = get_importance(&b.user_login, settings) == StreamerImportance::Favourite;
        b_fav.cmp(&a_fav).then(b.viewer_count.cmp(&a.viewer_count))
    });

    let live_limit = raw.config.live_menu_limit;
    let (live_visible_raw, live_overflow_raw) = if streams.len() > live_limit {
        let (main, over) = streams.split_at(live_limit);
        (main.to_vec(), over.to_vec())
    } else {
        (streams, vec![])
    };

    let live = LiveSectionDto {
        visible: live_visible_raw
            .into_iter()
            .map(|s| live_stream_to_dto(s, settings))
            .collect(),
        overflow: live_overflow_raw
            .into_iter()
            .map(|s| live_stream_to_dto(s, settings))
            .collect(),
    };

    // --- Category sections ---

    let mut categories = vec![];
    for category in &raw.followed_categories {
        if let Some(cat_streams) = raw.category_streams.get(&category.id) {
            if !cat_streams.is_empty() {
                let mut sorted = cat_streams.clone();
                sorted.sort_by(|a, b| b.viewer_count.cmp(&a.viewer_count));

                let total_viewers: u32 = sorted.iter().map(|s| s.viewer_count).sum();
                let streams_dto: Vec<CategoryStreamDto> = sorted
                    .into_iter()
                    .map(|s| {
                        let viewer_count_formatted = s.format_viewer_count();
                        CategoryStreamDto {
                            user_login: s.user_login,
                            user_name: s.user_name,
                            viewer_count_formatted,
                        }
                    })
                    .collect();

                let stream_count = streams_dto.len();
                let box_art_url = raw
                    .box_art_urls
                    .get(&category.id)
                    .cloned()
                    .unwrap_or_default();
                categories.push(CategorySectionDto {
                    id: category.id.clone(),
                    name: category.name.clone(),
                    box_art_url,
                    total_viewers_formatted: format_viewer_count(total_viewers),
                    stream_count_formatted: format!("{stream_count} live"),
                    streams: streams_dto,
                });
            }
        }
    }

    // --- Schedule section ---

    let soon_threshold = now + Duration::minutes(LIVE_COVERS_SCHEDULE_WINDOW_MIN);

    let mut scheduled = raw.scheduled_streams;
    scheduled
        .retain(|s| get_importance(&s.broadcaster_login, settings) != StreamerImportance::Ignore);
    scheduled.retain(|s| {
        !(live_logins.contains(&s.broadcaster_login) && s.start_time <= soon_threshold)
    });

    let schedule_limit = raw.config.schedule_menu_limit;
    let (sched_visible_raw, sched_overflow_raw) = if scheduled.len() > schedule_limit {
        let (main, over) = scheduled.split_at(schedule_limit);
        (main.to_vec(), over.to_vec())
    } else {
        (scheduled, vec![])
    };

    let schedule = ScheduleSectionDto {
        lookahead_hours: raw.config.schedule_lookahead_hours,
        loaded: raw.schedules_loaded,
        visible: sched_visible_raw
            .into_iter()
            .map(|s| scheduled_to_dto(s, settings, &raw.profile_image_urls))
            .collect(),
        overflow: sched_overflow_raw
            .into_iter()
            .map(|s| scheduled_to_dto(s, settings, &raw.profile_image_urls))
            .collect(),
    };

    PlasmoidState {
        authenticated: true,
        login_state: LoginStateDto::Idle,
        live,
        categories,
        schedule,
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use chrono::{Duration, Local, TimeZone, Utc};
    use twitch_backend::{
        config::{
            Config, FollowedCategory, StreamerImportance, StreamerSettings,
            DEFAULT_LIVE_MENU_LIMIT, DEFAULT_SCHEDULE_MENU_LIMIT,
        },
        handle::RawDisplayData,
        twitch::{ScheduledStream, Stream},
    };

    use super::*;
    use crate::dto::LoginStateDto;

    // =========================================================
    // Test helpers
    // =========================================================

    fn make_stream(user_id: &str, user_name: &str) -> Stream {
        Stream {
            id: format!("stream_{user_id}"),
            user_id: user_id.to_string(),
            user_login: user_name.to_lowercase(),
            user_name: user_name.to_string(),
            game_id: "game123".to_string(),
            game_name: "Test Game".to_string(),
            title: "Test Stream".to_string(),
            viewer_count: 1000,
            started_at: Utc::now() - Duration::hours(1),
            thumbnail_url: "https://example.com/thumb.jpg".to_string(),
            tags: vec![],
            profile_image_url: String::new(),
        }
    }

    fn make_scheduled(broadcaster_name: &str, hours_from_now: i64) -> ScheduledStream {
        ScheduledStream {
            id: format!("sched_{broadcaster_name}"),
            broadcaster_id: broadcaster_name.to_lowercase(),
            broadcaster_name: broadcaster_name.to_string(),
            broadcaster_login: broadcaster_name.to_lowercase(),
            title: "Scheduled Stream".to_string(),
            start_time: Utc::now() + Duration::hours(hours_from_now),
            end_time: None,
            category: Some("Gaming".to_string()),
            category_id: Some("123".to_string()),
            is_recurring: false,
            is_inferred: false,
        }
    }

    fn raw(streams: Vec<Stream>, scheduled: Vec<ScheduledStream>) -> RawDisplayData {
        RawDisplayData {
            is_authenticated: true,
            live_streams: streams,
            scheduled_streams: scheduled,
            schedules_loaded: true,
            followed_channels: vec![],
            followed_categories: vec![],
            category_streams: HashMap::new(),
            config: Config::default(),
            profile_image_urls: HashMap::new(),
            box_art_urls: HashMap::new(),
        }
    }

    fn raw_with_importance(
        user_login: &str,
        importance: StreamerImportance,
        streams: Vec<Stream>,
        scheduled: Vec<ScheduledStream>,
    ) -> RawDisplayData {
        let mut config = Config::default();
        config.streamer_settings.insert(
            user_login.to_string(),
            StreamerSettings {
                display_name: user_login.to_string(),
                importance,
            },
        );
        RawDisplayData {
            is_authenticated: true,
            live_streams: streams,
            scheduled_streams: scheduled,
            schedules_loaded: true,
            followed_channels: vec![],
            followed_categories: vec![],
            category_streams: HashMap::new(),
            config,
            profile_image_urls: HashMap::new(),
            box_art_urls: HashMap::new(),
        }
    }

    // =========================================================
    // Unauthenticated
    // =========================================================

    #[test]
    fn unauthenticated_raw_data_produces_unauthenticated_state() {
        let mut raw = raw(vec![], vec![]);
        raw.is_authenticated = false;
        let state = compute_plasmoid_state(raw, None, Utc::now());
        assert!(!state.authenticated);
        assert!(state.live.visible.is_empty());
        assert!(state.live.overflow.is_empty());
        assert!(state.categories.is_empty());
        assert!(state.schedule.visible.is_empty());
    }

    // =========================================================
    // Live section — filtering and sorting
    // =========================================================

    #[test]
    fn ignore_streamers_filtered_from_live() {
        let streams = vec![
            make_stream("1", "ignoreuser"),
            make_stream("2", "normaluser"),
        ];
        let raw = raw_with_importance("ignoreuser", StreamerImportance::Ignore, streams, vec![]);
        let state = compute_plasmoid_state(raw, None, Utc::now());

        let all: Vec<_> = state
            .live
            .visible
            .iter()
            .chain(state.live.overflow.iter())
            .collect();
        assert_eq!(all.len(), 1);
        assert_eq!(all[0].user_login, "normaluser");
    }

    #[test]
    fn live_streams_sorted_favourites_first() {
        let mut normal_high = make_stream("1", "normal_high");
        normal_high.viewer_count = 50_000;
        let mut fav_low = make_stream("2", "fav_low");
        fav_low.viewer_count = 100;

        let raw = raw_with_importance(
            "fav_low",
            StreamerImportance::Favourite,
            vec![normal_high, fav_low],
            vec![],
        );
        let state = compute_plasmoid_state(raw, None, Utc::now());

        assert_eq!(state.live.visible[0].user_login, "fav_low");
        assert_eq!(state.live.visible[1].user_login, "normal_high");
    }

    #[test]
    fn live_streams_sorted_by_viewers_within_group() {
        let mut low = make_stream("1", "low");
        low.viewer_count = 1_000;
        let mut high = make_stream("2", "high");
        high.viewer_count = 10_000;

        let state = compute_plasmoid_state(raw(vec![low, high], vec![]), None, Utc::now());

        assert_eq!(state.live.visible[0].user_login, "high");
        assert_eq!(state.live.visible[1].user_login, "low");
    }

    #[test]
    fn live_overflow_split_at_limit() {
        let streams: Vec<_> = (0..12)
            .map(|i| make_stream(&i.to_string(), &format!("streamer{i}")))
            .collect();
        let state = compute_plasmoid_state(raw(streams, vec![]), None, Utc::now());

        assert_eq!(state.live.visible.len(), DEFAULT_LIVE_MENU_LIMIT);
        assert_eq!(state.live.overflow.len(), 2);
    }

    #[test]
    fn favourite_stream_has_is_favourite_true() {
        let stream = make_stream("1", "favuser");
        let raw = raw_with_importance(
            "favuser",
            StreamerImportance::Favourite,
            vec![stream],
            vec![],
        );
        let state = compute_plasmoid_state(raw, None, Utc::now());

        assert!(state.live.visible[0].is_favourite);
    }

    // =========================================================
    // Live section — formatted fields
    // =========================================================

    #[test]
    fn title_mapped_to_dto() {
        let mut s = make_stream("1", "streamer");
        s.title = "Playing chess with viewers!".to_string();
        let state = compute_plasmoid_state(raw(vec![s], vec![]), None, Utc::now());
        assert_eq!(state.live.visible[0].title, "Playing chess with viewers!");
    }

    #[test]
    fn profile_image_url_mapped_to_dto() {
        let mut s = make_stream("1", "streamer");
        s.profile_image_url = "https://example.com/avatar.jpg".to_string();
        let state = compute_plasmoid_state(raw(vec![s], vec![]), None, Utc::now());
        assert_eq!(
            state.live.visible[0].profile_image_url,
            "https://example.com/avatar.jpg"
        );
    }

    #[test]
    fn viewer_count_formatted_correctly() {
        let mut s = make_stream("1", "streamer");
        s.viewer_count = 45_000;
        let state = compute_plasmoid_state(raw(vec![s], vec![]), None, Utc::now());
        assert_eq!(state.live.visible[0].viewer_count_formatted, "45k");

        let mut s2 = make_stream("2", "streamer2");
        s2.viewer_count = 856;
        let state2 = compute_plasmoid_state(raw(vec![s2], vec![]), None, Utc::now());
        assert_eq!(state2.live.visible[0].viewer_count_formatted, "856");
    }

    #[test]
    fn duration_formatted_correctly() {
        let mut s = make_stream("1", "streamer");
        s.started_at = Utc::now() - Duration::hours(2) - Duration::minutes(15);
        let state = compute_plasmoid_state(raw(vec![s], vec![]), None, Utc::now());
        assert_eq!(state.live.visible[0].duration_formatted, "2h 15m");
    }

    // =========================================================
    // Schedule section
    // =========================================================

    #[test]
    fn ignore_streamers_filtered_from_schedule() {
        let scheduled = vec![make_scheduled("ignored_bc", 2)];
        let raw = raw_with_importance("ignored_bc", StreamerImportance::Ignore, vec![], scheduled);
        let state = compute_plasmoid_state(raw, None, Utc::now());

        let total = state.schedule.visible.len() + state.schedule.overflow.len();
        assert_eq!(total, 0);
    }

    #[test]
    fn schedule_hidden_when_broadcaster_live_within_window() {
        let now = Utc::now();
        let mut sched = make_scheduled("livebc", 0);
        sched.broadcaster_login = "livebc".to_string();
        sched.start_time = now + Duration::minutes(30); // within 60-min window

        let mut live = make_stream("1", "LiveBc");
        live.user_login = "livebc".to_string();

        let state = compute_plasmoid_state(raw(vec![live], vec![sched]), None, now);

        let total = state.schedule.visible.len() + state.schedule.overflow.len();
        assert_eq!(total, 0);
    }

    #[test]
    fn schedule_shown_when_broadcaster_live_but_far_in_future() {
        let now = Utc::now();
        let mut sched = make_scheduled("livebc", 0);
        sched.broadcaster_login = "livebc".to_string();
        sched.start_time = now + Duration::minutes(90); // outside 60-min window

        let mut live = make_stream("1", "LiveBc");
        live.user_login = "livebc".to_string();

        let state = compute_plasmoid_state(raw(vec![live], vec![sched]), None, now);

        let total = state.schedule.visible.len() + state.schedule.overflow.len();
        assert_eq!(total, 1);
    }

    #[test]
    fn schedule_overflow_split_at_limit() {
        let scheduled: Vec<_> = (0..7)
            .map(|i| make_scheduled(&format!("bc{i}"), i as i64 + 1))
            .collect();
        let state = compute_plasmoid_state(raw(vec![], scheduled), None, Utc::now());

        assert_eq!(state.schedule.visible.len(), DEFAULT_SCHEDULE_MENU_LIMIT);
        assert_eq!(state.schedule.overflow.len(), 2);
    }

    #[test]
    fn inferred_schedule_has_is_inferred_true() {
        let mut sched = make_scheduled("inferredbc", 2);
        sched.is_inferred = true;
        let state = compute_plasmoid_state(raw(vec![], vec![sched]), None, Utc::now());

        assert!(state.schedule.visible[0].is_inferred);
    }

    #[test]
    fn start_time_formatted_correctly() {
        // Today's noon in local time → "Today ..."
        let now_local = Local::now();
        let today_noon = now_local.date_naive().and_hms_opt(12, 0, 0).unwrap();
        let today_utc = Local
            .from_local_datetime(&today_noon)
            .single()
            .unwrap()
            .with_timezone(&Utc);

        let mut sched = make_scheduled("testbc", 0);
        sched.start_time = today_utc;

        let state = compute_plasmoid_state(raw(vec![], vec![sched]), None, Utc::now());
        assert!(
            state.schedule.visible[0]
                .start_time_formatted
                .starts_with("Today "),
            "got: {}",
            state.schedule.visible[0].start_time_formatted
        );
    }

    // =========================================================
    // Category sections
    // =========================================================

    #[test]
    fn category_section_includes_all_streams_no_overflow() {
        let cat_id = "cat1".to_string();
        let cat_streams: Vec<_> = (0..15)
            .map(|i| make_stream(&i.to_string(), &format!("Streamer{i}")))
            .collect();

        let mut raw = raw(vec![], vec![]);
        raw.followed_categories = vec![FollowedCategory {
            id: cat_id.clone(),
            name: "Minecraft".to_string(),
        }];
        raw.category_streams = HashMap::from([(cat_id, cat_streams)]);

        let state = compute_plasmoid_state(raw, None, Utc::now());

        assert_eq!(state.categories.len(), 1);
        // all 15 streams present — no overflow for categories
        assert_eq!(state.categories[0].streams.len(), 15);
        assert_eq!(state.categories[0].id, "cat1");
        assert_eq!(state.categories[0].stream_count_formatted, "15 live");
    }

    #[test]
    fn category_section_includes_box_art_url_from_cache() {
        let cat_id = "cat1".to_string();
        let cat_streams = vec![make_stream("1", "Streamer1")];

        let mut raw = raw(vec![], vec![]);
        raw.followed_categories = vec![FollowedCategory {
            id: cat_id.clone(),
            name: "Minecraft".to_string(),
        }];
        raw.category_streams = HashMap::from([(cat_id.clone(), cat_streams)]);
        raw.box_art_urls =
            HashMap::from([(cat_id, "https://example.com/mc-144x192.jpg".to_string())]);

        let state = compute_plasmoid_state(raw, None, Utc::now());

        assert_eq!(
            state.categories[0].box_art_url,
            "https://example.com/mc-144x192.jpg"
        );
    }

    #[test]
    fn category_section_defaults_empty_box_art_url_when_not_cached() {
        let cat_id = "cat1".to_string();
        let cat_streams = vec![make_stream("1", "Streamer1")];

        let mut raw = raw(vec![], vec![]);
        raw.followed_categories = vec![FollowedCategory {
            id: cat_id.clone(),
            name: "Minecraft".to_string(),
        }];
        raw.category_streams = HashMap::from([(cat_id, cat_streams)]);

        let state = compute_plasmoid_state(raw, None, Utc::now());

        assert_eq!(state.categories[0].box_art_url, "");
    }

    // =========================================================
    // Login state mapping
    // =========================================================

    #[test]
    fn login_state_pending_code_propagated() {
        let mut raw = raw(vec![], vec![]);
        raw.is_authenticated = false;
        let progress = LoginProgress::PendingCode {
            user_code: "ABCD-1234".to_string(),
            verification_uri: "https://twitch.tv/activate".to_string(),
        };
        let state = compute_plasmoid_state(raw, Some(&progress), Utc::now());

        assert!(matches!(
            state.login_state,
            LoginStateDto::PendingCode { ref user_code, .. } if user_code == "ABCD-1234"
        ));
    }
}

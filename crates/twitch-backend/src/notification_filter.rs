use std::collections::HashMap;

use chrono::{DateTime, Utc};

use crate::config::{StreamerImportance, StreamerSettings};
use crate::state::{CategoryChange, StreamsUpdated};
use crate::twitch::Stream;

/// Streams and category changes that should be dispatched to the notifier.
pub struct NotificationDecision {
    pub streams_to_notify: Vec<Stream>,
    pub categories_to_notify: Vec<CategoryChange>,
}

/// Determines which notifications (if any) to send for a stream update event.
///
/// Returns an empty decision when:
/// - The initial load baseline is not yet complete (avoids startup spam)
/// - The gap since the previous event exceeds `max_gap_secs` (avoids floods
///   after wake from sleep/suspension)
///
/// Silent and Ignore streamers are always excluded regardless.
pub fn filter_notifications(
    event: &StreamsUpdated,
    last_event_time: Option<DateTime<Utc>>,
    now: DateTime<Utc>,
    max_gap_secs: u64,
    initial_load_done: bool,
    settings: &HashMap<String, StreamerSettings>,
) -> NotificationDecision {
    let empty = NotificationDecision {
        streams_to_notify: Vec::new(),
        categories_to_notify: Vec::new(),
    };

    // Suppress everything during the initial baseline load.
    if !initial_load_done {
        return empty;
    }

    // Suppress everything if the gap since the last event exceeds the threshold.
    // This protects against notification floods on wake from sleep/suspension.
    if let Some(last) = last_event_time {
        let elapsed = (now - last).num_seconds();
        if elapsed > max_gap_secs as i64 {
            tracing::info!(
                "Suppressing notifications: gap of {}s exceeds max of {}s",
                elapsed,
                max_gap_secs
            );
            return empty;
        }
    }

    // Filter by streamer importance — Silent and Ignore streamers are never notified.
    let is_silent_or_ignored = |user_login: &str| -> bool {
        let importance = settings
            .get(user_login)
            .map(|s| s.importance)
            .unwrap_or_default();
        importance == StreamerImportance::Silent || importance == StreamerImportance::Ignore
    };

    let streams_to_notify = event
        .newly_live
        .iter()
        .filter(|s| !is_silent_or_ignored(&s.user_login))
        .cloned()
        .collect();

    let categories_to_notify = event
        .category_changes
        .iter()
        .filter(|c| !is_silent_or_ignored(&c.stream.user_login))
        .cloned()
        .collect();

    NotificationDecision {
        streams_to_notify,
        categories_to_notify,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::StreamerSettings;
    use chrono::Duration;

    fn make_stream(user_login: &str) -> Stream {
        Stream {
            id: "1".to_string(),
            user_id: "100".to_string(),
            user_login: user_login.to_string(),
            user_name: user_login.to_string(),
            game_id: "game".to_string(),
            game_name: "Game".to_string(),
            title: "Title".to_string(),
            viewer_count: 1000,
            started_at: Utc::now() - Duration::hours(1),
            thumbnail_url: String::new(),
            tags: vec![],
            profile_image_url: String::new(),
        }
    }

    fn make_event(
        newly_live: Vec<Stream>,
        category_changes: Vec<CategoryChange>,
    ) -> StreamsUpdated {
        StreamsUpdated {
            streams: newly_live.clone(),
            newly_live,
            category_changes,
        }
    }

    fn settings_with(
        user_login: &str,
        importance: StreamerImportance,
    ) -> HashMap<String, StreamerSettings> {
        let mut map = HashMap::new();
        map.insert(
            user_login.to_string(),
            StreamerSettings {
                display_name: user_login.to_string(),
                importance,
                hotness_z_threshold_override: None,
            },
        );
        map
    }

    // === Initial load suppression ===

    #[test]
    fn notifications_suppressed_during_initial_load() {
        let event = make_event(vec![make_stream("streamer")], vec![]);
        let now = Utc::now();
        let decision = filter_notifications(&event, None, now, 600, false, &HashMap::new());
        assert!(decision.streams_to_notify.is_empty());
        assert!(decision.categories_to_notify.is_empty());
    }

    #[test]
    fn notifications_allowed_after_initial_load() {
        let event = make_event(vec![make_stream("streamer")], vec![]);
        let now = Utc::now();
        let decision = filter_notifications(&event, None, now, 600, true, &HashMap::new());
        assert_eq!(decision.streams_to_notify.len(), 1);
    }

    // === Sleep gap suppression ===

    #[test]
    fn first_event_is_never_suppressed_by_gap() {
        let event = make_event(vec![make_stream("streamer")], vec![]);
        let now = Utc::now();
        // last_event_time = None → no gap to check
        let decision = filter_notifications(&event, None, now, 600, true, &HashMap::new());
        assert_eq!(decision.streams_to_notify.len(), 1);
    }

    #[test]
    fn recent_event_not_suppressed_by_gap() {
        let event = make_event(vec![make_stream("streamer")], vec![]);
        let now = Utc::now();
        let last = now - Duration::seconds(60); // 60s ago, within 600s limit
        let decision = filter_notifications(&event, Some(last), now, 600, true, &HashMap::new());
        assert_eq!(decision.streams_to_notify.len(), 1);
    }

    #[test]
    fn event_exactly_at_boundary_not_suppressed() {
        let event = make_event(vec![make_stream("streamer")], vec![]);
        let now = Utc::now();
        let last = now - Duration::seconds(600); // exactly at 600s limit — not suppressed
        let decision = filter_notifications(&event, Some(last), now, 600, true, &HashMap::new());
        assert_eq!(decision.streams_to_notify.len(), 1);
    }

    #[test]
    fn notifications_suppressed_after_sleep_gap() {
        let event = make_event(vec![make_stream("streamer")], vec![]);
        let now = Utc::now();
        let last = now - Duration::seconds(601); // 1s over limit
        let decision = filter_notifications(&event, Some(last), now, 600, true, &HashMap::new());
        assert!(decision.streams_to_notify.is_empty());
    }

    #[test]
    fn very_long_gap_suppresses() {
        let event = make_event(vec![make_stream("streamer")], vec![]);
        let now = Utc::now();
        let last = now - Duration::hours(8);
        let decision = filter_notifications(&event, Some(last), now, 600, true, &HashMap::new());
        assert!(decision.streams_to_notify.is_empty());
    }

    #[test]
    fn custom_max_gap_respected() {
        let event = make_event(vec![make_stream("streamer")], vec![]);
        let now = Utc::now();
        let last = now - Duration::seconds(180); // 3 min > 2 min limit
        let decision = filter_notifications(&event, Some(last), now, 120, true, &HashMap::new());
        assert!(decision.streams_to_notify.is_empty());
    }

    // === Importance filtering ===

    #[test]
    fn silent_streamers_excluded_from_live_notifications() {
        let event = make_event(vec![make_stream("quietstreamer")], vec![]);
        let now = Utc::now();
        let settings = settings_with("quietstreamer", StreamerImportance::Silent);
        let decision = filter_notifications(&event, None, now, 600, true, &settings);
        assert!(decision.streams_to_notify.is_empty());
    }

    #[test]
    fn ignored_streamers_excluded_from_live_notifications() {
        let event = make_event(vec![make_stream("ignoredstreamer")], vec![]);
        let now = Utc::now();
        let settings = settings_with("ignoredstreamer", StreamerImportance::Ignore);
        let decision = filter_notifications(&event, None, now, 600, true, &settings);
        assert!(decision.streams_to_notify.is_empty());
    }

    #[test]
    fn normal_streamers_included() {
        let event = make_event(vec![make_stream("normalstreamer")], vec![]);
        let now = Utc::now();
        let settings = settings_with("normalstreamer", StreamerImportance::Normal);
        let decision = filter_notifications(&event, None, now, 600, true, &settings);
        assert_eq!(decision.streams_to_notify.len(), 1);
    }

    #[test]
    fn favourite_streamers_included() {
        let event = make_event(vec![make_stream("favstreamer")], vec![]);
        let now = Utc::now();
        let settings = settings_with("favstreamer", StreamerImportance::Favourite);
        let decision = filter_notifications(&event, None, now, 600, true, &settings);
        assert_eq!(decision.streams_to_notify.len(), 1);
    }

    #[test]
    fn unknown_streamers_use_normal_importance() {
        let event = make_event(vec![make_stream("unknownstreamer")], vec![]);
        let now = Utc::now();
        // No settings entry → defaults to Normal → included
        let decision = filter_notifications(&event, None, now, 600, true, &HashMap::new());
        assert_eq!(decision.streams_to_notify.len(), 1);
    }

    #[test]
    fn silent_streamer_excluded_from_category_changes() {
        let stream = make_stream("quietstreamer");
        let change = CategoryChange {
            stream,
            old_category: "Old Game".to_string(),
        };
        let event = make_event(vec![], vec![change]);
        let now = Utc::now();
        let settings = settings_with("quietstreamer", StreamerImportance::Silent);
        let decision = filter_notifications(&event, None, now, 600, true, &settings);
        assert!(decision.categories_to_notify.is_empty());
    }

    #[test]
    fn mixed_importance_only_normal_notified() {
        let silent = make_stream("silentone");
        let normal = make_stream("normalone");
        let event = make_event(vec![silent, normal], vec![]);
        let now = Utc::now();
        let mut settings = HashMap::new();
        settings.insert(
            "silentone".to_string(),
            StreamerSettings {
                display_name: "silentone".to_string(),
                importance: StreamerImportance::Silent,
                hotness_z_threshold_override: None,
            },
        );
        let decision = filter_notifications(&event, None, now, 600, true, &settings);
        assert_eq!(decision.streams_to_notify.len(), 1);
        assert_eq!(decision.streams_to_notify[0].user_login, "normalone");
    }
}

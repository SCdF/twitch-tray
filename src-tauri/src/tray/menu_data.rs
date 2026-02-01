//! Pure menu building logic, separated from Tauri-specific rendering
//!
//! This module contains all the logic for building menu data structures
//! that can be easily tested without Tauri dependencies.

use crate::twitch::{ScheduledStream, Stream};

/// Maximum number of live streams shown in the main menu
pub const LIVE_STREAM_LIMIT: usize = 10;

/// Maximum number of scheduled streams shown in the main menu
pub const SCHEDULED_STREAM_LIMIT: usize = 5;

/// An entry representing a live stream in the menu
#[derive(Debug, Clone, PartialEq)]
pub struct StreamEntry {
    pub id: String,
    pub label: String,
    pub user_login: String,
    pub viewer_count: i64,
}

/// An entry representing a scheduled stream in the menu
#[derive(Debug, Clone, PartialEq)]
pub struct ScheduledEntry {
    pub id: String,
    pub label: String,
    pub broadcaster_login: String,
}

/// Pure data structure representing the menu content
#[derive(Debug, Clone)]
pub struct MenuData {
    /// Live streams shown in main menu (sorted by viewers, max 10)
    pub live_streams: Vec<StreamEntry>,
    /// Live streams in overflow submenu
    pub live_overflow: Vec<StreamEntry>,
    /// Scheduled streams shown in main menu (sorted by time, max 5)
    pub scheduled: Vec<ScheduledEntry>,
    /// Scheduled streams in overflow submenu
    pub scheduled_overflow: Vec<ScheduledEntry>,
    /// Whether the user is authenticated
    pub authenticated: bool,
    /// Whether schedules have been loaded at least once
    pub schedules_loaded: bool,
}

impl MenuData {
    /// Builds menu data from application state
    pub fn from_state(
        mut streams: Vec<Stream>,
        scheduled: Vec<ScheduledStream>,
        authenticated: bool,
        schedules_loaded: bool,
    ) -> Self {
        // Sort streams by viewer count (highest first)
        streams.sort_by(|a, b| b.viewer_count.cmp(&a.viewer_count));

        // Split into main and overflow
        let (main_streams, overflow_streams) = if streams.len() > LIVE_STREAM_LIMIT {
            let (main, over) = streams.split_at(LIVE_STREAM_LIMIT);
            (main.to_vec(), over.to_vec())
        } else {
            (streams, Vec::new())
        };

        let live_streams: Vec<StreamEntry> = main_streams
            .iter()
            .map(|s| StreamEntry {
                id: format!("stream_{}", s.user_login),
                label: format_stream_label(s),
                user_login: s.user_login.clone(),
                viewer_count: s.viewer_count,
            })
            .collect();

        let live_overflow: Vec<StreamEntry> = overflow_streams
            .iter()
            .map(|s| StreamEntry {
                id: format!("stream_{}", s.user_login),
                label: format_stream_label(s),
                user_login: s.user_login.clone(),
                viewer_count: s.viewer_count,
            })
            .collect();

        // Scheduled streams are already sorted by time from the API
        let (main_scheduled, overflow_scheduled) = if scheduled.len() > SCHEDULED_STREAM_LIMIT {
            let (main, over) = scheduled.split_at(SCHEDULED_STREAM_LIMIT);
            (main.to_vec(), over.to_vec())
        } else {
            (scheduled, Vec::new())
        };

        let scheduled_entries: Vec<ScheduledEntry> = main_scheduled
            .iter()
            .map(|s| ScheduledEntry {
                id: format!("scheduled_{}", s.broadcaster_login),
                label: format_scheduled_label(s),
                broadcaster_login: s.broadcaster_login.clone(),
            })
            .collect();

        let scheduled_overflow_entries: Vec<ScheduledEntry> = overflow_scheduled
            .iter()
            .map(|s| ScheduledEntry {
                id: format!("scheduled_{}", s.broadcaster_login),
                label: format_scheduled_label(s),
                broadcaster_login: s.broadcaster_login.clone(),
            })
            .collect();

        Self {
            live_streams,
            live_overflow,
            scheduled: scheduled_entries,
            scheduled_overflow: scheduled_overflow_entries,
            authenticated,
            schedules_loaded,
        }
    }

    /// Returns the total count of live streams
    pub fn live_count(&self) -> usize {
        self.live_streams.len() + self.live_overflow.len()
    }

    /// Returns the total count of scheduled streams
    pub fn scheduled_count(&self) -> usize {
        self.scheduled.len() + self.scheduled_overflow.len()
    }

    /// Returns true if there are overflow live streams
    pub fn has_live_overflow(&self) -> bool {
        !self.live_overflow.is_empty()
    }

    /// Returns true if there are overflow scheduled streams
    pub fn has_scheduled_overflow(&self) -> bool {
        !self.scheduled_overflow.is_empty()
    }
}

/// Formats a stream for the menu label
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

/// Formats a scheduled stream for the menu label
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

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::{Duration, Utc};

    /// Helper to create a test stream
    fn make_stream(user_id: &str, user_name: &str, viewer_count: i64) -> Stream {
        Stream {
            id: format!("stream_{}", user_id),
            user_id: user_id.to_string(),
            user_login: user_name.to_lowercase(),
            user_name: user_name.to_string(),
            game_id: "game123".to_string(),
            game_name: "Test Game".to_string(),
            title: "Test Stream".to_string(),
            viewer_count,
            started_at: Utc::now() - Duration::hours(1),
            thumbnail_url: "https://example.com/thumb.jpg".to_string(),
            tags: vec![],
        }
    }

    /// Helper to create a scheduled stream
    fn make_scheduled(broadcaster_id: &str, broadcaster_name: &str, hours_from_now: i64) -> ScheduledStream {
        ScheduledStream {
            id: format!("sched_{}", broadcaster_id),
            broadcaster_id: broadcaster_id.to_string(),
            broadcaster_name: broadcaster_name.to_string(),
            broadcaster_login: broadcaster_name.to_lowercase(),
            title: "Scheduled Stream".to_string(),
            start_time: Utc::now() + Duration::hours(hours_from_now),
            end_time: None,
            category: Some("Gaming".to_string()),
            category_id: Some("123".to_string()),
            is_recurring: false,
        }
    }

    // === Sorting tests ===

    #[test]
    fn live_streams_sorted_by_viewers_highest_first() {
        let streams = vec![
            make_stream("1", "SmallStreamer", 100),
            make_stream("2", "BigStreamer", 5000),
            make_stream("3", "MediumStreamer", 1000),
        ];

        let menu = MenuData::from_state(streams, vec![], true, true);

        assert_eq!(menu.live_streams.len(), 3);
        assert_eq!(menu.live_streams[0].viewer_count, 5000);
        assert_eq!(menu.live_streams[1].viewer_count, 1000);
        assert_eq!(menu.live_streams[2].viewer_count, 100);
    }

    // === Overflow tests ===

    #[test]
    fn overflow_at_10_live_streams() {
        let streams: Vec<Stream> = (0..15)
            .map(|i| make_stream(&i.to_string(), &format!("Streamer{}", i), (15 - i) as i64 * 100))
            .collect();

        let menu = MenuData::from_state(streams, vec![], true, true);

        assert_eq!(menu.live_streams.len(), 10);
        assert_eq!(menu.live_overflow.len(), 5);
        assert_eq!(menu.live_count(), 15);
        assert!(menu.has_live_overflow());
    }

    #[test]
    fn no_overflow_under_10_streams() {
        let streams: Vec<Stream> = (0..8)
            .map(|i| make_stream(&i.to_string(), &format!("Streamer{}", i), (8 - i) as i64 * 100))
            .collect();

        let menu = MenuData::from_state(streams, vec![], true, true);

        assert_eq!(menu.live_streams.len(), 8);
        assert!(menu.live_overflow.is_empty());
        assert!(!menu.has_live_overflow());
    }

    #[test]
    fn exactly_10_streams_no_overflow() {
        let streams: Vec<Stream> = (0..10)
            .map(|i| make_stream(&i.to_string(), &format!("Streamer{}", i), (10 - i) as i64 * 100))
            .collect();

        let menu = MenuData::from_state(streams, vec![], true, true);

        assert_eq!(menu.live_streams.len(), 10);
        assert!(menu.live_overflow.is_empty());
    }

    #[test]
    fn scheduled_overflow_at_5() {
        let scheduled: Vec<ScheduledStream> = (0..8)
            .map(|i| make_scheduled(&i.to_string(), &format!("Broadcaster{}", i), i as i64 + 1))
            .collect();

        let menu = MenuData::from_state(vec![], scheduled, true, true);

        assert_eq!(menu.scheduled.len(), 5);
        assert_eq!(menu.scheduled_overflow.len(), 3);
        assert_eq!(menu.scheduled_count(), 8);
        assert!(menu.has_scheduled_overflow());
    }

    #[test]
    fn no_scheduled_overflow_under_5() {
        let scheduled: Vec<ScheduledStream> = (0..3)
            .map(|i| make_scheduled(&i.to_string(), &format!("Broadcaster{}", i), i as i64 + 1))
            .collect();

        let menu = MenuData::from_state(vec![], scheduled, true, true);

        assert_eq!(menu.scheduled.len(), 3);
        assert!(menu.scheduled_overflow.is_empty());
        assert!(!menu.has_scheduled_overflow());
    }

    // === Empty state tests ===

    #[test]
    fn empty_streams() {
        let menu = MenuData::from_state(vec![], vec![], true, true);

        assert!(menu.live_streams.is_empty());
        assert!(menu.live_overflow.is_empty());
        assert_eq!(menu.live_count(), 0);
    }

    #[test]
    fn empty_scheduled() {
        let menu = MenuData::from_state(vec![], vec![], true, true);

        assert!(menu.scheduled.is_empty());
        assert!(menu.scheduled_overflow.is_empty());
        assert_eq!(menu.scheduled_count(), 0);
    }

    // === Authentication state tests ===

    #[test]
    fn unauthenticated_menu() {
        let menu = MenuData::from_state(vec![], vec![], false, false);

        assert!(!menu.authenticated);
        assert!(!menu.schedules_loaded);
    }

    #[test]
    fn authenticated_schedules_not_loaded() {
        let menu = MenuData::from_state(vec![], vec![], true, false);

        assert!(menu.authenticated);
        assert!(!menu.schedules_loaded);
    }

    // === Label formatting tests ===

    #[test]
    fn stream_entry_has_correct_id_format() {
        let stream = make_stream("123", "TestStreamer", 1000);
        let menu = MenuData::from_state(vec![stream], vec![], true, true);

        assert_eq!(menu.live_streams[0].id, "stream_teststreamer");
        assert_eq!(menu.live_streams[0].user_login, "teststreamer");
    }

    #[test]
    fn scheduled_entry_has_correct_id_format() {
        let scheduled = make_scheduled("456", "TestBroadcaster", 2);
        let menu = MenuData::from_state(vec![], vec![scheduled], true, true);

        assert_eq!(menu.scheduled[0].id, "scheduled_testbroadcaster");
        assert_eq!(menu.scheduled[0].broadcaster_login, "testbroadcaster");
    }

    // === truncate function tests ===

    #[test]
    fn truncate_short_string() {
        assert_eq!(truncate("Hello", 10), "Hello");
    }

    #[test]
    fn truncate_long_string() {
        assert_eq!(truncate("Hello World", 8), "Hello...");
    }

    #[test]
    fn truncate_exact_length() {
        assert_eq!(truncate("Hello", 5), "Hello");
    }

    #[test]
    fn truncate_to_3_chars() {
        assert_eq!(truncate("Hello", 3), "Hel");
    }

    #[test]
    fn truncate_game_name() {
        let long_game = "Counter-Strike: Global Offensive";
        let truncated = truncate(long_game, 20);
        assert_eq!(truncated, "Counter-Strike: G...");
        assert_eq!(truncated.len(), 20);
    }
}

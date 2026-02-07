//! Common test utilities for integration tests

use chrono::{Duration, Utc};
use twitch_tray::twitch::{ScheduledStream, Stream};

/// Creates a test stream
pub fn make_stream(user_id: &str, user_name: &str, viewer_count: i64) -> Stream {
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

/// Creates a scheduled stream
pub fn make_scheduled(
    broadcaster_id: &str,
    broadcaster_name: &str,
    hours_from_now: i64,
) -> ScheduledStream {
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
        is_inferred: false,
    }
}

/// Creates many scheduled streams
pub fn make_many_scheduled(count: usize) -> Vec<ScheduledStream> {
    (0..count)
        .map(|i| make_scheduled(&i.to_string(), &format!("Broadcaster{}", i), i as i64 + 1))
        .collect()
}

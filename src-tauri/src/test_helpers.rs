//! Shared test helpers for unit and integration tests.
#![allow(dead_code)]
//!
//! Provides canonical constructors for test data so each module does not
//! need its own duplicated boilerplate.

use chrono::{Duration, Utc};

use crate::twitch::{ScheduledStream, Stream};

/// Creates a test stream with default values.
///
/// `user_id` is used for identity comparisons in state-change tests;
/// `user_name` becomes both `user_name` and (lowercased) `user_login`.
pub fn make_stream(user_id: &str, user_name: &str) -> Stream {
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
    }
}

/// Creates a test stream where `game_id` / `game_name` are meaningful.
///
/// Used in category-change detection tests.
pub fn make_stream_with_game(user_id: &str, game_id: &str, game_name: &str) -> Stream {
    Stream {
        id: format!("stream_{user_id}"),
        user_id: user_id.to_string(),
        user_login: format!("user_{user_id}"),
        user_name: format!("User {user_id}"),
        game_id: game_id.to_string(),
        game_name: game_name.to_string(),
        title: "Test Stream".to_string(),
        viewer_count: 1000,
        started_at: Utc::now() - Duration::hours(1),
        thumbnail_url: "https://example.com/thumb.jpg".to_string(),
        tags: vec![],
    }
}

/// Creates a scheduled stream starting `hours_from_now` hours in the future.
pub fn make_scheduled(broadcaster_name: &str, hours_from_now: i64) -> ScheduledStream {
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

/// Creates `count` sequential scheduled streams, each 1 hour later than the previous.
pub fn make_many_scheduled(count: usize) -> Vec<ScheduledStream> {
    (0..count)
        .map(|i| make_scheduled(&format!("Broadcaster{i}"), i as i64 + 1))
        .collect()
}

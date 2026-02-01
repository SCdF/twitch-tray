//! Test fixtures
//!
//! Pre-built test data for common testing scenarios.

use crate::twitch::{
    FollowedChannel, FollowedChannelsResponse, Pagination, ScheduledStream, Stream,
    StreamsResponse,
};
use chrono::{Duration, Utc};

use super::builders::{ScheduledStreamBuilder, StreamBuilder};

/// Creates a vector of test streams with varying viewer counts
pub fn sample_streams() -> Vec<Stream> {
    vec![
        StreamBuilder::new()
            .user_id("1")
            .streamer("BigStreamer")
            .game("1", "Fortnite")
            .viewers(50000)
            .started_hours_ago(3)
            .build(),
        StreamBuilder::new()
            .user_id("2")
            .streamer("MediumStreamer")
            .game("2", "Minecraft")
            .viewers(5000)
            .started_hours_ago(1)
            .build(),
        StreamBuilder::new()
            .user_id("3")
            .streamer("SmallStreamer")
            .game("3", "Valorant")
            .viewers(500)
            .started_minutes_ago(30)
            .build(),
    ]
}

/// Creates a large number of streams for overflow testing
pub fn many_streams(count: usize) -> Vec<Stream> {
    (0..count)
        .map(|i| {
            StreamBuilder::new()
                .user_id(format!("{}", i))
                .streamer(format!("Streamer{}", i))
                .game(format!("{}", i), format!("Game{}", i))
                .viewers((count - i) as i64 * 100)
                .build()
        })
        .collect()
}

/// Creates a vector of test scheduled streams
pub fn sample_scheduled_streams() -> Vec<ScheduledStream> {
    vec![
        ScheduledStreamBuilder::new()
            .id("sched1")
            .broadcaster("Streamer1")
            .title("Morning Stream")
            .starts_in_hours(2)
            .build(),
        ScheduledStreamBuilder::new()
            .id("sched2")
            .broadcaster("Streamer2")
            .title("Evening Stream")
            .starts_in_hours(8)
            .build(),
        ScheduledStreamBuilder::new()
            .id("sched3")
            .broadcaster("Streamer3")
            .title("Night Stream")
            .starts_in_hours(12)
            .build(),
    ]
}

/// Creates many scheduled streams for overflow testing
pub fn many_scheduled_streams(count: usize) -> Vec<ScheduledStream> {
    (0..count)
        .map(|i| {
            ScheduledStreamBuilder::new()
                .id(format!("sched{}", i))
                .broadcaster(format!("Broadcaster{}", i))
                .title(format!("Stream {}", i))
                .starts_in_hours(i as i64 + 1)
                .build()
        })
        .collect()
}

/// Creates sample followed channels
pub fn sample_followed_channels() -> Vec<FollowedChannel> {
    vec![
        FollowedChannel {
            broadcaster_id: "1".to_string(),
            broadcaster_login: "streamer1".to_string(),
            broadcaster_name: "Streamer1".to_string(),
            followed_at: Utc::now() - Duration::days(30),
        },
        FollowedChannel {
            broadcaster_id: "2".to_string(),
            broadcaster_login: "streamer2".to_string(),
            broadcaster_name: "Streamer2".to_string(),
            followed_at: Utc::now() - Duration::days(60),
        },
        FollowedChannel {
            broadcaster_id: "3".to_string(),
            broadcaster_login: "streamer3".to_string(),
            broadcaster_name: "Streamer3".to_string(),
            followed_at: Utc::now() - Duration::days(90),
        },
    ]
}

/// Creates a StreamsResponse for API mocking
pub fn streams_response(streams: Vec<Stream>, cursor: Option<&str>) -> StreamsResponse {
    StreamsResponse {
        data: streams,
        pagination: cursor.map(|c| Pagination {
            cursor: Some(c.to_string()),
        }),
    }
}

/// Creates a FollowedChannelsResponse for API mocking
pub fn followed_channels_response(
    channels: Vec<FollowedChannel>,
    cursor: Option<&str>,
) -> FollowedChannelsResponse {
    FollowedChannelsResponse {
        data: channels,
        pagination: cursor.map(|c| Pagination {
            cursor: Some(c.to_string()),
        }),
    }
}

/// Creates a stream that just went live (for notification testing)
pub fn just_went_live_stream(user_name: &str) -> Stream {
    StreamBuilder::new()
        .streamer(user_name)
        .started_minutes_ago(1)
        .build()
}

/// Creates an expired token fixture
pub fn expired_token() -> crate::auth::Token {
    crate::auth::Token {
        access_token: "expired_access_token".to_string(),
        refresh_token: "refresh_token".to_string(),
        expires_at: Utc::now() - Duration::hours(1),
        scopes: vec!["user:read:follows".to_string()],
        user_id: "123".to_string(),
        user_login: "testuser".to_string(),
    }
}

/// Creates a valid token fixture
pub fn valid_token() -> crate::auth::Token {
    crate::auth::Token {
        access_token: "valid_access_token".to_string(),
        refresh_token: "refresh_token".to_string(),
        expires_at: Utc::now() + Duration::hours(4),
        scopes: vec!["user:read:follows".to_string()],
        user_id: "123".to_string(),
        user_login: "testuser".to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sample_streams_are_sorted_by_viewers() {
        let streams = sample_streams();
        assert!(streams[0].viewer_count > streams[1].viewer_count);
        assert!(streams[1].viewer_count > streams[2].viewer_count);
    }

    #[test]
    fn many_streams_creates_correct_count() {
        let streams = many_streams(15);
        assert_eq!(streams.len(), 15);
    }

    #[test]
    fn many_streams_are_sorted_by_viewers() {
        let streams = many_streams(10);
        for i in 1..streams.len() {
            assert!(
                streams[i - 1].viewer_count >= streams[i].viewer_count,
                "Streams should be sorted by viewer count"
            );
        }
    }

    #[test]
    fn sample_scheduled_streams_are_sorted_by_time() {
        let scheduled = sample_scheduled_streams();
        for i in 1..scheduled.len() {
            assert!(
                scheduled[i - 1].start_time <= scheduled[i].start_time,
                "Scheduled streams should be sorted by start time"
            );
        }
    }

    #[test]
    fn valid_token_is_valid() {
        let token = valid_token();
        assert!(token.is_valid());
        assert!(!token.is_expired());
    }

    #[test]
    fn expired_token_is_expired() {
        let token = expired_token();
        assert!(token.is_expired());
        assert!(!token.is_valid());
    }
}

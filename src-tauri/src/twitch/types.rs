use chrono::{DateTime, Local, Utc};
use serde::{Deserialize, Serialize};

/// Represents a live stream
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Stream {
    pub id: String,
    pub user_id: String,
    pub user_login: String,
    pub user_name: String,
    pub game_id: String,
    pub game_name: String,
    pub title: String,
    pub viewer_count: i64,
    pub started_at: DateTime<Utc>,
    pub thumbnail_url: String,
    #[serde(default)]
    pub tags: Vec<String>,
}

impl Stream {
    /// Returns the duration since the stream started
    pub fn duration(&self) -> chrono::Duration {
        Utc::now().signed_duration_since(self.started_at)
    }

    /// Returns a human-readable duration string
    pub fn format_duration(&self) -> String {
        let duration = self.duration();
        let hours = duration.num_hours();
        let minutes = duration.num_minutes() % 60;

        if hours > 0 {
            format!("{}h {}m", hours, minutes)
        } else {
            format!("{}m", minutes)
        }
    }

    /// Returns a formatted viewer count
    pub fn format_viewer_count(&self) -> String {
        if self.viewer_count >= 1000 {
            let k = self.viewer_count as f64 / 1000.0;
            if k.fract() < 0.05 {
                format!("{}k", k as i64)
            } else {
                format!("{:.1}k", k)
            }
        } else {
            self.viewer_count.to_string()
        }
    }
}

/// Represents a scheduled broadcast
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScheduledStream {
    pub id: String,
    pub broadcaster_id: String,
    pub broadcaster_name: String,
    pub broadcaster_login: String,
    pub title: String,
    pub start_time: DateTime<Utc>,
    #[serde(default)]
    pub end_time: Option<DateTime<Utc>>,
    #[serde(default)]
    pub category: Option<String>,
    #[serde(default)]
    pub category_id: Option<String>,
    pub is_recurring: bool,
}

impl ScheduledStream {
    /// Returns a human-readable start time
    pub fn format_start_time(&self) -> String {
        let now = Local::now();
        let start_local = self.start_time.with_timezone(&Local);

        // Check if it's today
        if start_local.date_naive() == now.date_naive() {
            return format!("Today {}", start_local.format("%-I:%M %p"));
        }

        // Check if it's tomorrow
        let tomorrow = now.date_naive() + chrono::Duration::days(1);
        if start_local.date_naive() == tomorrow {
            return format!("Tomorrow {}", start_local.format("%-I:%M %p"));
        }

        // Otherwise show day and time
        start_local.format("%a %-I:%M %p").to_string()
    }
}

/// Represents a followed channel
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FollowedChannel {
    pub broadcaster_id: String,
    pub broadcaster_login: String,
    pub broadcaster_name: String,
    pub followed_at: DateTime<Utc>,
}

/// Helix API pagination
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct Pagination {
    #[serde(default)]
    pub cursor: Option<String>,
}

/// Followed channels response
#[derive(Debug, Clone, Deserialize)]
pub struct FollowedChannelsResponse {
    pub data: Vec<FollowedChannel>,
    #[serde(default)]
    pub pagination: Option<Pagination>,
}

/// Streams response from Helix API
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StreamsResponse {
    pub data: Vec<Stream>,
    #[serde(default)]
    pub pagination: Option<Pagination>,
}

/// Schedule segment from Helix API
#[derive(Debug, Clone, Deserialize)]
pub struct ScheduleSegment {
    pub id: String,
    pub start_time: DateTime<Utc>,
    #[serde(default)]
    pub end_time: Option<DateTime<Utc>>,
    pub title: String,
    #[serde(default)]
    pub canceled_until: Option<String>,
    #[serde(default)]
    pub category: Option<ScheduleCategory>,
    pub is_recurring: bool,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ScheduleCategory {
    pub id: String,
    pub name: String,
}

/// Schedule data from Helix API
#[derive(Debug, Clone, Deserialize)]
pub struct ScheduleData {
    pub segments: Option<Vec<ScheduleSegment>>,
    pub broadcaster_id: String,
    pub broadcaster_name: String,
    pub broadcaster_login: String,
}

/// Schedule response from Helix API
#[derive(Debug, Clone, Deserialize)]
pub struct ScheduleResponse {
    pub data: ScheduleData,
}

/// Represents a Twitch category/game
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Category {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub box_art_url: String,
}

/// Response from search categories endpoint
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SearchCategoriesResponse {
    pub data: Vec<Category>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::{Duration, TimeZone};

    /// Helper to create a test stream with specified viewer count
    fn stream_with_viewers(viewer_count: i64) -> Stream {
        Stream {
            id: "123".to_string(),
            user_id: "456".to_string(),
            user_login: "testuser".to_string(),
            user_name: "TestUser".to_string(),
            game_id: "789".to_string(),
            game_name: "Test Game".to_string(),
            title: "Test Stream".to_string(),
            viewer_count,
            started_at: Utc::now() - Duration::hours(1),
            thumbnail_url: "https://example.com/thumb.jpg".to_string(),
            tags: vec![],
        }
    }

    /// Helper to create a test stream started at a specific time
    fn stream_started_at(started_at: DateTime<Utc>) -> Stream {
        Stream {
            id: "123".to_string(),
            user_id: "456".to_string(),
            user_login: "testuser".to_string(),
            user_name: "TestUser".to_string(),
            game_id: "789".to_string(),
            game_name: "Test Game".to_string(),
            title: "Test Stream".to_string(),
            viewer_count: 1000,
            started_at,
            thumbnail_url: "https://example.com/thumb.jpg".to_string(),
            tags: vec![],
        }
    }

    /// Helper to create a scheduled stream at a specific time
    fn scheduled_at(start_time: DateTime<Utc>) -> ScheduledStream {
        ScheduledStream {
            id: "sched123".to_string(),
            broadcaster_id: "456".to_string(),
            broadcaster_name: "TestBroadcaster".to_string(),
            broadcaster_login: "testbroadcaster".to_string(),
            title: "Scheduled Stream".to_string(),
            start_time,
            end_time: None,
            category: Some("Gaming".to_string()),
            category_id: Some("123".to_string()),
            is_recurring: false,
        }
    }

    // === format_viewer_count tests ===

    #[test]
    fn viewer_count_small() {
        let stream = stream_with_viewers(856);
        assert_eq!(stream.format_viewer_count(), "856");
    }

    #[test]
    fn viewer_count_exactly_1k() {
        let stream = stream_with_viewers(1000);
        assert_eq!(stream.format_viewer_count(), "1k");
    }

    #[test]
    fn viewer_count_decimal() {
        let stream = stream_with_viewers(1234);
        assert_eq!(stream.format_viewer_count(), "1.2k");
    }

    #[test]
    fn viewer_count_round_down() {
        // 1049 / 1000 = 1.049, which has fract() < 0.05, so shows as "1k"
        let stream = stream_with_viewers(1049);
        assert_eq!(stream.format_viewer_count(), "1k");
    }

    #[test]
    fn viewer_count_round_to_decimal() {
        // 1050 / 1000 = 1.05, which has fract() >= 0.05, so shows as "1.1k"
        let stream = stream_with_viewers(1050);
        assert_eq!(stream.format_viewer_count(), "1.1k");
    }

    #[test]
    fn viewer_count_large() {
        let stream = stream_with_viewers(12345);
        assert_eq!(stream.format_viewer_count(), "12.3k");
    }

    #[test]
    fn viewer_count_very_large() {
        let stream = stream_with_viewers(100000);
        assert_eq!(stream.format_viewer_count(), "100k");
    }

    #[test]
    fn viewer_count_zero() {
        let stream = stream_with_viewers(0);
        assert_eq!(stream.format_viewer_count(), "0");
    }

    #[test]
    fn viewer_count_999() {
        let stream = stream_with_viewers(999);
        assert_eq!(stream.format_viewer_count(), "999");
    }

    // === format_duration tests ===

    #[test]
    fn duration_minutes_only() {
        let stream = stream_started_at(Utc::now() - Duration::minutes(45));
        assert_eq!(stream.format_duration(), "45m");
    }

    #[test]
    fn duration_hours_and_minutes() {
        let stream = stream_started_at(Utc::now() - Duration::hours(2) - Duration::minutes(15));
        assert_eq!(stream.format_duration(), "2h 15m");
    }

    #[test]
    fn duration_exactly_one_hour() {
        let stream = stream_started_at(Utc::now() - Duration::hours(1));
        assert_eq!(stream.format_duration(), "1h 0m");
    }

    #[test]
    fn duration_many_hours() {
        let stream = stream_started_at(Utc::now() - Duration::hours(12) - Duration::minutes(30));
        assert_eq!(stream.format_duration(), "12h 30m");
    }

    #[test]
    fn duration_zero() {
        let stream = stream_started_at(Utc::now());
        assert_eq!(stream.format_duration(), "0m");
    }

    // === format_start_time tests ===
    // Note: These tests are timezone-dependent, so we test the general format

    #[test]
    fn format_start_time_today() {
        // Create a time that's definitely today (a few hours from now)
        let start = Utc::now() + Duration::hours(3);
        let scheduled = scheduled_at(start);
        let formatted = scheduled.format_start_time();

        assert!(
            formatted.starts_with("Today "),
            "Expected 'Today ...', got: {}",
            formatted
        );
    }

    #[test]
    fn format_start_time_tomorrow() {
        // Create a time that's definitely tomorrow
        let now = Local::now();
        let tomorrow_noon = (now.date_naive() + Duration::days(1))
            .and_hms_opt(12, 0, 0)
            .unwrap();
        let tomorrow_utc = Local
            .from_local_datetime(&tomorrow_noon)
            .single()
            .unwrap()
            .with_timezone(&Utc);

        let scheduled = scheduled_at(tomorrow_utc);
        let formatted = scheduled.format_start_time();

        assert!(
            formatted.starts_with("Tomorrow "),
            "Expected 'Tomorrow ...', got: {}",
            formatted
        );
    }

    #[test]
    fn format_start_time_later_date() {
        // Create a time that's several days away
        let start = Utc::now() + Duration::days(5);
        let scheduled = scheduled_at(start);
        let formatted = scheduled.format_start_time();

        // Should show day of week, not "Today" or "Tomorrow"
        assert!(
            !formatted.starts_with("Today ") && !formatted.starts_with("Tomorrow "),
            "Expected day format, got: {}",
            formatted
        );
    }
}

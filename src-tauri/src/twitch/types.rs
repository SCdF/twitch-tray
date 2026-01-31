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
    /// Returns the duration until the scheduled stream starts
    pub fn time_until(&self) -> chrono::Duration {
        self.start_time.signed_duration_since(Utc::now())
    }

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
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Pagination {
    #[serde(default)]
    pub cursor: Option<String>,
}

/// Generic Helix API response wrapper
#[derive(Debug, Clone, Deserialize)]
pub struct HelixResponse<T> {
    pub data: T,
    #[serde(default)]
    pub pagination: Option<Pagination>,
}

/// Followed channels response
#[derive(Debug, Clone, Deserialize)]
pub struct FollowedChannelsResponse {
    pub data: Vec<FollowedChannel>,
    #[serde(default)]
    pub pagination: Option<Pagination>,
}

/// Streams response from Helix API
#[derive(Debug, Clone, Deserialize)]
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

/// Token validation response
#[derive(Debug, Clone, Deserialize)]
pub struct ValidateResponse {
    pub client_id: String,
    pub login: String,
    pub scopes: Vec<String>,
    pub user_id: String,
    pub expires_in: i64,
}

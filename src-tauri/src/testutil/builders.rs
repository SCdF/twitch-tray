//! Test data builders
//!
//! Provides builder patterns for creating test data with sensible defaults.

use chrono::{DateTime, Duration, Utc};

use crate::twitch::{ScheduledStream, Stream};

/// Builder for creating test Stream objects
#[derive(Debug, Clone)]
pub struct StreamBuilder {
    id: String,
    user_id: String,
    user_login: String,
    user_name: String,
    game_id: String,
    game_name: String,
    title: String,
    viewer_count: i64,
    started_at: DateTime<Utc>,
    thumbnail_url: String,
    tags: Vec<String>,
}

impl Default for StreamBuilder {
    fn default() -> Self {
        Self {
            id: "stream_123".to_string(),
            user_id: "user_456".to_string(),
            user_login: "teststreamer".to_string(),
            user_name: "TestStreamer".to_string(),
            game_id: "game_789".to_string(),
            game_name: "Test Game".to_string(),
            title: "Test Stream Title".to_string(),
            viewer_count: 1000,
            started_at: Utc::now() - Duration::hours(1),
            thumbnail_url: "https://example.com/thumb.jpg".to_string(),
            tags: vec![],
        }
    }
}

impl StreamBuilder {
    /// Creates a new stream builder with default values
    pub fn new() -> Self {
        Self::default()
    }

    /// Sets the stream ID
    pub fn id(mut self, id: impl Into<String>) -> Self {
        self.id = id.into();
        self
    }

    /// Sets the user ID
    pub fn user_id(mut self, user_id: impl Into<String>) -> Self {
        self.user_id = user_id.into();
        self
    }

    /// Sets the user login (lowercase username)
    pub fn user_login(mut self, user_login: impl Into<String>) -> Self {
        self.user_login = user_login.into();
        self
    }

    /// Sets the user display name
    pub fn user_name(mut self, user_name: impl Into<String>) -> Self {
        self.user_name = user_name.into();
        self
    }

    /// Sets both user_login (lowercase) and user_name from a single name
    pub fn streamer(mut self, name: impl Into<String>) -> Self {
        let name = name.into();
        self.user_login = name.to_lowercase();
        self.user_name = name;
        self
    }

    /// Sets the game ID
    pub fn game_id(mut self, game_id: impl Into<String>) -> Self {
        self.game_id = game_id.into();
        self
    }

    /// Sets the game name
    pub fn game_name(mut self, game_name: impl Into<String>) -> Self {
        self.game_name = game_name.into();
        self
    }

    /// Sets both game_id and game_name
    pub fn game(mut self, id: impl Into<String>, name: impl Into<String>) -> Self {
        self.game_id = id.into();
        self.game_name = name.into();
        self
    }

    /// Sets the stream title
    pub fn title(mut self, title: impl Into<String>) -> Self {
        self.title = title.into();
        self
    }

    /// Sets the viewer count
    pub fn viewer_count(mut self, count: i64) -> Self {
        self.viewer_count = count;
        self
    }

    /// Alias for viewer_count
    pub fn viewers(self, count: i64) -> Self {
        self.viewer_count(count)
    }

    /// Sets the stream start time
    pub fn started_at(mut self, started_at: DateTime<Utc>) -> Self {
        self.started_at = started_at;
        self
    }

    /// Sets the stream to have started N hours ago
    pub fn started_hours_ago(mut self, hours: i64) -> Self {
        self.started_at = Utc::now() - Duration::hours(hours);
        self
    }

    /// Sets the stream to have started N minutes ago
    pub fn started_minutes_ago(mut self, minutes: i64) -> Self {
        self.started_at = Utc::now() - Duration::minutes(minutes);
        self
    }

    /// Sets the thumbnail URL
    pub fn thumbnail_url(mut self, url: impl Into<String>) -> Self {
        self.thumbnail_url = url.into();
        self
    }

    /// Sets the stream tags
    pub fn tags(mut self, tags: Vec<String>) -> Self {
        self.tags = tags;
        self
    }

    /// Adds a tag to the stream
    pub fn tag(mut self, tag: impl Into<String>) -> Self {
        self.tags.push(tag.into());
        self
    }

    /// Builds the Stream
    pub fn build(self) -> Stream {
        Stream {
            id: self.id,
            user_id: self.user_id,
            user_login: self.user_login,
            user_name: self.user_name,
            game_id: self.game_id,
            game_name: self.game_name,
            title: self.title,
            viewer_count: self.viewer_count,
            started_at: self.started_at,
            thumbnail_url: self.thumbnail_url,
            tags: self.tags,
        }
    }
}

/// Builder for creating test ScheduledStream objects
#[derive(Debug, Clone)]
pub struct ScheduledStreamBuilder {
    id: String,
    broadcaster_id: String,
    broadcaster_name: String,
    broadcaster_login: String,
    title: String,
    start_time: DateTime<Utc>,
    end_time: Option<DateTime<Utc>>,
    category: Option<String>,
    category_id: Option<String>,
    is_recurring: bool,
}

impl Default for ScheduledStreamBuilder {
    fn default() -> Self {
        Self {
            id: "sched_123".to_string(),
            broadcaster_id: "broadcaster_456".to_string(),
            broadcaster_name: "TestBroadcaster".to_string(),
            broadcaster_login: "testbroadcaster".to_string(),
            title: "Scheduled Stream".to_string(),
            start_time: Utc::now() + Duration::hours(2),
            end_time: None,
            category: Some("Gaming".to_string()),
            category_id: Some("cat_123".to_string()),
            is_recurring: false,
        }
    }
}

impl ScheduledStreamBuilder {
    /// Creates a new scheduled stream builder with default values
    pub fn new() -> Self {
        Self::default()
    }

    /// Sets the schedule ID
    pub fn id(mut self, id: impl Into<String>) -> Self {
        self.id = id.into();
        self
    }

    /// Sets the broadcaster ID
    pub fn broadcaster_id(mut self, id: impl Into<String>) -> Self {
        self.broadcaster_id = id.into();
        self
    }

    /// Sets the broadcaster name and login
    pub fn broadcaster(mut self, name: impl Into<String>) -> Self {
        let name = name.into();
        self.broadcaster_login = name.to_lowercase();
        self.broadcaster_name = name;
        self
    }

    /// Sets the stream title
    pub fn title(mut self, title: impl Into<String>) -> Self {
        self.title = title.into();
        self
    }

    /// Sets the start time
    pub fn start_time(mut self, start_time: DateTime<Utc>) -> Self {
        self.start_time = start_time;
        self
    }

    /// Sets the start time to N hours from now
    pub fn starts_in_hours(mut self, hours: i64) -> Self {
        self.start_time = Utc::now() + Duration::hours(hours);
        self
    }

    /// Sets the start time to N minutes from now
    pub fn starts_in_minutes(mut self, minutes: i64) -> Self {
        self.start_time = Utc::now() + Duration::minutes(minutes);
        self
    }

    /// Sets the end time
    pub fn end_time(mut self, end_time: DateTime<Utc>) -> Self {
        self.end_time = Some(end_time);
        self
    }

    /// Sets the duration (end_time = start_time + duration)
    pub fn duration_hours(mut self, hours: i64) -> Self {
        self.end_time = Some(self.start_time + Duration::hours(hours));
        self
    }

    /// Sets the category
    pub fn category(mut self, id: impl Into<String>, name: impl Into<String>) -> Self {
        self.category_id = Some(id.into());
        self.category = Some(name.into());
        self
    }

    /// Removes the category
    pub fn no_category(mut self) -> Self {
        self.category = None;
        self.category_id = None;
        self
    }

    /// Sets whether this is a recurring schedule
    pub fn recurring(mut self, is_recurring: bool) -> Self {
        self.is_recurring = is_recurring;
        self
    }

    /// Builds the ScheduledStream
    pub fn build(self) -> ScheduledStream {
        ScheduledStream {
            id: self.id,
            broadcaster_id: self.broadcaster_id,
            broadcaster_name: self.broadcaster_name,
            broadcaster_login: self.broadcaster_login,
            title: self.title,
            start_time: self.start_time,
            end_time: self.end_time,
            category: self.category,
            category_id: self.category_id,
            is_recurring: self.is_recurring,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stream_builder_defaults() {
        let stream = StreamBuilder::new().build();

        assert!(!stream.id.is_empty());
        assert!(!stream.user_id.is_empty());
        assert!(!stream.user_name.is_empty());
        assert_eq!(stream.viewer_count, 1000);
    }

    #[test]
    fn stream_builder_with_values() {
        let stream = StreamBuilder::new()
            .streamer("Ninja")
            .game("12345", "Fortnite")
            .viewers(50000)
            .title("Championship Finals!")
            .started_hours_ago(3)
            .build();

        assert_eq!(stream.user_name, "Ninja");
        assert_eq!(stream.user_login, "ninja");
        assert_eq!(stream.game_name, "Fortnite");
        assert_eq!(stream.viewer_count, 50000);
    }

    #[test]
    fn scheduled_stream_builder_defaults() {
        let scheduled = ScheduledStreamBuilder::new().build();

        assert!(!scheduled.id.is_empty());
        assert!(!scheduled.broadcaster_name.is_empty());
        assert!(scheduled.start_time > Utc::now());
    }

    #[test]
    fn scheduled_stream_builder_with_values() {
        let scheduled = ScheduledStreamBuilder::new()
            .broadcaster("Pokimane")
            .title("Special Event!")
            .starts_in_hours(5)
            .category("123", "Just Chatting")
            .recurring(true)
            .build();

        assert_eq!(scheduled.broadcaster_name, "Pokimane");
        assert_eq!(scheduled.broadcaster_login, "pokimane");
        assert_eq!(scheduled.title, "Special Event!");
        assert!(scheduled.is_recurring);
    }
}

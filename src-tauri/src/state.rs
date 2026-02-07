use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use tokio::sync::{watch, RwLock};

use crate::twitch::{FollowedChannel, ScheduledStream, Stream};

/// Type of state change
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChangeType {
    FollowedStreams,
    ScheduledStreams,
    CategoryStreams,
    Authentication,
}

/// A category change event
#[derive(Debug, Clone)]
pub struct CategoryChange {
    pub stream: Stream,
    pub old_category: String,
}

/// Result of updating followed streams
pub struct StreamUpdateResult {
    pub newly_live: Vec<Stream>,
    pub category_changes: Vec<CategoryChange>,
}

/// Application state
#[derive(Default)]
struct StateInner {
    // Authentication state
    authenticated: bool,
    user_id: String,
    user_login: String,

    // Stream data
    followed_streams: Vec<Stream>,
    scheduled_streams: Vec<ScheduledStream>,
    schedules_loaded: bool,
    followed_channels: Vec<FollowedChannel>,

    // Categories being tracked (from followed live streams)
    tracked_categories: HashMap<String, String>, // game_id -> game_name

    // Track previous game per stream (by user_id) for category change detection
    stream_games: HashMap<String, (String, String)>, // user_id -> (game_id, game_name)

    // Streams by followed category (category_id -> streams)
    category_streams: HashMap<String, Vec<Stream>>,
}

/// Thread-safe application state manager
pub struct AppState {
    inner: RwLock<StateInner>,
    change_tx: watch::Sender<Option<ChangeType>>,
    change_rx: watch::Receiver<Option<ChangeType>>,
}

impl AppState {
    /// Creates a new state manager
    pub fn new() -> Arc<Self> {
        let (change_tx, change_rx) = watch::channel(None);
        Arc::new(Self {
            inner: RwLock::new(StateInner::default()),
            change_tx,
            change_rx,
        })
    }

    /// Returns a receiver for state change notifications
    pub fn subscribe(&self) -> watch::Receiver<Option<ChangeType>> {
        self.change_rx.clone()
    }

    fn notify_change(&self, change_type: ChangeType) {
        let _ = self.change_tx.send(Some(change_type));
    }

    /// Sets the authentication state
    pub async fn set_authenticated(
        &self,
        authenticated: bool,
        user_id: String,
        user_login: String,
    ) {
        let mut state = self.inner.write().await;
        let changed = state.authenticated != authenticated || state.user_id != user_id;
        state.authenticated = authenticated;
        state.user_id = user_id;
        state.user_login = user_login;
        drop(state);

        if changed {
            self.notify_change(ChangeType::Authentication);
        }
    }

    /// Returns whether the user is authenticated
    pub async fn is_authenticated(&self) -> bool {
        self.inner.read().await.authenticated
    }

    /// Updates the followed live streams and returns changes
    pub async fn set_followed_streams(&self, streams: Vec<Stream>) -> StreamUpdateResult {
        let mut state = self.inner.write().await;

        // Build set for comparison
        let old_by_id: HashSet<_> = state
            .followed_streams
            .iter()
            .map(|s| s.user_id.clone())
            .collect();

        // Find newly live streams
        let newly_live: Vec<_> = streams
            .iter()
            .filter(|s| !old_by_id.contains(&s.user_id))
            .cloned()
            .collect();

        // Find category changes for streams that were already live
        let mut category_changes = Vec::new();
        for stream in &streams {
            if let Some((old_game_id, old_game_name)) = state.stream_games.get(&stream.user_id) {
                // Stream was already live, check if category changed
                if *old_game_id != stream.game_id && !old_game_id.is_empty() {
                    category_changes.push(CategoryChange {
                        stream: stream.clone(),
                        old_category: old_game_name.clone(),
                    });
                }
            }
        }

        // Update tracked categories based on current live streams
        state.tracked_categories.clear();
        state.stream_games.clear();
        for stream in &streams {
            if !stream.game_id.is_empty() {
                state
                    .tracked_categories
                    .insert(stream.game_id.clone(), stream.game_name.clone());
            }
            state.stream_games.insert(
                stream.user_id.clone(),
                (stream.game_id.clone(), stream.game_name.clone()),
            );
        }

        state.followed_streams = streams;
        drop(state);

        self.notify_change(ChangeType::FollowedStreams);

        StreamUpdateResult {
            newly_live,
            category_changes,
        }
    }

    /// Returns the current followed live streams
    pub async fn get_followed_streams(&self) -> Vec<Stream> {
        self.inner.read().await.followed_streams.clone()
    }

    /// Updates the scheduled streams
    pub async fn set_scheduled_streams(&self, streams: Vec<ScheduledStream>) {
        let mut state = self.inner.write().await;
        state.scheduled_streams = streams;
        state.schedules_loaded = true;
        drop(state);

        self.notify_change(ChangeType::ScheduledStreams);
    }

    /// Returns whether schedules have been fetched at least once
    pub async fn schedules_loaded(&self) -> bool {
        self.inner.read().await.schedules_loaded
    }

    /// Returns the current scheduled streams
    pub async fn get_scheduled_streams(&self) -> Vec<ScheduledStream> {
        self.inner.read().await.scheduled_streams.clone()
    }

    /// Sets the list of followed channels
    pub async fn set_followed_channels(&self, channels: Vec<FollowedChannel>) {
        let mut state = self.inner.write().await;
        state.followed_channels = channels;
    }

    /// Returns the list of followed channels
    pub async fn get_followed_channels(&self) -> Vec<FollowedChannel> {
        self.inner.read().await.followed_channels.clone()
    }

    /// Updates streams for a specific category
    pub async fn set_category_streams(&self, category_id: String, streams: Vec<Stream>) {
        let mut state = self.inner.write().await;
        state.category_streams.insert(category_id, streams);
        drop(state);

        self.notify_change(ChangeType::CategoryStreams);
    }

    /// Returns all category streams
    pub async fn get_category_streams(&self) -> HashMap<String, Vec<Stream>> {
        self.inner.read().await.category_streams.clone()
    }

    /// Clears all state (used on logout)
    pub async fn clear(&self) {
        let mut state = self.inner.write().await;
        *state = StateInner::default();
        drop(state);

        self.notify_change(ChangeType::Authentication);
    }
}

impl Default for AppState {
    fn default() -> Self {
        let (change_tx, change_rx) = watch::channel(None);
        Self {
            inner: RwLock::new(StateInner::default()),
            change_tx,
            change_rx,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::{Duration, Utc};

    /// Helper to create a test stream with a specific user_id
    fn make_stream(user_id: &str, user_name: &str) -> Stream {
        Stream {
            id: format!("stream_{}", user_id),
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

    /// Helper to create a stream with a specific game
    fn make_stream_with_game(user_id: &str, game_id: &str, game_name: &str) -> Stream {
        Stream {
            id: format!("stream_{}", user_id),
            user_id: user_id.to_string(),
            user_login: format!("user_{}", user_id),
            user_name: format!("User {}", user_id),
            game_id: game_id.to_string(),
            game_name: game_name.to_string(),
            title: "Test Stream".to_string(),
            viewer_count: 1000,
            started_at: Utc::now() - Duration::hours(1),
            thumbnail_url: "https://example.com/thumb.jpg".to_string(),
            tags: vec![],
        }
    }

    // === set_followed_streams change detection tests ===

    #[tokio::test]
    async fn newly_live_detected() {
        let state = AppState::new();

        // Initial state: stream A is live
        let stream_a = make_stream("a", "StreamerA");
        state.set_followed_streams(vec![stream_a.clone()]).await;

        // Update: both A and B are live
        let stream_b = make_stream("b", "StreamerB");
        let result = state.set_followed_streams(vec![stream_a, stream_b]).await;

        assert_eq!(result.newly_live.len(), 1);
        assert_eq!(result.newly_live[0].user_id, "b");
    }

    #[tokio::test]
    async fn no_change_when_same_streams() {
        let state = AppState::new();

        let stream_a = make_stream("a", "StreamerA");
        let stream_b = make_stream("b", "StreamerB");

        // Set initial streams
        state
            .set_followed_streams(vec![stream_a.clone(), stream_b.clone()])
            .await;

        // Set same streams again
        let result = state.set_followed_streams(vec![stream_a, stream_b]).await;

        assert!(result.newly_live.is_empty());
    }

    #[tokio::test]
    async fn initial_load_all_newly_live() {
        let state = AppState::new();

        let stream_a = make_stream("a", "StreamerA");
        let stream_b = make_stream("b", "StreamerB");

        // First load - all streams are "newly live"
        let result = state.set_followed_streams(vec![stream_a, stream_b]).await;

        assert_eq!(result.newly_live.len(), 2);
    }

    #[tokio::test]
    async fn empty_to_streams() {
        let state = AppState::new();

        // Explicitly set empty first
        state.set_followed_streams(vec![]).await;

        let stream_a = make_stream("a", "StreamerA");
        let result = state.set_followed_streams(vec![stream_a]).await;

        assert_eq!(result.newly_live.len(), 1);
    }

    // === tracked_categories tests ===

    #[tokio::test]
    async fn categories_tracked_from_live_streams() {
        let state = AppState::new();

        let stream1 = make_stream_with_game("1", "game1", "Fortnite");
        let stream2 = make_stream_with_game("2", "game2", "Minecraft");

        state.set_followed_streams(vec![stream1, stream2]).await;

        // Verify categories are tracked (accessing internal state for test)
        let inner = state.inner.read().await;
        assert_eq!(inner.tracked_categories.len(), 2);
        assert_eq!(
            inner.tracked_categories.get("game1"),
            Some(&"Fortnite".to_string())
        );
        assert_eq!(
            inner.tracked_categories.get("game2"),
            Some(&"Minecraft".to_string())
        );
    }

    #[tokio::test]
    async fn categories_cleared_on_update() {
        let state = AppState::new();

        let stream1 = make_stream_with_game("1", "game1", "Fortnite");
        state.set_followed_streams(vec![stream1]).await;

        // Now update with different stream
        let stream2 = make_stream_with_game("2", "game2", "Minecraft");
        state.set_followed_streams(vec![stream2]).await;

        // Only the new category should be tracked
        let inner = state.inner.read().await;
        assert_eq!(inner.tracked_categories.len(), 1);
        assert_eq!(
            inner.tracked_categories.get("game2"),
            Some(&"Minecraft".to_string())
        );
        assert!(inner.tracked_categories.get("game1").is_none());
    }

    #[tokio::test]
    async fn empty_game_id_not_tracked() {
        let state = AppState::new();

        let mut stream = make_stream_with_game("1", "game1", "Fortnite");
        stream.game_id = "".to_string(); // Empty game ID

        state.set_followed_streams(vec![stream]).await;

        let inner = state.inner.read().await;
        assert!(inner.tracked_categories.is_empty());
    }

    // === category change detection tests ===

    #[tokio::test]
    async fn category_change_detected() {
        let state = AppState::new();

        // Initial: streamer is playing Fortnite
        let stream1 = make_stream_with_game("1", "game1", "Fortnite");
        state.set_followed_streams(vec![stream1]).await;

        // Update: streamer switched to Minecraft
        let stream2 = make_stream_with_game("1", "game2", "Minecraft");
        let result = state.set_followed_streams(vec![stream2]).await;

        assert_eq!(result.category_changes.len(), 1);
        assert_eq!(result.category_changes[0].old_category, "Fortnite");
        assert_eq!(result.category_changes[0].stream.game_name, "Minecraft");
    }

    #[tokio::test]
    async fn no_category_change_when_same_game() {
        let state = AppState::new();

        let stream1 = make_stream_with_game("1", "game1", "Fortnite");
        state.set_followed_streams(vec![stream1.clone()]).await;

        // Same game
        let result = state.set_followed_streams(vec![stream1]).await;

        assert!(result.category_changes.is_empty());
    }

    #[tokio::test]
    async fn no_category_change_for_newly_live() {
        let state = AppState::new();

        // Initial: nobody live
        state.set_followed_streams(vec![]).await;

        // New stream comes online
        let stream = make_stream_with_game("1", "game1", "Fortnite");
        let result = state.set_followed_streams(vec![stream]).await;

        // Should be newly_live, not a category change
        assert_eq!(result.newly_live.len(), 1);
        assert!(result.category_changes.is_empty());
    }

    #[tokio::test]
    async fn no_category_change_from_empty_game() {
        let state = AppState::new();

        // Initial: stream with no game (empty game_id)
        let mut stream1 = make_stream_with_game("1", "", "");
        stream1.game_name = "".to_string();
        state.set_followed_streams(vec![stream1]).await;

        // Update: now has a game
        let stream2 = make_stream_with_game("1", "game1", "Fortnite");
        let result = state.set_followed_streams(vec![stream2]).await;

        // Not counted as a category change (was empty before)
        assert!(result.category_changes.is_empty());
    }

    #[tokio::test]
    async fn multiple_category_changes() {
        let state = AppState::new();

        // Initial: two streams
        let stream1 = make_stream_with_game("1", "game1", "Fortnite");
        let stream2 = make_stream_with_game("2", "game2", "Minecraft");
        state.set_followed_streams(vec![stream1, stream2]).await;

        // Both change categories
        let stream1_new = make_stream_with_game("1", "game3", "Valorant");
        let stream2_new = make_stream_with_game("2", "game4", "Apex Legends");
        let result = state
            .set_followed_streams(vec![stream1_new, stream2_new])
            .await;

        assert_eq!(result.category_changes.len(), 2);
    }

    // === authentication state tests ===

    #[tokio::test]
    async fn authentication_state() {
        let state = AppState::new();

        assert!(!state.is_authenticated().await);

        state
            .set_authenticated(true, "user123".to_string(), "testuser".to_string())
            .await;

        assert!(state.is_authenticated().await);
    }

    #[tokio::test]
    async fn clear_resets_all_state() {
        let state = AppState::new();

        // Set up some state
        state
            .set_authenticated(true, "user123".to_string(), "testuser".to_string())
            .await;
        state
            .set_followed_streams(vec![make_stream("1", "Streamer")])
            .await;

        // Clear everything
        state.clear().await;

        assert!(!state.is_authenticated().await);
        assert!(state.get_followed_streams().await.is_empty());
    }

    // === category streams tests ===

    #[tokio::test]
    async fn set_category_streams() {
        let state = AppState::new();

        let stream = make_stream_with_game("1", "game1", "Fortnite");
        state
            .set_category_streams("game1".to_string(), vec![stream])
            .await;

        let streams = state.get_category_streams().await;
        assert_eq!(streams.len(), 1);
        assert!(streams.contains_key("game1"));
        assert_eq!(streams.get("game1").unwrap().len(), 1);
    }

    #[tokio::test]
    async fn set_multiple_category_streams() {
        let state = AppState::new();

        let stream1 = make_stream_with_game("1", "game1", "Fortnite");
        let stream2 = make_stream_with_game("2", "game2", "Minecraft");

        state
            .set_category_streams("game1".to_string(), vec![stream1])
            .await;
        state
            .set_category_streams("game2".to_string(), vec![stream2])
            .await;

        let streams = state.get_category_streams().await;
        assert_eq!(streams.len(), 2);
        assert!(streams.contains_key("game1"));
        assert!(streams.contains_key("game2"));
    }

    #[tokio::test]
    async fn category_streams_cleared_on_full_clear() {
        let state = AppState::new();

        let stream = make_stream_with_game("1", "game1", "Fortnite");
        state
            .set_category_streams("game1".to_string(), vec![stream])
            .await;

        state.clear().await;

        let streams = state.get_category_streams().await;
        assert!(streams.is_empty());
    }
}

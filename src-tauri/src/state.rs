use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use tokio::sync::{watch, RwLock};

use crate::twitch::{ScheduledStream, Stream};

/// Type of state change
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChangeType {
    FollowedStreams,
    ScheduledStreams,
    Authentication,
}

/// Result of updating followed streams
pub struct StreamUpdateResult {
    pub newly_live: Vec<Stream>,
    pub went_offline: Vec<Stream>,
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
    followed_channel_ids: Vec<String>,

    // Categories being tracked (from followed live streams)
    tracked_categories: HashMap<String, String>, // game_id -> game_name
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

    /// Returns the authenticated user's ID
    pub async fn get_user_id(&self) -> String {
        self.inner.read().await.user_id.clone()
    }

    /// Returns the authenticated user's login
    pub async fn get_user_login(&self) -> String {
        self.inner.read().await.user_login.clone()
    }

    /// Updates the followed live streams and returns changes
    pub async fn set_followed_streams(&self, streams: Vec<Stream>) -> StreamUpdateResult {
        let mut state = self.inner.write().await;

        // Build maps for comparison
        let old_by_id: HashSet<_> = state
            .followed_streams
            .iter()
            .map(|s| s.user_id.clone())
            .collect();

        let new_by_id: HashSet<_> = streams.iter().map(|s| s.user_id.clone()).collect();

        // Find newly live streams
        let newly_live: Vec<_> = streams
            .iter()
            .filter(|s| !old_by_id.contains(&s.user_id))
            .cloned()
            .collect();

        // Find streams that went offline
        let went_offline: Vec<_> = state
            .followed_streams
            .iter()
            .filter(|s| !new_by_id.contains(&s.user_id))
            .cloned()
            .collect();

        // Update tracked categories based on current live streams
        state.tracked_categories.clear();
        for stream in &streams {
            if !stream.game_id.is_empty() {
                state
                    .tracked_categories
                    .insert(stream.game_id.clone(), stream.game_name.clone());
            }
        }

        state.followed_streams = streams;
        drop(state);

        self.notify_change(ChangeType::FollowedStreams);

        StreamUpdateResult {
            newly_live,
            went_offline,
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

    /// Sets the list of followed channel IDs
    pub async fn set_followed_channel_ids(&self, ids: Vec<String>) {
        let mut state = self.inner.write().await;
        state.followed_channel_ids = ids;
    }

    /// Returns the list of followed channel IDs
    pub async fn get_followed_channel_ids(&self) -> Vec<String> {
        self.inner.read().await.followed_channel_ids.clone()
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

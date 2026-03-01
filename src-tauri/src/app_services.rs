use async_trait::async_trait;

use crate::config::{Config, FollowedCategory};
use crate::twitch::{ApiError, Category, FollowedChannel};

#[derive(serde::Serialize, Clone, Debug, PartialEq)]
pub struct DebugStreamEntry {
    pub is_inferred: bool,
    pub broadcaster_name: String,
    pub broadcaster_login: String,
    pub started_at: i64, // Unix timestamp (seconds)
}

/// Input port for Tauri command handlers.
///
/// Commands take `State<'_, Arc<dyn AppServices>>` so they can be tested
/// with `MockAppServices` independently of the real `App` infrastructure.
///
/// `refresh_category_streams` and `refresh_schedules_from_db` are included
/// so that `save_config` implementations can call them, and so the mock
/// can verify they were triggered.
#[async_trait]
pub trait AppServices: Send + Sync {
    fn get_config(&self) -> Config;
    async fn save_config(&self, config: Config) -> anyhow::Result<()>;
    async fn search_categories(&self, query: &str) -> Result<Vec<Category>, ApiError>;
    fn get_followed_categories(&self) -> Vec<FollowedCategory>;
    async fn get_followed_channels(&self) -> Vec<FollowedChannel>;
    async fn refresh_category_streams(&self);
    async fn refresh_schedules_from_db(&self);
    async fn get_debug_schedule_data(&self, start: i64, end: i64) -> Vec<DebugStreamEntry>;
}

#[cfg(test)]
pub mod mock {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Mutex;

    /// A controllable `AppServices` implementation for unit tests.
    ///
    /// Configure return values up front, then call methods, then inspect
    /// the recorded calls with `save_config_count()` etc.
    pub struct MockAppServices {
        config: Mutex<Config>,
        search_results: Mutex<Vec<Category>>,
        channels: Mutex<Vec<FollowedChannel>>,
        debug_entries: Mutex<Vec<super::DebugStreamEntry>>,
        save_config_count: AtomicUsize,
        refresh_category_count: AtomicUsize,
        refresh_schedules_count: AtomicUsize,
        debug_call_count: AtomicUsize,
    }

    impl MockAppServices {
        pub fn new() -> Self {
            Self {
                config: Mutex::new(Config::default()),
                search_results: Mutex::new(Vec::new()),
                channels: Mutex::new(Vec::new()),
                debug_entries: Mutex::new(Vec::new()),
                save_config_count: AtomicUsize::new(0),
                refresh_category_count: AtomicUsize::new(0),
                refresh_schedules_count: AtomicUsize::new(0),
                debug_call_count: AtomicUsize::new(0),
            }
        }

        /// Pre-configure the search results that `search_categories` will return.
        pub fn set_search_results(&self, results: Vec<Category>) {
            *self.search_results.lock().unwrap() = results;
        }

        /// Pre-configure the channel list that `get_followed_channels` will return.
        pub fn set_channels(&self, channels: Vec<FollowedChannel>) {
            *self.channels.lock().unwrap() = channels;
        }

        /// Pre-configure the debug entries that `get_debug_schedule_data` will return.
        pub fn set_debug_entries(&self, entries: Vec<super::DebugStreamEntry>) {
            *self.debug_entries.lock().unwrap() = entries;
        }

        pub fn save_config_count(&self) -> usize {
            self.save_config_count.load(Ordering::SeqCst)
        }

        pub fn refresh_category_count(&self) -> usize {
            self.refresh_category_count.load(Ordering::SeqCst)
        }

        pub fn refresh_schedules_count(&self) -> usize {
            self.refresh_schedules_count.load(Ordering::SeqCst)
        }

        pub fn debug_call_count(&self) -> usize {
            self.debug_call_count.load(Ordering::SeqCst)
        }
    }

    #[async_trait]
    impl AppServices for MockAppServices {
        fn get_config(&self) -> Config {
            self.config.lock().unwrap().clone()
        }

        async fn save_config(&self, config: Config) -> anyhow::Result<()> {
            self.save_config_count.fetch_add(1, Ordering::SeqCst);
            *self.config.lock().unwrap() = config;
            self.refresh_category_streams().await;
            self.refresh_schedules_from_db().await;
            Ok(())
        }

        async fn search_categories(&self, _query: &str) -> Result<Vec<Category>, ApiError> {
            Ok(self.search_results.lock().unwrap().clone())
        }

        fn get_followed_categories(&self) -> Vec<FollowedCategory> {
            self.config.lock().unwrap().followed_categories.clone()
        }

        async fn get_followed_channels(&self) -> Vec<FollowedChannel> {
            self.channels.lock().unwrap().clone()
        }

        async fn refresh_category_streams(&self) {
            self.refresh_category_count.fetch_add(1, Ordering::SeqCst);
        }

        async fn refresh_schedules_from_db(&self) {
            self.refresh_schedules_count.fetch_add(1, Ordering::SeqCst);
        }

        async fn get_debug_schedule_data(
            &self,
            _start: i64,
            _end: i64,
        ) -> Vec<super::DebugStreamEntry> {
            self.debug_call_count.fetch_add(1, Ordering::SeqCst);
            self.debug_entries.lock().unwrap().clone()
        }
    }
}

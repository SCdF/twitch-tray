/// MockAppServices for commands unit tests.
/// Lives here because #[cfg(test)] code cannot cross crate boundaries.
use async_trait::async_trait;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Mutex;
use twitch_backend::app_services::{AppServices, DebugHotnessEntry, DebugStreamEntry};
use twitch_backend::config::{Config, FollowedCategory};
use twitch_backend::twitch::{ApiError, Category, FollowedChannel};

pub struct MockAppServices {
    config: Mutex<Config>,
    search_results: Mutex<Vec<Category>>,
    channels: Mutex<Vec<FollowedChannel>>,
    debug_entries: Mutex<Vec<DebugStreamEntry>>,
    hotness_entries: Mutex<Vec<DebugHotnessEntry>>,
    save_config_count: AtomicUsize,
    refresh_category_count: AtomicUsize,
    refresh_schedules_count: AtomicUsize,
    debug_call_count: AtomicUsize,
    hotness_call_count: AtomicUsize,
}

impl MockAppServices {
    pub fn new() -> Self {
        Self {
            config: Mutex::new(Config::default()),
            search_results: Mutex::new(Vec::new()),
            channels: Mutex::new(Vec::new()),
            debug_entries: Mutex::new(Vec::new()),
            hotness_entries: Mutex::new(Vec::new()),
            save_config_count: AtomicUsize::new(0),
            refresh_category_count: AtomicUsize::new(0),
            refresh_schedules_count: AtomicUsize::new(0),
            debug_call_count: AtomicUsize::new(0),
            hotness_call_count: AtomicUsize::new(0),
        }
    }

    pub fn set_search_results(&self, results: Vec<Category>) {
        *self.search_results.lock().unwrap() = results;
    }

    pub fn set_channels(&self, channels: Vec<FollowedChannel>) {
        *self.channels.lock().unwrap() = channels;
    }

    pub fn set_debug_entries(&self, entries: Vec<DebugStreamEntry>) {
        *self.debug_entries.lock().unwrap() = entries;
    }

    pub fn set_hotness_entries(&self, entries: Vec<DebugHotnessEntry>) {
        *self.hotness_entries.lock().unwrap() = entries;
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

    pub fn hotness_call_count(&self) -> usize {
        self.hotness_call_count.load(Ordering::SeqCst)
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

    async fn get_debug_schedule_data(&self, _start: i64, _end: i64) -> Vec<DebugStreamEntry> {
        self.debug_call_count.fetch_add(1, Ordering::SeqCst);
        self.debug_entries.lock().unwrap().clone()
    }

    async fn get_debug_hotness_data(&self) -> Vec<DebugHotnessEntry> {
        self.hotness_call_count.fetch_add(1, Ordering::SeqCst);
        self.hotness_entries.lock().unwrap().clone()
    }
}

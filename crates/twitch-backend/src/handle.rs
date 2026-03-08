use std::collections::HashMap;
use std::sync::Arc;

use tokio::sync::{broadcast, mpsc, watch};
use tokio::task::JoinHandle;

use crate::app_services::AppServices;
use crate::config::{Config, FollowedCategory};
use crate::events::BackendEvent;
use crate::twitch::{FollowedChannel, ScheduledStream, Stream};

/// Raw display data sent by the backend whenever state changes.
///
/// The menu crate subscribes to `BackendHandle.display_rx` and calls
/// `compute_display_state` to produce a `DisplayState` from this.
#[derive(Clone, Debug, Default)]
pub struct RawDisplayData {
    pub is_authenticated: bool,
    pub live_streams: Vec<Stream>,
    pub scheduled_streams: Vec<ScheduledStream>,
    pub schedules_loaded: bool,
    pub followed_channels: Vec<FollowedChannel>,
    pub followed_categories: Vec<FollowedCategory>,
    pub category_streams: HashMap<String, Vec<Stream>>,
    pub config: Config,
    /// Cached profile image URLs keyed by user/broadcaster ID.
    pub profile_image_urls: HashMap<String, String>,
    /// Cached box art URLs keyed by category/game ID.
    pub box_art_urls: HashMap<String, String>,
}

/// Commands sent to the backend auth task.
#[derive(Debug)]
pub enum AuthCommand {
    Login,
    Logout,
}

/// Progress updates during the OAuth device code login flow.
///
/// Published on `BackendHandle.login_progress_rx` (a watch channel, last-value-wins).
/// `None` means no login is in progress.
#[derive(Clone, Debug, PartialEq)]
pub enum LoginProgress {
    /// Device code obtained; user should visit the URI and enter the code shown.
    PendingCode {
        user_code: String,
        verification_uri: String,
    },
    /// Token confirmed; the user has authorized the application.
    Confirmed,
    /// Login failed with the given reason.
    Failed(String),
}

/// Everything the app layer needs to interact with the backend.
///
/// Returned from `twitch_backend::start()`.
pub struct BackendHandle {
    /// Menu crate subscribes here: raw data whenever state changes.
    pub display_rx: watch::Receiver<RawDisplayData>,
    /// All consumers subscribe for async events.
    pub event_tx: broadcast::Sender<BackendEvent>,
    /// Settings commands go through this.
    pub services: Arc<dyn AppServices>,
    /// Auth commands (login / logout) go through this.
    pub auth_cmd_tx: mpsc::UnboundedSender<AuthCommand>,
    /// Login progress updates (device code flow).
    /// `None` = no login in progress; `Some(LoginProgress::PendingCode { .. })` = code shown.
    pub login_progress_rx: watch::Receiver<Option<LoginProgress>>,
    /// Background task handles (so main can join/abort on shutdown).
    pub tasks: Vec<JoinHandle<()>>,
}

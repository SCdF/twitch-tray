use crate::state::CategoryChange;
use crate::twitch::{ScheduledStream, Stream};

/// Events emitted by the backend to all subscribers.
#[derive(Debug, Clone)]
pub enum BackendEvent {
    /// Stream state was updated (includes diff information).
    StreamsUpdated {
        newly_live: Vec<Stream>,
        category_changes: Vec<CategoryChange>,
        all_live: Vec<Stream>,
    },
    /// Schedule state was updated.
    SchedulesUpdated(Vec<ScheduledStream>),
    /// Auth state changed (login or logout).
    AuthStateChanged { is_authenticated: bool },
    /// The user clicked the Settings button on a notification.
    /// The app layer should open a streamer settings window.
    OpenSettingsRequested {
        user_login: String,
        display_name: String,
    },
}

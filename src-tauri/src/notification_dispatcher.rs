//! Notification dispatching: listens for stream update events and fires desktop
//! notifications via the `Notifier` trait.
//!
//! `NotificationDispatcher` owns nothing about *how* notifications are rendered —
//! that is `Notifier`'s job. It owns the policy of *when* to notify, delegating
//! the heavy lifting to `filter_notifications`.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use chrono::{DateTime, Utc};
use tokio::sync::broadcast;
use tokio::task::JoinHandle;

use crate::config::ConfigManager;
use crate::notification_filter::filter_notifications;
use crate::notify::Notifier;
use crate::state::StreamsUpdated;

/// Listens for `StreamsUpdated` broadcast events and dispatches desktop
/// notifications according to the current config and notification filter.
pub struct NotificationDispatcher {
    notifier: Arc<dyn Notifier>,
    config: ConfigManager,
    initial_load_done: Arc<AtomicBool>,
}

impl NotificationDispatcher {
    pub fn new(
        notifier: Arc<dyn Notifier>,
        config: ConfigManager,
        initial_load_done: Arc<AtomicBool>,
    ) -> Self {
        Self {
            notifier,
            config,
            initial_load_done,
        }
    }

    /// Spawns the listener task and returns its handle.
    pub fn start(self: Arc<Self>, rx: broadcast::Receiver<StreamsUpdated>) -> JoinHandle<()> {
        tokio::spawn(async move {
            self.listen(rx).await;
        })
    }

    async fn listen(&self, mut rx: broadcast::Receiver<StreamsUpdated>) {
        let mut last_event_time: Option<DateTime<Utc>> = None;

        loop {
            match rx.recv().await {
                Ok(event) => {
                    let now = Utc::now();
                    let cfg = self.config.get();
                    let decision = filter_notifications(
                        &event,
                        last_event_time,
                        now,
                        cfg.notify_max_gap_min * 60,
                        self.initial_load_done.load(Ordering::SeqCst),
                        &cfg.streamer_settings,
                    );
                    last_event_time = Some(now);

                    for stream in decision.streams_to_notify {
                        if let Err(e) = self.notifier.stream_live(&stream) {
                            tracing::error!("Notification error: {}", e);
                        }
                    }
                    for change in decision.categories_to_notify {
                        if let Err(e) = self
                            .notifier
                            .category_changed(&change.stream, &change.old_category)
                        {
                            tracing::error!("Notification error: {}", e);
                        }
                    }
                }
                Err(broadcast::error::RecvError::Lagged(n)) => {
                    tracing::warn!("Notification listener lagged by {} events", n);
                }
                Err(broadcast::error::RecvError::Closed) => {
                    break;
                }
            }
        }
    }
}

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
    config: Arc<ConfigManager>,
    initial_load_done: Arc<AtomicBool>,
}

impl NotificationDispatcher {
    pub fn new(
        notifier: Arc<dyn Notifier>,
        config: Arc<ConfigManager>,
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

    pub(crate) async fn listen(&self, mut rx: broadcast::Receiver<StreamsUpdated>) {
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

                    if cfg.notify_on_live {
                        for stream in decision.streams_to_notify {
                            if let Err(e) = self.notifier.stream_live(&stream) {
                                tracing::error!("Notification error: {}", e);
                            }
                        }
                    }
                    if cfg.notify_on_category {
                        for change in decision.categories_to_notify {
                            if let Err(e) = self
                                .notifier
                                .category_changed(&change.stream, &change.old_category)
                            {
                                tracing::error!("Notification error: {}", e);
                            }
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Config;
    use crate::notify::mock::RecordingNotifier;
    use crate::state::StreamsUpdated;
    use crate::twitch::Stream;
    use chrono::Utc;

    fn make_stream(user_login: &str) -> Stream {
        Stream {
            id: "1".to_string(),
            user_id: "100".to_string(),
            user_login: user_login.to_string(),
            user_name: user_login.to_string(),
            game_id: "game".to_string(),
            game_name: "Game".to_string(),
            title: "Title".to_string(),
            viewer_count: 1000,
            started_at: Utc::now() - chrono::Duration::hours(1),
            thumbnail_url: String::new(),
            tags: vec![],
            profile_image_url: String::new(),
        }
    }

    fn make_event(user_login: &str) -> StreamsUpdated {
        let stream = make_stream(user_login);
        StreamsUpdated {
            streams: vec![stream.clone()],
            newly_live: vec![stream],
            category_changes: vec![],
        }
    }

    fn make_category_event(user_login: &str) -> StreamsUpdated {
        use crate::state::CategoryChange;
        let stream = make_stream(user_login);
        StreamsUpdated {
            streams: vec![stream.clone()],
            newly_live: vec![],
            category_changes: vec![CategoryChange {
                stream,
                old_category: "Old Game".to_string(),
            }],
        }
    }

    #[tokio::test]
    async fn live_notifications_suppressed_when_config_disabled_without_restart() {
        let notifier = Arc::new(RecordingNotifier::new());
        let config = Arc::new(ConfigManager::with_config(Config {
            notify_on_live: true,
            ..Config::default()
        }));
        let initial_load_done = Arc::new(AtomicBool::new(true));

        let dispatcher =
            NotificationDispatcher::new(notifier.clone(), config.clone(), initial_load_done);

        let (tx, rx) = broadcast::channel(16);
        let handle = tokio::spawn(async move { dispatcher.listen(rx).await });

        tx.send(make_event("streamer")).unwrap();
        tokio::time::sleep(tokio::time::Duration::from_millis(50)).await;
        assert_eq!(
            notifier.notification_count(),
            1,
            "expected notification when enabled"
        );

        // Disable live notifications at runtime — no restart
        config.set(Config {
            notify_on_live: false,
            ..Config::default()
        });
        notifier.clear();

        tx.send(make_event("streamer")).unwrap();
        tokio::time::sleep(tokio::time::Duration::from_millis(50)).await;
        assert_eq!(
            notifier.notification_count(),
            0,
            "no notification expected after config change"
        );

        handle.abort();
    }

    #[tokio::test]
    async fn category_notifications_suppressed_when_config_disabled_without_restart() {
        let notifier = Arc::new(RecordingNotifier::new());
        let config = Arc::new(ConfigManager::with_config(Config {
            notify_on_category: true,
            ..Config::default()
        }));
        let initial_load_done = Arc::new(AtomicBool::new(true));

        let dispatcher =
            NotificationDispatcher::new(notifier.clone(), config.clone(), initial_load_done);

        let (tx, rx) = broadcast::channel(16);
        let handle = tokio::spawn(async move { dispatcher.listen(rx).await });

        tx.send(make_category_event("streamer")).unwrap();
        tokio::time::sleep(tokio::time::Duration::from_millis(50)).await;
        assert_eq!(
            notifier.notification_count(),
            1,
            "expected notification when enabled"
        );

        // Disable category notifications at runtime — no restart
        config.set(Config {
            notify_on_category: false,
            ..Config::default()
        });
        notifier.clear();

        tx.send(make_category_event("streamer")).unwrap();
        tokio::time::sleep(tokio::time::Duration::from_millis(50)).await;
        assert_eq!(
            notifier.notification_count(),
            0,
            "no notification expected after config change"
        );

        handle.abort();
    }
}

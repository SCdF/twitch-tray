//! Desktop notification handling
//!
//! This module provides notification functionality with a trait-based
//! abstraction for testability.

use chrono::{DateTime, Duration, Utc};
use tokio::sync::mpsc;

use crate::hotness_detection::HotnessInfo;
use crate::twitch::Stream;

const APP_NAME: &str = "Twitch Tray";
const NOTIFICATION_TIMEOUT_MS: i32 = 10_000;
const SNOOZE_DURATION_MIN: i64 = 10;

/// A request to snooze a stream notification and re-notify after a delay
#[derive(Debug, Clone)]
pub struct SnoozeRequest {
    pub user_id: String,
    pub user_name: String,
    pub remind_at: DateTime<Utc>,
}

/// A request to open per-streamer settings from a notification
#[derive(Debug, Clone)]
pub struct StreamerSettingsRequest {
    pub user_login: String,
    pub display_name: String,
}

/// Info needed to attach a snooze button to a notification
struct SnoozeInfo {
    user_id: String,
    user_name: String,
    snooze_tx: mpsc::UnboundedSender<SnoozeRequest>,
}

/// Info needed to attach a settings button to a notification
struct SettingsInfo {
    user_login: String,
    display_name: String,
    settings_tx: mpsc::UnboundedSender<StreamerSettingsRequest>,
}

/// Trait for sending notifications
///
/// This abstraction allows easy mocking of notifications in tests.
pub trait Notifier: Send + Sync {
    /// Sends a notification when a streamer goes live
    fn stream_live(&self, stream: &Stream) -> anyhow::Result<()>;

    /// Sends a reminder notification for a snoozed stream
    fn stream_reminder(&self, stream: &Stream) -> anyhow::Result<()>;

    /// Sends a notification when a streamer changes category
    fn category_changed(&self, stream: &Stream, old_category: &str) -> anyhow::Result<()>;

    /// Sends a notification when a stream is detected as "hot"
    fn stream_hot(&self, stream: &Stream, info: &HotnessInfo) -> anyhow::Result<()>;

    /// Sends an error notification
    fn error(&self, message: &str) -> anyhow::Result<()>;
}

/// Desktop notification implementation
pub struct DesktopNotifier {
    snooze_tx: mpsc::UnboundedSender<SnoozeRequest>,
    settings_tx: mpsc::UnboundedSender<StreamerSettingsRequest>,
}

impl DesktopNotifier {
    /// Creates a new notifier.
    ///
    /// `notify_on_live` and `notify_on_category` are no longer stored here —
    /// gating is done by `NotificationDispatcher` which reads config live on
    /// each event so that changes take effect without a restart.
    pub fn new(
        snooze_tx: mpsc::UnboundedSender<SnoozeRequest>,
        settings_tx: mpsc::UnboundedSender<StreamerSettingsRequest>,
    ) -> Self {
        Self {
            snooze_tx,
            settings_tx,
        }
    }

    /// Platform-specific notification sending
    #[cfg(target_os = "linux")]
    fn send_notification(
        &self,
        title: &str,
        message: &str,
        url: Option<&str>,
        category: Option<&str>,
        snooze_info: Option<SnoozeInfo>,
        settings_info: Option<SettingsInfo>,
    ) -> anyhow::Result<()> {
        use notify_rust::{Hint, Notification};

        let mut notification = Notification::new();
        notification
            .summary(title)
            .body(message)
            .appname(APP_NAME)
            .timeout(NOTIFICATION_TIMEOUT_MS);

        // Set notification category if provided (freedesktop.org spec)
        // This allows users to configure different notification behaviors per category
        if let Some(cat) = category {
            notification.hint(Hint::Category(cat.to_string()));
        }

        if let Some(url) = url {
            notification.action("default", "Open Stream");
            if snooze_info.is_some() {
                notification.action("snooze_10", "Snooze 10m");
            }
            if settings_info.is_some() {
                notification.action("streamer-settings", "\u{2699}\u{fe0f}");
            }
            let handle = notification.show()?;
            let url = url.to_string();
            std::thread::spawn(move || {
                handle.wait_for_action(|action| match action {
                    "default" => {
                        let _ = open::that(&url);
                    }
                    "snooze_10" => {
                        if let Some(info) = &snooze_info {
                            let request = SnoozeRequest {
                                user_id: info.user_id.clone(),
                                user_name: info.user_name.clone(),
                                remind_at: Utc::now() + Duration::minutes(SNOOZE_DURATION_MIN),
                            };
                            let _ = info.snooze_tx.send(request);
                        }
                    }
                    "streamer-settings" => {
                        if let Some(info) = &settings_info {
                            let request = StreamerSettingsRequest {
                                user_login: info.user_login.clone(),
                                display_name: info.display_name.clone(),
                            };
                            let _ = info.settings_tx.send(request);
                        }
                    }
                    _ => {}
                });
            });
        } else {
            notification.show()?;
        }

        Ok(())
    }

    #[cfg(not(target_os = "linux"))]
    fn send_notification(
        &self,
        title: &str,
        message: &str,
        url: Option<&str>,
        _category: Option<&str>,
        _snooze_info: Option<SnoozeInfo>,
        _settings_info: Option<SettingsInfo>,
    ) -> anyhow::Result<()> {
        // On macOS and Windows, we'll use a simple approach
        // In a full implementation, you might want to use native APIs
        tracing::info!("Notification: {} - {}", title, message);

        // Try to use the system notification mechanism
        #[cfg(target_os = "macos")]
        {
            // macOS notifications via osascript don't support click actions directly
            // We show the notification but can't make it clickable without more native code
            let _ = std::process::Command::new("osascript")
                .args([
                    "-e",
                    &format!(
                        "display notification \"{}\" with title \"{}\"",
                        message, title
                    ),
                ])
                .output();

            // Log the URL so users know what stream went live
            if let Some(url) = url {
                tracing::info!("Stream URL: {}", url);
            }
        }

        #[cfg(target_os = "windows")]
        {
            // Windows toast notifications would require additional setup
            // For now, just log
            tracing::info!("Windows notification: {} - {}", title, message);
            if let Some(url) = url {
                tracing::info!("Stream URL: {}", url);
            }
        }

        Ok(())
    }
}

/// Notification categories (freedesktop.org spec)
/// These allow users to configure different behaviors per notification type at the OS level
mod categories {
    /// Category for "stream went live" notifications
    pub const STREAM_LIVE: &str = "presence.online";
    /// Category for "category changed" notifications
    pub const CATEGORY_CHANGE: &str = "category.changed";
    /// Category for "stream is hot" notifications
    pub const STREAM_HOT: &str = "presence.hot";
}

impl DesktopNotifier {
    fn make_snooze_info(&self, stream: &Stream) -> Option<SnoozeInfo> {
        Some(SnoozeInfo {
            user_id: stream.user_id.clone(),
            user_name: stream.user_name.clone(),
            snooze_tx: self.snooze_tx.clone(),
        })
    }

    fn make_settings_info(&self, stream: &Stream) -> Option<SettingsInfo> {
        Some(SettingsInfo {
            user_login: stream.user_login.clone(),
            display_name: stream.user_name.clone(),
            settings_tx: self.settings_tx.clone(),
        })
    }
}

impl Notifier for DesktopNotifier {
    fn stream_live(&self, stream: &Stream) -> anyhow::Result<()> {
        let title = format!("{} is now live!", stream.user_name);
        let message = if stream.title.is_empty() {
            stream.game_name.clone()
        } else {
            format!("{} - {}", stream.game_name, truncate(&stream.title, 50))
        };

        let url = stream.channel_url();
        let snooze = self.make_snooze_info(stream);
        let settings = self.make_settings_info(stream);
        self.send_notification(
            &title,
            &message,
            Some(&url),
            Some(categories::STREAM_LIVE),
            snooze,
            settings,
        )
    }

    fn stream_reminder(&self, stream: &Stream) -> anyhow::Result<()> {
        let title = format!("{} live for {}", stream.user_name, stream.format_duration());
        let message = if stream.title.is_empty() {
            stream.game_name.clone()
        } else {
            format!("{} - {}", stream.game_name, truncate(&stream.title, 50))
        };

        let url = stream.channel_url();
        let snooze = self.make_snooze_info(stream);
        let settings = self.make_settings_info(stream);
        self.send_notification(
            &title,
            &message,
            Some(&url),
            Some(categories::STREAM_LIVE),
            snooze,
            settings,
        )
    }

    fn category_changed(&self, stream: &Stream, old_category: &str) -> anyhow::Result<()> {
        let title = format!("{} changed category", stream.user_name);
        let message = format!("{} → {}", old_category, stream.game_name);

        let url = stream.channel_url();
        let settings = self.make_settings_info(stream);
        self.send_notification(
            &title,
            &message,
            Some(&url),
            Some(categories::CATEGORY_CHANGE),
            None,
            settings,
        )
    }

    fn stream_hot(&self, stream: &Stream, info: &HotnessInfo) -> anyhow::Result<()> {
        let title = format!(
            "\u{1f525}\u{1f525}\u{1f525} ({:.1}\u{03c3}) {} on {} IS HOT",
            info.z_score, stream.user_name, stream.game_name,
        );
        let message = truncate(&stream.title, 80);

        let url = stream.channel_url();
        let settings = self.make_settings_info(stream);
        self.send_notification(
            &title,
            &message,
            Some(&url),
            Some(categories::STREAM_HOT),
            None,
            settings,
        )
    }

    fn error(&self, message: &str) -> anyhow::Result<()> {
        self.send_notification(APP_NAME, message, None, None, None, None)
    }
}

/// Truncates a string to max byte length with ellipsis, respecting char boundaries
pub fn truncate(s: &str, max: usize) -> String {
    if s.len() <= max {
        s.to_string()
    } else if max <= 3 {
        s[..s.floor_char_boundary(max)].to_string()
    } else {
        let end = s.floor_char_boundary(max - 3);
        format!("{}...", &s[..end])
    }
}

/// Recording notifier for testing
///
/// Records all notifications for later verification.
#[cfg(test)]
pub mod mock {
    use super::*;
    use std::sync::{Arc, RwLock};

    /// A recorded notification
    #[derive(Debug, Clone, PartialEq)]
    pub struct RecordedNotification {
        pub notification_type: NotificationType,
        pub title: String,
        pub message: String,
    }

    /// Type of notification
    #[derive(Debug, Clone, PartialEq)]
    pub enum NotificationType {
        StreamLive,
        StreamReminder,
        CategoryChange,
        StreamHot,
        Error,
    }

    /// Recording notifier that captures all notifications
    #[derive(Debug, Default, Clone)]
    pub struct RecordingNotifier {
        notifications: Arc<RwLock<Vec<RecordedNotification>>>,
    }

    impl RecordingNotifier {
        /// Creates a new recording notifier
        pub fn new() -> Self {
            Self::default()
        }

        /// Returns all recorded notifications
        pub fn get_notifications(&self) -> Vec<RecordedNotification> {
            self.notifications.read().unwrap().clone()
        }

        /// Returns the number of notifications recorded
        pub fn notification_count(&self) -> usize {
            self.notifications.read().unwrap().len()
        }

        /// Returns notifications of a specific type
        pub fn get_by_type(
            &self,
            notification_type: NotificationType,
        ) -> Vec<RecordedNotification> {
            self.notifications
                .read()
                .unwrap()
                .iter()
                .filter(|n| n.notification_type == notification_type)
                .cloned()
                .collect()
        }

        /// Clears all recorded notifications
        pub fn clear(&self) {
            self.notifications.write().unwrap().clear();
        }
    }

    impl Notifier for RecordingNotifier {
        fn stream_live(&self, stream: &Stream) -> anyhow::Result<()> {
            let title = format!("{} is now live!", stream.user_name);
            let message = if !stream.title.is_empty() {
                format!("{} - {}", stream.game_name, stream.title)
            } else {
                stream.game_name.clone()
            };

            self.notifications
                .write()
                .unwrap()
                .push(RecordedNotification {
                    notification_type: NotificationType::StreamLive,
                    title,
                    message,
                });

            Ok(())
        }

        fn stream_reminder(&self, stream: &Stream) -> anyhow::Result<()> {
            let title = format!("{} live for {}", stream.user_name, stream.format_duration());
            let message = if !stream.title.is_empty() {
                format!("{} - {}", stream.game_name, stream.title)
            } else {
                stream.game_name.clone()
            };

            self.notifications
                .write()
                .unwrap()
                .push(RecordedNotification {
                    notification_type: NotificationType::StreamReminder,
                    title,
                    message,
                });

            Ok(())
        }

        fn category_changed(&self, stream: &Stream, old_category: &str) -> anyhow::Result<()> {
            let title = format!("{} changed category", stream.user_name);
            let message = format!("{} → {}", old_category, stream.game_name);

            self.notifications
                .write()
                .unwrap()
                .push(RecordedNotification {
                    notification_type: NotificationType::CategoryChange,
                    title,
                    message,
                });

            Ok(())
        }

        fn stream_hot(&self, stream: &Stream, info: &HotnessInfo) -> anyhow::Result<()> {
            let title = format!(
                "\u{1f525}\u{1f525}\u{1f525} ({:.1}\u{03c3}) {} on {} IS HOT",
                info.z_score, stream.user_name, stream.game_name,
            );
            let message = stream.title.clone();

            self.notifications
                .write()
                .unwrap()
                .push(RecordedNotification {
                    notification_type: NotificationType::StreamHot,
                    title,
                    message,
                });

            Ok(())
        }

        fn error(&self, message: &str) -> anyhow::Result<()> {
            self.notifications
                .write()
                .unwrap()
                .push(RecordedNotification {
                    notification_type: NotificationType::Error,
                    title: "Twitch Tray".to_string(),
                    message: message.to_string(),
                });

            Ok(())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::mock::{NotificationType, RecordingNotifier};
    use super::*;
    use chrono::Utc;

    fn make_stream(user_name: &str, game_name: &str, title: &str) -> Stream {
        Stream {
            id: "123".to_string(),
            user_id: "456".to_string(),
            user_login: user_name.to_lowercase(),
            user_name: user_name.to_string(),
            game_id: "789".to_string(),
            game_name: game_name.to_string(),
            title: title.to_string(),
            viewer_count: 1000,
            started_at: Utc::now() - chrono::Duration::hours(1),
            thumbnail_url: "https://example.com/thumb.jpg".to_string(),
            tags: vec![],
            profile_image_url: String::new(),
        }
    }

    // === RecordingNotifier tests ===

    #[test]
    fn recording_notifier_records_stream_live() {
        let notifier = RecordingNotifier::new();
        let stream = make_stream("TestStreamer", "Minecraft", "Building a castle!");

        notifier.stream_live(&stream).unwrap();

        let notifications = notifier.get_notifications();
        assert_eq!(notifications.len(), 1);
        assert_eq!(
            notifications[0].notification_type,
            NotificationType::StreamLive
        );
        assert!(notifications[0].title.contains("TestStreamer"));
        assert!(notifications[0].message.contains("Minecraft"));
    }

    #[test]
    fn recording_notifier_records_error() {
        let notifier = RecordingNotifier::new();

        notifier.error("Something went wrong").unwrap();

        let notifications = notifier.get_notifications();
        assert_eq!(notifications.len(), 1);
        assert_eq!(notifications[0].notification_type, NotificationType::Error);
        assert_eq!(notifications[0].message, "Something went wrong");
    }

    #[test]
    fn recording_notifier_get_by_type() {
        let notifier = RecordingNotifier::new();
        let stream = make_stream("Streamer", "Game", "Title");

        notifier.stream_live(&stream).unwrap();
        notifier.error("Error 1").unwrap();
        notifier.stream_live(&stream).unwrap();
        notifier.error("Error 2").unwrap();

        let live_notifications = notifier.get_by_type(NotificationType::StreamLive);
        assert_eq!(live_notifications.len(), 2);

        let error_notifications = notifier.get_by_type(NotificationType::Error);
        assert_eq!(error_notifications.len(), 2);
    }

    #[test]
    fn recording_notifier_clear() {
        let notifier = RecordingNotifier::new();

        notifier.error("Test").unwrap();
        assert_eq!(notifier.notification_count(), 1);

        notifier.clear();
        assert_eq!(notifier.notification_count(), 0);
    }

    #[test]
    fn recording_notifier_records_category_change() {
        let notifier = RecordingNotifier::new();
        let stream = make_stream("TestStreamer", "Minecraft", "Building a castle!");

        notifier.category_changed(&stream, "Fortnite").unwrap();

        let notifications = notifier.get_notifications();
        assert_eq!(notifications.len(), 1);
        assert_eq!(
            notifications[0].notification_type,
            NotificationType::CategoryChange
        );
        assert!(notifications[0].title.contains("TestStreamer"));
        assert!(notifications[0].message.contains("Fortnite"));
        assert!(notifications[0].message.contains("Minecraft"));
    }

    #[test]
    fn category_change_shows_arrow() {
        let notifier = RecordingNotifier::new();
        let stream = make_stream("Streamer", "New Game", "Title");

        notifier.category_changed(&stream, "Old Game").unwrap();

        let notifications = notifier.get_notifications();
        assert!(notifications[0].message.contains("→"));
        assert_eq!(notifications[0].message, "Old Game → New Game");
    }

    // === truncate tests ===

    #[test]
    fn truncate_short_string() {
        assert_eq!(truncate("Hello", 10), "Hello");
    }

    #[test]
    fn truncate_long_string() {
        assert_eq!(
            truncate("This is a very long title that should be truncated", 20),
            "This is a very lo..."
        );
    }

    #[test]
    fn truncate_exact_length() {
        assert_eq!(truncate("Hello", 5), "Hello");
    }

    #[test]
    fn truncate_max_3() {
        assert_eq!(truncate("Hello", 3), "Hel");
    }

    #[test]
    fn truncate_max_4() {
        assert_eq!(truncate("Hello", 4), "H...");
    }

    #[test]
    fn truncate_empty_string() {
        assert_eq!(truncate("", 10), "");
    }

    #[test]
    fn truncate_game_name_realistic() {
        let long_game = "Counter-Strike: Global Offensive";
        assert_eq!(truncate(long_game, 20), "Counter-Strike: G...");
    }

    #[test]
    fn truncate_multibyte_emoji() {
        let s = "🚨GOOD TAKES🚨";
        // Should not panic on multi-byte characters
        let result = truncate(s, 10);
        assert!(result.len() <= 10);
        assert!(result.ends_with("..."));
    }
}

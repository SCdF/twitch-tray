//! Desktop notification handling
//!
//! This module provides notification functionality with a trait-based
//! abstraction for testability.

use crate::twitch::Stream;

const APP_NAME: &str = "Twitch Tray";

/// Trait for sending notifications
///
/// This abstraction allows easy mocking of notifications in tests.
pub trait Notifier: Send + Sync {
    /// Sends a notification when a streamer goes live
    fn stream_live(&self, stream: &Stream) -> anyhow::Result<()>;

    /// Sends an error notification
    fn error(&self, message: &str) -> anyhow::Result<()>;
}

/// Desktop notification implementation
pub struct DesktopNotifier {
    enabled: bool,
    notify_on_live: bool,
}

impl DesktopNotifier {
    /// Creates a new notifier
    pub fn new(notify_on_live: bool) -> Self {
        Self {
            enabled: true,
            notify_on_live,
        }
    }

    /// Platform-specific notification sending
    #[cfg(target_os = "linux")]
    fn send_notification(
        &self,
        title: &str,
        message: &str,
        url: Option<&str>,
    ) -> anyhow::Result<()> {
        use notify_rust::Notification;

        let mut notification = Notification::new();
        notification
            .summary(title)
            .body(message)
            .appname(APP_NAME)
            .timeout(5000);

        if let Some(url) = url {
            notification.action("default", "Open Stream");
            let handle = notification.show()?;
            let url = url.to_string();
            std::thread::spawn(move || {
                handle.wait_for_action(|action| {
                    if action == "default" {
                        let _ = open::that(&url);
                    }
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

impl Notifier for DesktopNotifier {
    fn stream_live(&self, stream: &Stream) -> anyhow::Result<()> {
        if !self.enabled || !self.notify_on_live {
            return Ok(());
        }

        let title = format!("{} is now live!", stream.user_name);
        let message = if !stream.title.is_empty() {
            format!("{} - {}", stream.game_name, truncate(&stream.title, 50))
        } else {
            stream.game_name.clone()
        };

        let url = format!("https://twitch.tv/{}", stream.user_login);
        self.send_notification(&title, &message, Some(&url))
    }

    fn error(&self, message: &str) -> anyhow::Result<()> {
        self.send_notification(APP_NAME, message, None)
    }
}

/// Truncates a string to max length with ellipsis
fn truncate(s: &str, max: usize) -> String {
    if s.len() <= max {
        s.to_string()
    } else if max <= 3 {
        s[..max].to_string()
    } else {
        format!("{}...", &s[..max - 3])
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
    use chrono::{Duration, Utc};

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
            started_at: Utc::now() - Duration::hours(1),
            thumbnail_url: "https://example.com/thumb.jpg".to_string(),
            tags: vec![],
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

    // === DesktopNotifier tests ===

    #[test]
    fn desktop_notifier_respects_notify_on_live_flag() {
        let notifier = DesktopNotifier::new(false);

        let stream = make_stream("Test", "Game", "Title");
        // This should not send a notification (and not error)
        notifier.stream_live(&stream).unwrap();
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
}

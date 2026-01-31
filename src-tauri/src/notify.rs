use crate::twitch::Stream;

const APP_NAME: &str = "Twitch Tray";

/// Handles desktop notifications
pub struct Notifier {
    enabled: bool,
    notify_on_live: bool,
}

impl Notifier {
    /// Creates a new notifier
    pub fn new(notify_on_live: bool) -> Self {
        Self {
            enabled: true,
            notify_on_live,
        }
    }

    /// Enables or disables all notifications
    pub fn set_enabled(&mut self, enabled: bool) {
        self.enabled = enabled;
    }

    /// Sends a notification when a streamer goes live
    pub fn stream_live(&self, stream: &Stream) -> anyhow::Result<()> {
        if !self.enabled || !self.notify_on_live {
            return Ok(());
        }

        let title = format!("{} is now live!", stream.user_name);
        let message = if !stream.title.is_empty() {
            format!("{} - {}", stream.game_name, truncate(&stream.title, 50))
        } else {
            stream.game_name.clone()
        };

        self.send_notification(&title, &message)
    }

    /// Sends an error notification
    pub fn error(&self, message: &str) -> anyhow::Result<()> {
        self.send_notification(APP_NAME, message)
    }

    /// Platform-specific notification sending
    #[cfg(target_os = "linux")]
    fn send_notification(&self, title: &str, message: &str) -> anyhow::Result<()> {
        use notify_rust::Notification;

        Notification::new()
            .summary(title)
            .body(message)
            .appname(APP_NAME)
            .timeout(5000)
            .show()?;

        Ok(())
    }

    #[cfg(not(target_os = "linux"))]
    fn send_notification(&self, title: &str, message: &str) -> anyhow::Result<()> {
        // On macOS and Windows, we'll use a simple approach
        // In a full implementation, you might want to use native APIs
        tracing::info!("Notification: {} - {}", title, message);

        // Try to use the system notification mechanism
        #[cfg(target_os = "macos")]
        {
            let _ = std::process::Command::new("osascript")
                .args([
                    "-e",
                    &format!(
                        "display notification \"{}\" with title \"{}\"",
                        message, title
                    ),
                ])
                .output();
        }

        #[cfg(target_os = "windows")]
        {
            // Windows toast notifications would require additional setup
            // For now, just log
            tracing::info!("Windows notification: {} - {}", title, message);
        }

        Ok(())
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

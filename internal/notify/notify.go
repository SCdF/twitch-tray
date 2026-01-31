package notify

import (
	"fmt"

	"github.com/gen2brain/beeep"
	"github.com/user/twitch-tray/internal/twitch"
)

const (
	appName = "Twitch Tray"
)

// Notifier handles desktop notifications
type Notifier struct {
	enabled         bool
	notifyOnLive    bool
	notifyOnCategory bool
}

// New creates a new notifier
func New(notifyOnLive, notifyOnCategory bool) *Notifier {
	return &Notifier{
		enabled:         true,
		notifyOnLive:    notifyOnLive,
		notifyOnCategory: notifyOnCategory,
	}
}

// SetEnabled enables or disables notifications
func (n *Notifier) SetEnabled(enabled bool) {
	n.enabled = enabled
}

// SetNotifyOnLive enables or disables live notifications
func (n *Notifier) SetNotifyOnLive(enabled bool) {
	n.notifyOnLive = enabled
}

// SetNotifyOnCategory enables or disables category change notifications
func (n *Notifier) SetNotifyOnCategory(enabled bool) {
	n.notifyOnCategory = enabled
}

// StreamLive sends a notification when a streamer goes live
func (n *Notifier) StreamLive(stream twitch.Stream) error {
	if !n.enabled || !n.notifyOnLive {
		return nil
	}

	title := fmt.Sprintf("%s is now live!", stream.UserName)
	message := stream.GameName
	if stream.Title != "" {
		message = fmt.Sprintf("%s - %s", stream.GameName, truncate(stream.Title, 50))
	}

	return beeep.Notify(title, message, "")
}

// StreamLiveSimple sends a notification with basic stream info
func (n *Notifier) StreamLiveSimple(userName, gameName string) error {
	if !n.enabled || !n.notifyOnLive {
		return nil
	}

	title := fmt.Sprintf("%s is now live!", userName)
	message := gameName
	if gameName == "" {
		message = "Started streaming"
	}

	return beeep.Notify(title, message, "")
}

// StreamOffline sends a notification when a streamer goes offline
func (n *Notifier) StreamOffline(userName string) error {
	if !n.enabled {
		return nil
	}

	// Typically we don't notify on offline, but the method is here if needed
	return nil
}

// CategoryChange sends a notification when a streamer changes category
func (n *Notifier) CategoryChange(userName, oldCategory, newCategory string) error {
	if !n.enabled || !n.notifyOnCategory {
		return nil
	}

	title := fmt.Sprintf("%s changed category", userName)
	message := fmt.Sprintf("Now playing: %s", newCategory)

	return beeep.Notify(title, message, "")
}

// AuthCode sends a notification with the device code for authentication
func (n *Notifier) AuthCode(userCode, verificationURI string) error {
	title := "Twitch Login"
	message := fmt.Sprintf("Go to %s and enter code: %s", verificationURI, userCode)

	return beeep.Notify(title, message, "")
}

// AuthSuccess sends a notification on successful authentication
func (n *Notifier) AuthSuccess(userName string) error {
	title := appName
	message := fmt.Sprintf("Logged in as %s", userName)

	return beeep.Notify(title, message, "")
}

// Error sends an error notification
func (n *Notifier) Error(message string) error {
	return beeep.Notify(appName, message, "")
}

// ScheduledSoon sends a notification for an upcoming scheduled stream
func (n *Notifier) ScheduledSoon(scheduled twitch.ScheduledStream) error {
	if !n.enabled {
		return nil
	}

	title := fmt.Sprintf("%s starting soon!", scheduled.BroadcasterName)
	message := scheduled.Title
	if scheduled.Category != "" {
		message = fmt.Sprintf("%s - %s", scheduled.Category, scheduled.Title)
	}

	return beeep.Notify(title, message, "")
}

// truncate truncates a string to max length with ellipsis
func truncate(s string, max int) string {
	if len(s) <= max {
		return s
	}
	if max <= 3 {
		return s[:max]
	}
	return s[:max-3] + "..."
}

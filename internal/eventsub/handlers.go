package eventsub

import (
	"encoding/json"
	"time"
)

// StreamOnlineEvent is the event data for stream.online
type StreamOnlineEvent struct {
	ID                   string    `json:"id"`
	BroadcasterUserID    string    `json:"broadcaster_user_id"`
	BroadcasterUserLogin string    `json:"broadcaster_user_login"`
	BroadcasterUserName  string    `json:"broadcaster_user_name"`
	Type                 string    `json:"type"` // "live", "playlist", "watch_party", "premiere", "rerun"
	StartedAt            time.Time `json:"started_at"`
}

// StreamOfflineEvent is the event data for stream.offline
type StreamOfflineEvent struct {
	BroadcasterUserID    string `json:"broadcaster_user_id"`
	BroadcasterUserLogin string `json:"broadcaster_user_login"`
	BroadcasterUserName  string `json:"broadcaster_user_name"`
}

// ChannelUpdateEvent is the event data for channel.update
type ChannelUpdateEvent struct {
	BroadcasterUserID           string   `json:"broadcaster_user_id"`
	BroadcasterUserLogin        string   `json:"broadcaster_user_login"`
	BroadcasterUserName         string   `json:"broadcaster_user_name"`
	Title                       string   `json:"title"`
	Language                    string   `json:"language"`
	CategoryID                  string   `json:"category_id"`
	CategoryName                string   `json:"category_name"`
	ContentClassificationLabels []string `json:"content_classification_labels"`
}

// EventHandlers provides typed event handlers
type EventHandlers struct {
	OnStreamOnline  func(event StreamOnlineEvent)
	OnStreamOffline func(event StreamOfflineEvent)
	OnChannelUpdate func(event ChannelUpdateEvent)
}

// NewEventHandlers creates event handlers that parse and dispatch to typed callbacks
func NewEventHandlers(handlers EventHandlers) EventHandler {
	return func(eventType string, event json.RawMessage) {
		switch eventType {
		case string(SubStreamOnline):
			if handlers.OnStreamOnline != nil {
				var e StreamOnlineEvent
				if err := json.Unmarshal(event, &e); err == nil {
					handlers.OnStreamOnline(e)
				}
			}
		case string(SubStreamOffline):
			if handlers.OnStreamOffline != nil {
				var e StreamOfflineEvent
				if err := json.Unmarshal(event, &e); err == nil {
					handlers.OnStreamOffline(e)
				}
			}
		case string(SubChannelUpdate):
			if handlers.OnChannelUpdate != nil {
				var e ChannelUpdateEvent
				if err := json.Unmarshal(event, &e); err == nil {
					handlers.OnChannelUpdate(e)
				}
			}
		}
	}
}

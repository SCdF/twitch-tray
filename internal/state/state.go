package state

import (
	"sync"

	"github.com/user/twitch-tray/internal/twitch"
)

// ChangeType indicates what type of state change occurred
type ChangeType int

const (
	ChangeFollowedStreams ChangeType = iota
	ChangeCategoryStreams
	ChangeScheduledStreams
	ChangeAuthentication
)

// ChangeCallback is called when state changes
type ChangeCallback func(changeType ChangeType)

// State holds the application state
type State struct {
	mu sync.RWMutex

	// Authentication state
	authenticated bool
	userID        string
	userLogin     string

	// Stream data
	followedStreams   []twitch.Stream
	categoryStreams   map[string][]twitch.Stream // gameID -> streams
	scheduledStreams  []twitch.ScheduledStream
	followedChannelIDs []string

	// Categories being tracked (from followed live streams)
	trackedCategories map[string]string // gameID -> gameName

	// Change callbacks
	callbacks []ChangeCallback
}

// New creates a new state manager
func New() *State {
	return &State{
		categoryStreams:   make(map[string][]twitch.Stream),
		trackedCategories: make(map[string]string),
	}
}

// OnChange registers a callback for state changes
func (s *State) OnChange(cb ChangeCallback) {
	s.mu.Lock()
	defer s.mu.Unlock()
	s.callbacks = append(s.callbacks, cb)
}

func (s *State) notifyChange(changeType ChangeType) {
	s.mu.RLock()
	callbacks := make([]ChangeCallback, len(s.callbacks))
	copy(callbacks, s.callbacks)
	s.mu.RUnlock()

	for _, cb := range callbacks {
		cb(changeType)
	}
}

// SetAuthenticated updates the authentication state
func (s *State) SetAuthenticated(authenticated bool, userID, userLogin string) {
	s.mu.Lock()
	changed := s.authenticated != authenticated || s.userID != userID
	s.authenticated = authenticated
	s.userID = userID
	s.userLogin = userLogin
	s.mu.Unlock()

	if changed {
		s.notifyChange(ChangeAuthentication)
	}
}

// IsAuthenticated returns whether the user is authenticated
func (s *State) IsAuthenticated() bool {
	s.mu.RLock()
	defer s.mu.RUnlock()
	return s.authenticated
}

// GetUserID returns the authenticated user's ID
func (s *State) GetUserID() string {
	s.mu.RLock()
	defer s.mu.RUnlock()
	return s.userID
}

// GetUserLogin returns the authenticated user's login
func (s *State) GetUserLogin() string {
	s.mu.RLock()
	defer s.mu.RUnlock()
	return s.userLogin
}

// SetFollowedStreams updates the followed live streams
func (s *State) SetFollowedStreams(streams []twitch.Stream) (newlyLive []twitch.Stream, wentOffline []twitch.Stream) {
	s.mu.Lock()

	// Build maps for comparison
	oldByID := make(map[string]twitch.Stream)
	for _, stream := range s.followedStreams {
		oldByID[stream.UserID] = stream
	}

	newByID := make(map[string]twitch.Stream)
	for _, stream := range streams {
		newByID[stream.UserID] = stream
	}

	// Find newly live streams
	for _, stream := range streams {
		if _, existed := oldByID[stream.UserID]; !existed {
			newlyLive = append(newlyLive, stream)
		}
	}

	// Find streams that went offline
	for _, stream := range s.followedStreams {
		if _, exists := newByID[stream.UserID]; !exists {
			wentOffline = append(wentOffline, stream)
		}
	}

	// Update tracked categories based on current live streams
	s.trackedCategories = make(map[string]string)
	for _, stream := range streams {
		if stream.GameID != "" {
			s.trackedCategories[stream.GameID] = stream.GameName
		}
	}

	s.followedStreams = streams
	s.mu.Unlock()

	s.notifyChange(ChangeFollowedStreams)
	return
}

// GetFollowedStreams returns the current followed live streams
func (s *State) GetFollowedStreams() []twitch.Stream {
	s.mu.RLock()
	defer s.mu.RUnlock()

	result := make([]twitch.Stream, len(s.followedStreams))
	copy(result, s.followedStreams)
	return result
}

// GetTrackedCategories returns categories from currently live followed streams
func (s *State) GetTrackedCategories() map[string]string {
	s.mu.RLock()
	defer s.mu.RUnlock()

	result := make(map[string]string, len(s.trackedCategories))
	for k, v := range s.trackedCategories {
		result[k] = v
	}
	return result
}

// SetCategoryStreams updates the top streams for a category
func (s *State) SetCategoryStreams(gameID string, streams []twitch.Stream) {
	s.mu.Lock()
	s.categoryStreams[gameID] = streams
	s.mu.Unlock()

	s.notifyChange(ChangeCategoryStreams)
}

// GetCategoryStreams returns top streams for a category
func (s *State) GetCategoryStreams(gameID string) []twitch.Stream {
	s.mu.RLock()
	defer s.mu.RUnlock()

	streams := s.categoryStreams[gameID]
	result := make([]twitch.Stream, len(streams))
	copy(result, streams)
	return result
}

// GetAllCategoryStreams returns all category streams
func (s *State) GetAllCategoryStreams() map[string][]twitch.Stream {
	s.mu.RLock()
	defer s.mu.RUnlock()

	result := make(map[string][]twitch.Stream, len(s.categoryStreams))
	for k, v := range s.categoryStreams {
		streams := make([]twitch.Stream, len(v))
		copy(streams, v)
		result[k] = streams
	}
	return result
}

// SetScheduledStreams updates the scheduled streams
func (s *State) SetScheduledStreams(streams []twitch.ScheduledStream) {
	s.mu.Lock()
	s.scheduledStreams = streams
	s.mu.Unlock()

	s.notifyChange(ChangeScheduledStreams)
}

// GetScheduledStreams returns the current scheduled streams
func (s *State) GetScheduledStreams() []twitch.ScheduledStream {
	s.mu.RLock()
	defer s.mu.RUnlock()

	result := make([]twitch.ScheduledStream, len(s.scheduledStreams))
	copy(result, s.scheduledStreams)
	return result
}

// SetFollowedChannelIDs sets the list of followed channel IDs
func (s *State) SetFollowedChannelIDs(ids []string) {
	s.mu.Lock()
	s.followedChannelIDs = ids
	s.mu.Unlock()
}

// GetFollowedChannelIDs returns the list of followed channel IDs
func (s *State) GetFollowedChannelIDs() []string {
	s.mu.RLock()
	defer s.mu.RUnlock()

	result := make([]string, len(s.followedChannelIDs))
	copy(result, s.followedChannelIDs)
	return result
}

// FindStreamByUserID finds a stream by user ID
func (s *State) FindStreamByUserID(userID string) (twitch.Stream, bool) {
	s.mu.RLock()
	defer s.mu.RUnlock()

	for _, stream := range s.followedStreams {
		if stream.UserID == userID {
			return stream, true
		}
	}
	return twitch.Stream{}, false
}

// Clear resets all state (used on logout)
func (s *State) Clear() {
	s.mu.Lock()
	s.authenticated = false
	s.userID = ""
	s.userLogin = ""
	s.followedStreams = nil
	s.categoryStreams = make(map[string][]twitch.Stream)
	s.scheduledStreams = nil
	s.followedChannelIDs = nil
	s.trackedCategories = make(map[string]string)
	s.mu.Unlock()

	s.notifyChange(ChangeAuthentication)
}

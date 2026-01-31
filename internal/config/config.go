package config

import (
	"encoding/json"
	"os"
	"path/filepath"
	"sync"

	"github.com/adrg/xdg"
)

const (
	appName    = "twitch-tray"
	configFile = "config.json"
)

// Config holds the application configuration
type Config struct {
	ClientID          string   `json:"client_id"`
	PollIntervalSec   int      `json:"poll_interval_sec"`
	SchedulePollMin   int      `json:"schedule_poll_min"`
	TopStreamsPerGame int      `json:"top_streams_per_game"`
	NotifyOnLive      bool     `json:"notify_on_live"`
	NotifyOnCategory  bool     `json:"notify_on_category"`
	FollowedGames     []string `json:"followed_games,omitempty"`
}

// Manager handles configuration loading and saving
type Manager struct {
	mu       sync.RWMutex
	config   Config
	filePath string
}

// DefaultConfig returns the default configuration
func DefaultConfig() Config {
	return Config{
		ClientID:          "",
		PollIntervalSec:   60,
		SchedulePollMin:   5,
		TopStreamsPerGame: 5,
		NotifyOnLive:      true,
		NotifyOnCategory:  true,
	}
}

// NewManager creates a new configuration manager
func NewManager() (*Manager, error) {
	configPath, err := xdg.ConfigFile(filepath.Join(appName, configFile))
	if err != nil {
		return nil, err
	}

	m := &Manager{
		config:   DefaultConfig(),
		filePath: configPath,
	}

	if err := m.Load(); err != nil && !os.IsNotExist(err) {
		return nil, err
	}

	return m, nil
}

// Load reads the configuration from disk
func (m *Manager) Load() error {
	m.mu.Lock()
	defer m.mu.Unlock()

	data, err := os.ReadFile(m.filePath)
	if err != nil {
		return err
	}

	var cfg Config
	if err := json.Unmarshal(data, &cfg); err != nil {
		return err
	}

	// Merge with defaults for any missing fields
	defaults := DefaultConfig()
	if cfg.PollIntervalSec == 0 {
		cfg.PollIntervalSec = defaults.PollIntervalSec
	}
	if cfg.SchedulePollMin == 0 {
		cfg.SchedulePollMin = defaults.SchedulePollMin
	}
	if cfg.TopStreamsPerGame == 0 {
		cfg.TopStreamsPerGame = defaults.TopStreamsPerGame
	}

	m.config = cfg
	return nil
}

// Save writes the configuration to disk
func (m *Manager) Save() error {
	m.mu.RLock()
	defer m.mu.RUnlock()

	dir := filepath.Dir(m.filePath)
	if err := os.MkdirAll(dir, 0700); err != nil {
		return err
	}

	data, err := json.MarshalIndent(m.config, "", "  ")
	if err != nil {
		return err
	}

	return os.WriteFile(m.filePath, data, 0600)
}

// Get returns a copy of the current configuration
func (m *Manager) Get() Config {
	m.mu.RLock()
	defer m.mu.RUnlock()
	return m.config
}

// SetClientID updates the client ID and saves
func (m *Manager) SetClientID(clientID string) error {
	m.mu.Lock()
	m.config.ClientID = clientID
	m.mu.Unlock()
	return m.Save()
}

// GetClientID returns the current client ID
func (m *Manager) GetClientID() string {
	m.mu.RLock()
	defer m.mu.RUnlock()
	return m.config.ClientID
}

// FilePath returns the path to the config file
func (m *Manager) FilePath() string {
	return m.filePath
}

package app

import (
	"context"
	"fmt"
	"log"
	"sync"
	"time"

	"github.com/user/twitch-tray/internal/auth"
	"github.com/user/twitch-tray/internal/config"
	"github.com/user/twitch-tray/internal/notify"
	"github.com/user/twitch-tray/internal/state"
	"github.com/user/twitch-tray/internal/tray"
	"github.com/user/twitch-tray/internal/twitch"
)

// App orchestrates all application components
type App struct {
	config   *config.Manager
	store    *auth.Store
	state    *state.State
	client   *twitch.Client
	tray     *tray.Tray
	notifier *notify.Notifier

	ctx    context.Context
	cancel context.CancelFunc
	wg     sync.WaitGroup

	// Track if initial load is complete (don't notify until then)
	initialLoadDone bool
}

// New creates a new application instance
func New() (*App, error) {
	// Initialize config
	cfg, err := config.NewManager()
	if err != nil {
		return nil, fmt.Errorf("failed to initialize config: %w", err)
	}

	// Initialize token store
	store, err := auth.NewStore()
	if err != nil {
		return nil, fmt.Errorf("failed to initialize token store: %w", err)
	}

	// Initialize state
	appState := state.New()

	// Initialize notifier
	cfgData := cfg.Get()
	notifier := notify.New(cfgData.NotifyOnLive, cfgData.NotifyOnCategory)

	// Initialize tray
	appTray := tray.New(appState)

	app := &App{
		config:   cfg,
		store:    store,
		state:    appState,
		tray:     appTray,
		notifier: notifier,
	}

	// Set tray callbacks
	appTray.SetCallbacks(app.handleLogin, app.handleLogout, app.handleQuit)

	return app, nil
}

// Run starts the application
func (a *App) Run() error {
	a.ctx, a.cancel = context.WithCancel(context.Background())

	// Try to restore session from stored token
	if err := a.restoreSession(); err != nil {
		log.Printf("No stored session: %v", err)
	}

	// Run the tray (blocks until quit)
	a.tray.Run()

	return nil
}

func (a *App) restoreSession() error {
	token, err := a.store.LoadToken()
	if err != nil {
		return err
	}

	if !token.IsValid() {
		return fmt.Errorf("stored token is invalid or expired")
	}

	return a.initializeSession(auth.ClientID, token)
}

func (a *App) initializeSession(clientID string, token *auth.Token) error {
	// Create Twitch client
	client, err := twitch.NewClient(clientID)
	if err != nil {
		return fmt.Errorf("failed to create Twitch client: %w", err)
	}

	client.SetAccessToken(token.AccessToken)
	client.SetUserID(token.UserID)
	a.client = client

	// Update state
	a.state.SetAuthenticated(true, token.UserID, token.UserLogin)

	// Load followed channels
	if err := a.loadFollowedChannels(); err != nil {
		log.Printf("Failed to load followed channels: %v", err)
	}

	// Start polling (EventSub removed - too many subscriptions hit rate limits)
	a.startPolling()

	// Initial data fetch
	go a.refreshAllData()

	return nil
}

func (a *App) loadFollowedChannels() error {
	follows, err := a.client.GetAllFollowedChannels(a.ctx)
	if err != nil {
		return err
	}

	ids := make([]string, 0, len(follows))
	for _, f := range follows {
		ids = append(ids, f.BroadcasterID)
	}
	a.state.SetFollowedChannelIDs(ids)

	return nil
}

func (a *App) startPolling() {
	cfg := a.config.Get()

	// Poll for scheduled streams
	a.wg.Add(1)
	go func() {
		defer a.wg.Done()
		ticker := time.NewTicker(time.Duration(cfg.SchedulePollMin) * time.Minute)
		defer ticker.Stop()

		for {
			select {
			case <-a.ctx.Done():
				return
			case <-ticker.C:
				a.refreshScheduledStreams()
			}
		}
	}()

	// Also poll followed streams as backup to EventSub
	a.wg.Add(1)
	go func() {
		defer a.wg.Done()
		ticker := time.NewTicker(time.Duration(cfg.PollIntervalSec) * time.Second)
		defer ticker.Stop()

		for {
			select {
			case <-a.ctx.Done():
				return
			case <-ticker.C:
				a.refreshFollowedStreams()
			}
		}
	}()
}

func (a *App) refreshAllData() {
	a.refreshFollowedStreams()
	a.refreshScheduledStreams()
	a.initialLoadDone = true
}

func (a *App) refreshFollowedStreams() {
	if a.client == nil {
		return
	}

	streams, err := a.client.GetFollowedStreams(a.ctx)
	if err != nil {
		log.Printf("Failed to get followed streams: %v", err)
		return
	}

	newlyLive, _ := a.state.SetFollowedStreams(streams)

	// Notify for newly live streams (only after initial load)
	if a.initialLoadDone {
		for _, stream := range newlyLive {
			if err := a.notifier.StreamLive(stream); err != nil {
				log.Printf("Notification error: %v", err)
			}
		}
	}
}

func (a *App) refreshScheduledStreams() {
	if a.client == nil {
		return
	}

	log.Printf("Fetching scheduled streams...")
	scheduled, err := a.client.GetScheduledStreamsForFollowed(a.ctx)
	if err != nil {
		log.Printf("Failed to get scheduled streams: %v", err)
		return
	}

	a.state.SetScheduledStreams(scheduled)
}

func (a *App) handleLogin() {
	// Start device code flow
	flow := auth.NewDeviceFlow(auth.ClientID)

	go func() {
		token, err := flow.Authenticate(a.ctx, func(userCode, verificationURI string) {
			// Open browser to verification URL
			tray.OpenURL(verificationURI)
		})

		if err != nil {
			log.Printf("Authentication failed: %v", err)
			return
		}

		// Save token
		if err := a.store.SaveToken(token); err != nil {
			log.Printf("Failed to save token: %v", err)
		}

		// Initialize session
		if err := a.initializeSession(auth.ClientID, token); err != nil {
			log.Printf("Failed to initialize session: %v", err)
			a.notifier.Error("Failed to initialize session")
			return
		}

		log.Printf("Logged in as %s", token.UserLogin)
	}()
}

func (a *App) handleLogout() {
	// Clear stored token
	if err := a.store.DeleteToken(); err != nil {
		log.Printf("Failed to delete token: %v", err)
	}

	// Clear state
	a.state.Clear()
	a.client = nil
}

func (a *App) handleQuit() {
	// Cancel context to stop all goroutines
	if a.cancel != nil {
		a.cancel()
	}

	// Wait for goroutines
	a.wg.Wait()

	// Quit the tray
	a.tray.Quit()
}

package tray

import (
	"github.com/user/twitch-tray/assets"
	"github.com/user/twitch-tray/internal/state"

	"fyne.io/systray"
)

// Tray manages the system tray icon and menu
type Tray struct {
	state    *state.State
	handlers *Handlers
	menu     *Menu

	// Callbacks
	onLogin  func()
	onLogout func()
	onQuit   func()
}

// New creates a new tray manager
func New(s *state.State) *Tray {
	t := &Tray{
		state: s,
	}
	t.handlers = NewHandlers(t)
	t.menu = NewMenu(t)
	return t
}

// SetCallbacks sets the action callbacks
func (t *Tray) SetCallbacks(onLogin, onLogout, onQuit func()) {
	t.onLogin = onLogin
	t.onLogout = onLogout
	t.onQuit = onQuit
}

// Run starts the system tray (blocks until quit)
func (t *Tray) Run() {
	systray.Run(t.onReady, t.onExit)
}

// Quit exits the system tray
func (t *Tray) Quit() {
	systray.Quit()
}

func (t *Tray) onReady() {
	// Set the icon
	systray.SetIcon(assets.Icon)
	systray.SetTooltip("Twitch Tray")

	// Register for state changes
	t.state.OnChange(func(changeType state.ChangeType) {
		t.Refresh()
	})

	// Build initial menu
	t.menu.Build()
}

func (t *Tray) onExit() {
	// Cleanup if needed
}

// Refresh rebuilds the menu based on current state
func (t *Tray) Refresh() {
	t.menu.Rebuild()
}

// SetIconAuthenticated sets the icon based on auth state
func SetIconAuthenticated(authenticated bool) {
	if authenticated {
		systray.SetIcon(assets.Icon)
	} else {
		systray.SetIcon(assets.IconGrey)
	}
}

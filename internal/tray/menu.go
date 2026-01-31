package tray

import (
	"fmt"
	"sort"
	"sync"

	"fyne.io/systray"
	"github.com/user/twitch-tray/internal/twitch"
)

// Menu manages the dynamic menu structure
type Menu struct {
	tray *Tray
	mu   sync.Mutex
}

// NewMenu creates a new menu manager
func NewMenu(t *Tray) *Menu {
	return &Menu{
		tray: t,
	}
}

// Build creates the initial menu structure
func (m *Menu) Build() {
	m.Rebuild()
}

// Rebuild recreates the menu based on current state
func (m *Menu) Rebuild() {
	m.mu.Lock()
	defer m.mu.Unlock()

	// Reset menu
	systray.ResetMenu()

	if !m.tray.state.IsAuthenticated() {
		m.buildUnauthenticatedMenu()
	} else {
		m.buildAuthenticatedMenu()
	}

	// Update icon based on auth state
	SetIconAuthenticated(m.tray.state.IsAuthenticated())
}

func (m *Menu) buildUnauthenticatedMenu() {
	loginItem := systray.AddMenuItem("Login to Twitch", "Authenticate with Twitch")
	go func() {
		for range loginItem.ClickedCh {
			if m.tray.onLogin != nil {
				m.tray.onLogin()
			}
		}
	}()

	systray.AddSeparator()

	quitItem := systray.AddMenuItem("Quit", "Exit the application")
	go func() {
		for range quitItem.ClickedCh {
			if m.tray.onQuit != nil {
				m.tray.onQuit()
			}
		}
	}()
}

func (m *Menu) buildAuthenticatedMenu() {
	// Following Live section
	m.buildFollowingLiveSection()

	systray.AddSeparator()

	// Scheduled section
	m.buildScheduledSection()

	systray.AddSeparator()

	// Logout and Quit
	logoutItem := systray.AddMenuItem("Logout", "Sign out of Twitch")
	go func() {
		for range logoutItem.ClickedCh {
			if m.tray.onLogout != nil {
				m.tray.onLogout()
			}
		}
	}()

	quitItem := systray.AddMenuItem("Quit", "Exit the application")
	go func() {
		for range quitItem.ClickedCh {
			if m.tray.onQuit != nil {
				m.tray.onQuit()
			}
		}
	}()
}

func (m *Menu) buildFollowingLiveSection() {
	streams := m.tray.state.GetFollowedStreams()

	title := "Following Live"
	if len(streams) > 0 {
		title = fmt.Sprintf("Following Live (%d)", len(streams))
	}

	// Header (disabled, just a label)
	header := systray.AddMenuItem(title, "Live streams from channels you follow")
	header.Disable()

	if len(streams) == 0 {
		noneItem := systray.AddMenuItem("  No streams live", "")
		noneItem.Disable()
		return
	}

	// Sort by viewer count (highest first)
	sort.Slice(streams, func(i, j int) bool {
		return streams[i].ViewerCount > streams[j].ViewerCount
	})

	// Show top 10 in main menu
	const mainMenuLimit = 10
	showInMain := streams
	var overflow []twitch.Stream
	if len(streams) > mainMenuLimit {
		showInMain = streams[:mainMenuLimit]
		overflow = streams[mainMenuLimit:]
	}

	for _, stream := range showInMain {
		s := stream // capture for closure
		label := formatStreamLabel(s)
		tooltip := s.Title

		item := systray.AddMenuItem(label, tooltip)
		go func() {
			for range item.ClickedCh {
				m.tray.handlers.OpenStream(s.UserLogin)
			}
		}()
	}

	// Add "More" submenu if there are overflow streams
	if len(overflow) > 0 {
		moreMenu := systray.AddMenuItem(fmt.Sprintf("More (%d)...", len(overflow)), "Additional live streams")
		for _, stream := range overflow {
			s := stream // capture for closure
			label := formatStreamLabel(s)
			tooltip := s.Title

			item := moreMenu.AddSubMenuItem(label, tooltip)
			go func() {
				for range item.ClickedCh {
					m.tray.handlers.OpenStream(s.UserLogin)
				}
			}()
		}
	}
}

func (m *Menu) buildScheduledSection() {
	scheduled := m.tray.state.GetScheduledStreams()

	title := "Scheduled (Next 24h)"

	// Header (disabled, just a label)
	header := systray.AddMenuItem(title, "Upcoming scheduled streams")
	header.Disable()

	if len(scheduled) == 0 {
		var label string
		if m.tray.state.SchedulesLoaded() {
			label = "  No scheduled streams"
		} else {
			label = "  Loading..."
		}
		noneItem := systray.AddMenuItem(label, "")
		noneItem.Disable()
		return
	}

	// Show top 5 in main menu
	const mainMenuLimit = 5
	showInMain := scheduled
	var overflow []twitch.ScheduledStream
	if len(scheduled) > mainMenuLimit {
		showInMain = scheduled[:mainMenuLimit]
		overflow = scheduled[mainMenuLimit:]
	}

	// Already sorted by start time
	for _, sched := range showInMain {
		s := sched // capture for closure
		label := formatScheduledLabel(s)
		tooltip := s.Title
		if s.Category != "" {
			tooltip = fmt.Sprintf("%s - %s", s.Category, s.Title)
		}

		item := systray.AddMenuItem(label, tooltip)
		go func() {
			for range item.ClickedCh {
				m.tray.handlers.OpenStream(s.BroadcasterLogin)
			}
		}()
	}

	// Add "More" submenu if there are overflow scheduled streams
	if len(overflow) > 0 {
		moreMenu := systray.AddMenuItem(fmt.Sprintf("More (%d)...", len(overflow)), "Additional scheduled streams")
		for _, sched := range overflow {
			s := sched // capture for closure
			label := formatScheduledLabel(s)
			tooltip := s.Title
			if s.Category != "" {
				tooltip = fmt.Sprintf("%s - %s", s.Category, s.Title)
			}

			item := moreMenu.AddSubMenuItem(label, tooltip)
			go func() {
				for range item.ClickedCh {
					m.tray.handlers.OpenStream(s.BroadcasterLogin)
				}
			}()
		}
	}
}

// formatStreamLabel formats a stream for the Following Live menu
// Format: "StreamerName - GameName (1.2k, 2h 15m)"
func formatStreamLabel(s twitch.Stream) string {
	return fmt.Sprintf("%s - %s (%s, %s)",
		s.UserName,
		truncate(s.GameName, 20),
		s.FormatViewerCount(),
		s.FormatDuration(),
	)
}

// formatScheduledLabel formats a scheduled stream
// Format: "StreamerName - Tomorrow 3:00 PM"
func formatScheduledLabel(s twitch.ScheduledStream) string {
	return fmt.Sprintf("%s - %s", s.BroadcasterName, s.FormatStartTime())
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

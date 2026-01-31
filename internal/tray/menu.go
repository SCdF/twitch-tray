package tray

import (
	"fmt"
	"sort"

	"fyne.io/systray"
	"github.com/user/twitch-tray/internal/twitch"
)

// Menu manages the dynamic menu structure
type Menu struct {
	tray *Tray

	// Menu items (stored for cleanup/rebuild)
	menuItems []*systray.MenuItem
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

	// Top in Category sections
	m.buildCategorySections()

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

	followingMenu := systray.AddMenuItem(title, "Live streams from channels you follow")

	if len(streams) == 0 {
		noneItem := followingMenu.AddSubMenuItem("No streams live", "")
		noneItem.Disable()
		return
	}

	// Sort by viewer count (highest first)
	sort.Slice(streams, func(i, j int) bool {
		return streams[i].ViewerCount > streams[j].ViewerCount
	})

	for _, stream := range streams {
		s := stream // capture for closure
		label := formatStreamLabel(s)
		tooltip := s.Title

		item := followingMenu.AddSubMenuItem(label, tooltip)
		go func() {
			for range item.ClickedCh {
				m.tray.handlers.OpenStream(s.UserLogin)
			}
		}()
	}
}

func (m *Menu) buildCategorySections() {
	categories := m.tray.state.GetTrackedCategories()
	categoryStreams := m.tray.state.GetAllCategoryStreams()

	if len(categories) == 0 {
		return
	}

	// Sort categories by name for consistent ordering
	var categoryIDs []string
	for id := range categories {
		categoryIDs = append(categoryIDs, id)
	}
	sort.Slice(categoryIDs, func(i, j int) bool {
		return categories[categoryIDs[i]] < categories[categoryIDs[j]]
	})

	for _, gameID := range categoryIDs {
		gameName := categories[gameID]
		streams := categoryStreams[gameID]

		if len(streams) == 0 {
			continue
		}

		title := fmt.Sprintf("Top in %s", gameName)
		categoryMenu := systray.AddMenuItem(title, fmt.Sprintf("Top streams in %s", gameName))

		// Already sorted by viewer count from API
		for _, stream := range streams {
			s := stream // capture for closure
			label := formatCategoryStreamLabel(s)
			tooltip := s.Title

			item := categoryMenu.AddSubMenuItem(label, tooltip)
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
	scheduledMenu := systray.AddMenuItem(title, "Upcoming scheduled streams")

	if len(scheduled) == 0 {
		noneItem := scheduledMenu.AddSubMenuItem("No scheduled streams", "")
		noneItem.Disable()
		return
	}

	// Already sorted by start time
	for _, sched := range scheduled {
		s := sched // capture for closure
		label := formatScheduledLabel(s)
		tooltip := s.Title
		if s.Category != "" {
			tooltip = fmt.Sprintf("%s - %s", s.Category, s.Title)
		}

		item := scheduledMenu.AddSubMenuItem(label, tooltip)
		go func() {
			for range item.ClickedCh {
				m.tray.handlers.OpenStream(s.BroadcasterLogin)
			}
		}()
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

// formatCategoryStreamLabel formats a stream for the Top in Category menu
// Format: "StreamerName (45.2k)"
func formatCategoryStreamLabel(s twitch.Stream) string {
	return fmt.Sprintf("%s (%s)", s.UserName, s.FormatViewerCount())
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

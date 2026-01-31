package twitch

import (
	"time"
)

// Stream represents a live stream
type Stream struct {
	ID           string
	UserID       string
	UserLogin    string
	UserName     string
	GameID       string
	GameName     string
	Title        string
	ViewerCount  int
	StartedAt    time.Time
	ThumbnailURL string
	Tags         []string
}

// ScheduledStream represents a scheduled broadcast
type ScheduledStream struct {
	ID            string
	BroadcasterID string
	BroadcasterName string
	BroadcasterLogin string
	Title         string
	StartTime     time.Time
	EndTime       time.Time
	Category      string
	CategoryID    string
	IsRecurring   bool
}

// Category represents a game/category
type Category struct {
	ID        string
	Name      string
	BoxArtURL string
}

// StreamDuration returns the duration since the stream started
func (s *Stream) Duration() time.Duration {
	return time.Since(s.StartedAt)
}

// FormatDuration returns a human-readable duration string
func (s *Stream) FormatDuration() string {
	d := s.Duration()
	hours := int(d.Hours())
	minutes := int(d.Minutes()) % 60

	if hours > 0 {
		return formatPlural(hours, "h") + " " + formatPlural(minutes, "m")
	}
	return formatPlural(minutes, "m")
}

// FormatViewerCount returns a formatted viewer count
func (s *Stream) FormatViewerCount() string {
	if s.ViewerCount >= 1000 {
		return formatFloat(float64(s.ViewerCount)/1000) + "k"
	}
	return formatInt(s.ViewerCount)
}

// TimeUntil returns the duration until a scheduled stream starts
func (s *ScheduledStream) TimeUntil() time.Duration {
	return time.Until(s.StartTime)
}

// FormatStartTime returns a human-readable start time
func (s *ScheduledStream) FormatStartTime() string {
	now := time.Now()
	startLocal := s.StartTime.Local()

	// Check if it's today
	if startLocal.YearDay() == now.YearDay() && startLocal.Year() == now.Year() {
		return "Today " + startLocal.Format("3:04 PM")
	}

	// Check if it's tomorrow
	tomorrow := now.AddDate(0, 0, 1)
	if startLocal.YearDay() == tomorrow.YearDay() && startLocal.Year() == tomorrow.Year() {
		return "Tomorrow " + startLocal.Format("3:04 PM")
	}

	// Otherwise show day and time
	return startLocal.Format("Mon 3:04 PM")
}

func formatPlural(n int, suffix string) string {
	return formatInt(n) + suffix
}

func formatInt(n int) string {
	return intToString(n)
}

func formatFloat(f float64) string {
	// Simple float formatting to 1 decimal place
	whole := int(f)
	frac := int((f - float64(whole)) * 10)
	if frac == 0 {
		return intToString(whole)
	}
	return intToString(whole) + "." + intToString(frac)
}

func intToString(n int) string {
	if n == 0 {
		return "0"
	}

	negative := n < 0
	if negative {
		n = -n
	}

	var digits []byte
	for n > 0 {
		digits = append([]byte{byte('0' + n%10)}, digits...)
		n /= 10
	}

	if negative {
		return "-" + string(digits)
	}
	return string(digits)
}

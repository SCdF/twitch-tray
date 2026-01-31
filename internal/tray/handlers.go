package tray

import (
	"fmt"
	"os/exec"
	"runtime"
)

// Handlers manages click actions for menu items
type Handlers struct {
	tray *Tray
}

// NewHandlers creates a new handlers instance
func NewHandlers(t *Tray) *Handlers {
	return &Handlers{tray: t}
}

// OpenStream opens a Twitch stream in the default browser
func (h *Handlers) OpenStream(userLogin string) {
	url := fmt.Sprintf("https://twitch.tv/%s", userLogin)
	openBrowser(url)
}

// OpenTwitch opens the main Twitch page
func (h *Handlers) OpenTwitch() {
	openBrowser("https://twitch.tv")
}

// OpenURL opens any URL in the default browser
func OpenURL(url string) error {
	return openBrowser(url)
}

// OpenCategory opens a category/game page
func (h *Handlers) OpenCategory(gameName string) {
	// URL encode the game name
	url := fmt.Sprintf("https://twitch.tv/directory/game/%s", urlEncode(gameName))
	openBrowser(url)
}

// openBrowser opens the specified URL in the default browser
func openBrowser(url string) error {
	var cmd *exec.Cmd

	switch runtime.GOOS {
	case "linux":
		cmd = exec.Command("xdg-open", url)
	case "darwin":
		cmd = exec.Command("open", url)
	case "windows":
		cmd = exec.Command("rundll32", "url.dll,FileProtocolHandler", url)
	default:
		return fmt.Errorf("unsupported platform: %s", runtime.GOOS)
	}

	return cmd.Start()
}

// urlEncode performs basic URL encoding for path segments
func urlEncode(s string) string {
	var result []byte
	for i := 0; i < len(s); i++ {
		c := s[i]
		if isURLSafe(c) {
			result = append(result, c)
		} else {
			result = append(result, '%')
			result = append(result, hexDigit(c>>4))
			result = append(result, hexDigit(c&0x0F))
		}
	}
	return string(result)
}

func isURLSafe(c byte) bool {
	return (c >= 'a' && c <= 'z') ||
		(c >= 'A' && c <= 'Z') ||
		(c >= '0' && c <= '9') ||
		c == '-' || c == '_' || c == '.' || c == '~'
}

func hexDigit(n byte) byte {
	if n < 10 {
		return '0' + n
	}
	return 'A' + n - 10
}

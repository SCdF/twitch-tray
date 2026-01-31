# Twitch Tray

A cross-platform system tray application for Twitch viewers.

## Project Structure

```
twitch-tray/
├── cmd/twitch-tray/main.go     # Entry point
├── internal/
│   ├── app/app.go              # Lifecycle orchestration
│   ├── auth/
│   │   ├── auth.go             # Token management (keyring storage)
│   │   └── deviceflow.go       # OAuth Device Code Flow
│   ├── twitch/
│   │   ├── client.go           # Helix API wrapper
│   │   ├── streams.go          # Streams endpoints
│   │   ├── schedule.go         # Schedule endpoint
│   │   └── types.go            # Data types
│   ├── eventsub/
│   │   ├── client.go           # WebSocket connection
│   │   ├── handlers.go         # Event handlers
│   │   └── subscriptions.go    # Subscription management
│   ├── state/state.go          # Central state + change detection
│   ├── tray/
│   │   ├── tray.go             # Tray setup
│   │   ├── menu.go             # Menu construction
│   │   └── handlers.go         # Click handlers (open browser)
│   ├── notify/notify.go        # Desktop notifications
│   └── config/config.go        # XDG config management
├── assets/icon.png             # Tray icon
├── go.mod
├── Makefile
└── CLAUDE.md
```

## Build Commands

```bash
# Install dependencies and build for current platform
make

# Build for specific platforms
make build-linux
make build-darwin
make build-windows
make build-all

# Run locally
make run

# Clean build artifacts
make clean
```

## Dependencies

- **System tray**: `fyne.io/systray` - requires CGO
- **Notifications**: `gen2brain/beeep`
- **Secure storage**: `99designs/keyring`
- **Config paths**: `adrg/xdg`
- **Twitch API**: `nicklaw5/helix/v2`
- **WebSocket**: `gorilla/websocket`

### Platform-specific build dependencies

- **Linux**: `gcc`, `libgtk-3-dev`, `libayatana-appindicator3-dev`
- **macOS**: Xcode command line tools (`xcode-select --install`)
- **Windows**: MinGW or TDM-GCC

## Configuration

Config file location (XDG): `~/.config/twitch-tray/config.json`

```json
{
  "client_id": "YOUR_TWITCH_CLIENT_ID",
  "poll_interval_sec": 60,
  "schedule_poll_min": 5,
  "top_streams_per_game": 5,
  "notify_on_live": true,
  "notify_on_category": true
}
```

### Getting a Client ID

1. Go to https://dev.twitch.tv/console/apps
2. Register a new application
3. Set OAuth Redirect URL to `https://localhost` (not used for device flow)
4. Copy the Client ID to your config file

## Authentication Flow

Uses OAuth Device Code Flow:
1. App requests device code from Twitch
2. User is shown a code and directed to twitch.tv/activate
3. App polls for token completion
4. Tokens stored securely in system keyring

Required scope: `user:read:follows`

## Key Features

- **Following Live**: Shows all live streams from followed channels
- **Top in Category**: Shows top 5 streams in each category your followed streamers are in
- **Scheduled**: Shows scheduled streams for the next 24 hours
- **Notifications**: Desktop notifications when streamers go live or change category
- **Real-time**: Uses EventSub WebSocket for instant live/offline events

## Data Flow

```
EventSub WebSocket → stream.online/offline → state.Update() → tray.Refresh()
                                                   │
                                                   └→ notify.StreamOnline()

Polling (60s) → GetStreamsByCategory → state.Update() → tray.Refresh()
Polling (5m)  → GetScheduledStreams  → state.Update() → tray.Refresh()
```

## Testing

1. Build and run: `make run`
2. Click "Login to Twitch" in the tray menu
3. Enter the code shown at twitch.tv/activate
4. Verify followed streams appear in the menu
5. Click a stream to open in browser

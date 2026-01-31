# Twitch Tray

A cross-platform system tray application for Twitch viewers.

## Project Structure

```
twitch-tray/
├── cmd/twitch-tray/main.go     # Entry point
├── internal/
│   ├── app/app.go              # Lifecycle orchestration
│   ├── auth/
│   │   ├── auth.go             # Token management (file-based encrypted storage)
│   │   ├── deviceflow.go       # OAuth Device Code Flow
│   │   └── deviceflow_test.go  # Polling tests
│   ├── twitch/
│   │   ├── client.go           # Helix API wrapper
│   │   ├── streams.go          # Streams endpoints
│   │   ├── schedule.go         # Schedule endpoint
│   │   └── types.go            # Data types
│   ├── eventsub/               # (Not currently used - kept for future)
│   │   ├── client.go           # WebSocket connection
│   │   ├── handlers.go         # Event handlers
│   │   └── subscriptions.go    # Subscription management
│   ├── state/state.go          # Central state + change detection
│   ├── tray/
│   │   ├── tray.go             # Tray setup
│   │   ├── menu.go             # Menu construction (mutex-protected)
│   │   └── handlers.go         # Click handlers (open browser)
│   ├── notify/notify.go        # Desktop notifications
│   └── config/config.go        # XDG config management
├── assets/
│   ├── original.png            # Source icon (256x256)
│   ├── icon.png                # Tray icon (64x64, generated)
│   ├── icon_grey.png           # Dimmed icon for unauthenticated state
│   ├── assets.go               # go:embed for icons
│   └── README.md               # Icon conversion instructions
├── go.mod
├── Makefile
└── CLAUDE.md
```

## Build Commands

```bash
make          # Install deps and build
make run      # Build and run
make clean    # Remove build artifacts
```

Cross-platform builds:
```bash
make build-linux
make build-darwin
make build-windows
make build-all
```

## Dependencies

- **System tray**: `fyne.io/systray` - requires CGO
- **Notifications**: `gen2brain/beeep`
- **Secure storage**: `99designs/keyring` (file backend)
- **Config paths**: `adrg/xdg`
- **Twitch API**: `nicklaw5/helix/v2`

### Platform-specific build dependencies

- **Linux**: `gcc`, `libgtk-3-dev`, `libayatana-appindicator3-dev`
- **macOS**: Xcode command line tools (`xcode-select --install`)
- **Windows**: MinGW or TDM-GCC

## Configuration

Config file: `~/.config/twitch-tray/config.json`

```json
{
  "poll_interval_sec": 60,
  "schedule_poll_min": 5,
  "notify_on_live": true,
  "notify_on_category": true
}
```

**Note**: Client ID is hardcoded in `internal/auth/auth.go`. No user configuration needed.

Token storage: `~/.config/twitch-tray/keyring/` (encrypted file, not system keyring)

## Authentication

Uses OAuth Device Code Flow:
1. Click "Login to Twitch" in tray menu
2. Browser opens to twitch.tv/activate
3. Enter the code shown
4. App polls until authorized (no notification spam)
5. Token stored in encrypted file

Required scope: `user:read:follows`

## Menu Structure

**Unauthenticated:**
```
[Grey Icon]
├── Login to Twitch
└── Quit
```

**Authenticated:**
```
[Icon]
├── Following Live (N)
│   ├── StreamerA - GameName (1.2k, 2h 15m)
│   └── StreamerB - GameName (856, 45m)
├── ─────────────
├── Scheduled (Next 24h)
│   ├── StreamerC - Tomorrow 3:00 PM
│   └── StreamerD - Today 8:00 PM
├── ─────────────
├── Logout
└── Quit
```

## Data Flow

```
Polling (60s) → GetFollowedStreams  → state.Update() → tray.Refresh()
Polling (5m)  → GetScheduledStreams → state.Update() → tray.Refresh()
```

Notifications only fire for streams that go live AFTER initial load (no startup spam).

**Note**: EventSub WebSocket was removed - subscribing to all followed channels exceeds Twitch's rate limits. Polling is sufficient for this use case.

## Key Implementation Details

### Thread Safety
- `tray/menu.go`: Mutex protects menu rebuilds (systray isn't thread-safe)
- `state/state.go`: RWMutex protects all state access

### API Endpoints Used
- `GetFollowedChannels` - channels user follows (for schedules)
- `GetFollowedStream` - live streams from followed channels
- `GetSchedule` - broadcaster schedules

### Icon Assets
Icons are embedded at compile time via `go:embed`. To update:
```bash
cd assets
magick original.png -resize 64x64 icon.png
magick original.png -resize 64x64 -channel A -evaluate Multiply 0.4 +channel icon_grey.png
```

## Testing

```bash
go test -v ./internal/auth/...   # Auth/polling tests
go build ./cmd/twitch-tray       # Build check
go vet ./...                     # Static analysis
```

## Known Issues / Future Work

- Schedule fetching may fail silently for channels without schedules (404s are ignored)
- EventSub could be re-enabled with selective subscriptions (only subscribe to N most active channels)
- Category change notifications removed (would need EventSub or more frequent polling)

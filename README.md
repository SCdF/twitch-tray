# Twitch Tray

A lightweight, cross-platform system tray application that shows your followed Twitch streams at a glance.

## Features

- **Live stream notifications** - Get notified when followed channels go live
- **Quick access menu** - See who's live directly from your system tray
- **Upcoming schedules** - View scheduled streams in the next 24 hours
- **One-click viewing** - Click any stream to open it in your browser
- **Cross-platform** - Works on Linux, macOS, and Windows

## Installation

### Prerequisites

**Linux:**
```bash
sudo apt install gcc libgtk-3-dev libayatana-appindicator3-dev
```

**macOS:**
```bash
xcode-select --install
```

**Windows:**
- Install [MinGW](https://www.mingw-w64.org/) or [TDM-GCC](https://jmeubank.github.io/tdm-gcc/)

### Build

```bash
git clone https://github.com/yourusername/twitch-tray
cd twitch-tray
make
```

### Run

```bash
make run
# or
./bin/twitch-tray
```

## Usage

1. Click the tray icon to see the menu
2. Select **Login to Twitch** and follow the prompts
3. Once authenticated, you'll see:
   - Live streams from channels you follow (sorted by viewers)
   - Scheduled streams in the next 24 hours
4. Click any stream to open it in your browser

## Development

### Project Structure

```
twitch-tray/
├── cmd/twitch-tray/       # Application entry point
├── internal/
│   ├── app/               # Lifecycle orchestration
│   ├── auth/              # OAuth device flow authentication
│   ├── twitch/            # Helix API client
│   ├── state/             # Centralized state management
│   ├── tray/              # System tray UI
│   ├── notify/            # Desktop notifications
│   └── config/            # Configuration management
└── assets/                # Embedded icons
```

### Architecture

The app uses a polling-based architecture:
- **Streams**: Polled every 60 seconds via Twitch Helix API
- **Schedules**: Polled every 5 minutes
- **State**: Centralized with change detection to trigger UI updates
- **Auth**: OAuth Device Code Flow with encrypted token storage

### Testing

```bash
go test -v ./...           # Run all tests
go vet ./...               # Static analysis
go build ./cmd/twitch-tray # Build check
```

### Cross-Platform Builds

```bash
make build-linux
make build-darwin
make build-windows
make build-all
```

## Configuration

Config file: `~/.config/twitch-tray/config.json`

```json
{
  "poll_interval_sec": 60,
  "schedule_poll_min": 5,
  "notify_on_live": true
}
```

## Built With

This project was built entirely by [Claude](https://claude.ai), Anthropic's AI assistant, using [Claude Code](https://claude.ai/claude-code).

## License

MIT

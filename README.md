# Twitch Tray

A cross-platform system tray application for Twitch viewers. Get notified when your favorite streamers go live and quickly access their streams from your system tray.

## Features

- **Live Stream Notifications**: Get desktop notifications when followed streamers go live
- **Quick Access Menu**: Click the tray icon to see who's live and jump directly to their stream
- **Scheduled Streams**: View upcoming scheduled broadcasts in the next 24 hours
- **Cross-Platform**: Works on Linux, macOS, and Windows

## Installation

### Pre-built Binaries

Download the latest release for your platform from the [Releases](https://github.com/user/twitch-tray/releases) page.

### Building from Source

#### Prerequisites

- [Rust](https://rustup.rs/) (1.70+)
- Platform-specific dependencies:

**Linux (Debian/Ubuntu):**
```bash
sudo apt-get install -y libgtk-3-dev libwebkit2gtk-4.1-dev libayatana-appindicator3-dev librsvg2-dev
```

**macOS:**
```bash
xcode-select --install
```

**Windows:**
- Install Visual Studio Build Tools with C++ workload

#### Build

```bash
# Development build
make build

# Release build
make release

# Run
make run
```

## Usage

1. **Login**: Click the tray icon and select "Login to Twitch"
2. **Authorize**: A browser window opens - enter the code shown to authorize the app
3. **View Streams**: Click the tray icon to see live streams from channels you follow
4. **Open Stream**: Click on any stream to open it in your browser

## Configuration

Config file location: `~/.config/twitch-tray/config.json`

```json
{
  "poll_interval_sec": 60,
  "schedule_poll_min": 5,
  "notify_on_live": true,
  "notify_on_category": true
}
```

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
├── Following Live (N)         <- header (disabled)
├── StreamerA - GameName (1.2k, 2h 15m)
├── StreamerB - GameName (856, 45m)
├── ... (top 10 shown)
├── More (N)...                <- submenu for overflow
├── ─────────────
├── Scheduled (Next 24h)       <- header (disabled)
├── StreamerD - Tomorrow 3:00 PM
├── StreamerE - Today 8:00 PM
├── ... (top 5 shown)
├── ─────────────
├── Logout
└── Quit
```

## Development

```bash
# Install dependencies
make deps

# Run with hot reload
make dev

# Run lints
make lint

# Run tests
make test

# Format code
make fmt
```

## Built With

This project was built entirely by [Claude](https://claude.ai), Anthropic's AI assistant, using [Claude Code](https://claude.ai/claude-code).

## License

MIT

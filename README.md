# Twitch Tray

A cross-platform system tray application for Twitch viewers. Get notified when your favorite streamers go live and quickly access their streams from your system tray.

## Features

- **Live Stream Notifications**: Get desktop notifications when followed streamers go live
- **Quick Access Menu**: Click the tray icon to see who's live and jump directly to their stream
- **Scheduled Streams**: View upcoming scheduled broadcasts in the next 24 hours
- **Cross-Platform**: Works on Linux, macOS, and Windows
- **KDE Plasmoid**: Native KDE Plasma panel widget (Linux/KDE only)

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

**Linux (KDE target, additional):**

```bash
# Arch
sudo pacman -S qt6-declarative libplasma kirigami

# Debian/Ubuntu
sudo apt-get install -y qt6-declarative-dev plasma-framework-dev kirigami2-dev
```

> `qt6-declarative` / `qt6-declarative-dev` provides `qmltestrunner`, which is required for `make test-plasmoid`.

**macOS:**

```bash
xcode-select --install
```

**Windows:**

- Install Visual Studio Build Tools with C++ workload

#### Build

```bash
# Development build (system tray)
make build

# Development build (KDE daemon)
make build-kde

# Release build
make release

# Run (system tray)
make run

# Run (KDE daemon)
make run-kde
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
  "notify_on_live": true,
  "notify_on_category": true,
  "notify_max_gap_min": 10,
  "schedule_stale_hours": 24,
  "schedule_check_interval_sec": 10,
  "followed_refresh_min": 15
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

### KDE Plasmoid

The KDE target provides a native Plasma panel widget. It runs a background daemon (`twitch-kde`) that exposes state over D-Bus, and a QML plasmoid that displays it.

```bash
# Build and install the plasmoid package
make dist-kde

# Install plasmoid to KDE (development — uses source dir directly)
make install-plasmoid

# Install daemon binary
sudo cp target/release/twitch-kde /usr/bin/twitch-kde
```

For D-Bus activation (auto-start when plasmoid connects), install the service file:

```bash
sudo cp crates/twitch-kde/info.sdufresne.TwitchTray1.service /usr/share/dbus-1/services/
```

## Development

```bash
# Install dependencies
make deps

# Run with hot reload
make dev

# Run lints
make lint

# Run all tests (Rust + QML)
make test-all

# Run Rust tests only
make test

# Run QML plasmoid tests only
make test-plasmoid

# Format code
make fmt
```

## Built With

This project was built entirely by [Claude](https://claude.ai), Anthropic's AI assistant, using [Claude Code](https://claude.ai/claude-code).

## License

MIT

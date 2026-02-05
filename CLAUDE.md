# Twitch Tray

A cross-platform system tray application for Twitch viewers built with Rust and Tauri 2.0.

## Project Structure

```
twitch-tray/
├── src-tauri/
│   ├── Cargo.toml
│   ├── tauri.conf.json
│   ├── build.rs
│   ├── icons/
│   │   ├── icon.png           # Tray icon (64x64 RGBA)
│   │   └── icon_grey.png      # Dimmed icon for unauthenticated state
│   └── src/
│       ├── main.rs            # Entry point, Tauri setup
│       ├── app.rs             # Lifecycle, polling, session management
│       ├── auth/
│       │   ├── mod.rs
│       │   ├── store.rs       # Keyring token storage
│       │   └── deviceflow.rs  # OAuth Device Code Flow
│       ├── twitch/
│       │   ├── mod.rs
│       │   ├── client.rs      # reqwest-based Helix client
│       │   └── types.rs       # Stream, ScheduledStream structs
│       ├── state.rs           # Central state, change detection
│       ├── config.rs          # XDG config management
│       ├── tray.rs            # Tray icon + menu construction
│       └── notify.rs          # Desktop notifications
├── src/                       # Frontend (empty - tray-only app)
├── .github/workflows/
│   ├── ci.yml                 # Clippy, tests on every push
│   └── release.yml            # Build binaries on git tag push
├── Makefile
└── README.md
```

## Build Commands

```bash
make build     # Development build
make release   # Release build
make run       # Build and run
make dev       # Development with hot reload
make clean     # Remove build artifacts
make lint      # Run clippy and fmt check
make test      # Run tests
make fmt       # Format code
make dist      # Build for distribution (Tauri bundler)
```

## Dependencies

Key crates:
- **tauri**: System tray, menu, platform integration
- **tokio**: Async runtime for polling and HTTP
- **reqwest**: HTTP client for Twitch API
- **keyring**: Secure token storage
- **notify-rust**: Desktop notifications (Linux)
- **chrono**: Date/time handling

### Platform-specific build dependencies

- **Linux**: `libgtk-3-dev libwebkit2gtk-4.1-dev libayatana-appindicator3-dev librsvg2-dev`
- **macOS**: Xcode command line tools (`xcode-select --install`)
- **Windows**: Visual Studio Build Tools with C++ workload

## Configuration

Config file: `~/.config/twitch-tray/config.json`

```json
{
  "poll_interval_sec": 60,
  "schedule_poll_min": 5,
  "notify_on_live": true,
  "notify_on_category": true,
  "notify_max_gap_min": 10
}
```

**Settings:**
- `poll_interval_sec`: How often to check for live streams (default: 60 seconds)
- `schedule_poll_min`: How often to check for scheduled streams (default: 5 minutes)
- `notify_on_live`: Send desktop notifications when streams go live (default: true)
- `notify_on_category`: Send notifications on category changes (default: true)
- `notify_max_gap_min`: Maximum gap between refreshes to still send notifications (default: 10 minutes). If the app was asleep/suspended longer than this, notifications are suppressed to avoid a flood of alerts on wake.

**Note**: Client ID is hardcoded in `src/auth/mod.rs`. No user configuration needed.

Token storage: System keyring with file fallback at `~/.config/twitch-tray/token.json`

## Authentication

Uses OAuth Device Code Flow:
1. Click "Login to Twitch" in tray menu
2. Browser opens to twitch.tv/activate
3. Enter the code shown
4. App polls until authorized
5. Token stored securely

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
├── Following Live (N)         <- header (disabled)
├── StreamerA - GameName (1.2k, 2h 15m)
├── StreamerB - GameName (856, 45m)
├── ... (top 10 shown)
├── More (N)...                <- submenu for overflow
│   └── StreamerC - GameName (...)
├── ─────────────
├── Scheduled (Next 24h)       <- header (disabled)
├── StreamerD - Tomorrow 3:00 PM
├── StreamerE - Today 8:00 PM
├── ... (top 5 shown)
├── More (N)...                <- submenu for overflow
├── ─────────────
├── Logout
└── Quit
```

## Data Flow

```
Polling (60s) → GetFollowedStreams  → state.set_followed_streams() → tray.rebuild_menu()
Polling (5m)  → GetScheduledStreams → state.set_scheduled_streams() → tray.rebuild_menu()
```

Notifications only fire for streams that go live AFTER initial load (no startup spam).

## Key Implementation Details

### Thread Safety
- `state.rs`: `tokio::sync::RwLock` protects all state access
- State changes trigger menu rebuilds via watch channel

### API Endpoints Used
- `GET /channels/followed` - channels user follows (for schedules)
- `GET /streams/followed` - live streams from followed channels
- `GET /schedule` - broadcaster schedules

### Icon Assets
Icons are loaded at runtime from embedded PNG bytes. Must be 64x64 RGBA format.

To regenerate icons:
```bash
cd src-tauri/icons
convert original.png -resize 64x64 -define png:color-type=6 icon.png
convert original.png -resize 64x64 -channel A -evaluate Multiply 0.4 +channel -define png:color-type=6 icon_grey.png
```

## Testing

```bash
make lint    # Run clippy and fmt check
make test    # Run tests
make build   # Build check
```

## Definition of Done

Before considering any code change complete:

1. **Formatting**: Run `cargo fmt` - code must be formatted
2. **Linting**: Run `make lint` - no clippy warnings (warnings are errors in CI)
3. **Tests**: Run `make test` - all tests must pass
4. **No dead code**: Remove unused code rather than using `#[allow(dead_code)]`

## Versioning & Releases

Version is set in `src-tauri/Cargo.toml` and `src-tauri/tauri.conf.json`.

**To release a new version:**
1. Update version in both files
2. Commit and tag:
```bash
git tag v1.0.0
git push origin v1.0.0
```

This triggers the release workflow which:
1. Builds binaries for Linux, macOS (amd64/arm64), and Windows
2. Creates a GitHub release with binaries and checksums

## Architecture

- **Tauri 2.0**: Provides system tray, menu API, and cross-platform support
- **Tokio**: Multi-threaded async runtime for concurrent polling
- **State Management**: `Arc<AppState>` with `RwLock` and watch channels for change notification
- **No Frontend**: This is a tray-only app - the `src/` directory contains only a placeholder HTML

## Debugging Core Dumps

This system uses `systemd-coredump`. When the app crashes with "Segmentation fault (core dumped)":

**List recent crashes:**
```bash
coredumpctl list twitch-tray
```

**Get crash info and stack trace:**
```bash
coredumpctl info twitch-tray
```

**Full backtrace with gdb (install with `sudo pacman -S gdb` if needed):**
```bash
coredumpctl debug twitch-tray
# In gdb: bt full
# Or non-interactive: coredumpctl debug twitch-tray --debugger-arguments="-batch -ex 'bt full'"
```

**For a specific crash by PID:**
```bash
coredumpctl info <PID>
```

**Enable Rust backtraces on panic:**
```bash
RUST_BACKTRACE=1 make run
```

## Known Issues / Future Work

- Schedule fetching may fail silently for channels without schedules (404s are ignored)
- EventSub could be added for real-time notifications (selective subscriptions to avoid rate limits)

### Fixed: Menu rebuild crashes (Linux)

`libayatana-appindicator3` was crashing (SIGSEGV/SIGABRT) when multiple async tasks called `tray.set_menu()` concurrently. Fixed by:
1. Adding `Arc<Mutex<()>>` to `TrayManager` to serialize menu rebuilds
2. Using `app.run_on_main_thread()` to dispatch GTK operations to the main thread
3. Making `TrayManager` clonable so all instances share the same mutex

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
│   │   ├── icon.png                    # Tray icon (64x64 RGBA)
│   │   └── icon_grey.png               # Dimmed icon for unauthenticated state
│   ├── src/
│   │   ├── main.rs                     # Entry point, Tauri setup
│   │   ├── lib.rs                      # Library root, module declarations
│   │   ├── app.rs                      # Thin wiring layer; implements AppServices
│   │   ├── commands.rs                 # Tauri command handlers (thin adapters)
│   │   ├── state.rs                    # AppState: thread-safe view of live data
│   │   ├── config.rs                   # ConfigManager, Config, named defaults
│   │   ├── db.rs                       # Database: SQLite persistence (no domain logic)
│   │   ├── notify.rs                   # DesktopNotifier: implements Notifier trait
│   │   ├── test_helpers.rs             # Shared test helper types (cfg(test))
│   │   │
│   │   ├── # Domain logic (no Tauri/GTK imports)
│   │   ├── display_state.rs            # DisplayState, compute_display_state()
│   │   ├── notification_filter.rs      # Pure notification suppression policy
│   │   ├── schedule_inference.rs       # Pure schedule inference algorithm
│   │   │
│   │   ├── # Output ports (traits)
│   │   ├── display.rs                  # DisplayBackend trait + RecordingDisplayBackend
│   │   ├── app_services.rs             # AppServices trait + MockAppServices
│   │   │
│   │   ├── # Orchestration
│   │   ├── session.rs                  # SessionManager: auth lifecycle
│   │   ├── schedule_walker.rs          # ScheduleWalker: schedule queue
│   │   ├── notification_dispatcher.rs  # NotificationDispatcher: event → notify
│   │   │
│   │   ├── auth/
│   │   │   ├── mod.rs                  # CLIENT_ID constant, module declarations
│   │   │   ├── store.rs                # Keyring token storage
│   │   │   └── deviceflow.rs          # OAuth Device Code Flow
│   │   ├── tray/
│   │   │   └── mod.rs                  # TrayBackend: implements DisplayBackend (AppHandle lives here only)
│   │   └── twitch/
│   │       ├── mod.rs                  # with_retry helper, re-exports
│   │       ├── http.rs                 # HttpClient trait, ReqwestClient, MockHttpClient
│   │       ├── client.rs               # TwitchClient: reqwest-based Helix API client
│   │       └── types.rs                # Stream, ScheduledStream, FollowedChannel, etc.
│   └── tests/
│       └── common/
│           └── mod.rs                  # Integration test helpers (make_stream, etc.)
├── src/                                # Frontend (empty - tray-only app)
├── .github/workflows/
│   ├── ci.yml                          # Clippy, tests on every push
│   └── release.yml                     # Build binaries on git tag push
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
  "notify_on_live": true,
  "notify_on_category": true,
  "notify_max_gap_min": 10,
  "schedule_stale_hours": 24,
  "schedule_check_interval_sec": 10,
  "followed_refresh_min": 15
}
```

**Settings:**
- `poll_interval_sec`: How often to check for live streams (default: 60 seconds)
- `notify_on_live`: Send desktop notifications when streams go live (default: true)
- `notify_on_category`: Send notifications on category changes (default: true)
- `notify_max_gap_min`: Maximum gap between refreshes to still send notifications (default: 10 minutes). If the app was asleep/suspended longer than this, notifications are suppressed to avoid a flood of alerts on wake.
- `schedule_stale_hours`: How many hours before a channel's schedule is re-fetched (default: 24)
- `schedule_check_interval_sec`: How often the schedule queue walker checks the next channel (default: 10 seconds)
- `followed_refresh_min`: How often to refresh the followed channels list from the API (default: 15 minutes)

**Note**: Client ID is hardcoded in `src-tauri/src/auth/mod.rs`. No user configuration needed.

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
Polling (60s)           → GetFollowedStreams      → state.set_followed_streams()
                                                       ├─ watch channel → compute_display_state()
                                                       │                → DisplayBackend.update()
                                                       └─ broadcast StreamsUpdated
                                                            → NotificationDispatcher.listen()
                                                            → NotificationFilter (suppression policy)
                                                            → Notifier.stream_live() / .category_change()

Queue walker (10s)      → GetSchedule(1 ch)       → db.replace_future_schedules()
                                                   → state.set_scheduled_streams()
                                                       └─ watch channel → DisplayBackend.update()

Followed refresh (15m)  → GetAllFollowedChannels   → db.sync_followed()
                                                   → state.set_followed_channels()
```

### Hexagonal layer boundaries

```
┌──────────────────────────────────────────────┐
│  Domain logic (pure Rust, no framework deps) │
│  display_state.rs, notification_filter.rs,   │
│  schedule_inference.rs, state.rs, config.rs  │
└──────────────┬───────────────────────────────┘
               │ uses traits (ports)
       ┌───────┴────────┐
       ▼                ▼
DisplayBackend       Notifier          ← output ports
       ▲                ▲
       │                │ implements
TrayBackend      DesktopNotifier       ← adapters (tray/, notify.rs)

HttpClient  ←── ReqwestClient / MockHttpClient
AppServices ←── App (wiring) / MockAppServices
```

Schedule fetching uses a queue-based approach: instead of bulk-fetching all channels at once,
the walker picks the most-stale broadcaster every 10 seconds and checks one at a time. This
ensures ALL followed channels eventually get checked, not just the first 50. Results are stored
in SQLite (`data.db`) and read back for display.

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

- **Tauri 2.0**: Provides system tray, menu API, and cross-platform support. Confined to `TrayBackend` and `main.rs`.
- **Tokio**: Multi-threaded async runtime for concurrent polling tasks.
- **State Management**: `Arc<AppState>` with `RwLock`; a watch channel drives reactive menu rebuilds, a broadcast channel carries `StreamsUpdated` events to the notification path.
- **`App`**: Thin wiring layer. Constructs `SessionManager`, `ScheduleWalker`, `NotificationDispatcher`, wires them to shared state, and starts polling tasks via `start_polling()`.
- **No Frontend**: This is a tray-only app — the `src/` directory contains only a placeholder HTML file.

## Architectural Principles

### Hexagonal Architecture (Ports and Adapters)

Business logic must have **no dependency** on Tauri, GTK, D-Bus, or any display framework.

**Output ports (traits):**
- `DisplayBackend` — implemented by `TrayBackend` (Tauri system tray)
- `Notifier` — implemented by `DesktopNotifier`

**Input/infrastructure ports:**
- `HttpClient` — production: `ReqwestClient`; tests: `MockHttpClient`
- `AppServices` — consumed by Tauri command handlers

**Rule:** `AppHandle` must not appear outside of `tray/mod.rs` (the `TrayBackend`) and `main.rs`. If you need UI behaviour in domain code, add a method to `DisplayBackend` instead.

### Single Responsibility Principle

Each type has one reason to change:
- `SessionManager` — auth lifecycle only
- `ScheduleWalker` — schedule queue only
- `NotificationDispatcher` — notification dispatch only
- `NotificationFilter` — notification suppression policy only
- `Database` — persistence only (no domain logic)

### Dependency Direction

```
Domain code → Traits (ports) ← Adapters
```

Domain code never imports adapter types directly.

## Test-Driven Development

This codebase follows **Red → Green → Refactor** TDD. Before writing production code:

1. **Red**: Write a failing test that describes the desired behaviour.
2. **Green**: Write the minimum production code to make the test pass.
3. **Refactor**: Clean up without changing behaviour. Tests stay green.

### Testing layers

**Unit tests** (`#[cfg(test)]` inline modules) — test a single function or type in isolation.
Use mocks for all external dependencies:
- `MockHttpClient` — `twitch/http.rs`
- `RecordingNotifier` — `notify.rs` (mod mock)
- `RecordingDisplayBackend` — `display.rs` (mod mock)
- `MockAppServices` — `app_services.rs` (mod mock)
- Test helpers (`make_stream` etc.) — `tests/common/mod.rs`

**Integration tests** (`tests/`) — test collaboration between two or more real components.

### Do not test through the wiring layer

`App` is a wiring layer. Tests that construct `App` to test notification logic or display
state are testing the wrong layer. Test `NotificationFilter`, `compute_display_state`,
`ScheduleWalker::tick` directly.

### Test naming

Test names describe behaviour, not implementation:
- ✅ `notifications_suppressed_after_sleep_gap`
- ✅ `favourite_streamers_sorted_before_others`
- ❌ `test_filter_notifications_function`
- ❌ `test_build_menu`

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
1. Adding `Arc<Mutex<()>>` to `TrayBackend` to serialize menu rebuilds
2. Using `app.run_on_main_thread()` to dispatch GTK operations to the main thread
3. Making `TrayBackend` clonable so all instances share the same mutex

# KDE Plasmoid Target — Implementation Plan

## Overview

Add a KDE-native display target alongside the existing cross-platform Tauri target.
The plasmoid replaces the system tray menu on KDE. The Tauri settings window is
reused unchanged. The Tauri cross-platform binary (`twitch-app-tauri`) is untouched.

### What changes

| Component | Change |
|---|---|
| `twitch-app-tauri` | Unchanged — remains the cross-platform target |
| `twitch-settings-tauri` | Add `window.rs` (move window-opening fns from `twitch-menu-tauri`) |
| `twitch-menu-tauri` | Update imports to use `twitch-settings-tauri::window` |
| `twitch-backend` | Small addition: relay `user_code` during login flow |
| `twitch-kde` | New crate — daemon binary + QML plasmoid package |
| `Makefile` | New targets: `build-kde`, `run-kde`, `test-plasmoid`, `test-all` |
| `README.md` | KDE target build deps, `qmltestrunner` requirement |

### What the KDE target does NOT do (deferred)

- No settings UI in the plasmoid (settings window is opened via the existing Tauri WebviewWindow)
- No "Quit" menu item (daemon lifecycle is tied to the plasmoid)
- No `OpenSettingsRequested` handling is deferred — this is NOT deferred,
  see Phase 3: the D-Bus service routes it to `window_tx` same as the Tauri target

---

## Architecture

### Runtime model

`twitch-kde` is a **Tauri binary** (like `twitch-app-tauri`) but with no system tray
and no tray menu. It runs:

1. `twitch_backend::start()` on tokio
2. A `zbus` D-Bus service as a tokio task (spawned in Tauri `setup`)
3. Tauri's WebKit runtime idling — only used to spawn the settings WebviewWindow on demand

The plasmoid (QML) connects to the D-Bus service. When the user clicks "Settings" or
a notification fires `OpenSettingsRequested`, the D-Bus service sends a `WindowRequest`
on a channel; `main.rs` receives it and calls `open_settings_window(app_handle, ...)`.

### Daemon lifecycle

**Command-line:** `twitch-kde` runs until killed. Ctrl+C / SIGTERM shuts everything down.

**Packaged install:** D-Bus activation. A `.service` file deployed alongside the plasmoid
tells D-Bus which binary to start on first use. The daemon starts automatically when the
plasmoid first connects, and is never started if the plasmoid is not installed.

```
# /usr/share/dbus-1/services/org.twitch.TwitchTray1.service
[D-BUS Service]
Name=org.twitch.TwitchTray1
Exec=/usr/bin/twitch-kde
```

### D-Bus interface

```
Interface: org.twitch.TwitchTray1
Path:      /org/twitch/TwitchTray

Properties:
  State: string   (JSON-serialised PlasmoidState — see DTO shapes below)

Methods:
  Login()
  Logout()
  OpenStream(user_login: string)
  OpenSettings()
  OpenStreamerSettings(user_login: string, display_name: string)
  CancelLogin()

Signals:
  StateChanged(state: string)   (emitted whenever State changes)
```

### Window request channel (Option B — AppHandle stays in main.rs)

```
DbusService --[WindowRequest]--> mpsc channel --> main.rs --> open_*_window(app_handle)
```

`DbusService` never holds an `AppHandle`. It holds a `mpsc::Sender<WindowRequest>`.
`main.rs` owns the `AppHandle` and the receiver.

```rust
enum WindowRequest {
    OpenSettings,
    OpenStreamerSettings { user_login: String, display_name: String },
}
```

---

## DTO Shapes — `twitch-kde/src/dto.rs`

All structs derive `Serialize, Deserialize, PartialEq, Debug, Clone`.
`PartialEq` is required for contract tests.

```rust
pub struct PlasmoidState {
    pub authenticated: bool,
    pub login_state: LoginStateDto,
    pub live: LiveSectionDto,
    pub categories: Vec<CategorySectionDto>,
    pub schedule: ScheduleSectionDto,
}

pub enum LoginStateDto {
    Idle,
    PendingCode { user_code: String, verification_uri: String },
    AwaitingConfirmation,
}

pub struct LiveSectionDto {
    pub visible: Vec<LiveStreamDto>,    // up to config.live_limit
    pub overflow: Vec<LiveStreamDto>,   // remainder; QML renders as ExpandableSection
}

pub struct LiveStreamDto {
    pub user_login: String,
    pub user_name: String,
    pub game_name: String,              // truncated to 20 chars
    pub viewer_count_formatted: String, // "45k", "856"
    pub duration_formatted: String,     // "2h 15m"
    pub is_favourite: bool,
}

pub struct CategorySectionDto {
    pub name: String,
    pub total_viewers_formatted: String,
    pub streams: Vec<CategoryStreamDto>, // all streams, no overflow (section is collapsible)
}

pub struct CategoryStreamDto {
    pub user_login: String,
    pub user_name: String,
    pub viewer_count_formatted: String,
}

pub struct ScheduleSectionDto {
    pub lookahead_hours: u64,               // QML builds "Scheduled (Next {N}h)"
    pub loaded: bool,                       // false → show BusyIndicator
    pub visible: Vec<ScheduledStreamDto>,
    pub overflow: Vec<ScheduledStreamDto>,
}

pub struct ScheduledStreamDto {
    pub broadcaster_login: String,
    pub broadcaster_name: String,
    pub start_time_formatted: String,   // "Today 8:00 PM", "Tomorrow 3:00 PM", "Mon 3:00 PM"
    pub is_inferred: bool,              // QML renders ✨ if true
    pub is_favourite: bool,             // QML renders ★ if true
}
```

---

## New crate layout — `crates/twitch-kde/`

```
crates/twitch-kde/
  Cargo.toml                  # deps: tauri, twitch-backend, twitch-settings-tauri,
                              #       zbus, serde, serde_json, tokio, anyhow, tracing
  src/
    main.rs                   # wiring: backend + D-Bus task + window request listener
    dto.rs                    # PlasmoidState and all DTO types
    plasmoid_state.rs         # compute_plasmoid_state(raw, now) -> PlasmoidState (pure fn)
    dbus_service.rs           # zbus interface impl (holds channels, no AppHandle)
  plasmoid/
    metadata.json
    contents/
      ui/
        main.qml                    # full popup representation
        CompactRepresentation.qml   # panel icon + live count badge
        SectionHeader.qml           # bold label + Kirigami.Separator
        ExpandableSection.qml       # collapsible section (chevron toggle)
        StreamItem.qml              # live stream row (name, game, viewers, duration, star)
        ScheduleItem.qml            # scheduled stream row (name, time, ✨, ★)
        LoginView.qml               # device code display + copy button + cancel
      tests/
        MockDbusService.qml         # same property shape as real D-Bus service
        tst_ExpandableSection.qml
        tst_StreamItem.qml
        tst_ScheduleItem.qml
        tst_LoginView.qml
        tst_CompactRepresentation.qml
        tst_SectionHeader.qml
```

---

## Plasmoid UI design

### Section headers

`Kirigami.Heading` (level 5, bold) + `Kirigami.Separator` with `Layout.fillWidth`.
Matches the KDE Audio Volume plasmoid "Output Devices" / "Input Devices" style.

### Collapsible sections

`ExpandableSection.qml`: chevron icon (`▶`/`▼`) + header label.
Clicking toggles `expanded` property which shows/hides a child `Column`.
Collapsed by default.

Replaces both:
- "More (N)..." overflow submenus in the live and schedule sections
- Category submenus (each category becomes its own collapsible section)

### Inferred schedule indicator

Keep the ✨ emoji. No KDE icon communicates "AI-inferred" as clearly.
QML renders emoji inline with `Text`.

### Login view (device code)

Shown in the popup when `login_state == PendingCode`:

```
Visit: twitch.tv/activate
Enter code:  [ABCD-1234]  [Copy]

[Browser opened · Waiting...]
[Cancel]
```

`[Copy]` button calls `Qt.copyToClipboard(user_code)`.
`[Cancel]` calls the `CancelLogin()` D-Bus method.

### Compact representation (panel icon)

Twitch icon. When `live.visible.length + live.overflow.length > 0`, overlay a small
badge with the count using Plasma's badge API.

### Bottom actions

Authenticated: `[Logout]` button only (no Quit — daemon lifecycle tied to plasmoid).
Unauthenticated: `[Login to Twitch]` button only.

---

## Implementation phases

Work through these in order. Each phase is independently committable.
Follow Red -> Green -> Refactor within each phase.

---

### Phase 1: Move window-opening functions

**Goal:** `open_settings_window` and `open_streamer_settings_window` live in
`twitch-settings-tauri::window` so both `twitch-app-tauri` and `twitch-kde` can use them
without depending on `twitch-menu-tauri`.

**Files touched:**
- `crates/twitch-settings-tauri/src/window.rs` — NEW
- `crates/twitch-settings-tauri/src/lib.rs` — re-export `window` module
- `crates/twitch-menu-tauri/src/tray/mod.rs` — remove the two functions, update imports
- `crates/twitch-app-tauri/Cargo.toml` — already depends on `twitch-settings-tauri`, no change needed
- `crates/twitch-app-tauri/src/main.rs` — update import path

**Steps:**
1. Create `window.rs` in `twitch-settings-tauri` with the two functions copied from
   `tray/mod.rs` (same code, same `SETTINGS_WINDOW_SIZE` const).
2. Add `pub mod window;` to `twitch-settings-tauri/src/lib.rs`.
3. In `twitch-app-tauri/src/main.rs`, change:
   `use twitch_menu_tauri::tray::{..., open_streamer_settings_window}`
   to `use twitch_settings_tauri::window::{..., open_streamer_settings_window}`
4. Remove the two functions from `twitch-menu-tauri/src/tray/mod.rs`.
5. `make lint && make test` — must pass.

Note: these functions call Tauri APIs and cannot be unit tested. No new tests needed
for this phase — the existing integration tests cover the behaviour.

---

### Phase 2: Relay login user_code from backend

**Goal:** During the device code flow, the `user_code` is currently discarded.
The backend needs to surface it so `twitch-kde` can show it in the plasmoid.

**Files touched:**
- `crates/twitch-backend/src/handle.rs` — add `login_progress_tx` / `login_progress_rx`
- `crates/twitch-backend/src/backend.rs` — wire the new channel
- `crates/twitch-backend/src/session.rs` — `handle_login` callback sends progress
- `crates/twitch-backend/src/lib.rs` — re-export `LoginProgress`

**New type in `handle.rs`:**
```rust
pub enum LoginProgress {
    PendingCode { user_code: String, verification_uri: String },
    Confirmed,
    Failed(String),
}
```

`BackendHandle` gains:
```rust
pub login_progress_rx: watch::Receiver<Option<LoginProgress>>,
```

`handle_login` sends `LoginProgress::PendingCode` via its callback instead of
silently discarding the code. On completion sends `LoginProgress::Confirmed`.

**Tests (in `session.rs` or a new `login_progress_tests` module):**
- `login_progress_sends_pending_code_with_user_code` — mock device flow returns
  a code, verify `login_progress_rx` receives `PendingCode { user_code: "ABC" }`
- `login_progress_sends_confirmed_on_success`
- `login_progress_sends_failed_on_error`

`twitch-app-tauri` ignores `login_progress_rx` (no change needed there).

---

### Phase 3: `twitch-kde` crate skeleton + DTO types

**Goal:** Create the new crate with `dto.rs`, stub `plasmoid_state.rs`, stub
`dbus_service.rs`, and a `main.rs` that compiles.

**Steps:**
1. Create `crates/twitch-kde/Cargo.toml` with correct deps.
2. Create `crates/twitch-kde/src/dto.rs` with all DTO structs (as above).
   All fields public. All structs derive `Serialize, Deserialize, PartialEq, Debug, Clone`.
3. Add `twitch-kde` to workspace `Cargo.toml`.
4. `cargo build -p twitch-kde` — must compile.

**Tests (in `dto.rs`):**
- `unauthenticated_state_round_trips_through_json` — serialize then deserialize
  `PlasmoidState { authenticated: false, login_state: Idle, ... }`, assert equal.
- `live_stream_dto_round_trips_through_json`
- `scheduled_stream_dto_round_trips_through_json`
- `login_state_pending_code_round_trips_through_json`

These pin the JSON shape and will catch any accidental field renames.

---

### Phase 4: `plasmoid_state.rs` — pure mapping function

**Goal:** `compute_plasmoid_state(raw: RawDisplayData, now: DateTime<Utc>) -> PlasmoidState`
Implements the same filtering/sorting/splitting logic as `compute_display_state` in
`twitch-menu-tauri`, but produces `PlasmoidState` DTOs.

**Shared logic to reimplement:**
- Filter `Ignore` streamers from live and schedule
- Sort live: favourites first, then by viewer_count descending
- Split live into visible (up to `config.live_limit`) and overflow
- Suppress scheduled streams covered by a live broadcast within 60 min
- Split schedule into visible (up to `config.schedule_limit`) and overflow
- Category sections: sort by viewer_count, no overflow (collapsible)

**Tests** — mirror the `compute_display_state` tests in `twitch-menu-tauri`,
adapted for the new output type. Names should describe behaviour:
- `ignore_streamers_filtered_from_live`
- `ignore_streamers_filtered_from_schedule`
- `live_streams_sorted_favourites_first`
- `live_streams_sorted_by_viewers_within_group`
- `live_overflow_split_at_limit`
- `schedule_hidden_when_broadcaster_live_within_window`
- `schedule_shown_when_broadcaster_live_but_far_in_future`
- `schedule_overflow_split_at_limit`
- `favourite_stream_has_is_favourite_true`
- `inferred_schedule_has_is_inferred_true`
- `viewer_count_formatted_correctly` — "45000" → "45k", "856" → "856"
- `duration_formatted_correctly`
- `start_time_formatted_correctly` — today/tomorrow/day-of-week
- `category_section_includes_all_streams_no_overflow`
- `unauthenticated_raw_data_produces_unauthenticated_state`

---

### Phase 5: `dbus_service.rs`

**Goal:** `DbusService` struct implementing the D-Bus interface via `zbus`.
Holds channels only — no `AppHandle`, no Tauri types.

**Fields:**
```rust
struct DbusService {
    state: Arc<Mutex<PlasmoidState>>,
    auth_cmd_tx: mpsc::UnboundedSender<AuthCommand>,
    window_tx: mpsc::Sender<WindowRequest>,
    open_url: Arc<dyn Fn(&str) + Send + Sync>,  // seam for testing; prod: open::that
}
```

`DbusService` also holds a background task (spawned separately) that watches
`display_rx` and `login_progress_rx`, calls `compute_plasmoid_state`, updates
`state`, and emits the `StateChanged` D-Bus signal.

**Unit tests** (call methods directly, no D-Bus connection needed):
- `login_method_sends_auth_command_login`
- `logout_method_sends_auth_command_logout`
- `open_stream_opens_correct_url` — verify the injected `open_url` fn is called with
  `"https://twitch.tv/{user_login}"`
- `open_settings_sends_window_request`
- `open_streamer_settings_sends_window_request_with_login_and_name`
- `cancel_login_sends_auth_command` (or a dedicated cancel channel — TBD in impl)

**zbus integration tests** (in `crates/twitch-kde/tests/dbus_integration.rs`):
Use `zbus::connection::Builder::peer()` for in-process connections (no session bus needed).
- `state_property_reflects_initial_display_data`
- `state_changed_signal_emitted_when_display_rx_updates`
- `login_method_reachable_over_dbus`

---

### Phase 6: `main.rs` wiring

**Goal:** Wire all the pieces together. Mirrors `twitch-app-tauri/src/main.rs`
but without tray creation and with D-Bus service added.

**Key differences from `twitch-app-tauri/src/main.rs`:**
- No `TrayBackend::new`, no `tray_backend.create_tray()`
- No `twitch_menu_tauri::start_listener()`
- ADD: spawn D-Bus service task in `setup`
- ADD: `window_rx` listener loop in `run` (receives `WindowRequest`, calls
  `open_settings_window` / `open_streamer_settings_window` from `twitch-settings-tauri::window`)
- Keep: `OpenSettingsRequested` backend event subscription (routes to `window_tx`)
- Keep: `prevent_exit` on window close (daemon stays alive)
- Keep: login/logout routing from D-Bus methods to `auth_cmd_tx`

No unit tests for `main.rs` (wiring layer — same policy as current codebase).

---

### Phase 7: QML plasmoid — components

Build and test each QML component in isolation before assembling `main.qml`.
Each component gets a `tst_*.qml` file.

**Order:**

#### 7a. `SectionHeader.qml`
Props: `text: string`
Renders: `Kirigami.Heading` (level 5) + `Kirigami.Separator` (`Layout.fillWidth`).
Tests: heading text rendered; separator present in component tree.

#### 7b. `ExpandableSection.qml`
Props: `heading: string`, `count: int`, default `expanded: false`
Renders: clickable header row with chevron + child slot (`default property alias content`).
Tests:
- `test_collapsed_by_default`
- `test_click_header_expands`
- `test_click_again_collapses`
- `test_heading_text_includes_count`

#### 7c. `StreamItem.qml`
Props: `userLogin`, `userName`, `gameName`, `viewerCountFormatted`,
       `durationFormatted`, `isFavourite: bool`
Renders: `PlasmaComponents3.ItemDelegate` with `label` (userName) and
`subtitle` (gameName · viewerCount · duration). Star icon visible iff `isFavourite`.
Signal: `clicked(userLogin: string)`
Tests:
- `test_user_name_displayed`
- `test_subtitle_contains_game_and_viewers`
- `test_star_visible_when_favourite`
- `test_star_hidden_when_not_favourite`
- `test_click_emits_user_login`

#### 7d. `ScheduleItem.qml`
Props: `broadcasterLogin`, `broadcasterName`, `startTimeFormatted`,
       `isInferred: bool`, `isFavourite: bool`
Renders: name + time; ✨ visible iff `isInferred`; ★ visible iff `isFavourite`.
Signal: `clicked(broadcasterLogin: string)`
Tests:
- `test_broadcaster_name_displayed`
- `test_sparkle_visible_when_inferred`
- `test_sparkle_hidden_when_not_inferred`
- `test_star_visible_when_favourite`
- `test_click_emits_broadcaster_login`

#### 7e. `LoginView.qml`
Props: `loginState: var` (bound to `MockDbusService.state.login_state`)
Renders:
- When `Idle`: `[Login to Twitch]` button
- When `PendingCode`: URL text, code text, Copy button, Cancel button, waiting label
- When `AwaitingConfirmation`: BusyIndicator + "Waiting for confirmation..." + Cancel
Tests:
- `test_login_button_shown_when_idle`
- `test_code_and_copy_shown_when_pending`
- `test_copy_button_copies_user_code_to_clipboard`
- `test_busy_indicator_shown_when_awaiting`
- `test_cancel_button_calls_cancel_login`

#### 7f. `CompactRepresentation.qml`
Props: bound to `MockDbusService.state`
Renders: Twitch icon. Badge with live count when `liveCount > 0`.
Tests:
- `test_no_badge_when_zero_live_streams`
- `test_badge_shows_correct_live_count`

---

### Phase 8: QML plasmoid — `main.qml` assembly

Wire all components together using `MockDbusService` for QML tests, real D-Bus
service in production.

**Structure:**
```qml
ScrollView {
  Column {
    // if not authenticated: LoginView
    // if authenticated:
    SectionHeader { text: "Following Live (" + liveCount + ")" }
    Repeater { model: state.live.visible; delegate: StreamItem { ... } }
    ExpandableSection { heading: "More"; count: state.live.overflow.length
      Repeater { model: state.live.overflow; delegate: StreamItem { ... } }
    }

    // categories (one ExpandableSection per category)
    Repeater {
      model: state.categories
      ExpandableSection { heading: modelData.name + " (" + modelData.totalViewersFormatted + ")"
        Repeater { model: modelData.streams; delegate: CategoryStreamItem { ... } }
      }
    }

    SectionHeader { text: "Scheduled (Next " + state.schedule.lookahead_hours + "h)" }
    // BusyIndicator if !state.schedule.loaded
    Repeater { model: state.schedule.visible; delegate: ScheduleItem { ... } }
    ExpandableSection { ... overflow ... }

    Kirigami.Separator {}
    PlasmaComponents3.Button { text: "Logout"; onClicked: dbusService.logout() }
  }
}
```

No unit tests for `main.qml` assembly (integration concerns only).
Verify manually by running the daemon and adding the plasmoid to a KDE panel.

---

### Phase 9: Makefile + README

**New Makefile targets:**
```makefile
build-kde:
    cd crates/twitch-kde && cargo build

run-kde:
    cd crates/twitch-kde && cargo build && ./../../target/debug/twitch-kde

test-plasmoid:
    @command -v qmltestrunner >/dev/null 2>&1 || { \
        echo "ERROR: qmltestrunner not found."; \
        echo "Install: qt6-declarative-dev (Debian/Ubuntu) or qt6-declarative (Arch)"; \
        exit 1; \
    }
    qmltestrunner -input crates/twitch-kde/plasmoid/tests/

test-all: test test-plasmoid

lint-kde:
    cargo clippy -p twitch-kde -- -D warnings
```

**README additions:**
- "KDE target" section in the build commands table
- System dependencies table entry:
  `qmltestrunner` — part of `qt6-declarative-dev` (Debian/Ubuntu) /
  `qt6-declarative` (Arch) — required for `make test-plasmoid`
- D-Bus activation setup instructions for packaged installs

---

## Definition of done (per phase)

Same as existing project DoD, extended for QML:

1. `cargo fmt` — formatted
2. `make lint` — no clippy warnings
3. `make test` — all Rust tests pass
4. `make test-plasmoid` — all QML tests pass (for phases 7+)
5. No dead code

## Notes on future refactoring

- `compute_plasmoid_state` (Phase 4) duplicates the filtering/sorting logic from
  `compute_display_state` in `twitch-menu-tauri`. Future work: extract shared
  filtering/sorting helpers to `twitch-backend` so both display crates reuse them.
- The `window.rs` module in `twitch-settings-tauri` (Phase 1) could grow to include
  other cross-target window management as the KDE target matures.

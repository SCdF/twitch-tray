use crate::display_state::DisplayState;

/// Output port for rendering the tray menu.
///
/// The domain core calls `update` whenever state changes; the adapter
/// layer (`TrayBackend`) translates the pure `DisplayState` into Tauri
/// menu items.  Test code uses `RecordingDisplayBackend` to capture the
/// states that would have been rendered.
pub trait DisplayBackend: Send + Sync {
    fn update(&self, state: DisplayState) -> anyhow::Result<()>;
}

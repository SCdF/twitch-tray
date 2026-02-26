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

#[cfg(test)]
pub mod mock {
    use super::*;
    use std::sync::{Arc, Mutex};

    /// A `DisplayBackend` that records every `DisplayState` passed to it.
    ///
    /// Use `take_updates()` in tests to inspect what the display layer received.
    pub struct RecordingDisplayBackend {
        updates: Arc<Mutex<Vec<DisplayState>>>,
    }

    impl RecordingDisplayBackend {
        pub fn new() -> Self {
            Self {
                updates: Arc::new(Mutex::new(Vec::new())),
            }
        }

        /// Drains and returns all recorded display states.
        pub fn take_updates(&self) -> Vec<DisplayState> {
            self.updates.lock().unwrap().drain(..).collect()
        }

        /// Returns the number of recorded updates without draining.
        pub fn update_count(&self) -> usize {
            self.updates.lock().unwrap().len()
        }
    }

    impl DisplayBackend for RecordingDisplayBackend {
        fn update(&self, state: DisplayState) -> anyhow::Result<()> {
            self.updates.lock().unwrap().push(state);
            Ok(())
        }
    }
}

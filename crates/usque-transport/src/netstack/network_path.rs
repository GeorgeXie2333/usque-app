//! Private path identity for measurements, separate from connection health.
use std::sync::{Arc, Mutex};

#[derive(Clone, Default)]
pub(crate) struct ActiveNetworkPath(Arc<Mutex<State>>);

#[derive(Default)]
struct State {
    epoch: u64,
    physical: Option<u64>,
    active: bool,
}

impl ActiveNetworkPath {
    pub(crate) fn activate(&self, physical: Option<u64>) {
        let mut state = self.0.lock().unwrap_or_else(|error| error.into_inner());
        // Startup publishes before exposing Connected; the pump confirms that
        // same path. Replacements deactivate first, even on the same network.
        if state.active && state.physical == physical {
            return;
        }
        state.epoch = state.epoch.wrapping_add(1);
        state.physical = physical;
        state.active = true;
    }

    pub(crate) fn deactivate(&self) {
        self.0
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .active = false;
    }

    #[cfg(any(feature = "wireguard", test))]
    pub(crate) fn generation(&self, physical: Option<u64>) -> Option<u64> {
        let state = self.0.lock().unwrap_or_else(|error| error.into_inner());
        (state.active && state.physical == physical).then_some(state.epoch)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unavailable_untracked_and_generation_zero_are_distinct_and_replacements_advance_epoch() {
        let path = ActiveNetworkPath::default();
        assert_eq!(path.generation(None), None);
        path.activate(None);
        let untracked = path.generation(None).unwrap();
        assert_eq!(path.generation(Some(0)), None);
        path.activate(Some(0));
        let zero = path.generation(Some(0)).unwrap();
        assert_ne!(zero, untracked);
        assert_eq!(path.generation(None), None);
        path.deactivate();
        assert_eq!(path.generation(Some(0)), None);
        path.activate(Some(0));
        assert_ne!(path.generation(Some(0)), Some(zero));
    }
}

//! Host-selected hardware enumerator (native on Windows, mock elsewhere).

use multiseat_core::{CoreResult, HardwareEnumerator, HardwareTree, PlatformCapabilities};
use tracing::info;

use crate::mock::MockHardwareEnumerator;
#[cfg(windows)]
use crate::WindowsHardwareEnumerator;

/// Concrete enumerator used by the daemon.
#[derive(Debug)]
pub enum HostHardwareEnumerator {
    /// Deterministic lab fixture / non-Windows development.
    Mock(MockHardwareEnumerator),
    /// DXGI + SetupAPI inventory.
    #[cfg(windows)]
    Native(WindowsHardwareEnumerator),
}

impl HostHardwareEnumerator {
    /// Selects mock or native based on platform and `MULTISEAT_USE_MOCK=1`.
    pub fn auto() -> Self {
        if std::env::var_os("MULTISEAT_USE_MOCK").is_some_and(|v| v == "1") {
            info!("MULTISEAT_USE_MOCK=1; using mock hardware enumerator");
            return Self::Mock(MockHardwareEnumerator::with_lab_fixture());
        }

        #[cfg(windows)]
        {
            match WindowsHardwareEnumerator::try_create() {
                Ok(native) => {
                    info!("using native Windows hardware enumerator");
                    return Self::Native(native);
                }
                Err(err) => {
                    info!(error = %err, "native enumerator unavailable; falling back to mock");
                }
            }
        }

        info!("using mock hardware enumerator");
        Self::Mock(MockHardwareEnumerator::with_lab_fixture())
    }

    /// Forces the mock lab fixture.
    pub fn mock() -> Self {
        Self::Mock(MockHardwareEnumerator::with_lab_fixture())
    }

    /// Returns true when the mock backend is active.
    pub fn is_mock(&self) -> bool {
        matches!(self, Self::Mock(_))
    }

    /// Mock-only: reopen a HID device under a new instance id (hot-plug lab).
    ///
    /// Returns `false` when not mock or when no interface matched.
    pub fn simulate_input_reopen(
        &mut self,
        interface_substr: &str,
        new_instance: impl Into<String>,
    ) -> bool {
        match self {
            Self::Mock(inner) => inner.reopen_input_instance(interface_substr, new_instance),
            #[cfg(windows)]
            Self::Native(_) => false,
        }
    }
}

impl HardwareEnumerator for HostHardwareEnumerator {
    fn capabilities(&self) -> CoreResult<PlatformCapabilities> {
        match self {
            Self::Mock(inner) => inner.capabilities(),
            #[cfg(windows)]
            Self::Native(inner) => inner.capabilities(),
        }
    }

    fn snapshot(&self) -> CoreResult<HardwareTree> {
        match self {
            Self::Mock(inner) => inner.snapshot(),
            #[cfg(windows)]
            Self::Native(inner) => inner.snapshot(),
        }
    }

    fn start_notifications(&mut self) -> CoreResult<()> {
        match self {
            Self::Mock(inner) => inner.start_notifications(),
            #[cfg(windows)]
            Self::Native(inner) => inner.start_notifications(),
        }
    }

    fn poll_invalidated(&mut self) -> bool {
        match self {
            Self::Mock(inner) => inner.poll_invalidated(),
            #[cfg(windows)]
            Self::Native(inner) => inner.poll_invalidated(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn auto_defaults_to_mock_outside_forced_native() {
        // On macOS/Linux CI this must be mock. On Windows without DXGI it may
        // also fall back to mock; with DXGI it can be native unless forced.
        std::env::set_var("MULTISEAT_USE_MOCK", "1");
        let host = HostHardwareEnumerator::auto();
        assert!(host.is_mock());
        std::env::remove_var("MULTISEAT_USE_MOCK");
    }
}

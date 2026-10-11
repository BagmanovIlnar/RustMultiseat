//! Host-selected input router (native stub on Windows, mock elsewhere).

use multiseat_core::{
    BackendHealth, CoreResult, InputAssignments, InputRouter, RollbackToken,
};
use tracing::info;

use crate::input::backend::WindowsInputRouter;
use crate::mock::MockInputRouter;

/// Concrete input router used by the daemon.
#[derive(Debug)]
pub enum HostInputRouter {
    /// Deterministic mock for tests and non-Windows hosts.
    Mock(MockInputRouter),
    /// Probe-only native stub until Stage 0 filter PoC.
    Native(WindowsInputRouter),
}

impl HostInputRouter {
    /// Selects mock or native using `MULTISEAT_USE_MOCK=1` / platform.
    pub fn auto(keyboard_kits: usize, pointer_kits: usize) -> Self {
        if std::env::var_os("MULTISEAT_USE_MOCK").is_some_and(|v| v == "1") {
            info!("MULTISEAT_USE_MOCK=1; using mock input router");
            return Self::mock();
        }

        #[cfg(windows)]
        {
            info!("using native WindowsInputRouter");
            return Self::Native(WindowsInputRouter::new(keyboard_kits, pointer_kits));
        }

        #[cfg(not(windows))]
        {
            let _ = (keyboard_kits, pointer_kits);
            info!("non-Windows host; using mock input router");
            Self::mock()
        }
    }

    /// Forces the mock input router.
    pub fn mock() -> Self {
        Self::Mock(MockInputRouter::default())
    }

    /// Aligns with a mock hardware enumerator choice.
    pub fn matching_hardware(
        is_mock_hardware: bool,
        keyboard_kits: usize,
        pointer_kits: usize,
    ) -> Self {
        if is_mock_hardware {
            Self::mock()
        } else {
            Self::auto(keyboard_kits, pointer_kits)
        }
    }

    /// True when the mock backend is active.
    pub fn is_mock(&self) -> bool {
        matches!(self, Self::Mock(_))
    }
}

impl InputRouter for HostInputRouter {
    fn validate(&self, assignments: &InputAssignments) -> CoreResult<()> {
        match self {
            Self::Mock(inner) => inner.validate(assignments),
            Self::Native(inner) => inner.validate(assignments),
        }
    }

    fn apply(&mut self, assignments: &InputAssignments) -> CoreResult<RollbackToken> {
        match self {
            Self::Mock(inner) => inner.apply(assignments),
            Self::Native(inner) => inner.apply(assignments),
        }
    }

    fn rollback(&mut self, token: &RollbackToken) -> CoreResult<()> {
        match self {
            Self::Mock(inner) => inner.rollback(token),
            Self::Native(inner) => inner.rollback(token),
        }
    }

    fn health(&self) -> CoreResult<BackendHealth> {
        match self {
            Self::Mock(inner) => inner.health(),
            Self::Native(inner) => inner.health(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn matching_hardware_keeps_mock_pair() {
        let host = HostInputRouter::matching_hardware(true, 2, 2);
        assert!(host.is_mock());
    }
}

//! Host-selected session backend (native WTS on Windows, mock elsewhere).

use multiseat_core::{
    BackendHealth, CoreResult, SeatConfig, SessionBackend, SessionHandle,
};
use tracing::info;

use crate::mock::MockSessionBackend;
use crate::session::backend::WindowsSessionBackend;

/// Concrete session backend used by the daemon.
#[derive(Debug)]
pub enum HostSessionBackend {
    /// Deterministic mock for tests and non-Windows hosts.
    Mock(MockSessionBackend),
    /// Documented WTS observation + observational bind only.
    Native(WindowsSessionBackend),
}

impl HostSessionBackend {
    /// Selects mock or native using the same policy as hardware (`MULTISEAT_USE_MOCK=1`).
    pub fn auto() -> Self {
        if std::env::var_os("MULTISEAT_USE_MOCK").is_some_and(|v| v == "1") {
            info!("MULTISEAT_USE_MOCK=1; using mock session backend");
            return Self::mock();
        }

        #[cfg(windows)]
        {
            info!("using native WindowsSessionBackend");
            return Self::Native(WindowsSessionBackend::new());
        }

        #[cfg(not(windows))]
        {
            info!("non-Windows host; using mock session backend");
            Self::mock()
        }
    }

    /// Forces the mock session backend (probe succeeds).
    pub fn mock() -> Self {
        Self::Mock(MockSessionBackend::available())
    }

    /// Aligns with a mock hardware enumerator choice.
    pub fn matching_hardware(is_mock_hardware: bool) -> Self {
        if is_mock_hardware {
            Self::mock()
        } else {
            Self::auto()
        }
    }

    /// True when the mock backend is active.
    pub fn is_mock(&self) -> bool {
        matches!(self, Self::Mock(_))
    }
}

impl SessionBackend for HostSessionBackend {
    fn probe(&self) -> CoreResult<bool> {
        match self {
            Self::Mock(inner) => inner.probe(),
            Self::Native(inner) => inner.probe(),
        }
    }

    fn create_or_connect(&mut self, seat: &SeatConfig) -> CoreResult<SessionHandle> {
        match self {
            Self::Mock(inner) => inner.create_or_connect(seat),
            Self::Native(inner) => inner.create_or_connect(seat),
        }
    }

    fn start_agent(&mut self, session: &SessionHandle) -> CoreResult<()> {
        match self {
            Self::Mock(inner) => inner.start_agent(session),
            Self::Native(inner) => inner.start_agent(session),
        }
    }

    fn lock(&mut self, session: &SessionHandle) -> CoreResult<()> {
        match self {
            Self::Mock(inner) => inner.lock(session),
            Self::Native(inner) => inner.lock(session),
        }
    }

    fn disconnect(&mut self, session: &SessionHandle) -> CoreResult<()> {
        match self {
            Self::Mock(inner) => inner.disconnect(session),
            Self::Native(inner) => inner.disconnect(session),
        }
    }

    fn health(&self, session: &SessionHandle) -> CoreResult<BackendHealth> {
        match self {
            Self::Mock(inner) => inner.health(session),
            Self::Native(inner) => inner.health(session),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mock_force_via_env() {
        std::env::set_var("MULTISEAT_USE_MOCK", "1");
        let host = HostSessionBackend::auto();
        assert!(host.is_mock());
        assert!(host.probe().unwrap());
        std::env::remove_var("MULTISEAT_USE_MOCK");
    }

    #[test]
    fn matching_hardware_keeps_mock_pair() {
        let host = HostSessionBackend::matching_hardware(true);
        assert!(host.is_mock());
    }
}

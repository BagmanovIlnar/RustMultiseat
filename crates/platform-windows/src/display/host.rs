//! Host-selected display backend (native stub on Windows, mock elsewhere).

use multiseat_core::{
    CoreResult, DisplayBackend, DisplayCapabilities, RollbackToken, SeatConfig, SeatId,
    SessionHandle,
};
use tracing::info;

use crate::display::backend::WindowsDisplayBackend;
use crate::mock::MockDisplayBackend;

/// Concrete display backend used by the daemon.
#[derive(Debug)]
pub enum HostDisplayBackend {
    /// Deterministic mock for tests and non-Windows hosts.
    Mock(MockDisplayBackend),
    /// Probe-only native stub until Stage 0 display PoC.
    Native(WindowsDisplayBackend),
}

impl HostDisplayBackend {
    /// Selects mock or native using `MULTISEAT_USE_MOCK=1` / platform.
    pub fn auto(physical_outputs: usize) -> Self {
        if std::env::var_os("MULTISEAT_USE_MOCK").is_some_and(|v| v == "1") {
            info!("MULTISEAT_USE_MOCK=1; using mock display backend");
            return Self::mock();
        }

        #[cfg(windows)]
        {
            info!("using native WindowsDisplayBackend");
            return Self::Native(WindowsDisplayBackend::new(physical_outputs));
        }

        #[cfg(not(windows))]
        {
            let _ = physical_outputs;
            info!("non-Windows host; using mock display backend");
            Self::mock()
        }
    }

    /// Forces the mock display backend.
    pub fn mock() -> Self {
        Self::Mock(MockDisplayBackend::default())
    }

    /// Aligns with a mock hardware enumerator choice.
    pub fn matching_hardware(is_mock_hardware: bool, physical_outputs: usize) -> Self {
        if is_mock_hardware {
            Self::mock()
        } else {
            Self::auto(physical_outputs)
        }
    }

    /// True when the mock backend is active.
    pub fn is_mock(&self) -> bool {
        matches!(self, Self::Mock(_))
    }
}

impl DisplayBackend for HostDisplayBackend {
    fn capabilities(&self) -> CoreResult<DisplayCapabilities> {
        match self {
            Self::Mock(inner) => inner.capabilities(),
            Self::Native(inner) => inner.capabilities(),
        }
    }

    fn prepare(&mut self, seats: &[SeatConfig]) -> CoreResult<()> {
        match self {
            Self::Mock(inner) => inner.prepare(seats),
            Self::Native(inner) => inner.prepare(seats),
        }
    }

    fn attach(&mut self, seat: &SeatConfig, session: &SessionHandle) -> CoreResult<RollbackToken> {
        match self {
            Self::Mock(inner) => inner.attach(seat, session),
            Self::Native(inner) => inner.attach(seat, session),
        }
    }

    fn detach(&mut self, seat_id: SeatId) -> CoreResult<()> {
        match self {
            Self::Mock(inner) => inner.detach(seat_id),
            Self::Native(inner) => inner.detach(seat_id),
        }
    }

    fn rollback(&mut self, token: &RollbackToken) -> CoreResult<()> {
        match self {
            Self::Mock(inner) => inner.rollback(token),
            Self::Native(inner) => inner.rollback(token),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn matching_hardware_keeps_mock_pair() {
        let host = HostDisplayBackend::matching_hardware(true, 2);
        assert!(host.is_mock());
        assert!(host.capabilities().unwrap().attach_supported);
    }
}

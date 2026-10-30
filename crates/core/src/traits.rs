//! Platform backend traits. Implementations live in `platform-windows` / drivers.

use crate::error::CoreResult;
use crate::hardware::{HardwareTree, PlatformCapabilities};
use crate::ids::{SeatId, SessionId};
use crate::journal::RollbackToken;
use crate::seat::{MultiseatConfig, SeatConfig};

/// Health report shared by backends.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BackendHealth {
    /// Whether the backend is usable.
    pub healthy: bool,
    /// Optional detail.
    pub detail: String,
}

impl BackendHealth {
    /// Healthy backend.
    pub fn ok() -> Self {
        Self {
            healthy: true,
            detail: String::new(),
        }
    }

    /// Unhealthy backend with a reason.
    pub fn down(detail: impl Into<String>) -> Self {
        Self {
            healthy: false,
            detail: detail.into(),
        }
    }
}

/// Handle describing a connected Windows session.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SessionHandle {
    /// Seat owning the session.
    pub seat_id: SeatId,
    /// Windows session id.
    pub session_id: SessionId,
}

/// Display backend feature flags for the current OS.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct DisplayCapabilities {
    /// Backend name (`iddcx`, `wddm-legacy`, `win7-legacy`, `none`).
    pub backend_name: String,
    /// Whether attach/detach is available.
    pub attach_supported: bool,
}

/// Input assignment map derived from configuration.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InputAssignments {
    /// Full multiseat config used for routing.
    pub config: MultiseatConfig,
}

/// Enumerates hardware and platform capabilities.
pub trait HardwareEnumerator {
    /// Returns capabilities available on this OS build.
    fn capabilities(&self) -> CoreResult<PlatformCapabilities>;

    /// Builds a consistent hardware snapshot.
    fn snapshot(&self) -> CoreResult<HardwareTree>;

    /// Starts hot-plug notifications that trigger debounced rescans.
    fn start_notifications(&mut self) -> CoreResult<()>;

    /// Returns true when a debounced hot-plug signal is ready for rescan.
    fn poll_invalidated(&mut self) -> bool {
        false
    }
}

/// Routes physical input devices to seats.
pub trait InputRouter {
    /// Validates assignments against current hardware and policy.
    fn validate(&self, assignments: &InputAssignments) -> CoreResult<()>;

    /// Applies routes atomically and returns a rollback token.
    fn apply(&mut self, assignments: &InputAssignments) -> CoreResult<RollbackToken>;

    /// Rolls back a previous apply.
    fn rollback(&mut self, token: &RollbackToken) -> CoreResult<()>;

    /// Returns driver heartbeat / queue health.
    fn health(&self) -> CoreResult<BackendHealth>;

    /// Immediately clears active routes (emergency fail-open).
    ///
    /// Default is a no-op; journal rollback remains the primary compensation path.
    /// Implementations should clear in-memory routes without permanently disabling
    /// the router (so a later Start can succeed).
    fn force_fail_open(&mut self) {}
}

/// Attaches displays to sessions (experimental per-OS backends).
pub trait DisplayBackend {
    /// Reports which display model is available.
    fn capabilities(&self) -> CoreResult<DisplayCapabilities>;

    /// Prepares adapters/outputs for the requested seats.
    fn prepare(&mut self, seats: &[SeatConfig]) -> CoreResult<()>;

    /// Experimentally binds a seat display to a session.
    fn attach(&mut self, seat: &SeatConfig, session: &SessionHandle) -> CoreResult<RollbackToken>;

    /// Detaches a seat display binding.
    fn detach(&mut self, seat_id: SeatId) -> CoreResult<()>;

    /// Restores previous display configuration.
    fn rollback(&mut self, token: &RollbackToken) -> CoreResult<()>;
}

/// Creates/connects interactive sessions and session agents.
pub trait SessionBackend {
    /// Probes whether the required session model is available.
    fn probe(&self) -> CoreResult<bool>;

    /// Creates or connects a session for the seat.
    fn create_or_connect(&mut self, seat: &SeatConfig) -> CoreResult<SessionHandle>;

    /// Starts the session agent with a least-privilege token.
    fn start_agent(&mut self, session: &SessionHandle) -> CoreResult<()>;

    /// Locks the session desktop.
    fn lock(&mut self, session: &SessionHandle) -> CoreResult<()>;

    /// Disconnects the session.
    fn disconnect(&mut self, session: &SessionHandle) -> CoreResult<()>;

    /// Returns session/agent health.
    fn health(&self, session: &SessionHandle) -> CoreResult<BackendHealth>;
}

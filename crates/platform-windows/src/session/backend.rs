//! Native [`SessionBackend`] backed by documented WTS observation only.

use std::collections::HashMap;

use multiseat_core::{
    BackendHealth, CheckStatus, CoreError, CoreResult, DualSessionProbeResult, SeatConfig, SeatId,
    SessionBackend, SessionConnectState, SessionHandle, SessionId,
};
use tracing::{info, warn};

use super::probe_dual_sessions;

/// Windows session backend for Stage 0 experiments.
///
/// - [`SessionBackend::probe`] is true only when two+ active sessions are already
///   visible via WTS.
/// - [`SessionBackend::create_or_connect`] never invents a new local interactive
///   session; it may only bind a seat to an already-active session id when the
///   probe passed.
#[derive(Debug)]
pub struct WindowsSessionBackend {
    probe: DualSessionProbeResult,
    seats: HashMap<SeatId, SessionId>,
}

impl Default for WindowsSessionBackend {
    fn default() -> Self {
        Self::new()
    }
}

impl WindowsSessionBackend {
    /// Runs an initial dual-session probe.
    pub fn new() -> Self {
        let probe = probe_dual_sessions();
        info!(
            status = ?probe.status,
            active = probe.active_count,
            "WindowsSessionBackend initialized"
        );
        Self {
            probe,
            seats: HashMap::new(),
        }
    }

    /// Latest dual-session probe evidence.
    pub fn last_probe(&self) -> &DualSessionProbeResult {
        &self.probe
    }

    fn refresh_probe(&mut self) {
        self.probe = probe_dual_sessions();
    }

    fn next_unassigned_active(&self) -> Option<u32> {
        let assigned: std::collections::HashSet<u32> =
            self.seats.values().map(|id| id.0).collect();
        self.probe
            .sessions
            .iter()
            .filter(|s| s.state == SessionConnectState::Active)
            .map(|s| s.session_id)
            .find(|id| !assigned.contains(id))
    }
}

impl SessionBackend for WindowsSessionBackend {
    fn probe(&self) -> CoreResult<bool> {
        Ok(self.probe.status == CheckStatus::Pass)
    }

    fn create_or_connect(&mut self, seat: &SeatConfig) -> CoreResult<SessionHandle> {
        self.refresh_probe();
        if self.probe.status != CheckStatus::Pass {
            return Err(CoreError::Backend(format!(
                "native session create unavailable: {} (documented APIs cannot open a second local interactive session)",
                self.probe.detail()
            )));
        }

        if let Some(existing) = self.seats.get(&seat.id).copied() {
            return Ok(SessionHandle {
                seat_id: seat.id,
                session_id: existing,
            });
        }

        let Some(session_id) = self.next_unassigned_active() else {
            return Err(CoreError::Backend(
                "dual-session probe passed but no unassigned active WTS session remains".into(),
            ));
        };

        let handle = SessionHandle {
            seat_id: seat.id,
            session_id: SessionId(session_id),
        };
        self.seats.insert(seat.id, handle.session_id);
        info!(
            seat = %seat.id,
            session = session_id,
            "bound seat to existing active WTS session (observational connect)"
        );
        Ok(handle)
    }

    fn start_agent(&mut self, session: &SessionHandle) -> CoreResult<()> {
        if !self.seats.contains_key(&session.seat_id) {
            return Err(CoreError::Backend("unknown session".into()));
        }
        // Agent launch across sessions needs CreateProcessAsUser / service rights;
        // keep honest until Stage 0 defines a licensed path.
        warn!(
            seat = %session.seat_id,
            session = session.session_id.0,
            "start_agent not implemented for native WindowsSessionBackend"
        );
        Err(CoreError::Backend(
            "native start_agent not implemented (awaiting Stage 0 licensed session-agent launch path)"
                .into(),
        ))
    }

    fn lock(&mut self, session: &SessionHandle) -> CoreResult<()> {
        if !self.seats.contains_key(&session.seat_id) {
            return Err(CoreError::Backend("unknown session".into()));
        }
        Err(CoreError::Backend(
            "native session lock not implemented".into(),
        ))
    }

    fn disconnect(&mut self, session: &SessionHandle) -> CoreResult<()> {
        self.seats.remove(&session.seat_id);
        Ok(())
    }

    fn health(&self, session: &SessionHandle) -> CoreResult<BackendHealth> {
        if self.seats.contains_key(&session.seat_id) {
            Ok(BackendHealth::ok())
        } else {
            Ok(BackendHealth::down("session not bound"))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use multiseat_core::{DeviceId, DisplayId, SeatId, UserSid};

    fn sample_seat() -> SeatConfig {
        SeatConfig {
            id: SeatId(1),
            user_sid: UserSid("S-1-5-21-1".into()),
            display_ids: vec![DisplayId(r"\\.\DISPLAY1".into())],
            keyboard_ids: vec![DeviceId::from_instance("kbd1")],
            pointer_ids: vec![DeviceId::from_instance("ptr1")],
            enabled: true,
        }
    }

    #[test]
    fn create_fails_when_dual_session_blocked() {
        // Dev/CI and typical client Windows: probe is not Pass.
        let mut backend = WindowsSessionBackend::new();
        if backend.probe().unwrap() {
            // Rare lab host already showing two active sessions.
            return;
        }
        let err = backend.create_or_connect(&sample_seat()).unwrap_err();
        assert!(err.to_string().contains("native session create unavailable"));
    }
}

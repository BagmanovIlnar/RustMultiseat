//! Native [`SessionBackend`] backed by documented WTS observation only.

use std::collections::HashMap;

use multiseat_core::{
    BackendHealth, CheckStatus, CoreError, CoreResult, DualSessionProbeResult, SeatConfig, SeatId,
    SessionBackend, SessionConnectState, SessionHandle, SessionId,
};
use tracing::{info, warn};

use super::agent_as_user::{launch_agent_in_session, stop_native_agent};
use super::probe_dual_sessions;
use super::wts_control::{disconnect_session, lock_session};

/// Windows session backend for Stage 0 experiments.
///
/// - [`SessionBackend::probe`] is true only when two+ active sessions are already
///   visible via WTS.
/// - [`SessionBackend::create_or_connect`] never invents a new local interactive
///   session; it may only bind a seat to an already-active session id when the
///   probe passed.
/// - [`SessionBackend::start_agent`] uses `CreateProcessAsUser` only when
///   `MULTISEAT_NATIVE_AGENT_LAUNCH=1` (honest Win32 errors otherwise / when off).
/// - [`SessionBackend::lock`] / WTS disconnect side effects require
///   `MULTISEAT_NATIVE_SESSION_CONTROL=1`.
#[derive(Debug)]
pub struct WindowsSessionBackend {
    probe: DualSessionProbeResult,
    seats: HashMap<SeatId, SessionId>,
    /// PIDs of agents launched via CreateProcessAsUser.
    agents: HashMap<SeatId, u32>,
    /// Daemon bind/connect string passed to agents as `--addr`.
    daemon_addr: Option<String>,
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
            agents: HashMap::new(),
            daemon_addr: None,
        }
    }

    /// Sets the daemon endpoint that native agent launch will use.
    pub fn set_daemon_addr(&mut self, addr: impl Into<String>) {
        let addr = addr.into();
        self.daemon_addr = if addr.is_empty() { None } else { Some(addr) };
    }

    /// Number of tracked native agent PIDs (tests).
    pub fn agent_count(&self) -> usize {
        self.agents.len()
    }

    /// Latest dual-session probe evidence.
    pub fn last_probe(&self) -> &DualSessionProbeResult {
        &self.probe
    }

    fn refresh_probe(&mut self) {
        self.probe = probe_dual_sessions();
    }

    fn next_unassigned_active(&self) -> Option<u32> {
        let assigned: std::collections::HashSet<u32> = self.seats.values().map(|id| id.0).collect();
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
        self.refresh_probe();
        if let Some(prev) = self.agents.remove(&session.seat_id) {
            stop_native_agent(prev);
        }

        match launch_agent_in_session(
            session.session_id.0,
            session.seat_id,
            self.daemon_addr.as_deref(),
        ) {
            Ok(launch) => {
                self.agents.insert(session.seat_id, launch.pid);
                Ok(())
            }
            Err(err) => {
                warn!(
                    seat = %session.seat_id,
                    session = session.session_id.0,
                    daemon_addr = ?self.daemon_addr,
                    error = %err,
                    "native start_agent failed"
                );
                Err(err)
            }
        }
    }

    fn lock(&mut self, session: &SessionHandle) -> CoreResult<()> {
        if !self.seats.contains_key(&session.seat_id) {
            return Err(CoreError::Backend("unknown session".into()));
        }
        self.refresh_probe();
        match lock_session(session.session_id.0) {
            Ok(()) => Ok(()),
            Err(err) => {
                warn!(
                    seat = %session.seat_id,
                    session = session.session_id.0,
                    error = %err,
                    "native session lock failed"
                );
                Err(err)
            }
        }
    }

    fn disconnect(&mut self, session: &SessionHandle) -> CoreResult<()> {
        self.refresh_probe();
        if let Some(pid) = self.agents.remove(&session.seat_id) {
            stop_native_agent(pid);
        }
        let session_id = self.seats.remove(&session.seat_id);
        if let Some(session_id) = session_id {
            // Soft-skip when flag off; real Win32 errors when enabled.
            if let Err(err) = disconnect_session(session_id.0) {
                warn!(
                    seat = %session.seat_id,
                    session = session_id.0,
                    error = %err,
                    "native WTS disconnect failed after local unbind"
                );
                return Err(err);
            }
        }
        Ok(())
    }

    fn health(&self, session: &SessionHandle) -> CoreResult<BackendHealth> {
        let Some(bound) = self.seats.get(&session.seat_id).copied() else {
            return Ok(BackendHealth::down("session not bound"));
        };
        // Live WTS enumerate — do not trust a stale boot-time probe snapshot.
        let live = probe_dual_sessions();
        let still_listed = live.sessions.iter().any(|s| s.session_id == bound.0);
        if still_listed {
            Ok(BackendHealth::ok())
        } else {
            Ok(BackendHealth::down(
                "bound WTS session id missing from live probe",
            ))
        }
    }
}

impl Drop for WindowsSessionBackend {
    fn drop(&mut self) {
        for (_, pid) in self.agents.drain() {
            stop_native_agent(pid);
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
        assert!(err
            .to_string()
            .contains("native session create unavailable"));
    }

    #[test]
    fn start_agent_unknown_session_errs() {
        let mut backend = WindowsSessionBackend::new();
        let session = SessionHandle {
            seat_id: SeatId(99),
            session_id: SessionId(1),
        };
        assert!(backend.start_agent(&session).is_err());
    }

    #[test]
    fn lock_unknown_session_errs() {
        let mut backend = WindowsSessionBackend::new();
        let session = SessionHandle {
            seat_id: SeatId(99),
            session_id: SessionId(1),
        };
        assert!(backend.lock(&session).is_err());
    }
}

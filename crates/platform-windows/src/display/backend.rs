//! Native [`DisplayBackend`] stub: probe-only until Stage 0 display PoC.

use std::collections::HashMap;

use multiseat_core::{
    CoreError, CoreResult, DisplayBackend, DisplayCapabilities, DisplayProbeResult, RollbackToken,
    SeatConfig, SeatId, SessionHandle,
};
use tracing::{info, warn};

use super::probe_display;

/// Windows display backend that reports OS candidates but refuses attach.
#[derive(Debug)]
pub struct WindowsDisplayBackend {
    probe: DisplayProbeResult,
    attached: HashMap<SeatId, RollbackToken>,
}

impl Default for WindowsDisplayBackend {
    fn default() -> Self {
        Self::new(0)
    }
}

impl WindowsDisplayBackend {
    /// Creates a backend seeded with the given attached-output count.
    pub fn new(physical_outputs: usize) -> Self {
        let probe = probe_display(physical_outputs);
        info!(
            backend = %probe.backend_name,
            status = ?probe.status,
            outputs = physical_outputs,
            "WindowsDisplayBackend initialized"
        );
        Self {
            probe,
            attached: HashMap::new(),
        }
    }

    /// Latest display probe evidence.
    pub fn last_probe(&self) -> &DisplayProbeResult {
        &self.probe
    }

    /// Refreshes probe using a new output count (e.g. after inventory snapshot).
    pub fn refresh(&mut self, physical_outputs: usize) {
        self.probe = probe_display(physical_outputs);
    }
}

impl DisplayBackend for WindowsDisplayBackend {
    fn capabilities(&self) -> CoreResult<DisplayCapabilities> {
        Ok(DisplayCapabilities {
            backend_name: self.probe.backend_name.clone(),
            attach_supported: self.probe.attach_supported,
        })
    }

    fn prepare(&mut self, seats: &[SeatConfig]) -> CoreResult<()> {
        if seats.is_empty() {
            return Err(CoreError::Backend("no seats to prepare".into()));
        }
        let needed: usize = seats.iter().map(|s| s.display_ids.len().max(1)).sum();
        if self.probe.physical_outputs > 0 && self.probe.physical_outputs < needed {
            return Err(CoreError::Backend(format!(
                "not enough outputs: have={} need≈{needed}",
                self.probe.physical_outputs
            )));
        }
        if !self.probe.attach_supported {
            return Err(CoreError::Backend(format!(
                "native display prepare blocked: {}",
                self.probe.detail()
            )));
        }
        Ok(())
    }

    fn attach(&mut self, seat: &SeatConfig, session: &SessionHandle) -> CoreResult<RollbackToken> {
        warn!(
            seat = %seat.id,
            session = session.session_id.0,
            backend = %self.probe.backend_name,
            "native display attach refused (Stage 0 PoC pending)"
        );
        Err(CoreError::Backend(format!(
            "native display attach unavailable for candidate={}: {}",
            self.probe.backend_name,
            self.probe.detail()
        )))
    }

    fn detach(&mut self, seat_id: SeatId) -> CoreResult<()> {
        self.attached.remove(&seat_id);
        Ok(())
    }

    fn rollback(&mut self, token: &RollbackToken) -> CoreResult<()> {
        self.attached.retain(|_, t| t != token);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use multiseat_core::{DeviceId, DisplayId, SeatId, SessionId, UserSid};

    fn seat() -> SeatConfig {
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
    fn attach_always_errors_until_poc() {
        let mut backend = WindowsDisplayBackend::new(2);
        let session = SessionHandle {
            seat_id: SeatId(1),
            session_id: SessionId(1),
        };
        let err = backend.attach(&seat(), &session).unwrap_err();
        assert!(err.to_string().contains("native display attach unavailable"));
    }

    #[test]
    fn prepare_blocked_without_attach_support() {
        let mut backend = WindowsDisplayBackend::new(2);
        if backend.capabilities().unwrap().attach_supported {
            return;
        }
        let err = backend.prepare(&[seat()]).unwrap_err();
        assert!(err.to_string().contains("native display prepare blocked"));
    }
}

//! Native [`InputRouter`] stub: validate-only until Stage 0 filter PoC.

use multiseat_core::{
    BackendHealth, CoreError, CoreResult, InputAssignments, InputProbeResult, InputRouter,
    RollbackToken,
};
use tracing::{info, warn};

use super::probe_input;

/// Windows input router that refuses apply until a KMDF filter PoC exists.
#[derive(Debug)]
pub struct WindowsInputRouter {
    probe: InputProbeResult,
    active: Option<RollbackToken>,
}

impl Default for WindowsInputRouter {
    fn default() -> Self {
        Self::new(0, 0)
    }
}

impl WindowsInputRouter {
    /// Creates a router seeded with kit counts from inventory.
    pub fn new(keyboard_kits: usize, pointer_kits: usize) -> Self {
        let probe = probe_input(keyboard_kits, pointer_kits);
        info!(
            filter = probe.filter_installed,
            status = ?probe.status,
            "WindowsInputRouter initialized"
        );
        Self {
            probe,
            active: None,
        }
    }

    /// Latest input probe evidence.
    pub fn last_probe(&self) -> &InputProbeResult {
        &self.probe
    }

    /// Refreshes probe using new kit counts.
    pub fn refresh(&mut self, keyboard_kits: usize, pointer_kits: usize) {
        self.probe = probe_input(keyboard_kits, pointer_kits);
    }
}

impl InputRouter for WindowsInputRouter {
    fn validate(&self, assignments: &InputAssignments) -> CoreResult<()> {
        if assignments.config.emergency_keyboard.is_none()
            || assignments.config.emergency_pointer.is_none()
        {
            return Err(CoreError::Backend(
                "emergency keyboard/pointer required for fail-open".into(),
            ));
        }
        if !self.probe.filter_installed {
            return Err(CoreError::Backend(format!(
                "native input validate blocked: {}",
                self.probe.detail()
            )));
        }
        Ok(())
    }

    fn apply(&mut self, assignments: &InputAssignments) -> CoreResult<RollbackToken> {
        self.validate(assignments)?;
        warn!(
            filter = self.probe.filter_installed,
            "native input apply refused (Stage 0 KMDF filter PoC pending; fail-open retained)"
        );
        Err(CoreError::Backend(format!(
            "native input apply unavailable (fail-open retained): {}",
            self.probe.detail()
        )))
    }

    fn rollback(&mut self, token: &RollbackToken) -> CoreResult<()> {
        if self.active.as_ref() == Some(token) {
            self.active = None;
        }
        // Fail-open: clearing local state is always safe.
        Ok(())
    }

    fn health(&self) -> CoreResult<BackendHealth> {
        if self.probe.fail_open_ready {
            Ok(BackendHealth::ok())
        } else {
            Ok(BackendHealth::down("fail-open path unhealthy"))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use multiseat_core::{DeviceId, MultiseatConfig};

    fn assignments() -> InputAssignments {
        InputAssignments {
            config: MultiseatConfig {
                seats: vec![],
                emergency_keyboard: Some(DeviceId::from_instance("host-kbd")),
                emergency_pointer: Some(DeviceId::from_instance("host-ptr")),
            },
        }
    }

    #[test]
    fn apply_errors_and_keeps_fail_open() {
        std::env::remove_var("MULTISEAT_INPUT_FILTER");
        let mut router = WindowsInputRouter::new(2, 2);
        let err = router.apply(&assignments()).unwrap_err();
        assert!(err.to_string().contains("native input"));
        assert!(router.health().unwrap().healthy);
        assert!(router.active.is_none());
    }
}

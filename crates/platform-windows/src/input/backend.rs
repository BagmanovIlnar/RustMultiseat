//! Native [`InputRouter`]: builds SET_ROUTES from config and talks to filter control.

use multiseat_core::{
    BackendHealth, CoreError, CoreResult, InputAssignments, InputProbeResult, InputRouter,
    RollbackToken,
};
use tracing::{info, warn};

use super::probe::{probe_input, INPUT_SOFT_APPLY_ENV};
use crate::poc_status::input_soft_apply_enabled;

/// Windows input router backed by [`super::filter_control::InputFilterControl`].
///
/// When the control device is soft-present (`MULTISEAT_INPUT_FILTER=1`) or native,
/// `apply` pushes a route table then arms the filter. Without a control path,
/// soft bookkeeping still requires `MULTISEAT_INPUT_SOFT_APPLY=1`.
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
        let table = super::routes::route_table_from_assignments(assignments)
            .map_err(|e| CoreError::Backend(format!("route table build failed: {e}")))?;

        // Prefer real/soft filter control when the device (or lab soft-present) is up.
        let ctl = super::filter_control::InputFilterControl::open()
            .map_err(|e| CoreError::Backend(format!("input filter control open failed: {e}")))?;
        if ctl.is_present() {
            // Native device: push routes then arm (fail_open=0). Soft: bookkeeping only.
            ctl.set_routes(&table)
                .map_err(|e| CoreError::Backend(format!("set_routes failed: {e}")))?;
            ctl.set_fail_open(false)
                .map_err(|e| CoreError::Backend(format!("arm filter failed: {e}")))?;
            let token = RollbackToken::new(if ctl.is_soft() {
                "soft-input-routes"
            } else {
                "native-input-routes"
            });
            info!(
                token = %token.0,
                entries = table.entry_count,
                soft = ctl.is_soft(),
                "input routes applied via filter control"
            );
            self.active = Some(token.clone());
            return Ok(token);
        }

        if !input_soft_apply_enabled() {
            warn!(
                filter = self.probe.filter_installed,
                soft_env = INPUT_SOFT_APPLY_ENV,
                "native input apply refused (no control device; set soft apply env or install filter)"
            );
            return Err(CoreError::Backend(format!(
                "native input apply unavailable (fail-open retained); install filter or set MULTISEAT_INPUT_FILTER=1 / {INPUT_SOFT_APPLY_ENV}=1: {}",
                self.probe.detail()
            )));
        }
        // Lab: filter claimed via probe env but control open stayed absent — keep prior soft path.
        let token = RollbackToken::new("soft-input-routes");
        info!(
            token = %token.0,
            entries = table.entry_count,
            "SOFT native input apply (no control device; routes computed only)"
        );
        self.active = Some(token.clone());
        Ok(token)
    }

    fn rollback(&mut self, token: &RollbackToken) -> CoreResult<()> {
        if self.active.as_ref() == Some(token) {
            self.active = None;
        }
        // Fail-open: clear filter routes when possible.
        let _ = super::filter_control::emergency_fail_open();
        Ok(())
    }

    fn health(&self) -> CoreResult<BackendHealth> {
        if self.probe.fail_open_ready {
            Ok(BackendHealth::ok())
        } else {
            Ok(BackendHealth::down("fail-open path unhealthy"))
        }
    }

    fn force_fail_open(&mut self) {
        if self.active.take().is_some() {
            info!("native input force_fail_open cleared soft routes");
        }
        match crate::input::emergency_fail_open() {
            Ok(()) => {}
            Err(err) => warn!(error = %err, "input filter emergency_fail_open"),
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
        let _g = crate::test_env::lock();
        std::env::remove_var("MULTISEAT_INPUT_FILTER");
        std::env::remove_var(INPUT_SOFT_APPLY_ENV);
        let mut router = WindowsInputRouter::new(2, 2);
        let err = router.apply(&assignments()).unwrap_err();
        assert!(err.to_string().contains("native input"));
        assert!(router.health().unwrap().healthy);
        assert!(router.active.is_none());
    }

    #[test]
    fn soft_apply_when_filter_env_soft_control() {
        let _g = crate::test_env::lock();
        std::env::set_var("MULTISEAT_INPUT_FILTER", "1");
        std::env::remove_var(INPUT_SOFT_APPLY_ENV);
        let mut router = WindowsInputRouter::new(2, 2);
        let token = router.apply(&assignments()).unwrap();
        assert_eq!(token.0, "soft-input-routes");
        assert!(router.active.is_some());
        router.force_fail_open();
        assert!(router.active.is_none());
        std::env::remove_var("MULTISEAT_INPUT_FILTER");
    }
}

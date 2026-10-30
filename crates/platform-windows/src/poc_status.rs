//! Aggregated Stage 0 native PoC readiness for lab / `ctl NativeStatus`.

use serde::{Deserialize, Serialize};

use crate::display::{probe_display, DISPLAY_ATTACH_ENV, DISPLAY_BACKEND_ENV};
use crate::input::{probe_input, INPUT_SOFT_APPLY_ENV};
use crate::session::probe_dual_sessions;
use multiseat_core::{CheckStatus, DisplayProbeResult, DualSessionProbeResult, InputProbeResult};

/// Snapshot of native backend readiness and lab soft-path flags.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NativePocStatus {
    /// Host OS (`windows` / `macos` / …).
    pub host_os: String,
    /// Dual-session probe.
    pub session: DualSessionProbeResult,
    /// Display-model probe.
    pub display: DisplayProbeResult,
    /// Input-filter probe.
    pub input: InputProbeResult,
    /// Soft display attach would be accepted (`MULTISEAT_DISPLAY_ATTACH=1`).
    pub soft_display_attach: bool,
    /// Soft input apply would be accepted (`MULTISEAT_INPUT_FILTER=1` + soft apply).
    pub soft_input_apply: bool,
    /// Ordered next actions for the lab operator.
    pub next_actions: Vec<String>,
}

impl NativePocStatus {
    /// Collects probes for the given inventory kit/output counts.
    pub fn collect(physical_outputs: usize, keyboard_kits: usize, pointer_kits: usize) -> Self {
        let session = probe_dual_sessions();
        let display = probe_display(physical_outputs);
        let input = probe_input(keyboard_kits, pointer_kits);
        let soft_display_attach = display.attach_supported;
        let soft_input_apply = input.filter_installed && input_soft_apply_enabled();

        let mut next_actions = Vec::new();
        match session.status {
            CheckStatus::Pass => {}
            CheckStatus::Blocked | CheckStatus::Fail | CheckStatus::Unknown => {
                next_actions.push(
                    "dual_session: prove ≥2 active interactive local sessions via WTS (or document Blocked)"
                        .into(),
                );
            }
        }
        if !display.attach_supported {
            next_actions.push(format!(
                "display: install/bind {} PoC or set {DISPLAY_BACKEND_ENV}+{DISPLAY_ATTACH_ENV}=1 for soft bookkeeping attach",
                display.backend_name
            ));
        }
        if !input.filter_installed {
            next_actions.push(
                "input: install KMDF kb/mou filters (or MULTISEAT_INPUT_FILTER=1 for lab presence)"
                    .into(),
            );
        } else if !soft_input_apply {
            next_actions.push(format!(
                "input: set {INPUT_SOFT_APPLY_ENV}=1 to allow soft bookkeeping apply (no real filter IOCTL yet)"
            ));
        }
        if next_actions.is_empty() {
            next_actions.push(
                "native soft path ready for orchestrator Start bookkeeping; real IddCx/KMDF still pending"
                    .into(),
            );
        }

        Self {
            host_os: std::env::consts::OS.to_string(),
            session,
            display,
            input,
            soft_display_attach,
            soft_input_apply,
            next_actions,
        }
    }

    /// True when soft attach + soft apply are both armed (session still independent).
    pub fn soft_backends_armed(&self) -> bool {
        self.soft_display_attach && self.soft_input_apply
    }
}

/// Env enabling soft input apply after filter presence is claimed.
pub fn input_soft_apply_enabled() -> bool {
    std::env::var_os(INPUT_SOFT_APPLY_ENV).is_some_and(|v| v == "1")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn collect_lists_actions_when_blocked() {
        let _g = crate::test_env::lock();
        std::env::remove_var(DISPLAY_ATTACH_ENV);
        std::env::remove_var(DISPLAY_BACKEND_ENV);
        std::env::remove_var("MULTISEAT_INPUT_FILTER");
        std::env::remove_var(INPUT_SOFT_APPLY_ENV);
        let status = NativePocStatus::collect(2, 2, 2);
        assert!(!status.next_actions.is_empty());
        assert!(!status.soft_display_attach);
        assert!(!status.soft_input_apply);
    }

    #[test]
    fn soft_flags_follow_env() {
        let _g = crate::test_env::lock();
        std::env::set_var(DISPLAY_BACKEND_ENV, "iddcx");
        std::env::set_var(DISPLAY_ATTACH_ENV, "1");
        std::env::set_var("MULTISEAT_INPUT_FILTER", "1");
        std::env::set_var(INPUT_SOFT_APPLY_ENV, "1");
        let status = NativePocStatus::collect(2, 2, 2);
        assert!(status.soft_display_attach);
        assert!(status.soft_input_apply);
        assert!(status.soft_backends_armed());
        std::env::remove_var(DISPLAY_ATTACH_ENV);
        std::env::remove_var(DISPLAY_BACKEND_ENV);
        std::env::remove_var("MULTISEAT_INPUT_FILTER");
        std::env::remove_var(INPUT_SOFT_APPLY_ENV);
    }
}

//! Stage 0 go/no-go checklist models and inventory-derived scaffolding.

use serde::{Deserialize, Serialize};

use crate::hardware::{HardwareTree, InputDeviceKind, PlatformCapabilities};

/// Outcome of a single Stage 0 experiment item.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum CheckStatus {
    /// Experiment passed on the current host/lab run.
    Pass,
    /// Experiment failed; blocks production path when required.
    Fail,
    /// Not yet executed on physical lab hardware.
    Unknown,
    /// Cannot run with the current OS/tooling (documented blocker).
    Blocked,
}

/// Observed WTS / session connect state for Stage 0 evidence.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum SessionConnectState {
    /// Interactive and currently active.
    Active,
    /// Connected but not necessarily interactive console.
    Connected,
    /// User logged on but disconnected.
    Disconnected,
    /// Any other WTS state (idle, listen, shadow, …).
    Other(String),
}

/// One session row from the dual-session probe.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ObservedSession {
    /// Windows session id.
    pub session_id: u32,
    /// WinStation name (`Console`, `RDP-Tcp#…`, …).
    pub station_name: String,
    /// Connect state.
    pub state: SessionConnectState,
    /// True when station looks like the local console.
    pub is_console: bool,
}

/// Evidence produced by the Stage 0 dual-session harness.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DualSessionProbeResult {
    /// Enumerated sessions (best-effort).
    pub sessions: Vec<ObservedSession>,
    /// Count of sessions in [`SessionConnectState::Active`].
    pub active_count: usize,
    /// Count of active console-like sessions.
    pub active_console_count: usize,
    /// Whether a documented API can create a second local interactive session.
    pub documented_create_available: bool,
    /// Checklist status derived from this probe.
    pub status: CheckStatus,
    /// Human-readable evidence.
    detail: String,
}

impl DualSessionProbeResult {
    /// Evidence detail string.
    pub fn detail(&self) -> &str {
        &self.detail
    }

    /// Builds a result from enumerated sessions (no create attempt).
    ///
    /// ```
    /// use multiseat_core::{CheckStatus, DualSessionProbeResult, ObservedSession, SessionConnectState};
    ///
    /// let result = DualSessionProbeResult::from_sessions(
    ///     vec![ObservedSession {
    ///         session_id: 1,
    ///         station_name: "Console".into(),
    ///         state: SessionConnectState::Active,
    ///         is_console: true,
    ///     }],
    ///     false,
    /// );
    /// assert_eq!(result.status, CheckStatus::Blocked);
    /// assert_eq!(result.active_count, 1);
    /// ```
    pub fn from_sessions(
        sessions: Vec<ObservedSession>,
        documented_create_available: bool,
    ) -> Self {
        let active_count = sessions
            .iter()
            .filter(|s| s.state == SessionConnectState::Active)
            .count();
        let active_console_count = sessions
            .iter()
            .filter(|s| s.state == SessionConnectState::Active && s.is_console)
            .count();

        let (status, detail) = if active_count >= 2 {
            (
                CheckStatus::Pass,
                format!(
                    "observed {active_count} active sessions ({active_console_count} console-like); dual interactive presence confirmed"
                ),
            )
        } else if documented_create_available {
            (
                CheckStatus::Unknown,
                format!(
                    "active={active_count}; documented create path available but second session not yet verified"
                ),
            )
        } else {
            (
                CheckStatus::Blocked,
                format!(
                    "active={active_count} console_active={active_console_count}; no documented API to create a second local interactive session on this host class"
                ),
            )
        };

        Self {
            sessions,
            active_count,
            active_console_count,
            documented_create_available,
            status,
            detail,
        }
    }

    /// Non-Windows / unavailable probe placeholder.
    pub fn unavailable(reason: impl Into<String>) -> Self {
        Self {
            sessions: vec![],
            active_count: 0,
            active_console_count: 0,
            documented_create_available: false,
            status: CheckStatus::Blocked,
            detail: reason.into(),
        }
    }
}

/// Evidence from the Stage 0 display-model probe (IddCx / legacy candidates).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DisplayProbeResult {
    /// Candidate backend name (`iddcx`, `wddm-legacy`, `win7-legacy`, `none`).
    pub backend_name: String,
    /// Whether attach is actually implemented and ready for lab verification.
    pub attach_supported: bool,
    /// Attached physical/virtual outputs counted for the probe.
    pub physical_outputs: usize,
    /// Checklist status for `per_session_display`.
    pub status: CheckStatus,
    detail: String,
}

impl DisplayProbeResult {
    /// Evidence detail string.
    pub fn detail(&self) -> &str {
        &self.detail
    }

    /// Builds a display probe verdict from OS candidate + output count.
    ///
    /// ```
    /// use multiseat_core::{CheckStatus, DisplayProbeResult};
    ///
    /// let result = DisplayProbeResult::evaluate("iddcx", false, 2);
    /// assert_eq!(result.status, CheckStatus::Blocked);
    /// assert_eq!(result.backend_name, "iddcx");
    /// ```
    pub fn evaluate(
        backend_name: impl Into<String>,
        attach_supported: bool,
        physical_outputs: usize,
    ) -> Self {
        let backend_name = backend_name.into();
        let (status, detail) = if physical_outputs < 2 {
            (
                CheckStatus::Fail,
                format!(
                    "outputs={physical_outputs}; need ≥2 attached displays for independent seat content"
                ),
            )
        } else if backend_name == "none" {
            (
                CheckStatus::Blocked,
                format!(
                    "outputs={physical_outputs}; no display-model candidate for this OS"
                ),
            )
        } else if !attach_supported {
            (
                CheckStatus::Blocked,
                format!(
                    "outputs={physical_outputs}; candidate={backend_name}; driver/session-bind PoC not installed (IddCx alone does not bind a display to a local interactive session)"
                ),
            )
        } else {
            (
                CheckStatus::Unknown,
                format!(
                    "outputs={physical_outputs}; candidate={backend_name}; attach flagged — lab verification pending"
                ),
            )
        };

        Self {
            backend_name,
            attach_supported,
            physical_outputs,
            status,
            detail,
        }
    }

    /// Placeholder when the probe cannot run.
    pub fn unavailable(reason: impl Into<String>) -> Self {
        Self {
            backend_name: "none".into(),
            attach_supported: false,
            physical_outputs: 0,
            status: CheckStatus::Blocked,
            detail: reason.into(),
        }
    }
}

/// Evidence from the Stage 0 input-router / KMDF filter probe.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct InputProbeResult {
    /// True when a RustMultiseat (or lab) keyboard/mouse filter service is present.
    pub filter_installed: bool,
    /// True when the host is in fail-open (no active suppression) — the safe default.
    pub fail_open_ready: bool,
    /// Distinct keyboard instances counted for the probe.
    pub keyboard_kits: usize,
    /// Distinct pointer instances counted for the probe.
    pub pointer_kits: usize,
    /// Checklist status for `input_isolation`.
    pub status: CheckStatus,
    detail: String,
}

impl InputProbeResult {
    /// Evidence detail string.
    pub fn detail(&self) -> &str {
        &self.detail
    }

    /// Builds an input probe verdict.
    ///
    /// ```
    /// use multiseat_core::{CheckStatus, InputProbeResult};
    ///
    /// let result = InputProbeResult::evaluate(false, true, 2, 2);
    /// assert_eq!(result.status, CheckStatus::Blocked);
    /// assert!(result.fail_open_ready);
    /// ```
    pub fn evaluate(
        filter_installed: bool,
        fail_open_ready: bool,
        keyboard_kits: usize,
        pointer_kits: usize,
    ) -> Self {
        let (status, detail) = if keyboard_kits < 2 || pointer_kits < 2 {
            (
                CheckStatus::Fail,
                format!(
                    "keyboards={keyboard_kits} pointers={pointer_kits}; need ≥2 kits before isolation can be proven"
                ),
            )
        } else if !filter_installed {
            (
                CheckStatus::Blocked,
                format!(
                    "keyboards={keyboard_kits} pointers={pointer_kits}; KMDF input filter not installed (SendInput cannot route physical devices per session); fail_open={fail_open_ready}"
                ),
            )
        } else if !fail_open_ready {
            (
                CheckStatus::Fail,
                "filter present but fail-open path not healthy; host emergency input at risk"
                    .into(),
            )
        } else {
            (
                CheckStatus::Unknown,
                format!(
                    "keyboards={keyboard_kits} pointers={pointer_kits}; filter present and fail-open ready — cross-seat suppression lab verification pending"
                ),
            )
        };

        Self {
            filter_installed,
            fail_open_ready,
            keyboard_kits,
            pointer_kits,
            status,
            detail,
        }
    }

    /// Placeholder when the probe cannot run.
    pub fn unavailable(reason: impl Into<String>) -> Self {
        Self {
            filter_installed: false,
            fail_open_ready: true,
            keyboard_kits: 0,
            pointer_kits: 0,
            status: CheckStatus::Blocked,
            detail: reason.into(),
        }
    }
}

/// One Stage 0 checklist row.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FeasibilityCheck {
    /// Stable machine id used in reports and CI artifacts.
    pub id: String,
    /// Short human-readable title.
    pub title: String,
    /// Whether a Fail/Blocked result stops the production path.
    pub required: bool,
    /// Current status.
    pub status: CheckStatus,
    /// Notes / evidence / next action.
    pub detail: String,
}

/// Aggregated Stage 0 report for a host snapshot.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FeasibilityReport {
    /// Capabilities at report time.
    pub capabilities: PlatformCapabilities,
    /// Checklist rows in evaluation order.
    pub checks: Vec<FeasibilityCheck>,
    /// True when every required check is `Pass`.
    pub go: bool,
    /// Short summary for CLI/GUI.
    pub summary: String,
}

impl FeasibilityReport {
    /// Builds a scaffold report from inventory only (session check stays Unknown).
    ///
    /// ```
    /// use multiseat_core::{FeasibilityReport, HardwareTree, PlatformCapabilities};
    ///
    /// let tree = HardwareTree {
    ///     revision: 1,
    ///     gpus: vec![],
    ///     displays: vec![],
    ///     usb_roots: vec![],
    ///     input_devices: vec![],
    ///     capabilities: PlatformCapabilities {
    ///         os: "mock".into(),
    ///         os_version: "0".into(),
    ///         display_backend_available: false,
    ///         session_backend_available: false,
    ///         input_router_available: false,
    ///         secure_boot_expected: true,
    ///     },
    /// };
    /// let report = FeasibilityReport::from_hardware(&tree);
    /// assert!(!report.go);
    /// assert!(report.checks.iter().any(|c| c.id == "dual_session"));
    /// ```
    pub fn from_hardware(tree: &HardwareTree) -> Self {
        Self::from_hardware_and_probes(tree, None, None, None)
    }

    /// Builds a report merging inventory with optional dual-session probe evidence.
    pub fn from_hardware_and_session(
        tree: &HardwareTree,
        session: Option<&DualSessionProbeResult>,
    ) -> Self {
        Self::from_hardware_and_probes(tree, session, None, None)
    }

    /// Builds a report merging inventory with session, display, and input probes.
    pub fn from_hardware_and_probes(
        tree: &HardwareTree,
        session: Option<&DualSessionProbeResult>,
        display: Option<&DisplayProbeResult>,
        input: Option<&InputProbeResult>,
    ) -> Self {
        let caps = tree.capabilities.clone();
        let keyboards = tree
            .input_devices
            .iter()
            .filter(|d| d.kind == InputDeviceKind::Keyboard)
            .count();
        let pointers = tree
            .input_devices
            .iter()
            .filter(|d| d.kind == InputDeviceKind::Pointer)
            .count();
        let displays = tree.displays.iter().filter(|d| d.attached).count();

        let inventory_ready = keyboards >= 2 && pointers >= 2 && displays >= 2;
        let inventory_status = if inventory_ready {
            CheckStatus::Pass
        } else {
            CheckStatus::Fail
        };
        let inventory_detail = format!(
            "attached_displays={displays} keyboards={keyboards} pointers={} usb_roots={} gpus={}",
            pointers,
            tree.usb_roots.len(),
            tree.gpus.len()
        );

        let (dual_status, dual_detail) = match session {
            Some(probe) => (probe.status, probe.detail().to_string()),
            None => (
                CheckStatus::Unknown,
                "Requires Stage 0 session PoC on physical Windows lab".into(),
            ),
        };

        let (display_status, display_detail) = match display {
            Some(probe) => (probe.status, probe.detail().to_string()),
            None => {
                if caps.display_backend_available {
                    (
                        CheckStatus::Unknown,
                        "Display backend flagged available; lab verification pending".into(),
                    )
                } else {
                    (
                        CheckStatus::Blocked,
                        "No display backend wired yet (IddCx/legacy PoC pending)".into(),
                    )
                }
            }
        };

        let checks = vec![
            FeasibilityCheck {
                id: "inventory_baseline".into(),
                title: "Hardware inventory sees ≥2 displays and ≥2 keyboard+pointer kits".into(),
                required: true,
                status: inventory_status,
                detail: inventory_detail,
            },
            FeasibilityCheck {
                id: "dual_session".into(),
                title: "Two simultaneous interactive local sessions".into(),
                required: true,
                status: dual_status,
                detail: dual_detail,
            },
            FeasibilityCheck {
                id: "per_session_display".into(),
                title: "Independent dynamic content on assigned displays".into(),
                required: true,
                status: display_status,
                detail: display_detail,
            },
            FeasibilityCheck {
                id: "input_device_identity".into(),
                title: "Distinguish two keyboard+mouse kits by device instance".into(),
                required: true,
                status: if keyboards >= 2 && pointers >= 2 {
                    CheckStatus::Pass
                } else {
                    CheckStatus::Fail
                },
                detail: format!("distinct HID instances keyboards={keyboards} pointers={pointers}"),
            },
            FeasibilityCheck {
                id: "input_isolation".into(),
                title: "Suppress cross-seat input and deliver only to target session".into(),
                required: true,
                status: match input {
                    Some(probe) => probe.status,
                    None if caps.input_router_available => CheckStatus::Unknown,
                    None => CheckStatus::Blocked,
                },
                detail: match input {
                    Some(probe) => probe.detail().to_string(),
                    None => "KMDF filter / virtual HID PoC not installed".into(),
                },
            },
            FeasibilityCheck {
                id: "secure_desktop".into(),
                title: "Login, lock/UAC/Secure Desktop, CAD, sleep/resume, FUS, hot-plug".into(),
                required: true,
                status: CheckStatus::Unknown,
                detail: "Manual lab matrix; not auto-verifiable from inventory".into(),
            },
            FeasibilityCheck {
                id: "fail_open".into(),
                title: "Crash of service/agent/driver restores host emergency input".into(),
                required: true,
                status: match input {
                    Some(probe) if probe.fail_open_ready && !probe.filter_installed => {
                        // Safe default: no filter means no suppression to unwind.
                        CheckStatus::Unknown
                    }
                    Some(probe) if probe.fail_open_ready && probe.filter_installed => {
                        CheckStatus::Unknown
                    }
                    Some(_) => CheckStatus::Fail,
                    None => CheckStatus::Unknown,
                },
                detail: match input {
                    Some(probe) => format!(
                        "fail_open_ready={} filter_installed={}; watchdog/crash injection lab pending",
                        probe.fail_open_ready, probe.filter_installed
                    ),
                    None => {
                        "Watchdog/fail-open path not implemented for real backends".into()
                    }
                },
            },
            FeasibilityCheck {
                id: "signing_path".into(),
                title: "Licensed install/signing path without disabling Secure Boot".into(),
                required: true,
                status: if caps.secure_boot_expected {
                    CheckStatus::Unknown
                } else {
                    CheckStatus::Blocked
                },
                detail: "WHCP/HCK / attestation policy must be proven per OS generation".into(),
            },
        ];

        let go = checks
            .iter()
            .filter(|c| c.required)
            .all(|c| c.status == CheckStatus::Pass);
        let passed = checks
            .iter()
            .filter(|c| c.status == CheckStatus::Pass)
            .count();
        let failed = checks
            .iter()
            .filter(|c| c.status == CheckStatus::Fail)
            .count();
        let blocked = checks
            .iter()
            .filter(|c| c.status == CheckStatus::Blocked)
            .count();
        let unknown = checks
            .iter()
            .filter(|c| c.status == CheckStatus::Unknown)
            .count();

        let summary = if go {
            format!("GO: all {passed} required checks passed")
        } else {
            format!(
                "NO-GO: pass={passed} fail={failed} blocked={blocked} unknown={unknown} (os={} {})",
                caps.os, caps.os_version
            )
        };

        Self {
            capabilities: caps,
            checks,
            go,
            summary,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::hardware::{DisplayInfo, GpuInfo, InputDeviceInfo};
    use crate::ids::{DeviceId, DisplayId};

    fn tree_with(
        displays: usize,
        keyboards: usize,
        pointers: usize,
    ) -> HardwareTree {
        let mut tree = HardwareTree {
            revision: 1,
            gpus: vec![GpuInfo {
                id: "gpu-0".into(),
                description: "t".into(),
                vendor_id: None,
                device_id: None,
            }],
            displays: (0..displays)
                .map(|i| DisplayInfo {
                    id: DisplayId(format!("d{i}")),
                    gpu_id: "gpu-0".into(),
                    width: Some(1920),
                    height: Some(1080),
                    attached: true,
                    virtual_display: false,
                })
                .collect(),
            usb_roots: vec![],
            input_devices: Vec::new(),
            capabilities: PlatformCapabilities {
                os: "test".into(),
                os_version: "1".into(),
                display_backend_available: false,
                session_backend_available: false,
                input_router_available: false,
                secure_boot_expected: true,
            },
        };
        for i in 0..keyboards {
            tree.input_devices.push(InputDeviceInfo {
                id: DeviceId::from_instance(format!("kbd{i}")),
                kind: InputDeviceKind::Keyboard,
                name: format!("kbd{i}"),
                vendor_id: None,
                product_id: None,
            });
        }
        for i in 0..pointers {
            tree.input_devices.push(InputDeviceInfo {
                id: DeviceId::from_instance(format!("ptr{i}")),
                kind: InputDeviceKind::Pointer,
                name: format!("ptr{i}"),
                vendor_id: None,
                product_id: None,
            });
        }
        tree
    }

    #[test]
    fn inventory_fail_when_kits_missing() {
        let report = FeasibilityReport::from_hardware(&tree_with(1, 1, 1));
        assert!(!report.go);
        let inv = report
            .checks
            .iter()
            .find(|c| c.id == "inventory_baseline")
            .unwrap();
        assert_eq!(inv.status, CheckStatus::Fail);
    }

    #[test]
    fn inventory_pass_still_no_go_without_session_poc() {
        let report = FeasibilityReport::from_hardware(&tree_with(2, 2, 2));
        let inv = report
            .checks
            .iter()
            .find(|c| c.id == "inventory_baseline")
            .unwrap();
        assert_eq!(inv.status, CheckStatus::Pass);
        assert!(!report.go);
        assert!(report.summary.starts_with("NO-GO"));
    }

    #[test]
    fn dual_session_probe_blocks_single_active() {
        let probe = DualSessionProbeResult::from_sessions(
            vec![ObservedSession {
                session_id: 1,
                station_name: "Console".into(),
                state: SessionConnectState::Active,
                is_console: true,
            }],
            false,
        );
        let report =
            FeasibilityReport::from_hardware_and_session(&tree_with(2, 2, 2), Some(&probe));
        let dual = report
            .checks
            .iter()
            .find(|c| c.id == "dual_session")
            .unwrap();
        assert_eq!(dual.status, CheckStatus::Blocked);
        assert!(!report.go);
    }

    #[test]
    fn dual_session_probe_passes_two_active() {
        let probe = DualSessionProbeResult::from_sessions(
            vec![
                ObservedSession {
                    session_id: 1,
                    station_name: "Console".into(),
                    state: SessionConnectState::Active,
                    is_console: true,
                },
                ObservedSession {
                    session_id: 2,
                    station_name: "Console".into(),
                    state: SessionConnectState::Active,
                    is_console: true,
                },
            ],
            false,
        );
        assert_eq!(probe.status, CheckStatus::Pass);
    }

    #[test]
    fn display_probe_blocks_without_attach() {
        let probe = DisplayProbeResult::evaluate("iddcx", false, 2);
        let report = FeasibilityReport::from_hardware_and_probes(
            &tree_with(2, 2, 2),
            None,
            Some(&probe),
            None,
        );
        let row = report
            .checks
            .iter()
            .find(|c| c.id == "per_session_display")
            .unwrap();
        assert_eq!(row.status, CheckStatus::Blocked);
    }

    #[test]
    fn input_probe_blocks_without_filter() {
        let probe = InputProbeResult::evaluate(false, true, 2, 2);
        let report = FeasibilityReport::from_hardware_and_probes(
            &tree_with(2, 2, 2),
            None,
            None,
            Some(&probe),
        );
        let row = report
            .checks
            .iter()
            .find(|c| c.id == "input_isolation")
            .unwrap();
        assert_eq!(row.status, CheckStatus::Blocked);
    }
}

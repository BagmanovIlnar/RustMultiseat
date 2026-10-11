//! Configuration validation rules shared by daemon and GUI.

use serde::{Deserialize, Serialize};
use std::collections::HashSet;

use crate::hardware::{HardwareTree, InputDeviceKind};
use crate::ids::DeviceId;
use crate::seat::{MultiseatConfig, SeatConfig};

/// Severity of a validation finding.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ValidationSeverity {
    /// Blocks start.
    Error,
    /// Does not block start, but should be shown.
    Warning,
}

/// One validation finding.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ValidationIssue {
    /// Severity.
    pub severity: ValidationSeverity,
    /// Stable machine-readable code.
    pub code: String,
    /// Human-readable message.
    pub message: String,
}

/// Aggregate validation report.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct ValidationReport {
    /// Findings.
    pub issues: Vec<ValidationIssue>,
}

impl ValidationReport {
    /// Empty successful report.
    pub fn ok() -> Self {
        Self::default()
    }

    /// Whether any error-severity issue exists.
    pub fn has_errors(&self) -> bool {
        self.issues
            .iter()
            .any(|i| i.severity == ValidationSeverity::Error)
    }

    /// Appends an error.
    pub fn error(&mut self, code: impl Into<String>, message: impl Into<String>) {
        self.issues.push(ValidationIssue {
            severity: ValidationSeverity::Error,
            code: code.into(),
            message: message.into(),
        });
    }

    /// Appends a warning.
    pub fn warning(&mut self, code: impl Into<String>, message: impl Into<String>) {
        self.issues.push(ValidationIssue {
            severity: ValidationSeverity::Warning,
            code: code.into(),
            message: message.into(),
        });
    }
}

/// Validates a multiseat configuration against a hardware snapshot.
pub fn validate_config(config: &MultiseatConfig, hardware: &HardwareTree) -> ValidationReport {
    let mut report = ValidationReport::ok();

    if config.emergency_keyboard.is_none() || config.emergency_pointer.is_none() {
        report.error(
            "emergency_input_required",
            "configuration must reserve emergency keyboard and pointer for the host",
        );
    }

    if let Some(kbd) = &config.emergency_keyboard {
        check_input_kind(
            &mut report,
            hardware,
            kbd,
            InputDeviceKind::Keyboard,
            "emergency_keyboard",
        );
    }
    if let Some(ptr) = &config.emergency_pointer {
        check_input_kind(
            &mut report,
            hardware,
            ptr,
            InputDeviceKind::Pointer,
            "emergency_pointer",
        );
    }

    let enabled: Vec<&SeatConfig> = config.seats.iter().filter(|s| s.enabled).collect();
    if enabled.is_empty() {
        report.error("no_enabled_seats", "at least one enabled seat is required");
    }

    let mut seat_ids = HashSet::new();
    let mut used_devices: HashSet<&DeviceId> = HashSet::new();
    let mut used_displays = HashSet::new();

    if let Some(kbd) = &config.emergency_keyboard {
        used_devices.insert(kbd);
    }
    if let Some(ptr) = &config.emergency_pointer {
        used_devices.insert(ptr);
    }

    for seat in &enabled {
        if !seat_ids.insert(seat.id) {
            report.error(
                "duplicate_seat_id",
                format!("duplicate seat id {}", seat.id),
            );
        }

        if seat.user_sid.0.trim().is_empty() {
            report.error(
                "empty_user_sid",
                format!("seat {} has empty user SID", seat.id),
            );
        }

        if seat.display_ids.is_empty() {
            report.error(
                "seat_needs_display",
                format!("seat {} needs at least one display", seat.id),
            );
        }
        if seat.keyboard_ids.is_empty() || seat.pointer_ids.is_empty() {
            report.error(
                "seat_needs_input",
                format!("seat {} needs keyboard and pointer", seat.id),
            );
        }

        for display in &seat.display_ids {
            if !hardware.contains_display(display) {
                report.error(
                    "unknown_display",
                    format!("seat {} references unknown display {}", seat.id, display),
                );
            }
            if !used_displays.insert(display) {
                report.error(
                    "display_conflict",
                    format!("display {} assigned to multiple seats", display),
                );
            }
        }

        for kbd in &seat.keyboard_ids {
            check_input_kind(
                &mut report,
                hardware,
                kbd,
                InputDeviceKind::Keyboard,
                &format!("seat_{}_keyboard", seat.id.0),
            );
            if !used_devices.insert(kbd) {
                report.error(
                    "device_conflict",
                    format!("device {} assigned more than once", kbd),
                );
            }
        }

        for ptr in &seat.pointer_ids {
            check_input_kind(
                &mut report,
                hardware,
                ptr,
                InputDeviceKind::Pointer,
                &format!("seat_{}_pointer", seat.id.0),
            );
            if !used_devices.insert(ptr) {
                report.error(
                    "device_conflict",
                    format!("device {} assigned more than once", ptr),
                );
            }
        }
    }

    if !hardware.capabilities.session_backend_available {
        report.warning(
            "session_backend_unavailable",
            "session backend is not available on this host; start will fail until PoC succeeds",
        );
    }
    if !hardware.capabilities.display_backend_available {
        report.warning(
            "display_backend_unavailable",
            "display backend is not available on this host",
        );
    }
    if !hardware.capabilities.input_router_available {
        report.warning(
            "input_router_unavailable",
            "input router is not available on this host",
        );
    }

    report
}

fn check_input_kind(
    report: &mut ValidationReport,
    hardware: &HardwareTree,
    id: &DeviceId,
    expected: InputDeviceKind,
    context: &str,
) {
    match hardware.input_device(id) {
        None => report.error(
            "unknown_device",
            format!("{context}: device {id} not found in hardware snapshot"),
        ),
        Some(dev) if dev.kind != expected => report.error(
            "wrong_device_kind",
            format!(
                "{context}: device {id} is {:?}, expected {:?}",
                dev.kind, expected
            ),
        ),
        Some(_) => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::hardware::{DisplayInfo, InputDeviceInfo, InputDeviceKind, PlatformCapabilities};
    use crate::ids::{DisplayId, SeatId, UserSid};

    fn sample_hardware() -> HardwareTree {
        HardwareTree {
            revision: 1,
            gpus: vec![],
            displays: vec![
                DisplayInfo {
                    id: DisplayId("d1".into()),
                    gpu_id: "gpu0".into(),
                    width: Some(1920),
                    height: Some(1080),
                    attached: true,
                    virtual_display: false,
                },
                DisplayInfo {
                    id: DisplayId("d2".into()),
                    gpu_id: "gpu0".into(),
                    width: Some(1920),
                    height: Some(1080),
                    attached: true,
                    virtual_display: false,
                },
            ],
            usb_roots: vec![],
            input_devices: vec![
                InputDeviceInfo {
                    id: DeviceId::from_instance("kbd-host"),
                    kind: InputDeviceKind::Keyboard,
                    name: "Host Kbd".into(),
                    vendor_id: None,
                    product_id: None,
                },
                InputDeviceInfo {
                    id: DeviceId::from_instance("ptr-host"),
                    kind: InputDeviceKind::Pointer,
                    name: "Host Ptr".into(),
                    vendor_id: None,
                    product_id: None,
                },
                InputDeviceInfo {
                    id: DeviceId::from_instance("kbd-1"),
                    kind: InputDeviceKind::Keyboard,
                    name: "Seat1 Kbd".into(),
                    vendor_id: None,
                    product_id: None,
                },
                InputDeviceInfo {
                    id: DeviceId::from_instance("ptr-1"),
                    kind: InputDeviceKind::Pointer,
                    name: "Seat1 Ptr".into(),
                    vendor_id: None,
                    product_id: None,
                },
            ],
            capabilities: PlatformCapabilities {
                os: "windows".into(),
                os_version: "mock".into(),
                display_backend_available: true,
                session_backend_available: true,
                input_router_available: true,
                secure_boot_expected: true,
            },
        }
    }

    fn valid_config() -> MultiseatConfig {
        MultiseatConfig {
            seats: vec![SeatConfig {
                id: SeatId(1),
                user_sid: UserSid("S-1-5-21-1".into()),
                display_ids: vec![DisplayId("d1".into())],
                keyboard_ids: vec![DeviceId::from_instance("kbd-1")],
                pointer_ids: vec![DeviceId::from_instance("ptr-1")],
                enabled: true,
            }],
            emergency_keyboard: Some(DeviceId::from_instance("kbd-host")),
            emergency_pointer: Some(DeviceId::from_instance("ptr-host")),
        }
    }

    #[test]
    fn accepts_valid_config() {
        let report = validate_config(&valid_config(), &sample_hardware());
        assert!(!report.has_errors(), "{report:?}");
    }

    #[test]
    fn rejects_missing_emergency_input() {
        let mut cfg = valid_config();
        cfg.emergency_keyboard = None;
        let report = validate_config(&cfg, &sample_hardware());
        assert!(report.has_errors());
        assert!(report
            .issues
            .iter()
            .any(|i| i.code == "emergency_input_required"));
    }

    #[test]
    fn rejects_device_conflict_with_emergency() {
        let mut cfg = valid_config();
        cfg.seats[0].keyboard_ids = vec![DeviceId::from_instance("kbd-host")];
        let report = validate_config(&cfg, &sample_hardware());
        assert!(report.issues.iter().any(|i| i.code == "device_conflict"));
    }
}

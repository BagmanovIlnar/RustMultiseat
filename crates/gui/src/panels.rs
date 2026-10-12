//! Dashboard column widgets (hardware, seats, validation, feasibility).

use iced::widget::{column, text, Column};
use multiseat_core::{
    CheckStatus, FeasibilityReport, HardwareTree, MultiseatConfig, ValidationReport,
    ValidationSeverity,
};

use crate::Message;

/// Hardware inventory column.
pub fn device_list(hardware: &HardwareTree) -> Column<'_, Message> {
    let mut col = column![].spacing(4);
    if hardware.input_devices.is_empty() && hardware.displays.is_empty() {
        return col.push(text("No hardware loaded yet."));
    }
    col = col.push(
        text(format!(
            "rev {} · caps os={} display={} session={} input={}",
            hardware.revision,
            hardware.capabilities.os,
            hardware.capabilities.display_backend_available,
            hardware.capabilities.session_backend_available,
            hardware.capabilities.input_router_available,
        ))
        .size(12),
    );
    for gpu in &hardware.gpus {
        col = col.push(text(format!("GPU: {}", gpu.description)).size(14));
    }
    for display in &hardware.displays {
        col = col.push(text(format!(
            "Display {} ({}x{}) attached={}",
            display.id,
            display.width.unwrap_or(0),
            display.height.unwrap_or(0),
            display.attached
        )));
    }
    for device in &hardware.input_devices {
        col = col.push(text(format!("{:?} · {}", device.kind, device.name)));
    }
    for root in &hardware.usb_roots {
        col = col.push(text(format!("USB root: {}", root.description)));
        for child in &root.children {
            col = col.push(text(format!(
                "  hub port {:?}: {}",
                child.port, child.description
            )));
        }
    }
    col
}

/// Saved seats / emergency devices column.
pub fn seat_list(config: &MultiseatConfig) -> Column<'_, Message> {
    let mut col = column![].spacing(4);
    if let Some(kbd) = &config.emergency_keyboard {
        col = col.push(text(format!("Emergency kbd: {kbd}")));
    }
    if let Some(ptr) = &config.emergency_pointer {
        col = col.push(text(format!("Emergency ptr: {ptr}")));
    }
    if config.seats.is_empty() {
        col = col.push(text("No seats in saved config (Validate/Start will use lab sample)."));
    }
    for seat in &config.seats {
        col = col.push(text(format!(
            "{} enabled={} user={} displays={} kbd={} ptr={}",
            seat.id,
            seat.enabled,
            seat.user_sid,
            seat.display_ids.len(),
            seat.keyboard_ids.len(),
            seat.pointer_ids.len()
        )));
    }
    col
}

/// Last validation report column.
pub fn validation_list(report: Option<&ValidationReport>) -> Column<'_, Message> {
    let mut col = column![].spacing(4);
    match report {
        None => col = col.push(text("No validation run yet.")),
        Some(report) if report.issues.is_empty() => col = col.push(text("No issues.")),
        Some(report) => {
            for issue in &report.issues {
                let mark = match issue.severity {
                    ValidationSeverity::Error => "ERR",
                    ValidationSeverity::Warning => "WARN",
                };
                col = col.push(text(format!("{mark} {}: {}", issue.code, issue.message)));
            }
        }
    }
    col
}

/// Stage 0 feasibility checklist column.
pub fn feasibility_list(report: Option<&FeasibilityReport>) -> Column<'_, Message> {
    let mut col = column![].spacing(4);
    match report {
        None => col = col.push(text("No feasibility report yet. Press Feasibility.")),
        Some(report) => {
            col = col.push(text(format!(
                "go={} · {}",
                report.go,
                report.summary
            )));
            col = col.push(text(format!(
                "caps: os={} {} secure_boot_expected={}",
                report.capabilities.os,
                report.capabilities.os_version,
                report.capabilities.secure_boot_expected
            )).size(12));
            for check in &report.checks {
                let mark = match check.status {
                    CheckStatus::Pass => "PASS",
                    CheckStatus::Fail => "FAIL",
                    CheckStatus::Unknown => "????",
                    CheckStatus::Blocked => "BLOCK",
                };
                let req = if check.required { "*" } else { " " };
                col = col.push(text(format!(
                    "{mark}{req} {}: {}",
                    check.id, check.title
                )));
                if !check.detail.is_empty() {
                    col = col.push(text(format!("    {}", check.detail)).size(12));
                }
            }
        }
    }
    col
}

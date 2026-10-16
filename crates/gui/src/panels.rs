//! Dashboard column widgets (hardware pickers, seats drop zones, reports).

use iced::widget::{button, column, container, row, text, Column};
use iced::Length;
use multiseat_core::{
    CheckStatus, FeasibilityReport, HardwareTree, MultiseatConfig, ValidationReport,
    ValidationSeverity,
};

use crate::assignment::{Assignable, DragState};
use crate::Message;

/// Assignable hardware list: click picks a drag source.
pub fn device_list<'a>(
    hardware: &'a HardwareTree,
    drag: Option<&'a DragState>,
) -> Column<'a, Message> {
    let mut col = column![].spacing(4);
    if hardware.input_devices.is_empty() && hardware.displays.is_empty() {
        return col.push(text("No hardware loaded yet."));
    }
    col = col.push(
        text(format!(
            "rev {} · click a device, then a seat / Emergency",
            hardware.revision
        ))
        .size(12),
    );
    if let Some(drag) = drag {
        col = col.push(text(format!("Picked: {}", drag.item.label())).size(14));
        col = col.push(button("Cancel pick").on_press(Message::CancelDrag));
    }

    for gpu in &hardware.gpus {
        col = col.push(text(format!("GPU: {}", gpu.description)).size(14));
    }
    for display in &hardware.displays {
        let item = Assignable::Display(display.id.clone());
        let label = format!(
            "Display {} ({}x{})",
            display.id,
            display.width.unwrap_or(0),
            display.height.unwrap_or(0)
        );
        col = col.push(pick_button(label, item, drag));
    }
    for device in &hardware.input_devices {
        if let Some(item) = Assignable::from_input(device.kind, device.id.clone()) {
            let label = format!("{:?} · {}", device.kind, device.name);
            col = col.push(pick_button(label, item, drag));
        } else {
            col = col.push(text(format!("Other · {}", device.name)).size(12));
        }
    }
    for root in &hardware.usb_roots {
        col = col.push(text(format!("USB root: {}", root.description)).size(12));
        for child in &root.children {
            col = col.push(
                text(format!("  hub port {:?}: {}", child.port, child.description)).size(12),
            );
        }
    }
    col
}

fn pick_button<'a>(
    label: String,
    item: Assignable,
    drag: Option<&'a DragState>,
) -> iced::widget::Button<'a, Message> {
    let selected = drag.map(|d| &d.item) == Some(&item);
    let prefix = if selected { "● " } else { "○ " };
    button(text(format!("{prefix}{label}")))
        .on_press(Message::DragStarted(item))
        .width(Length::Fill)
}

/// Draft seats as drop targets + clear controls.
pub fn seat_editor<'a>(
    draft: &'a MultiseatConfig,
    drag: Option<&'a DragState>,
) -> Column<'a, Message> {
    let mut col = column![].spacing(8);
    col = col.push(
        row![
            button("Add seat").on_press(Message::AddSeat),
            button("Load sample → draft").on_press(Message::LoadSampleDraft),
            button("Reset draft").on_press(Message::ResetDraft),
        ]
        .spacing(8),
    );

    col = col.push(emergency_zone(draft, drag));

    if draft.seats.is_empty() {
        col = col.push(text("No seats in draft — Add seat or Load sample."));
    }

    for seat in &draft.seats {
        col = col.push(seat_drop_zone(seat, drag));
    }
    col
}

fn emergency_zone<'a>(
    draft: &'a MultiseatConfig,
    drag: Option<&'a DragState>,
) -> iced::widget::Container<'a, Message> {
    let mut inner = column![text("Emergency (host)").size(16)].spacing(4);
    if drag.is_some() {
        inner = inner.push(button("Drop here (emergency)").on_press(Message::DroppedOnEmergency));
    }
    match &draft.emergency_keyboard {
        Some(id) => {
            inner = inner.push(
                row![
                    text(format!("kbd {id}")),
                    button("x").on_press(Message::ClearEmergencyKeyboard),
                ]
                .spacing(8),
            );
        }
        None => inner = inner.push(text("kbd: (none)").size(12)),
    }
    match &draft.emergency_pointer {
        Some(id) => {
            inner = inner.push(
                row![
                    text(format!("ptr {id}")),
                    button("x").on_press(Message::ClearEmergencyPointer),
                ]
                .spacing(8),
            );
        }
        None => inner = inner.push(text("ptr: (none)").size(12)),
    }
    container(inner.padding(8)).width(Length::Fill)
}

fn seat_drop_zone<'a>(
    seat: &'a multiseat_core::SeatConfig,
    drag: Option<&'a DragState>,
) -> iced::widget::Container<'a, Message> {
    let seat_id = seat.id;
    let mut inner = column![text(format!("{} · {}", seat.id, seat.user_sid)).size(16)].spacing(4);
    if drag.is_some() {
        inner = inner.push(
            button(text(format!("Drop on {seat_id}")))
                .on_press(Message::DroppedOnSeat(seat_id))
                .width(Length::Fill),
        );
    }

    if seat.display_ids.is_empty() {
        inner = inner.push(text("displays: (none)").size(12));
    }
    for id in &seat.display_ids {
        let id = id.clone();
        inner = inner.push(
            row![
                text(format!("display {id}")),
                button("x").on_press(Message::ClearSeatDisplay(seat_id, id)),
            ]
            .spacing(8),
        );
    }
    if seat.keyboard_ids.is_empty() {
        inner = inner.push(text("keyboards: (none)").size(12));
    }
    for id in &seat.keyboard_ids {
        let id = id.clone();
        inner = inner.push(
            row![
                text(format!("kbd {id}")),
                button("x").on_press(Message::ClearSeatKeyboard(seat_id, id)),
            ]
            .spacing(8),
        );
    }
    if seat.pointer_ids.is_empty() {
        inner = inner.push(text("pointers: (none)").size(12));
    }
    for id in &seat.pointer_ids {
        let id = id.clone();
        inner = inner.push(
            row![
                text(format!("ptr {id}")),
                button("x").on_press(Message::ClearSeatPointer(seat_id, id)),
            ]
            .spacing(8),
        );
    }

    container(inner.padding(8)).width(Length::Fill)
}

/// Last validation report column (daemon or local preview).
pub fn validation_list(report: Option<&ValidationReport>, preview: bool) -> Column<'_, Message> {
    let mut col = column![].spacing(4);
    let title = if preview {
        "Local preview (not yet sent)"
    } else {
        "Last daemon validation"
    };
    col = col.push(text(title).size(12));
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
            col = col.push(text(format!("go={} · {}", report.go, report.summary)));
            col = col.push(
                text(format!(
                    "caps: os={} {} secure_boot_expected={}",
                    report.capabilities.os,
                    report.capabilities.os_version,
                    report.capabilities.secure_boot_expected
                ))
                .size(12),
            );
            for check in &report.checks {
                let mark = match check.status {
                    CheckStatus::Pass => "PASS",
                    CheckStatus::Fail => "FAIL",
                    CheckStatus::Unknown => "????",
                    CheckStatus::Blocked => "BLOCK",
                };
                let req = if check.required { "*" } else { " " };
                col = col.push(text(format!("{mark}{req} {}: {}", check.id, check.title)));
                if !check.detail.is_empty() {
                    col = col.push(text(format!("    {}", check.detail)).size(12));
                }
            }
        }
    }
    col
}

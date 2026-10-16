//! Draft seat assignment helpers (GUI-side DnD / click-to-assign).
//!
//! Drop targets mutate only the local draft config. Daemon validation runs
//! afterward via IPC; local [`validate_config`] is used for instant preview.

use multiseat_core::{
    validate_config, DeviceId, DisplayId, HardwareTree, InputDeviceKind, MultiseatConfig,
    SeatConfig, SeatId, UserSid, ValidationReport,
};

/// Something the user can drag from the hardware list.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Assignable {
    /// Physical/virtual display output.
    Display(DisplayId),
    /// Keyboard (or keypad) input device.
    Keyboard(DeviceId),
    /// Pointer / mouse input device.
    Pointer(DeviceId),
}

impl Assignable {
    /// Short label for status / buttons.
    pub fn label(&self) -> String {
        match self {
            Self::Display(id) => format!("display {id}"),
            Self::Keyboard(id) => format!("kbd {id}"),
            Self::Pointer(id) => format!("ptr {id}"),
        }
    }

    /// Builds an assignable from inventory rows.
    pub fn from_input(kind: InputDeviceKind, id: DeviceId) -> Option<Self> {
        match kind {
            InputDeviceKind::Keyboard => Some(Self::Keyboard(id)),
            InputDeviceKind::Pointer => Some(Self::Pointer(id)),
            InputDeviceKind::Other => None,
        }
    }
}

/// In-progress pick (click-DnD; pointer-capture DnD can reuse the same type).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DragState {
    /// Source item.
    pub item: Assignable,
}

/// Ensures `draft` has at least one enabled seat for editing.
pub fn ensure_editable_seat(draft: &mut MultiseatConfig) {
    if draft.seats.is_empty() {
        draft.seats.push(empty_seat(SeatId(1)));
    }
}

/// Creates an empty seat shell.
pub fn empty_seat(id: SeatId) -> SeatConfig {
    SeatConfig {
        id,
        user_sid: UserSid(format!("S-1-5-21-draft-{}", id.0)),
        display_ids: Vec::new(),
        keyboard_ids: Vec::new(),
        pointer_ids: Vec::new(),
        enabled: true,
    }
}

/// Appends the next seat id (`max+1`).
pub fn add_seat(draft: &mut MultiseatConfig) -> SeatId {
    let next = draft.seats.iter().map(|s| s.id.0).max().unwrap_or(0) + 1;
    let id = SeatId(next);
    draft.seats.push(empty_seat(id));
    id
}

/// Removes an item from every seat and emergency slots (exclusive ownership).
pub fn detach_everywhere(draft: &mut MultiseatConfig, item: &Assignable) {
    match item {
        Assignable::Display(id) => {
            for seat in &mut draft.seats {
                seat.display_ids.retain(|d| d != id);
            }
        }
        Assignable::Keyboard(id) => {
            for seat in &mut draft.seats {
                seat.keyboard_ids.retain(|d| d != id);
            }
            if draft.emergency_keyboard.as_ref() == Some(id) {
                draft.emergency_keyboard = None;
            }
        }
        Assignable::Pointer(id) => {
            for seat in &mut draft.seats {
                seat.pointer_ids.retain(|d| d != id);
            }
            if draft.emergency_pointer.as_ref() == Some(id) {
                draft.emergency_pointer = None;
            }
        }
    }
}

/// Assigns `item` to `seat_id`, moving it off any previous owner.
pub fn assign_to_seat(
    draft: &mut MultiseatConfig,
    seat_id: SeatId,
    item: Assignable,
) -> Result<(), String> {
    ensure_editable_seat(draft);
    if !draft.seats.iter().any(|s| s.id == seat_id) {
        return Err(format!("unknown seat {seat_id}"));
    }
    detach_everywhere(draft, &item);
    let seat = draft
        .seats
        .iter_mut()
        .find(|s| s.id == seat_id)
        .ok_or_else(|| format!("unknown seat {seat_id}"))?;
    match item {
        Assignable::Display(id) => {
            if !seat.display_ids.contains(&id) {
                seat.display_ids.push(id);
            }
        }
        Assignable::Keyboard(id) => {
            if !seat.keyboard_ids.contains(&id) {
                seat.keyboard_ids.push(id);
            }
        }
        Assignable::Pointer(id) => {
            if !seat.pointer_ids.contains(&id) {
                seat.pointer_ids.push(id);
            }
        }
    }
    Ok(())
}

/// Assigns a keyboard/pointer to the host emergency slots.
pub fn assign_emergency(draft: &mut MultiseatConfig, item: Assignable) -> Result<(), String> {
    match &item {
        Assignable::Display(_) => {
            return Err("displays cannot be emergency devices".into());
        }
        Assignable::Keyboard(_) | Assignable::Pointer(_) => {}
    }
    detach_everywhere(draft, &item);
    match item {
        Assignable::Keyboard(id) => draft.emergency_keyboard = Some(id),
        Assignable::Pointer(id) => draft.emergency_pointer = Some(id),
        Assignable::Display(_) => unreachable!(),
    }
    Ok(())
}

/// Removes one assigned id from a seat (does not touch emergency).
pub fn clear_seat_display(draft: &mut MultiseatConfig, seat_id: SeatId, id: &DisplayId) {
    if let Some(seat) = draft.seats.iter_mut().find(|s| s.id == seat_id) {
        seat.display_ids.retain(|d| d != id);
    }
}

/// Removes one keyboard from a seat.
pub fn clear_seat_keyboard(draft: &mut MultiseatConfig, seat_id: SeatId, id: &DeviceId) {
    if let Some(seat) = draft.seats.iter_mut().find(|s| s.id == seat_id) {
        seat.keyboard_ids.retain(|d| d != id);
    }
}

/// Removes one pointer from a seat.
pub fn clear_seat_pointer(draft: &mut MultiseatConfig, seat_id: SeatId, id: &DeviceId) {
    if let Some(seat) = draft.seats.iter_mut().find(|s| s.id == seat_id) {
        seat.pointer_ids.retain(|d| d != id);
    }
}

/// Local preview validation (does not call the daemon).
pub fn preview_validation(draft: &MultiseatConfig, hardware: &HardwareTree) -> ValidationReport {
    validate_config(draft, hardware)
}

#[cfg(test)]
mod tests {
    use super::*;
    use multiseat_core::{
        DisplayInfo, HardwareTree, InputDeviceInfo, InputDeviceKind, PlatformCapabilities,
    };

    fn sample_hw() -> HardwareTree {
        HardwareTree {
            revision: 1,
            gpus: vec![],
            displays: vec![DisplayInfo {
                id: DisplayId("\\\\.\\DISPLAY2".into()),
                gpu_id: "gpu0".into(),
                width: Some(1920),
                height: Some(1080),
                attached: true,
                virtual_display: false,
            }],
            usb_roots: vec![],
            input_devices: vec![
                InputDeviceInfo {
                    id: DeviceId::from_instance("KBD1"),
                    kind: InputDeviceKind::Keyboard,
                    name: "Kbd".into(),
                    vendor_id: None,
                    product_id: None,
                },
                InputDeviceInfo {
                    id: DeviceId::from_instance("PTR1"),
                    kind: InputDeviceKind::Pointer,
                    name: "Ptr".into(),
                    vendor_id: None,
                    product_id: None,
                },
            ],
            capabilities: PlatformCapabilities::default(),
        }
    }

    #[test]
    fn assign_moves_between_seats() {
        let mut draft = MultiseatConfig::default();
        add_seat(&mut draft);
        add_seat(&mut draft);
        let kbd = Assignable::Keyboard(DeviceId::from_instance("KBD1"));
        assign_to_seat(&mut draft, SeatId(1), kbd.clone()).unwrap();
        assign_to_seat(&mut draft, SeatId(2), kbd).unwrap();
        assert!(draft.seats[0].keyboard_ids.is_empty());
        assert_eq!(draft.seats[1].keyboard_ids.len(), 1);
    }

    #[test]
    fn emergency_clears_seat_assignment() {
        let mut draft = MultiseatConfig::default();
        ensure_editable_seat(&mut draft);
        let kbd = Assignable::Keyboard(DeviceId::from_instance("KBD1"));
        assign_to_seat(&mut draft, SeatId(1), kbd.clone()).unwrap();
        assign_emergency(&mut draft, kbd).unwrap();
        assert!(draft.seats[0].keyboard_ids.is_empty());
        assert_eq!(
            draft.emergency_keyboard,
            Some(DeviceId::from_instance("KBD1"))
        );
    }

    #[test]
    fn preview_reports_missing_emergency() {
        let mut draft = MultiseatConfig::default();
        ensure_editable_seat(&mut draft);
        let report = preview_validation(&draft, &sample_hw());
        assert!(report.has_errors());
    }
}

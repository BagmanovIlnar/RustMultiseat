//! Build [`RmsIfRouteTable`] from [`MultiseatConfig`] / [`InputAssignments`].

use multiseat_core::{InputAssignments, MultiseatConfig};

use super::ioctl::{
    RmsIfRouteEntry, RmsIfRouteTable, RMS_IF_MAX_ROUTES, RMS_IF_ROUTE_F_KEYBOARD,
    RMS_IF_ROUTE_F_POINTER, RMS_IF_ROUTE_F_SUPPRESS_HOST, RMS_IF_TABLE_F_REPLACE_ALL,
};

/// Error building a route table for the filter IOCTL.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RouteBuildError {
    /// More devices than [`RMS_IF_MAX_ROUTES`].
    TooManyRoutes {
        /// Attempted entry count.
        count: usize,
    },
    /// Config missing emergency keyboard or pointer.
    MissingEmergency,
}

impl std::fmt::Display for RouteBuildError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::TooManyRoutes { count } => {
                write!(
                    f,
                    "route table has {count} entries; max is {RMS_IF_MAX_ROUTES}"
                )
            }
            Self::MissingEmergency => write!(f, "emergency keyboard/pointer required"),
        }
    }
}

impl std::error::Error for RouteBuildError {}

/// Builds a replace-all route table from assignments.
///
/// - Emergency keyboard/pointer → seat 0, no `SUPPRESS_HOST`
/// - Enabled seat keyboards/pointers → `seat_id` + `SUPPRESS_HOST`
pub fn route_table_from_assignments(
    assignments: &InputAssignments,
) -> Result<RmsIfRouteTable, RouteBuildError> {
    route_table_from_config(&assignments.config)
}

/// Builds a replace-all route table from a [`MultiseatConfig`].
pub fn route_table_from_config(
    config: &MultiseatConfig,
) -> Result<RmsIfRouteTable, RouteBuildError> {
    let mut entries = Vec::new();

    let Some(ek) = config.emergency_keyboard.as_ref() else {
        return Err(RouteBuildError::MissingEmergency);
    };
    let Some(ep) = config.emergency_pointer.as_ref() else {
        return Err(RouteBuildError::MissingEmergency);
    };

    entries.push(RmsIfRouteEntry::new(
        0,
        RMS_IF_ROUTE_F_KEYBOARD,
        &ek.instance_id,
    ));
    entries.push(RmsIfRouteEntry::new(
        0,
        RMS_IF_ROUTE_F_POINTER,
        &ep.instance_id,
    ));

    for seat in config.seats.iter().filter(|s| s.enabled) {
        let seat_id = seat.id.0;
        for kbd in &seat.keyboard_ids {
            entries.push(RmsIfRouteEntry::new(
                seat_id,
                RMS_IF_ROUTE_F_KEYBOARD | RMS_IF_ROUTE_F_SUPPRESS_HOST,
                &kbd.instance_id,
            ));
        }
        for ptr in &seat.pointer_ids {
            entries.push(RmsIfRouteEntry::new(
                seat_id,
                RMS_IF_ROUTE_F_POINTER | RMS_IF_ROUTE_F_SUPPRESS_HOST,
                &ptr.instance_id,
            ));
        }
    }

    if entries.len() > RMS_IF_MAX_ROUTES {
        return Err(RouteBuildError::TooManyRoutes {
            count: entries.len(),
        });
    }

    RmsIfRouteTable::from_entries(&entries, RMS_IF_TABLE_F_REPLACE_ALL).map_err(|_| {
        RouteBuildError::TooManyRoutes {
            count: entries.len(),
        }
    })
}

/// Human-readable flag labels for a route row.
pub fn route_flags_label(flags: u32) -> String {
    let mut parts = Vec::new();
    if flags & RMS_IF_ROUTE_F_KEYBOARD != 0 {
        parts.push("KEYBOARD");
    }
    if flags & RMS_IF_ROUTE_F_POINTER != 0 {
        parts.push("POINTER");
    }
    if flags & RMS_IF_ROUTE_F_SUPPRESS_HOST != 0 {
        parts.push("SUPPRESS_HOST");
    }
    if parts.is_empty() {
        format!("0x{flags:X}")
    } else {
        parts.join("|")
    }
}

/// Instance id bytes as a trimmed UTF-8 string.
pub fn route_instance_id(entry: &RmsIfRouteEntry) -> String {
    std::str::from_utf8(&entry.instance_id)
        .unwrap_or("")
        .trim_end_matches('\0')
        .to_string()
}

/// Multiline preview for `multiseat-ctl input-routes-preview` (no IOCTL).
pub fn format_route_table_preview(table: &RmsIfRouteTable) -> String {
    let mut out = String::new();
    out.push_str(&format!(
        "abi={} entries={} flags=0x{:X} (REPLACE_ALL={})\n",
        table.abi_version,
        table.entry_count,
        table.flags,
        table.flags & RMS_IF_TABLE_F_REPLACE_ALL != 0
    ));
    for (i, entry) in table.entries[..table.entry_count as usize]
        .iter()
        .enumerate()
    {
        out.push_str(&format!(
            "  [{i}] seat={} flags={} ({}) id={}\n",
            entry.seat_id,
            entry.flags,
            route_flags_label(entry.flags),
            route_instance_id(entry)
        ));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use multiseat_core::{DeviceId, SeatConfig, SeatId, UserSid};

    fn sample_config() -> MultiseatConfig {
        MultiseatConfig {
            seats: vec![SeatConfig {
                id: SeatId(1),
                user_sid: UserSid("S-1-5-21-1".into()),
                display_ids: vec![],
                keyboard_ids: vec![DeviceId::from_instance("HID\\SEAT1_KBD")],
                pointer_ids: vec![DeviceId::from_instance("HID\\SEAT1_PTR")],
                enabled: true,
            }],
            emergency_keyboard: Some(DeviceId::from_instance("HID\\HOST_KBD")),
            emergency_pointer: Some(DeviceId::from_instance("HID\\HOST_PTR")),
        }
    }

    #[test]
    fn builds_emergency_and_seat_rows() {
        let table = route_table_from_config(&sample_config()).unwrap();
        assert_eq!(table.entry_count, 4);
        assert_eq!(table.entries[0].seat_id, 0);
        assert_eq!(table.entries[0].flags, RMS_IF_ROUTE_F_KEYBOARD);
        assert_eq!(table.entries[2].seat_id, 1);
        assert_eq!(
            table.entries[2].flags,
            RMS_IF_ROUTE_F_KEYBOARD | RMS_IF_ROUTE_F_SUPPRESS_HOST
        );
        table.validate().unwrap();
    }

    #[test]
    fn rejects_missing_emergency() {
        let mut cfg = sample_config();
        cfg.emergency_keyboard = None;
        assert_eq!(
            route_table_from_config(&cfg),
            Err(RouteBuildError::MissingEmergency)
        );
    }

    #[test]
    fn preview_lists_emergency_and_seat() {
        let table = route_table_from_config(&sample_config()).unwrap();
        let text = format_route_table_preview(&table);
        assert!(text.contains("entries=4"));
        assert!(text.contains("SUPPRESS_HOST"));
        assert!(text.contains("HID\\HOST_KBD"));
        assert!(text.contains("HID\\SEAT1_KBD"));
    }
}

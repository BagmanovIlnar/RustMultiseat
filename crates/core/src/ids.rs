//! Stable identifiers used across IPC and persistence.

use serde::{Deserialize, Serialize};
use std::fmt;

/// Stable device identity. Prefer instance/interface IDs over DXGI ordinals.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct DeviceId {
    /// Windows device instance ID when available.
    pub instance_id: String,
    /// Device interface path when available.
    pub interface_path: Option<String>,
    /// Container ID when available.
    pub container_id: Option<String>,
}

impl DeviceId {
    /// Creates a device id from an instance id only.
    ///
    /// Instance IDs are normalized to ASCII uppercase so Windows case folding
    /// does not produce unstable inventory keys across rescans.
    pub fn from_instance(instance_id: impl Into<String>) -> Self {
        Self {
            instance_id: stabilize_instance_id(instance_id.into()),
            interface_path: None,
            container_id: None,
        }
    }

    /// Creates a device id with optional interface / container fields.
    ///
    /// `interface_path` is kept as returned by SetupAPI (`\\?\…`); only the
    /// instance id is case-folded. Container IDs are lowercased GUID strings.
    pub fn new(
        instance_id: impl Into<String>,
        interface_path: Option<String>,
        container_id: Option<String>,
    ) -> Self {
        Self {
            instance_id: stabilize_instance_id(instance_id.into()),
            interface_path: interface_path
                .map(|p| p.trim().to_string())
                .filter(|p| !p.is_empty()),
            container_id: container_id
                .map(|s| normalize_container_id(&s))
                .filter(|s| !s.is_empty()),
        }
    }

    /// True when two ids refer to the same HID collection / device node.
    ///
    /// Prefers equal instance ids; otherwise matches on equal interface paths
    /// (SetupAPI `\\?\…`). Container id alone is **not** enough — composite USB
    /// devices share one container across keyboard+pointer collections.
    pub fn same_device(&self, other: &Self) -> bool {
        if self.instance_id == other.instance_id {
            return true;
        }
        matches!(
            (&self.interface_path, &other.interface_path),
            (Some(a), Some(b)) if !a.is_empty() && a.eq_ignore_ascii_case(b)
        )
    }

    /// True when both ids belong to the same physical container (composite kit).
    pub fn same_container(&self, other: &Self) -> bool {
        matches!(
            (&self.container_id, &other.container_id),
            (Some(a), Some(b)) if !a.is_empty() && a == b
        )
    }
}

fn stabilize_instance_id(raw: String) -> String {
    raw.to_ascii_uppercase()
}

fn normalize_container_id(raw: &str) -> String {
    raw.trim()
        .trim_matches(|c| c == '{' || c == '}')
        .to_ascii_lowercase()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn instance_normalized_uppercase() {
        let id = DeviceId::from_instance("hid\\vid_bbbb&pid_0001\\seat1_kbd");
        assert_eq!(id.instance_id, "HID\\VID_BBBB&PID_0001\\SEAT1_KBD");
    }

    #[test]
    fn interface_path_not_uppercased() {
        let id = DeviceId::new(
            "HID\\VID_1\\X",
            Some(r"\\?\hid#vid_1#guid".into()),
            Some("{AaBbCcDd-1234-5678-90ab-cdef01234567}".into()),
        );
        assert_eq!(id.interface_path.as_deref(), Some(r"\\?\hid#vid_1#guid"));
        assert_eq!(
            id.container_id.as_deref(),
            Some("aabbccdd-1234-5678-90ab-cdef01234567")
        );
    }

    #[test]
    fn same_device_by_interface_path() {
        let a = DeviceId::new(
            "HID\\A",
            Some(r"\\?\hid#vid_1#col01".into()),
            Some("11111111-2222-3333-4444-555555555555".into()),
        );
        let b = DeviceId::new(
            "HID\\A_REOPEN",
            Some(r"\\?\HID#VID_1#COL01".into()),
            Some("{11111111-2222-3333-4444-555555555555}".into()),
        );
        assert!(a.same_device(&b));
        assert!(a.same_container(&b));
        let sibling = DeviceId::new(
            "HID\\PTR",
            Some(r"\\?\hid#vid_1#col02".into()),
            Some("11111111-2222-3333-4444-555555555555".into()),
        );
        assert!(!a.same_device(&sibling));
        assert!(a.same_container(&sibling));
    }
}

impl fmt::Display for DeviceId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.instance_id)
    }
}

/// Stable display / output identity.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct DisplayId(pub String);

impl fmt::Display for DisplayId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// Seat identity used in configuration and events.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct SeatId(pub u32);

impl fmt::Display for SeatId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "seat-{}", self.0)
    }
}

/// Opaque Windows session identifier.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct SessionId(pub u32);

/// User security identifier as a string SID.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct UserSid(pub String);

impl fmt::Display for UserSid {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

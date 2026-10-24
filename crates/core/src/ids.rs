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
    pub fn new(
        instance_id: impl Into<String>,
        interface_path: Option<String>,
        container_id: Option<String>,
    ) -> Self {
        Self {
            instance_id: stabilize_instance_id(instance_id.into()),
            interface_path: interface_path.map(stabilize_instance_id),
            container_id: container_id.map(|s| s.to_ascii_lowercase()),
        }
    }
}

fn stabilize_instance_id(raw: String) -> String {
    raw.to_ascii_uppercase()
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

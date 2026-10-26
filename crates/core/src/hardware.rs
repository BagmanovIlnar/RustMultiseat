//! Hardware inventory models returned by [`crate::traits::HardwareEnumerator`].

use serde::{Deserialize, Serialize};

use crate::ids::{DeviceId, DisplayId};

/// Capabilities detected for the current OS build.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct PlatformCapabilities {
    /// OS family label, e.g. "windows".
    pub os: String,
    /// Human-readable OS version string.
    pub os_version: String,
    /// Whether a display backend is available for this OS.
    pub display_backend_available: bool,
    /// Whether an experimental session backend is enabled.
    pub session_backend_available: bool,
    /// Whether input routing drivers are installed and healthy.
    pub input_router_available: bool,
    /// Whether Secure Boot / signature enforcement is expected to be on.
    pub secure_boot_expected: bool,
}

/// GPU adapter summary.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GpuInfo {
    /// Stable adapter identity string.
    pub id: String,
    /// Adapter description from DXGI/SetupAPI.
    pub description: String,
    /// Vendor identifier when known.
    pub vendor_id: Option<u32>,
    /// Device identifier when known.
    pub device_id: Option<u32>,
}

/// Physical or virtual display output.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DisplayInfo {
    /// Stable display id.
    pub id: DisplayId,
    /// Adapter that owns the output.
    pub gpu_id: String,
    /// Current width in pixels when known.
    pub width: Option<u32>,
    /// Current height in pixels when known.
    pub height: Option<u32>,
    /// Whether the output is currently attached.
    pub attached: bool,
    /// Whether this is a virtual/indirect display.
    pub virtual_display: bool,
}

/// Kind of input device.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum InputDeviceKind {
    /// Keyboard or keypad.
    Keyboard,
    /// Mouse or absolute pointer.
    Pointer,
    /// Other HID collection that may be routed later.
    Other,
}

/// Input device discovered by inventory.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct InputDeviceInfo {
    /// Stable device id.
    pub id: DeviceId,
    /// Device class.
    pub kind: InputDeviceKind,
    /// Friendly name when available.
    pub name: String,
    /// USB VID when known.
    pub vendor_id: Option<u16>,
    /// USB PID when known.
    pub product_id: Option<u16>,
}

/// USB topology node.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct UsbNode {
    /// Device instance for this node.
    pub device_id: DeviceId,
    /// Human-readable description.
    pub description: String,
    /// Hub port number when known (prefer hub IOCTL over CM address guess).
    pub port: Option<u32>,
    /// First CM/SetupAPI location path when known.
    #[serde(default)]
    pub location_path: Option<String>,
    /// Child nodes.
    pub children: Vec<UsbNode>,
}

/// Consistent hardware snapshot.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HardwareTree {
    /// Monotonic snapshot revision.
    pub revision: u64,
    /// Detected GPUs.
    pub gpus: Vec<GpuInfo>,
    /// Detected displays.
    pub displays: Vec<DisplayInfo>,
    /// USB root hubs and children.
    pub usb_roots: Vec<UsbNode>,
    /// Keyboard/pointer devices.
    pub input_devices: Vec<InputDeviceInfo>,
    /// Platform feature flags.
    pub capabilities: PlatformCapabilities,
}

impl HardwareTree {
    /// Empty snapshot used before the first enumeration.
    pub fn empty() -> Self {
        Self {
            revision: 0,
            gpus: Vec::new(),
            displays: Vec::new(),
            usb_roots: Vec::new(),
            input_devices: Vec::new(),
            capabilities: PlatformCapabilities::default(),
        }
    }

    /// Returns true when the tree contains the device (exact or [`DeviceId::same_device`]).
    pub fn contains_device(&self, id: &DeviceId) -> bool {
        self.resolve_input(id).is_some()
    }

    /// Returns true when the tree contains the display.
    pub fn contains_display(&self, id: &DisplayId) -> bool {
        self.displays.iter().any(|d| &d.id == id)
    }

    /// Finds an input device by exact id equality.
    pub fn input_device(&self, id: &DeviceId) -> Option<&InputDeviceInfo> {
        self.input_devices.iter().find(|d| &d.id == id)
    }

    /// Resolves a configured id against the live snapshot.
    ///
    /// Order: [`DeviceId::same_device`] (instance / interface path), then
    /// [`DeviceId::same_container`] (composite kit; first match of any kind).
    /// Prefer [`crate::remap::remap_config`] when kind-aware claiming is needed.
    pub fn resolve_input(&self, id: &DeviceId) -> Option<&InputDeviceInfo> {
        self.input_devices
            .iter()
            .find(|d| id.same_device(&d.id))
            .or_else(|| self.input_devices.iter().find(|d| id.same_container(&d.id)))
    }
}

//! Hardware inventory backends (native Windows + shared helpers).

#[cfg(windows)]
mod cm_usb;
#[cfg(windows)]
mod dxgi;
#[cfg(windows)]
mod hotplug;
#[cfg(windows)]
mod setupapi_hid;

#[cfg(windows)]
mod windows_native;

#[cfg(windows)]
pub use windows_native::WindowsHardwareEnumerator;

#[cfg(not(windows))]
mod windows_stub;

#[cfg(not(windows))]
pub use windows_stub::WindowsHardwareEnumerator;

use multiseat_core::PlatformCapabilities;

use crate::probe::detect_capabilities;

/// Best-effort capability probe without a full device walk.
pub fn probe_capabilities() -> PlatformCapabilities {
    detect_capabilities()
}

/// Attempts to create the native enumerator for the current host.
pub fn prefer_native() -> Result<WindowsHardwareEnumerator, multiseat_core::CoreError> {
    WindowsHardwareEnumerator::try_create()
}

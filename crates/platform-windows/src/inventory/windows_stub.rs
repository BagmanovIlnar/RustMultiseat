//! Non-Windows placeholder for the native enumerator type.

use multiseat_core::{
    CoreError, CoreResult, HardwareEnumerator, HardwareTree, PlatformCapabilities,
};

use crate::probe::detect_capabilities;

/// Stand-in type so call sites compile on macOS/Linux CI.
#[derive(Debug, Default)]
pub struct WindowsHardwareEnumerator;

impl WindowsHardwareEnumerator {
    /// Always fails: native inventory needs a Windows host.
    pub fn try_create() -> CoreResult<Self> {
        Err(CoreError::Backend(
            "WindowsHardwareEnumerator requires a Windows host".into(),
        ))
    }
}

impl HardwareEnumerator for WindowsHardwareEnumerator {
    fn capabilities(&self) -> CoreResult<PlatformCapabilities> {
        Ok(detect_capabilities())
    }

    fn snapshot(&self) -> CoreResult<HardwareTree> {
        Err(CoreError::Backend(
            "native inventory requires Windows".into(),
        ))
    }

    fn start_notifications(&mut self) -> CoreResult<()> {
        Err(CoreError::Backend(
            "native inventory notifications require Windows".into(),
        ))
    }
}

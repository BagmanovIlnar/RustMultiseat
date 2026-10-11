//! Native Windows inventory (DXGI + SetupAPI + CM + hot-plug).

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

use multiseat_core::{
    CoreError, CoreResult, HardwareEnumerator, HardwareTree, PlatformCapabilities,
};
use tracing::{info, warn};

use crate::debounce::DebounceGate;
use crate::probe::detect_capabilities;

use super::cm_usb;
use super::dxgi;
use super::hotplug::HotplugWatcher;
use super::setupapi_hid;

static SNAPSHOT_REVISION: AtomicU64 = AtomicU64::new(1);

/// Native enumerator backed by DXGI, SetupAPI, CM, and optional hot-plug.
#[derive(Debug)]
pub struct WindowsHardwareEnumerator {
    gate: Arc<DebounceGate>,
    watcher: Option<HotplugWatcher>,
}

impl WindowsHardwareEnumerator {
    /// Creates the enumerator after probing DXGI availability.
    pub fn try_create() -> CoreResult<Self> {
        let _ = dxgi::enumerate_gpus_and_displays()
            .map_err(|e| CoreError::Backend(format!("native inventory unavailable: {e}")))?;
        Ok(Self {
            gate: Arc::new(DebounceGate::default_hotplug()),
            watcher: None,
        })
    }
}

impl HardwareEnumerator for WindowsHardwareEnumerator {
    fn capabilities(&self) -> CoreResult<PlatformCapabilities> {
        let mut caps = detect_capabilities();
        caps.display_backend_available = false;
        caps.session_backend_available = false;
        caps.input_router_available = false;
        Ok(caps)
    }

    fn snapshot(&self) -> CoreResult<HardwareTree> {
        let (gpus, displays) = dxgi::enumerate_gpus_and_displays()?;
        let input_devices = match setupapi_hid::enumerate_input_devices() {
            Ok(devices) => devices,
            Err(err) => {
                warn!(error = %err, "HID enumeration failed; continuing with GPUs/displays only");
                Vec::new()
            }
        };
        let usb_roots = match cm_usb::build_usb_topology() {
            Ok(roots) => roots,
            Err(err) => {
                warn!(error = %err, "CM USB topology failed; falling back to SetupAPI list");
                setupapi_hid::enumerate_usb_roots().unwrap_or_default()
            }
        };

        let revision = SNAPSHOT_REVISION.fetch_add(1, Ordering::Relaxed);
        info!(
            revision,
            gpus = gpus.len(),
            displays = displays.len(),
            inputs = input_devices.len(),
            usb = usb_roots.len(),
            "native hardware snapshot"
        );

        Ok(HardwareTree {
            revision,
            gpus,
            displays,
            usb_roots,
            input_devices,
            capabilities: self.capabilities()?,
        })
    }

    fn start_notifications(&mut self) -> CoreResult<()> {
        if self.watcher.is_some() {
            return Ok(());
        }
        match HotplugWatcher::start(Arc::clone(&self.gate)) {
            Ok(watcher) => {
                self.watcher = Some(watcher);
                info!("native hot-plug watcher started");
                Ok(())
            }
            Err(err) => {
                warn!(error = %err, "failed to start hot-plug watcher");
                Err(CoreError::Backend(err))
            }
        }
    }

    fn poll_invalidated(&mut self) -> bool {
        self.gate.take_ready()
    }
}

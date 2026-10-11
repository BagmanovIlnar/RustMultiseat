//! Deterministic mock backends for daemon/CLI development and tests.

use std::collections::HashMap;
use std::sync::Arc;

use multiseat_core::{
    BackendHealth, CoreError, CoreResult, DeviceId, DisplayBackend, DisplayCapabilities, DisplayId,
    DisplayInfo, GpuInfo, HardwareEnumerator, HardwareTree, InputAssignments, InputDeviceInfo,
    InputDeviceKind, InputRouter, PlatformCapabilities, RollbackToken, SeatConfig, SeatId,
    SessionBackend, SessionHandle, SessionId, UsbNode,
};

use crate::debounce::DebounceGate;

/// In-memory hardware enumerator with a fixed lab topology.
#[derive(Debug, Clone)]
pub struct MockHardwareEnumerator {
    tree: HardwareTree,
    notifications: bool,
    gate: Arc<DebounceGate>,
}

impl Default for MockHardwareEnumerator {
    fn default() -> Self {
        Self::with_lab_fixture()
    }
}

impl MockHardwareEnumerator {
    /// Two displays, host emergency input, and one seat kit.
    pub fn with_lab_fixture() -> Self {
        let caps = PlatformCapabilities {
            os: "mock".into(),
            os_version: "0.1.0".into(),
            display_backend_available: true,
            session_backend_available: true,
            input_router_available: true,
            secure_boot_expected: true,
        };

        let tree = HardwareTree {
            revision: 1,
            gpus: vec![GpuInfo {
                id: "gpu-0".into(),
                description: "Mock GPU".into(),
                vendor_id: Some(0x10DE),
                device_id: Some(0x1234),
            }],
            displays: vec![
                DisplayInfo {
                    id: DisplayId("\\\\.\\DISPLAY1".into()),
                    gpu_id: "gpu-0".into(),
                    width: Some(1920),
                    height: Some(1080),
                    attached: true,
                    virtual_display: false,
                },
                DisplayInfo {
                    id: DisplayId("\\\\.\\DISPLAY2".into()),
                    gpu_id: "gpu-0".into(),
                    width: Some(1920),
                    height: Some(1080),
                    attached: true,
                    virtual_display: false,
                },
            ],
            usb_roots: vec![UsbNode {
                device_id: DeviceId::from_instance("USB\\ROOT_HUB30\\1"),
                description: "Root Hub".into(),
                port: None,
                children: vec![
                    UsbNode {
                        device_id: DeviceId::from_instance("USB\\VID_1A40&PID_0101\\HUB1"),
                        description: "External Hub".into(),
                        port: Some(1),
                        children: vec![
                            UsbNode {
                                device_id: DeviceId::from_instance(
                                    "HID\\VID_BBBB&PID_0001\\SEAT1_KBD",
                                ),
                                description: "Seat 1 Keyboard".into(),
                                port: Some(1),
                                children: vec![],
                            },
                            UsbNode {
                                device_id: DeviceId::from_instance(
                                    "HID\\VID_BBBB&PID_0002\\SEAT1_PTR",
                                ),
                                description: "Seat 1 Pointer".into(),
                                port: Some(2),
                                children: vec![],
                            },
                        ],
                    },
                    UsbNode {
                        device_id: DeviceId::from_instance("USB\\VID_1A40&PID_0101\\HUB2"),
                        description: "External Hub 2".into(),
                        port: Some(2),
                        children: vec![
                            UsbNode {
                                device_id: DeviceId::from_instance(
                                    "HID\\VID_CCCC&PID_0001\\SEAT2_KBD",
                                ),
                                description: "Seat 2 Keyboard".into(),
                                port: Some(1),
                                children: vec![],
                            },
                            UsbNode {
                                device_id: DeviceId::from_instance(
                                    "HID\\VID_CCCC&PID_0002\\SEAT2_PTR",
                                ),
                                description: "Seat 2 Pointer".into(),
                                port: Some(2),
                                children: vec![],
                            },
                        ],
                    },
                ],
            }],
            input_devices: vec![
                InputDeviceInfo {
                    id: DeviceId::from_instance("HID\\VID_AAAA&PID_0001\\HOST_KBD"),
                    kind: InputDeviceKind::Keyboard,
                    name: "Host Keyboard".into(),
                    vendor_id: Some(0xAAAA),
                    product_id: Some(0x0001),
                },
                InputDeviceInfo {
                    id: DeviceId::from_instance("HID\\VID_AAAA&PID_0002\\HOST_PTR"),
                    kind: InputDeviceKind::Pointer,
                    name: "Host Pointer".into(),
                    vendor_id: Some(0xAAAA),
                    product_id: Some(0x0002),
                },
                InputDeviceInfo {
                    id: DeviceId::from_instance("HID\\VID_BBBB&PID_0001\\SEAT1_KBD"),
                    kind: InputDeviceKind::Keyboard,
                    name: "Seat 1 Keyboard".into(),
                    vendor_id: Some(0xBBBB),
                    product_id: Some(0x0001),
                },
                InputDeviceInfo {
                    id: DeviceId::from_instance("HID\\VID_BBBB&PID_0002\\SEAT1_PTR"),
                    kind: InputDeviceKind::Pointer,
                    name: "Seat 1 Pointer".into(),
                    vendor_id: Some(0xBBBB),
                    product_id: Some(0x0002),
                },
                InputDeviceInfo {
                    id: DeviceId::from_instance("HID\\VID_CCCC&PID_0001\\SEAT2_KBD"),
                    kind: InputDeviceKind::Keyboard,
                    name: "Seat 2 Keyboard".into(),
                    vendor_id: Some(0xCCCC),
                    product_id: Some(0x0001),
                },
                InputDeviceInfo {
                    id: DeviceId::from_instance("HID\\VID_CCCC&PID_0002\\SEAT2_PTR"),
                    kind: InputDeviceKind::Pointer,
                    name: "Seat 2 Pointer".into(),
                    vendor_id: Some(0xCCCC),
                    product_id: Some(0x0002),
                },
            ],
            capabilities: caps,
        };

        Self {
            tree,
            notifications: false,
            gate: Arc::new(DebounceGate::default_hotplug()),
        }
    }

    /// Bumps revision and signals a debounced hot-plug rescan.
    pub fn bump_revision(&mut self) {
        self.tree.revision = self.tree.revision.saturating_add(1);
        self.gate.signal();
    }
}

impl HardwareEnumerator for MockHardwareEnumerator {
    fn capabilities(&self) -> CoreResult<PlatformCapabilities> {
        Ok(self.tree.capabilities.clone())
    }

    fn snapshot(&self) -> CoreResult<HardwareTree> {
        Ok(self.tree.clone())
    }

    fn start_notifications(&mut self) -> CoreResult<()> {
        self.notifications = true;
        tracing::info!("mock hardware notifications enabled");
        Ok(())
    }

    fn poll_invalidated(&mut self) -> bool {
        self.gate.take_ready()
    }
}

/// Mock session backend that allocates monotonic session ids.
#[derive(Debug, Default)]
pub struct MockSessionBackend {
    next_id: u32,
    sessions: HashMap<SeatId, SessionId>,
    /// When true, [`SessionBackend::probe`] returns false.
    pub probe_ok: bool,
}

impl MockSessionBackend {
    /// Creates a backend that reports probe success.
    pub fn available() -> Self {
        Self {
            next_id: 10,
            sessions: HashMap::new(),
            probe_ok: true,
        }
    }
}

impl SessionBackend for MockSessionBackend {
    fn probe(&self) -> CoreResult<bool> {
        Ok(self.probe_ok)
    }

    fn create_or_connect(&mut self, seat: &SeatConfig) -> CoreResult<SessionHandle> {
        if !self.probe_ok {
            return Err(CoreError::Backend("session backend disabled".into()));
        }
        self.next_id += 1;
        let session_id = SessionId(self.next_id);
        self.sessions.insert(seat.id, session_id);
        Ok(SessionHandle {
            seat_id: seat.id,
            session_id,
        })
    }

    fn start_agent(&mut self, session: &SessionHandle) -> CoreResult<()> {
        if !self.sessions.contains_key(&session.seat_id) {
            return Err(CoreError::Backend("unknown session".into()));
        }
        Ok(())
    }

    fn lock(&mut self, _session: &SessionHandle) -> CoreResult<()> {
        Ok(())
    }

    fn disconnect(&mut self, session: &SessionHandle) -> CoreResult<()> {
        self.sessions.remove(&session.seat_id);
        Ok(())
    }

    fn health(&self, session: &SessionHandle) -> CoreResult<BackendHealth> {
        if self.sessions.contains_key(&session.seat_id) {
            Ok(BackendHealth::ok())
        } else {
            Ok(BackendHealth::down("session missing"))
        }
    }
}

/// Mock display backend.
#[derive(Debug, Default)]
pub struct MockDisplayBackend {
    attached: HashMap<SeatId, RollbackToken>,
}

impl DisplayBackend for MockDisplayBackend {
    fn capabilities(&self) -> CoreResult<DisplayCapabilities> {
        Ok(DisplayCapabilities {
            backend_name: "mock".into(),
            attach_supported: true,
        })
    }

    fn prepare(&mut self, seats: &[SeatConfig]) -> CoreResult<()> {
        if seats.is_empty() {
            return Err(CoreError::Backend("no seats to prepare".into()));
        }
        Ok(())
    }

    fn attach(&mut self, seat: &SeatConfig, _session: &SessionHandle) -> CoreResult<RollbackToken> {
        let token = RollbackToken::new(format!("display-{}", seat.id));
        self.attached.insert(seat.id, token.clone());
        Ok(token)
    }

    fn detach(&mut self, seat_id: SeatId) -> CoreResult<()> {
        self.attached.remove(&seat_id);
        Ok(())
    }

    fn rollback(&mut self, token: &RollbackToken) -> CoreResult<()> {
        self.attached.retain(|_, t| t != token);
        Ok(())
    }
}

/// Mock input router with fail-open semantics.
#[derive(Debug, Default)]
pub struct MockInputRouter {
    active: Option<RollbackToken>,
    fail_open: bool,
}

impl MockInputRouter {
    /// Forces fail-open mode, clearing active routes.
    pub fn set_fail_open(&mut self, enabled: bool) {
        self.fail_open = enabled;
        if enabled {
            self.active = None;
        }
    }
}

impl InputRouter for MockInputRouter {
    fn validate(&self, assignments: &InputAssignments) -> CoreResult<()> {
        if assignments.config.emergency_keyboard.is_none() {
            return Err(CoreError::Backend("emergency keyboard missing".into()));
        }
        Ok(())
    }

    fn apply(&mut self, assignments: &InputAssignments) -> CoreResult<RollbackToken> {
        if self.fail_open {
            return Err(CoreError::Backend("router in fail-open".into()));
        }
        self.validate(assignments)?;
        let token = RollbackToken::new("input-routes");
        self.active = Some(token.clone());
        Ok(token)
    }

    fn rollback(&mut self, token: &RollbackToken) -> CoreResult<()> {
        if self.active.as_ref() == Some(token) {
            self.active = None;
        }
        Ok(())
    }

    fn health(&self) -> CoreResult<BackendHealth> {
        if self.fail_open {
            Ok(BackendHealth::down("fail-open"))
        } else {
            Ok(BackendHealth::ok())
        }
    }
}

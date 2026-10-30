//! Deterministic mock backends for daemon/CLI development and tests.

use std::collections::HashMap;
use std::process::Child;
use std::sync::Arc;

use multiseat_core::{
    BackendHealth, CoreError, CoreResult, DeviceId, DisplayBackend, DisplayCapabilities, DisplayId,
    DisplayInfo, GpuInfo, HardwareEnumerator, HardwareTree, InputAssignments, InputDeviceInfo,
    InputDeviceKind, InputRouter, PlatformCapabilities, RollbackToken, SeatConfig, SeatId,
    SessionBackend, SessionHandle, SessionId, UsbNode,
};
use tracing::{info, warn};

use crate::debounce::DebounceGate;
use crate::inventory::SnapshotCache;
use crate::session::{require_agent, spawn_session_agent, stop_agent_child};

/// Env var selecting a mock backend failure stage for fault-injection tests
/// and lab runs: `session_create` / `agent_start` / `display_prepare` /
/// `display_attach` / `input_apply`.
pub const MOCK_FAIL_STAGE_ENV: &str = "MULTISEAT_MOCK_FAIL_STAGE";

/// True when [`MOCK_FAIL_STAGE_ENV`] currently selects `stage`.
fn mock_fail_stage_is(stage: &str) -> bool {
    std::env::var_os(MOCK_FAIL_STAGE_ENV).is_some_and(|v| v == stage)
}

fn mock_fail_stage_err(stage: &'static str) -> CoreError {
    CoreError::Backend(format!("mock fail stage: {stage}"))
}

/// In-memory hardware enumerator with a fixed lab topology.
///
/// Hot-plug simulation: [`Self::bump_revision`] signals the debounce gate;
/// after the quiet window [`HardwareEnumerator::poll_invalidated`] clears the
/// cache and the next [`snapshot`](HardwareEnumerator::snapshot) rebuilds from
/// the in-memory tree (with bumped revision).
#[derive(Debug)]
pub struct MockHardwareEnumerator {
    tree: HardwareTree,
    notifications: bool,
    cache: SnapshotCache,
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
                location_path: Some("PCIROOT(0)#PCI(1400)#USBROOT(0)".into()),
                children: vec![
                    UsbNode {
                        device_id: DeviceId::from_instance("USB\\VID_1A40&PID_0101\\HUB1"),
                        description: "External Hub".into(),
                        port: Some(1),
                        location_path: Some("PCIROOT(0)#PCI(1400)#USBROOT(0)#USB(1)".into()),
                        children: vec![
                            UsbNode {
                                device_id: DeviceId::from_instance(
                                    "HID\\VID_BBBB&PID_0001\\SEAT1_KBD",
                                ),
                                description: "Seat 1 Keyboard".into(),
                                port: Some(1),
                                location_path: None,
                                children: vec![],
                            },
                            UsbNode {
                                device_id: DeviceId::from_instance(
                                    "HID\\VID_BBBB&PID_0002\\SEAT1_PTR",
                                ),
                                description: "Seat 1 Pointer".into(),
                                port: Some(2),
                                location_path: None,
                                children: vec![],
                            },
                        ],
                    },
                    UsbNode {
                        device_id: DeviceId::from_instance("USB\\VID_1A40&PID_0101\\HUB2"),
                        description: "External Hub 2".into(),
                        port: Some(2),
                        location_path: Some("PCIROOT(0)#PCI(1400)#USBROOT(0)#USB(2)".into()),
                        children: vec![
                            UsbNode {
                                device_id: DeviceId::from_instance(
                                    "HID\\VID_CCCC&PID_0001\\SEAT2_KBD",
                                ),
                                description: "Seat 2 Keyboard".into(),
                                port: Some(1),
                                location_path: None,
                                children: vec![],
                            },
                            UsbNode {
                                device_id: DeviceId::from_instance(
                                    "HID\\VID_CCCC&PID_0002\\SEAT2_PTR",
                                ),
                                description: "Seat 2 Pointer".into(),
                                port: Some(2),
                                location_path: None,
                                children: vec![],
                            },
                        ],
                    },
                ],
            }],
            input_devices: vec![
                InputDeviceInfo {
                    id: DeviceId::new(
                        "HID\\VID_AAAA&PID_0001\\HOST_KBD",
                        Some(r"\\?\hid#vid_aaaa&pid_0001#host_kbd".into()),
                        Some("aaaaaaaa-0000-0000-0000-000000000001".into()),
                    ),
                    kind: InputDeviceKind::Keyboard,
                    name: "Host Keyboard".into(),
                    vendor_id: Some(0xAAAA),
                    product_id: Some(0x0001),
                },
                InputDeviceInfo {
                    id: DeviceId::new(
                        "HID\\VID_AAAA&PID_0002\\HOST_PTR",
                        Some(r"\\?\hid#vid_aaaa&pid_0002#host_ptr".into()),
                        Some("aaaaaaaa-0000-0000-0000-000000000001".into()),
                    ),
                    kind: InputDeviceKind::Pointer,
                    name: "Host Pointer".into(),
                    vendor_id: Some(0xAAAA),
                    product_id: Some(0x0002),
                },
                InputDeviceInfo {
                    id: DeviceId::new(
                        "HID\\VID_BBBB&PID_0001\\SEAT1_KBD",
                        Some(r"\\?\hid#vid_bbbb&pid_0001#seat1_kbd".into()),
                        Some("bbbbbbbb-0000-0000-0000-000000000001".into()),
                    ),
                    kind: InputDeviceKind::Keyboard,
                    name: "Seat 1 Keyboard".into(),
                    vendor_id: Some(0xBBBB),
                    product_id: Some(0x0001),
                },
                InputDeviceInfo {
                    id: DeviceId::new(
                        "HID\\VID_BBBB&PID_0002\\SEAT1_PTR",
                        Some(r"\\?\hid#vid_bbbb&pid_0002#seat1_ptr".into()),
                        Some("bbbbbbbb-0000-0000-0000-000000000001".into()),
                    ),
                    kind: InputDeviceKind::Pointer,
                    name: "Seat 1 Pointer".into(),
                    vendor_id: Some(0xBBBB),
                    product_id: Some(0x0002),
                },
                InputDeviceInfo {
                    id: DeviceId::new(
                        "HID\\VID_CCCC&PID_0001\\SEAT2_KBD",
                        Some(r"\\?\hid#vid_cccc&pid_0001#seat2_kbd".into()),
                        Some("cccccccc-0000-0000-0000-000000000001".into()),
                    ),
                    kind: InputDeviceKind::Keyboard,
                    name: "Seat 2 Keyboard".into(),
                    vendor_id: Some(0xCCCC),
                    product_id: Some(0x0001),
                },
                InputDeviceInfo {
                    id: DeviceId::new(
                        "HID\\VID_CCCC&PID_0002\\SEAT2_PTR",
                        Some(r"\\?\hid#vid_cccc&pid_0002#seat2_ptr".into()),
                        Some("cccccccc-0000-0000-0000-000000000001".into()),
                    ),
                    kind: InputDeviceKind::Pointer,
                    name: "Seat 2 Pointer".into(),
                    vendor_id: Some(0xCCCC),
                    product_id: Some(0x0002),
                },
            ],
            capabilities: caps,
        };

        let gate = Arc::new(DebounceGate::default_hotplug());
        Self {
            tree,
            notifications: false,
            cache: SnapshotCache::new(gate),
        }
    }

    /// Bumps revision and signals a debounced hot-plug rescan.
    pub fn bump_revision(&mut self) {
        self.tree.revision = self.tree.revision.saturating_add(1);
        self.cache.gate().signal();
    }

    /// Simulates Windows reopening a HID collection with a new instance id.
    ///
    /// Keeps `interface_path` / `container_id` so [`multiseat_core::remap_config`]
    /// can rematch seat assignments after hot-plug.
    pub fn reopen_input_instance(
        &mut self,
        interface_substr: &str,
        new_instance: impl Into<String>,
    ) -> bool {
        let needle = interface_substr.to_ascii_lowercase();
        let new_instance = new_instance.into().to_ascii_uppercase();
        let mut found = false;
        for device in &mut self.tree.input_devices {
            let matches = device
                .id
                .interface_path
                .as_ref()
                .is_some_and(|p| p.to_ascii_lowercase().contains(&needle));
            if matches {
                device.id.instance_id = new_instance;
                found = true;
                break;
            }
        }
        if found {
            self.bump_revision();
        }
        found
    }

    /// Whether notifications were started (tests).
    pub fn notifications_enabled(&self) -> bool {
        self.notifications
    }
}

impl Clone for MockHardwareEnumerator {
    fn clone(&self) -> Self {
        let gate = Arc::new(DebounceGate::new(self.cache.gate().window()));
        Self {
            tree: self.tree.clone(),
            notifications: self.notifications,
            cache: SnapshotCache::new(gate),
        }
    }
}

impl HardwareEnumerator for MockHardwareEnumerator {
    fn capabilities(&self) -> CoreResult<PlatformCapabilities> {
        Ok(self.tree.capabilities.clone())
    }

    fn snapshot(&self) -> CoreResult<HardwareTree> {
        self.cache.get_or_build(|| Ok(self.tree.clone()))
    }

    fn start_notifications(&mut self) -> CoreResult<()> {
        self.notifications = true;
        tracing::info!("mock hardware notifications enabled");
        Ok(())
    }

    fn poll_invalidated(&mut self) -> bool {
        let ready = self.cache.poll_invalidated();
        if ready {
            tracing::info!(
                revision = self.tree.revision,
                "mock hot-plug debounce ready; snapshot cache invalidated"
            );
        }
        ready
    }
}

/// Mock session backend that allocates monotonic session ids.
///
/// [`SessionBackend::start_agent`] tries to spawn `multiseat-session-agent`.
/// If the binary is missing and `MULTISEAT_REQUIRE_AGENT` is unset, the start
/// still succeeds so CI/orchestrator tests remain usable.
pub struct MockSessionBackend {
    next_id: u32,
    sessions: HashMap<SeatId, SessionId>,
    agents: HashMap<SeatId, Child>,
    /// Daemon bind/connect string passed to spawned agents as `--addr`.
    daemon_addr: Option<String>,
    /// When true, [`SessionBackend::probe`] returns false.
    pub probe_ok: bool,
}

impl std::fmt::Debug for MockSessionBackend {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("MockSessionBackend")
            .field("next_id", &self.next_id)
            .field("sessions", &self.sessions)
            .field("agents", &self.agents.len())
            .field("daemon_addr", &self.daemon_addr)
            .field("probe_ok", &self.probe_ok)
            .finish()
    }
}

impl Default for MockSessionBackend {
    fn default() -> Self {
        Self {
            next_id: 10,
            sessions: HashMap::new(),
            agents: HashMap::new(),
            daemon_addr: None,
            probe_ok: false,
        }
    }
}

impl MockSessionBackend {
    /// Creates a backend that reports probe success.
    pub fn available() -> Self {
        Self {
            next_id: 10,
            sessions: HashMap::new(),
            agents: HashMap::new(),
            daemon_addr: None,
            probe_ok: true,
        }
    }

    /// Sets the daemon endpoint used when spawning session agents.
    pub fn set_daemon_addr(&mut self, addr: impl Into<String>) {
        let addr = addr.into();
        self.daemon_addr = if addr.is_empty() { None } else { Some(addr) };
    }

    /// Number of tracked agent child processes (tests).
    pub fn agent_count(&self) -> usize {
        self.agents.len()
    }
}

impl Drop for MockSessionBackend {
    fn drop(&mut self) {
        for (_, mut child) in self.agents.drain() {
            stop_agent_child(&mut child);
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
        if mock_fail_stage_is("session_create") {
            return Err(mock_fail_stage_err("session_create"));
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
        if mock_fail_stage_is("agent_start") {
            return Err(mock_fail_stage_err("agent_start"));
        }
        if let Some(mut prev) = self.agents.remove(&session.seat_id) {
            stop_agent_child(&mut prev);
        }

        match spawn_session_agent(session.seat_id, self.daemon_addr.as_deref()) {
            Ok(child) => {
                info!(
                    seat = %session.seat_id,
                    pid = child.id(),
                    "mock session-agent spawned"
                );
                self.agents.insert(session.seat_id, child);
                Ok(())
            }
            Err(err) if !require_agent() => {
                warn!(
                    seat = %session.seat_id,
                    error = %err,
                    "mock start_agent: binary unavailable; continuing without child (set MULTISEAT_REQUIRE_AGENT=1 to fail)"
                );
                Ok(())
            }
            Err(err) => Err(err),
        }
    }

    fn lock(&mut self, _session: &SessionHandle) -> CoreResult<()> {
        Ok(())
    }

    fn disconnect(&mut self, session: &SessionHandle) -> CoreResult<()> {
        if let Some(mut child) = self.agents.remove(&session.seat_id) {
            stop_agent_child(&mut child);
        }
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

#[cfg(test)]
mod tests {
    use super::*;
    use multiseat_core::SeatConfig;
    use std::thread;
    use std::time::Duration;

    fn sample_seat() -> SeatConfig {
        SeatConfig {
            id: SeatId(1),
            user_sid: multiseat_core::UserSid("S-1-5-21-1".into()),
            display_ids: vec![],
            keyboard_ids: vec![],
            pointer_ids: vec![],
            enabled: true,
        }
    }

    #[test]
    fn start_agent_soft_ok_without_binary() {
        let mut backend = MockSessionBackend::available();
        backend.set_daemon_addr("127.0.0.1:17890");
        let session = backend.create_or_connect(&sample_seat()).unwrap();
        // PATH name fallback usually cannot spawn in CI → soft Ok unless REQUIRE set.
        backend.start_agent(&session).unwrap();
        backend.disconnect(&session).unwrap();
        assert_eq!(backend.agent_count(), 0);
    }

    #[test]
    fn start_agent_unknown_session_errs() {
        let mut backend = MockSessionBackend::available();
        let session = SessionHandle {
            seat_id: SeatId(99),
            session_id: SessionId(1),
        };
        assert!(backend.start_agent(&session).is_err());
    }

    #[test]
    fn fail_stage_env_injects_backend_errors() {
        let _env = crate::test_env::lock();
        let seat = sample_seat();

        std::env::set_var(MOCK_FAIL_STAGE_ENV, "session_create");
        let mut sessions = MockSessionBackend::available();
        assert!(sessions.create_or_connect(&seat).is_err());

        std::env::set_var(MOCK_FAIL_STAGE_ENV, "agent_start");
        let session = sessions.create_or_connect(&seat).unwrap();
        let err = sessions.start_agent(&session).unwrap_err();
        assert!(err.to_string().contains("agent_start"));

        std::env::set_var(MOCK_FAIL_STAGE_ENV, "display_prepare");
        let mut displays = MockDisplayBackend::default();
        assert!(displays.prepare(&[seat.clone()]).is_err());

        std::env::set_var(MOCK_FAIL_STAGE_ENV, "display_attach");
        let session = sessions.create_or_connect(&seat).unwrap();
        assert!(displays.attach(&seat, &session).is_err());

        std::env::set_var(MOCK_FAIL_STAGE_ENV, "input_apply");
        let mut router = MockInputRouter::default();
        let assignments = InputAssignments {
            config: multiseat_core::MultiseatConfig::default(),
        };
        let err = router.apply(&assignments).unwrap_err();
        assert!(err.to_string().contains("input_apply"));
        // Sticky fail-open is NOT engaged by the injected stage failure.
        assert!(!router.is_fail_open());

        std::env::remove_var(MOCK_FAIL_STAGE_ENV);
        let session = sessions.create_or_connect(&seat).unwrap();
        assert!(displays.attach(&seat, &session).is_ok());
    }

    #[test]
    fn hotplug_signal_debounces_then_rescans_with_new_revision() {
        let mut hw = MockHardwareEnumerator::with_lab_fixture();
        // Short window for the test: rebuild enumerator with fast gate via bump path.
        // Replace cache gate by constructing with custom debounce through bump + sleep.
        let first = hw.snapshot().unwrap();
        assert_eq!(first.revision, 1);
        // Cached hit — same revision without invalidate.
        assert_eq!(hw.snapshot().unwrap().revision, 1);

        hw.bump_revision();
        assert!(!hw.poll_invalidated(), "quiet window not elapsed yet");
        thread::sleep(Duration::from_millis(350));
        assert!(hw.poll_invalidated());
        let second = hw.snapshot().unwrap();
        assert_eq!(second.revision, 2);
        assert_eq!(hw.snapshot().unwrap().revision, 2, "cache hit after rescan");
    }

    #[test]
    fn lab_fixture_hid_ids_carry_interface_and_container() {
        let hw = MockHardwareEnumerator::with_lab_fixture();
        let tree = hw.snapshot().unwrap();
        let seat1_kbd = tree
            .input_devices
            .iter()
            .find(|d| d.name == "Seat 1 Keyboard")
            .unwrap();
        let seat1_ptr = tree
            .input_devices
            .iter()
            .find(|d| d.name == "Seat 1 Pointer")
            .unwrap();
        assert!(seat1_kbd.id.interface_path.is_some());
        assert!(seat1_kbd.id.container_id.is_some());
        // Composite kit: keyboard + pointer share container, not the same HID node.
        assert!(!seat1_kbd.id.same_device(&seat1_ptr.id));
        assert!(seat1_kbd.id.same_container(&seat1_ptr.id));

        // After "hot-plug", same interface path still identifies the collection.
        let resurfaced = DeviceId::new(
            "HID\\VID_BBBB&PID_0001\\SEAT1_KBD_REOPEN",
            seat1_kbd.id.interface_path.clone(),
            seat1_kbd.id.container_id.clone(),
        );
        assert!(seat1_kbd.id.same_device(&resurfaced));
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
        if mock_fail_stage_is("display_prepare") {
            return Err(mock_fail_stage_err("display_prepare"));
        }
        Ok(())
    }

    fn attach(&mut self, seat: &SeatConfig, _session: &SessionHandle) -> CoreResult<RollbackToken> {
        if mock_fail_stage_is("display_attach") {
            return Err(mock_fail_stage_err("display_attach"));
        }
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
    /// Sticky fail-open mode (blocks later apply until cleared). Prefer
    /// [`InputRouter::force_fail_open`] for emergency route clear.
    pub fn set_fail_open(&mut self, enabled: bool) {
        self.fail_open = enabled;
        if enabled {
            self.active = None;
        }
    }

    /// Whether sticky fail-open is engaged (tests).
    pub fn is_fail_open(&self) -> bool {
        self.fail_open
    }

    /// Whether routes are currently applied (tests).
    pub fn has_active_routes(&self) -> bool {
        self.active.is_some()
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
        if mock_fail_stage_is("input_apply") {
            return Err(mock_fail_stage_err("input_apply"));
        }
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

    fn force_fail_open(&mut self) {
        // Clear routes without sticky disable so Start can recover after emergency.
        self.active = None;
        // Best-effort: notify KMDF control device if a lab filter is present.
        let _ = crate::input::emergency_fail_open();
    }
}

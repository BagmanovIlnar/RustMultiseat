//! Transactional seat start/stop orchestration.

use crate::error::{CoreError, CoreResult};
use crate::hardware::HardwareTree;
use crate::journal::{JournalEntry, OperationJournal};
use crate::remap::{remap_config, RemapReport};
use crate::seat::{MultiseatConfig, SeatState};
use crate::traits::{
    DisplayBackend, HardwareEnumerator, InputAssignments, InputRouter, SessionBackend,
    SessionHandle,
};
use crate::validation::{validate_config, ValidationReport};

/// Coordinates hardware, session, display and input backends for one host.
pub struct SeatOrchestrator<H, S, D, I> {
    hardware: H,
    sessions: S,
    displays: D,
    inputs: I,
    state: SeatState,
    config: MultiseatConfig,
    journal: Option<OperationJournal>,
    active_sessions: Vec<SessionHandle>,
}

impl<H, S, D, I> SeatOrchestrator<H, S, D, I>
where
    H: HardwareEnumerator,
    S: SessionBackend,
    D: DisplayBackend,
    I: InputRouter,
{
    /// Creates an orchestrator in `Idle` with empty configuration.
    pub fn new(hardware: H, sessions: S, displays: D, inputs: I) -> Self {
        Self {
            hardware,
            sessions,
            displays,
            inputs,
            state: SeatState::Idle,
            config: MultiseatConfig::default(),
            journal: None,
            active_sessions: Vec::new(),
        }
    }

    /// Current aggregated state.
    pub fn state(&self) -> &SeatState {
        &self.state
    }

    /// Saved configuration.
    pub fn config(&self) -> &MultiseatConfig {
        &self.config
    }

    /// Hardware enumerator access for snapshots.
    pub fn hardware(&self) -> &H {
        &self.hardware
    }

    /// Mutable hardware enumerator for notifications.
    pub fn hardware_mut(&mut self) -> &mut H {
        &mut self.hardware
    }

    /// Current operation journal, if any.
    pub fn journal(&self) -> Option<&OperationJournal> {
        self.journal.as_ref()
    }

    /// Adopts a durable journal loaded from disk before recovery.
    pub fn adopt_journal(&mut self, journal: OperationJournal) {
        if journal.active && journal.completed {
            self.state = SeatState::Active;
            self.config = journal.config.clone();
        } else if !journal.completed {
            self.state = SeatState::Failed {
                error: "incomplete journal loaded from disk".into(),
            };
            self.config = journal.config.clone();
        }
        self.journal = Some(journal);
    }

    /// Validates configuration against the current hardware snapshot.
    ///
    /// Device ids are rematched (`same_device` / `same_container`) before
    /// validation so hot-plug instance churn does not spuriously fail.
    pub fn validate_config(&self, config: &MultiseatConfig) -> CoreResult<ValidationReport> {
        let hardware = self.hardware.snapshot()?;
        let remapped = remap_config(config, &hardware);
        let report = validate_config(&remapped.config, &hardware);
        if report.has_errors() {
            return Err(CoreError::Validation(report.clone()));
        }
        Ok(report)
    }

    /// Rematches the saved config against `hardware` and updates it in memory.
    pub fn apply_hardware_remap(&mut self, hardware: &HardwareTree) -> RemapReport {
        let report = remap_config(&self.config, hardware);
        if report.changed() {
            self.config = report.config.clone();
        }
        report
    }

    /// Persists a previously validated configuration (rematched to live hardware).
    pub fn save_config(&mut self, config: MultiseatConfig) -> CoreResult<()> {
        let hardware = self.hardware.snapshot()?;
        let remapped = remap_config(&config, &hardware);
        let report = validate_config(&remapped.config, &hardware);
        if report.has_errors() {
            return Err(CoreError::Validation(report));
        }
        self.config = remapped.config;
        Ok(())
    }

    /// Starts multiseat using the provided configuration.
    ///
    /// Order: validate → snapshot → sessions → agents → displays → inputs.
    /// Any failure rolls back already applied steps.
    pub fn start(&mut self, config: MultiseatConfig) -> CoreResult<()> {
        if !self.state.can_start() {
            return Err(CoreError::InvalidState(format!(
                "cannot start from {:?}",
                self.state
            )));
        }

        self.state = SeatState::Validating;
        let hardware_for_remap = match self.hardware.snapshot() {
            Ok(tree) => tree,
            Err(err) => {
                self.state = SeatState::Failed {
                    error: err.to_string(),
                };
                return Err(err);
            }
        };
        let config = remap_config(&config, &hardware_for_remap).config;
        let report = validate_config(&config, &hardware_for_remap);
        if report.has_errors() {
            let err = CoreError::Validation(report);
            self.state = SeatState::Failed {
                error: err.to_string(),
            };
            return Err(err);
        }

        if !self.sessions.probe().unwrap_or(false) {
            self.state = SeatState::Failed {
                error: "session backend probe failed".into(),
            };
            return Err(CoreError::Backend(
                "session backend is not available on this host".into(),
            ));
        }

        let input_health = self.inputs.health()?;
        if !input_health.healthy {
            self.state = SeatState::Failed {
                error: input_health.detail.clone(),
            };
            return Err(CoreError::Backend(input_health.detail));
        }

        self.state = SeatState::Applying;
        self.config = config.clone();
        let mut journal = OperationJournal::begin(config.clone());
        journal.push(JournalEntry::HostSnapshotSaved);

        let enabled: Vec<_> = config.seats.iter().filter(|s| s.enabled).cloned().collect();

        if let Err(err) = self.displays.prepare(&enabled) {
            self.state = SeatState::Failed {
                error: err.to_string(),
            };
            return Err(err);
        }

        let mut created_sessions = Vec::new();
        for seat in &enabled {
            match self.sessions.create_or_connect(seat) {
                Ok(session) => {
                    journal.push(JournalEntry::SessionCreated {
                        seat_id: seat.id,
                        session_id: session.session_id,
                    });
                    match self.sessions.start_agent(&session) {
                        Ok(()) => {
                            journal.push(JournalEntry::AgentStarted {
                                seat_id: seat.id,
                                session_id: session.session_id,
                            });
                            created_sessions.push((seat.clone(), session));
                        }
                        Err(err) => return Err(self.fail_start(journal, err)),
                    }
                }
                Err(err) => return Err(self.fail_start(journal, err)),
            }
        }

        for (seat, session) in &created_sessions {
            match self.displays.attach(seat, session) {
                Ok(token) => {
                    journal.push(JournalEntry::DisplayAttached {
                        seat_id: seat.id,
                        token,
                    });
                }
                Err(err) => return Err(self.fail_start(journal, err)),
            }
        }

        let assignments = InputAssignments {
            config: config.clone(),
        };
        match self.inputs.apply(&assignments) {
            Ok(token) => {
                journal.push(JournalEntry::InputRoutesApplied { token });
            }
            Err(err) => return Err(self.fail_start(journal, err)),
        }

        journal.mark_completed();
        self.active_sessions = created_sessions.into_iter().map(|(_, s)| s).collect();
        self.journal = Some(journal);
        self.state = SeatState::Active;
        Ok(())
    }

    /// Stops multiseat: fail-open inputs first, then displays, then sessions.
    pub fn stop(&mut self) -> CoreResult<()> {
        if !self.state.can_stop() && !matches!(self.state, SeatState::Idle) {
            return Err(CoreError::InvalidState(format!(
                "cannot stop from {:?}",
                self.state
            )));
        }

        if matches!(self.state, SeatState::Idle | SeatState::Disabled) {
            return Ok(());
        }

        self.state = SeatState::RollingBack;
        // Belt-and-suspenders: clear input routes before journal compensation.
        self.inputs.force_fail_open();
        let result = self.rollback_internal();
        self.active_sessions.clear();
        self.journal = None;
        self.state = SeatState::Idle;
        result
    }

    /// Recovers after a crash using the in-memory / adopted journal.
    ///
    /// Incomplete journals are rolled back. Completed+active journals perform
    /// a clean stop so host input returns to fail-open.
    pub fn recover(&mut self) -> CoreResult<()> {
        let needs = self.journal.as_ref().is_some_and(|j| j.needs_recovery());
        if !needs {
            return Ok(());
        }

        self.state = SeatState::RollingBack;
        self.inputs.force_fail_open();
        self.rollback_internal()?;
        self.active_sessions.clear();
        self.journal = None;
        self.state = SeatState::Idle;
        Ok(())
    }

    fn fail_start(&mut self, journal: OperationJournal, err: CoreError) -> CoreError {
        self.journal = Some(journal);
        let _ = self.rollback_internal();
        self.journal = None;
        self.active_sessions.clear();
        self.state = SeatState::Failed {
            error: err.to_string(),
        };
        err
    }

    fn rollback_internal(&mut self) -> CoreResult<()> {
        let Some(journal) = self.journal.clone() else {
            return Ok(());
        };

        for entry in journal.rollback_order() {
            match entry {
                JournalEntry::InputRoutesApplied { token } => {
                    self.inputs.rollback(token)?;
                }
                JournalEntry::DisplayAttached { seat_id, token } => {
                    let _ = self.displays.detach(*seat_id);
                    self.displays.rollback(token)?;
                }
                JournalEntry::AgentStarted { .. } => {
                    // Agent lifetime is tied to session disconnect for now.
                }
                JournalEntry::SessionCreated {
                    seat_id,
                    session_id,
                } => {
                    let handle = SessionHandle {
                        seat_id: *seat_id,
                        session_id: *session_id,
                    };
                    let _ = self.sessions.lock(&handle);
                    self.sessions.disconnect(&handle)?;
                }
                JournalEntry::HostSnapshotSaved => {}
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::error::CoreError;
    use crate::hardware::{
        DisplayInfo, HardwareTree, InputDeviceInfo, InputDeviceKind, PlatformCapabilities,
    };
    use crate::ids::{DeviceId, DisplayId, SeatId, SessionId, UserSid};
    use crate::journal::RollbackToken;
    use crate::seat::SeatConfig;
    use crate::traits::{BackendHealth, DisplayCapabilities};
    use std::cell::{Cell, RefCell};
    use std::rc::Rc;

    struct MockHw {
        tree: HardwareTree,
    }

    impl HardwareEnumerator for MockHw {
        fn capabilities(&self) -> CoreResult<PlatformCapabilities> {
            Ok(self.tree.capabilities.clone())
        }
        fn snapshot(&self) -> CoreResult<HardwareTree> {
            Ok(self.tree.clone())
        }
        fn start_notifications(&mut self) -> CoreResult<()> {
            Ok(())
        }
    }

    /// Session mock with op-log and per-stage failure switches (fault injection).
    #[derive(Default)]
    struct MockSessions {
        fail_on_create: bool,
        fail_on_agent: bool,
        next_id: u32,
        connected: Vec<SessionId>,
        ops: Rc<RefCell<Vec<String>>>,
    }

    impl MockSessions {
        fn op(&self, entry: &str) {
            self.ops.borrow_mut().push(entry.to_string());
        }
    }

    impl SessionBackend for MockSessions {
        fn probe(&self) -> CoreResult<bool> {
            Ok(true)
        }
        fn create_or_connect(&mut self, seat: &SeatConfig) -> CoreResult<SessionHandle> {
            if self.fail_on_create {
                self.op(&format!("create_fail:{}", seat.id));
                return Err(CoreError::Backend("create failed".into()));
            }
            self.next_id += 1;
            let id = SessionId(self.next_id);
            self.connected.push(id);
            self.op(&format!("create:{}", seat.id));
            Ok(SessionHandle {
                seat_id: seat.id,
                session_id: id,
            })
        }
        fn start_agent(&mut self, session: &SessionHandle) -> CoreResult<()> {
            if self.fail_on_agent {
                self.op(&format!("agent_fail:{}", session.seat_id));
                return Err(CoreError::Backend("agent start failed".into()));
            }
            self.op(&format!("agent:{}", session.seat_id));
            Ok(())
        }
        fn lock(&mut self, _session: &SessionHandle) -> CoreResult<()> {
            Ok(())
        }
        fn disconnect(&mut self, session: &SessionHandle) -> CoreResult<()> {
            self.connected.retain(|id| *id != session.session_id);
            self.op(&format!("disconnect:{}", session.seat_id));
            Ok(())
        }
        fn health(&self, _session: &SessionHandle) -> CoreResult<BackendHealth> {
            Ok(BackendHealth::ok())
        }
    }

    /// Display mock with op-log and per-stage failure switches.
    #[derive(Default)]
    struct MockDisplay {
        attached: Vec<SeatId>,
        fail_prepare: bool,
        fail_attach: bool,
        ops: Rc<RefCell<Vec<String>>>,
    }

    impl MockDisplay {
        fn op(&self, entry: &str) {
            self.ops.borrow_mut().push(entry.to_string());
        }
    }

    impl DisplayBackend for MockDisplay {
        fn capabilities(&self) -> CoreResult<DisplayCapabilities> {
            Ok(DisplayCapabilities {
                backend_name: "mock".into(),
                attach_supported: true,
            })
        }
        fn prepare(&mut self, _seats: &[SeatConfig]) -> CoreResult<()> {
            if self.fail_prepare {
                self.op("prepare_fail");
                return Err(CoreError::Backend("prepare failed".into()));
            }
            self.op("prepare");
            Ok(())
        }
        fn attach(
            &mut self,
            seat: &SeatConfig,
            _session: &SessionHandle,
        ) -> CoreResult<RollbackToken> {
            if self.fail_attach {
                self.op(&format!("attach_fail:{}", seat.id));
                return Err(CoreError::Backend("attach failed".into()));
            }
            self.attached.push(seat.id);
            self.op(&format!("attach:{}", seat.id));
            Ok(RollbackToken::new(format!("disp-{}", seat.id)))
        }
        fn detach(&mut self, seat_id: SeatId) -> CoreResult<()> {
            self.attached.retain(|id| *id != seat_id);
            self.op(&format!("detach:{}", seat_id));
            Ok(())
        }
        fn rollback(&mut self, token: &RollbackToken) -> CoreResult<()> {
            self.op(&format!("rollback:{}", token.0));
            Ok(())
        }
    }

    /// Input mock with toggleable failure (mid-test fault clearing).
    struct MockInput {
        applied: Rc<RefCell<bool>>,
        fail: Rc<Cell<bool>>,
        ops: Rc<RefCell<Vec<String>>>,
    }

    impl MockInput {
        fn ok(applied: &Rc<RefCell<bool>>) -> Self {
            Self {
                applied: Rc::clone(applied),
                fail: Rc::new(Cell::new(false)),
                ops: Rc::new(RefCell::new(Vec::new())),
            }
        }

        fn failing(applied: &Rc<RefCell<bool>>) -> Self {
            let fail = Rc::new(Cell::new(true));
            Self {
                fail,
                ..Self::ok(applied)
            }
        }

        fn op(&self, entry: &str) {
            self.ops.borrow_mut().push(entry.to_string());
        }
    }

    impl InputRouter for MockInput {
        fn validate(&self, _assignments: &InputAssignments) -> CoreResult<()> {
            Ok(())
        }
        fn apply(&mut self, _assignments: &InputAssignments) -> CoreResult<RollbackToken> {
            if self.fail.get() {
                self.op("input_apply_fail");
                return Err(CoreError::Backend("apply failed".into()));
            }
            *self.applied.borrow_mut() = true;
            self.op("input_apply");
            Ok(RollbackToken::new("input"))
        }
        fn rollback(&mut self, _token: &RollbackToken) -> CoreResult<()> {
            *self.applied.borrow_mut() = false;
            self.op("input_rollback");
            Ok(())
        }
        fn health(&self) -> CoreResult<BackendHealth> {
            Ok(BackendHealth::ok())
        }
    }

    fn sample_tree() -> HardwareTree {
        HardwareTree {
            revision: 1,
            gpus: vec![],
            displays: vec![
                DisplayInfo {
                    id: DisplayId("d1".into()),
                    gpu_id: "g0".into(),
                    width: Some(1920),
                    height: Some(1080),
                    attached: true,
                    virtual_display: false,
                },
                DisplayInfo {
                    id: DisplayId("d2".into()),
                    gpu_id: "g0".into(),
                    width: Some(1280),
                    height: Some(1024),
                    attached: true,
                    virtual_display: false,
                },
            ],
            usb_roots: vec![],
            input_devices: vec![
                InputDeviceInfo {
                    id: DeviceId::from_instance("kbd-host"),
                    kind: InputDeviceKind::Keyboard,
                    name: "hk".into(),
                    vendor_id: None,
                    product_id: None,
                },
                InputDeviceInfo {
                    id: DeviceId::from_instance("ptr-host"),
                    kind: InputDeviceKind::Pointer,
                    name: "hp".into(),
                    vendor_id: None,
                    product_id: None,
                },
                InputDeviceInfo {
                    id: DeviceId::from_instance("kbd-1"),
                    kind: InputDeviceKind::Keyboard,
                    name: "k1".into(),
                    vendor_id: None,
                    product_id: None,
                },
                InputDeviceInfo {
                    id: DeviceId::from_instance("ptr-1"),
                    kind: InputDeviceKind::Pointer,
                    name: "p1".into(),
                    vendor_id: None,
                    product_id: None,
                },
                InputDeviceInfo {
                    id: DeviceId::from_instance("kbd-2"),
                    kind: InputDeviceKind::Keyboard,
                    name: "k2".into(),
                    vendor_id: None,
                    product_id: None,
                },
                InputDeviceInfo {
                    id: DeviceId::from_instance("ptr-2"),
                    kind: InputDeviceKind::Pointer,
                    name: "p2".into(),
                    vendor_id: None,
                    product_id: None,
                },
            ],
            capabilities: PlatformCapabilities {
                os: "mock".into(),
                os_version: "1".into(),
                display_backend_available: true,
                session_backend_available: true,
                input_router_available: true,
                secure_boot_expected: true,
            },
        }
    }

    fn seat(n: u32) -> SeatConfig {
        SeatConfig {
            id: SeatId(n),
            user_sid: UserSid(format!("S-1-5-21-{n}")),
            display_ids: vec![DisplayId(format!("d{n}"))],
            keyboard_ids: vec![DeviceId::from_instance(format!("kbd-{n}"))],
            pointer_ids: vec![DeviceId::from_instance(format!("ptr-{n}"))],
            enabled: true,
        }
    }

    fn sample_config() -> MultiseatConfig {
        MultiseatConfig {
            seats: vec![seat(1)],
            emergency_keyboard: Some(DeviceId::from_instance("kbd-host")),
            emergency_pointer: Some(DeviceId::from_instance("ptr-host")),
        }
    }

    fn sample_config_two_seats() -> MultiseatConfig {
        MultiseatConfig {
            seats: vec![seat(1), seat(2)],
            emergency_keyboard: Some(DeviceId::from_instance("kbd-host")),
            emergency_pointer: Some(DeviceId::from_instance("ptr-host")),
        }
    }

    #[test]
    fn start_and_stop_happy_path() {
        let applied = Rc::new(RefCell::new(false));
        let mut orch = SeatOrchestrator::new(
            MockHw {
                tree: sample_tree(),
            },
            MockSessions::default(),
            MockDisplay::default(),
            MockInput::ok(&applied),
        );

        orch.start(sample_config()).unwrap();
        assert_eq!(orch.state(), &SeatState::Active);
        assert!(*applied.borrow());

        orch.stop().unwrap();
        assert_eq!(orch.state(), &SeatState::Idle);
        assert!(!*applied.borrow());
    }

    #[test]
    fn rolls_back_when_input_apply_fails() {
        let applied = Rc::new(RefCell::new(false));
        let mut orch = SeatOrchestrator::new(
            MockHw {
                tree: sample_tree(),
            },
            MockSessions::default(),
            MockDisplay::default(),
            MockInput::failing(&applied),
        );

        let err = orch.start(sample_config()).unwrap_err();
        assert!(matches!(err, CoreError::Backend(_)));
        assert!(!*applied.borrow());
        assert!(matches!(orch.state(), SeatState::Failed { .. }));
    }

    #[test]
    fn rolls_back_when_session_create_fails() {
        let applied = Rc::new(RefCell::new(false));
        let mut orch = SeatOrchestrator::new(
            MockHw {
                tree: sample_tree(),
            },
            MockSessions {
                fail_on_create: true,
                ..MockSessions::default()
            },
            MockDisplay::default(),
            MockInput::ok(&applied),
        );

        assert!(orch.start(sample_config()).is_err());
        assert!(!*applied.borrow());
        assert!(matches!(orch.state(), SeatState::Failed { .. }));
    }

    #[test]
    fn rolls_back_when_display_prepare_fails() {
        let applied = Rc::new(RefCell::new(false));
        let sessions = MockSessions::default();
        let session_ops = Rc::clone(&sessions.ops);
        let mut orch = SeatOrchestrator::new(
            MockHw {
                tree: sample_tree(),
            },
            sessions,
            MockDisplay {
                fail_prepare: true,
                ..MockDisplay::default()
            },
            MockInput::ok(&applied),
        );

        assert!(orch.start(sample_config()).is_err());
        assert!(matches!(orch.state(), SeatState::Failed { .. }));
        // Failure before Applying-later steps: no session may be created.
        assert!(session_ops.borrow().is_empty());
    }

    #[test]
    fn rolls_back_when_agent_start_fails() {
        let applied = Rc::new(RefCell::new(false));
        let sessions = MockSessions {
            fail_on_agent: true,
            ..MockSessions::default()
        };
        let session_ops = Rc::clone(&sessions.ops);
        let mut orch = SeatOrchestrator::new(
            MockHw {
                tree: sample_tree(),
            },
            sessions,
            MockDisplay::default(),
            MockInput::ok(&applied),
        );

        assert!(orch.start(sample_config()).is_err());
        assert!(matches!(orch.state(), SeatState::Failed { .. }));
        // The already-created session must be compensated by disconnect.
        assert_eq!(
            *session_ops.borrow(),
            vec![
                "create:seat-1".to_string(),
                "agent_fail:seat-1".to_string(),
                "disconnect:seat-1".to_string()
            ]
        );
    }

    #[test]
    fn rolls_back_when_display_attach_fails() {
        let applied = Rc::new(RefCell::new(false));
        let sessions = MockSessions::default();
        let session_ops = Rc::clone(&sessions.ops);
        let displays = MockDisplay {
            fail_attach: true,
            ..MockDisplay::default()
        };
        let display_ops = Rc::clone(&displays.ops);
        let mut orch = SeatOrchestrator::new(
            MockHw {
                tree: sample_tree(),
            },
            sessions,
            displays,
            MockInput::ok(&applied),
        );

        assert!(orch.start(sample_config()).is_err());
        assert!(matches!(orch.state(), SeatState::Failed { .. }));
        // Input routes were never applied…
        assert!(!*applied.borrow());
        // …the failing attach left no display attached…
        assert_eq!(*display_ops.borrow(), vec!["prepare", "attach_fail:seat-1"]);
        // …and the created session was compensated.
        assert_eq!(
            *session_ops.borrow(),
            vec![
                "create:seat-1".to_string(),
                "agent:seat-1".to_string(),
                "disconnect:seat-1".to_string()
            ]
        );
    }

    #[test]
    fn rollback_compensates_in_reverse_order() {
        let applied = Rc::new(RefCell::new(false));
        let sessions = MockSessions::default();
        let session_ops = Rc::clone(&sessions.ops);
        let displays = MockDisplay::default();
        let display_ops = Rc::clone(&displays.ops);
        let input = MockInput::failing(&applied);
        let input_ops = Rc::clone(&input.ops);
        let mut orch = SeatOrchestrator::new(
            MockHw {
                tree: sample_tree(),
            },
            sessions,
            displays,
            input,
        );

        // Fail at the LAST step (input apply) after two seats fully applied.
        assert!(orch.start(sample_config_two_seats()).is_err());
        assert!(matches!(orch.state(), SeatState::Failed { .. }));

        // Compensations run in exact reverse journal order:
        // displays (2→1), then sessions (2→1); input never applied.
        assert_eq!(
            *display_ops.borrow(),
            vec![
                "prepare",
                "attach:seat-1",
                "attach:seat-2",
                "detach:seat-2",
                "rollback:disp-seat-2",
                "detach:seat-1",
                "rollback:disp-seat-1",
            ]
        );
        assert_eq!(
            *session_ops.borrow(),
            vec![
                "create:seat-1",
                "agent:seat-1",
                "create:seat-2",
                "agent:seat-2",
                "disconnect:seat-2",
                "disconnect:seat-1",
            ]
        );
        assert_eq!(*input_ops.borrow(), vec!["input_apply_fail"]);
    }

    #[test]
    fn retry_after_failed_start_succeeds() {
        let applied = Rc::new(RefCell::new(false));
        let input = MockInput::failing(&applied);
        let fail_switch = Rc::clone(&input.fail);
        let mut orch = SeatOrchestrator::new(
            MockHw {
                tree: sample_tree(),
            },
            MockSessions::default(),
            MockDisplay::default(),
            input,
        );

        assert!(orch.start(sample_config()).is_err());
        assert!(matches!(orch.state(), SeatState::Failed { .. }));

        // Clear the fault: a subsequent start must fully recover.
        fail_switch.set(false);
        orch.start(sample_config()).unwrap();
        assert_eq!(orch.state(), &SeatState::Active);
        assert!(*applied.borrow());

        orch.stop().unwrap();
        assert_eq!(orch.state(), &SeatState::Idle);
    }
}

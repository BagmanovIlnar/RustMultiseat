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
    use std::cell::RefCell;
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

    #[derive(Default)]
    struct MockSessions {
        fail_on_create: bool,
        next_id: u32,
        connected: Vec<SessionId>,
    }

    impl SessionBackend for MockSessions {
        fn probe(&self) -> CoreResult<bool> {
            Ok(true)
        }
        fn create_or_connect(&mut self, seat: &SeatConfig) -> CoreResult<SessionHandle> {
            if self.fail_on_create {
                return Err(CoreError::Backend("create failed".into()));
            }
            self.next_id += 1;
            let id = SessionId(self.next_id);
            self.connected.push(id);
            Ok(SessionHandle {
                seat_id: seat.id,
                session_id: id,
            })
        }
        fn start_agent(&mut self, _session: &SessionHandle) -> CoreResult<()> {
            Ok(())
        }
        fn lock(&mut self, _session: &SessionHandle) -> CoreResult<()> {
            Ok(())
        }
        fn disconnect(&mut self, session: &SessionHandle) -> CoreResult<()> {
            self.connected.retain(|id| *id != session.session_id);
            Ok(())
        }
        fn health(&self, _session: &SessionHandle) -> CoreResult<BackendHealth> {
            Ok(BackendHealth::ok())
        }
    }

    #[derive(Default)]
    struct MockDisplay {
        attached: Vec<SeatId>,
    }

    impl DisplayBackend for MockDisplay {
        fn capabilities(&self) -> CoreResult<DisplayCapabilities> {
            Ok(DisplayCapabilities {
                backend_name: "mock".into(),
                attach_supported: true,
            })
        }
        fn prepare(&mut self, _seats: &[SeatConfig]) -> CoreResult<()> {
            Ok(())
        }
        fn attach(
            &mut self,
            seat: &SeatConfig,
            _session: &SessionHandle,
        ) -> CoreResult<RollbackToken> {
            self.attached.push(seat.id);
            Ok(RollbackToken::new(format!("disp-{}", seat.id)))
        }
        fn detach(&mut self, seat_id: SeatId) -> CoreResult<()> {
            self.attached.retain(|id| *id != seat_id);
            Ok(())
        }
        fn rollback(&mut self, _token: &RollbackToken) -> CoreResult<()> {
            Ok(())
        }
    }

    struct MockInput {
        applied: Rc<RefCell<bool>>,
        fail_apply: bool,
    }

    impl InputRouter for MockInput {
        fn validate(&self, _assignments: &InputAssignments) -> CoreResult<()> {
            Ok(())
        }
        fn apply(&mut self, _assignments: &InputAssignments) -> CoreResult<RollbackToken> {
            if self.fail_apply {
                return Err(CoreError::Backend("apply failed".into()));
            }
            *self.applied.borrow_mut() = true;
            Ok(RollbackToken::new("input"))
        }
        fn rollback(&mut self, _token: &RollbackToken) -> CoreResult<()> {
            *self.applied.borrow_mut() = false;
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
            displays: vec![DisplayInfo {
                id: DisplayId("d1".into()),
                gpu_id: "g0".into(),
                width: Some(1920),
                height: Some(1080),
                attached: true,
                virtual_display: false,
            }],
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

    fn sample_config() -> MultiseatConfig {
        MultiseatConfig {
            seats: vec![SeatConfig {
                id: SeatId(1),
                user_sid: UserSid("S-1-5-21-1".into()),
                display_ids: vec![DisplayId("d1".into())],
                keyboard_ids: vec![DeviceId::from_instance("kbd-1")],
                pointer_ids: vec![DeviceId::from_instance("ptr-1")],
                enabled: true,
            }],
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
            MockInput {
                applied: Rc::clone(&applied),
                fail_apply: false,
            },
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
            MockInput {
                applied: Rc::clone(&applied),
                fail_apply: true,
            },
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
            MockInput {
                applied: Rc::clone(&applied),
                fail_apply: false,
            },
        );

        assert!(orch.start(sample_config()).is_err());
        assert!(!*applied.borrow());
        assert!(matches!(orch.state(), SeatState::Failed { .. }));
    }
}

//! Request handling against [`SeatOrchestrator`] and mock backends.

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Instant;

use multiseat_core::{
    ClientRequest, CoreError, DaemonEvent, FeasibilityReport, HardwareEnumerator, JournalStore,
    MultiseatConfig, ProtocolError, SeatId, SeatOrchestrator, SeatState, ServerResponse,
    PROTOCOL_VERSION,
};
use multiseat_core::InputDeviceKind;
use multiseat_platform_windows::{
    log_native_session_flags, probe_display, probe_dual_sessions, probe_input, HostDisplayBackend,
    HostHardwareEnumerator, HostInputRouter, HostSessionBackend,
};
use tokio::sync::{broadcast, Mutex};
use tracing::{info, warn};

use crate::agents::AgentRegistry;
use crate::persist::{FileConfigStore, FileJournalStore};

type HostOrchestrator = SeatOrchestrator<
    HostHardwareEnumerator,
    HostSessionBackend,
    HostDisplayBackend,
    HostInputRouter,
>;

/// Shared daemon state.
pub struct DaemonState {
    /// Orchestrator guarded for concurrent clients.
    pub orchestrator: Mutex<HostOrchestrator>,
    /// Broadcast channel for push events.
    pub events: broadcast::Sender<DaemonEvent>,
    /// Live session-agent heartbeats.
    pub agents: Mutex<AgentRegistry>,
    /// Durable journal path store.
    journal_store: FileJournalStore,
    /// Durable saved configuration.
    config_store: FileConfigStore,
}

impl DaemonState {
    /// Creates a daemon with mock backends and temp journal directory.
    pub fn mock() -> Arc<Self> {
        let dir = std::env::temp_dir().join("multiseat-daemon-dev");
        Self::mock_with_data_dir(dir)
    }

    /// Deterministic mock enumerator (tests / MULTISEAT_USE_MOCK workflows).
    pub fn mock_with_data_dir(data_dir: impl Into<PathBuf>) -> Arc<Self> {
        Self::with_enumerator(data_dir, HostHardwareEnumerator::mock(), None)
    }

    /// Production/dev daemon: native inventory on Windows, mock elsewhere.
    pub fn with_data_dir(data_dir: impl Into<PathBuf>) -> Arc<Self> {
        Self::with_data_dir_and_bind(data_dir, "")
    }

    /// Like [`Self::with_data_dir`], and configures session-agent `--addr` to `bind`.
    pub fn with_data_dir_and_bind(
        data_dir: impl Into<PathBuf>,
        bind: impl Into<String>,
    ) -> Arc<Self> {
        let bind = bind.into();
        let addr = if bind.is_empty() { None } else { Some(bind) };
        Self::with_enumerator(data_dir, HostHardwareEnumerator::auto(), addr)
    }

    fn with_enumerator(
        data_dir: impl Into<PathBuf>,
        mut hardware: HostHardwareEnumerator,
        daemon_addr: Option<String>,
    ) -> Arc<Self> {
        let data_dir = data_dir.into();
        let journal_store = FileJournalStore::in_dir(&data_dir);
        let config_store = FileConfigStore::in_dir(&data_dir);
        let (events, _) = broadcast::channel(64);
        let mut sessions = HostSessionBackend::matching_hardware(hardware.is_mock());
        if let Some(addr) = daemon_addr {
            info!(%addr, "session backends will spawn agents against daemon bind");
            sessions.set_daemon_addr(addr);
        }
        let snapshot = hardware.snapshot().ok();
        let output_count = snapshot
            .as_ref()
            .map(|t| t.displays.iter().filter(|d| d.attached).count())
            .unwrap_or(0);
        let keyboard_kits = snapshot
            .as_ref()
            .map(|t| {
                t.input_devices
                    .iter()
                    .filter(|d| d.kind == InputDeviceKind::Keyboard)
                    .count()
            })
            .unwrap_or(0);
        let pointer_kits = snapshot
            .as_ref()
            .map(|t| {
                t.input_devices
                    .iter()
                    .filter(|d| d.kind == InputDeviceKind::Pointer)
                    .count()
            })
            .unwrap_or(0);
        let displays = HostDisplayBackend::matching_hardware(hardware.is_mock(), output_count);
        let inputs =
            HostInputRouter::matching_hardware(hardware.is_mock(), keyboard_kits, pointer_kits);
        info!(
            hardware_mock = hardware.is_mock(),
            session_mock = sessions.is_mock(),
            display_mock = displays.is_mock(),
            input_mock = inputs.is_mock(),
            outputs = output_count,
            "backends selected"
        );
        log_native_session_flags();
        let _ = hardware.start_notifications();
        let mut orchestrator = SeatOrchestrator::new(hardware, sessions, displays, inputs);

        match config_store.load() {
            Ok(Some(config)) => {
                if let Err(err) = orchestrator.save_config(config) {
                    warn!(error = %err, "saved config failed validation on boot");
                } else {
                    info!("loaded saved configuration");
                }
            }
            Ok(None) => {}
            Err(err) => warn!(error = %err, "failed to load saved configuration"),
        }

        match journal_store.load() {
            Ok(Some(journal)) if journal.needs_recovery() => {
                warn!("incomplete/active journal found; running recover()");
                orchestrator.adopt_journal(journal);
                if let Err(err) = orchestrator.recover() {
                    warn!(error = %err, "journal recovery failed");
                } else if let Err(err) = journal_store.clear() {
                    warn!(error = %err, "failed to clear journal after recovery");
                } else {
                    info!("journal recovery completed");
                }
            }
            Ok(Some(_)) => {
                let _ = journal_store.clear();
            }
            Ok(None) => {}
            Err(err) => warn!(error = %err, "failed to load journal"),
        }

        let state = Arc::new(Self {
            orchestrator: Mutex::new(orchestrator),
            events,
            agents: Mutex::new(AgentRegistry::new()),
            journal_store,
            config_store,
        });
        Self::spawn_hotplug_poller(Arc::clone(&state));
        Self::spawn_agent_watchdog(Arc::clone(&state));
        state
    }

    fn spawn_agent_watchdog(state: Arc<Self>) {
        let Ok(handle) = tokio::runtime::Handle::try_current() else {
            return;
        };
        handle.spawn(async move {
            let mut ticker = tokio::time::interval(std::time::Duration::from_millis(500));
            ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
            loop {
                ticker.tick().await;
                let dead = {
                    let mut agents = state.agents.lock().await;
                    agents.reap_timed_out(Instant::now())
                };
                for seat_id in dead {
                    warn!(%seat_id, "session agent heartbeat timed out; fail-open");
                    let _ = state.events.send(DaemonEvent::AgentOffline {
                        seat_id,
                        reason: "heartbeat timeout".into(),
                    });
                    let _ = state.events.send(DaemonEvent::EmergencyFailOpen {
                        reason: format!("agent {seat_id} heartbeat timeout"),
                    });
                    let mut orch = state.orchestrator.lock().await;
                    if matches!(orch.state(), SeatState::Active | SeatState::Degraded { .. }) {
                        if let Err(err) = orch.stop() {
                            warn!(error = %err, "fail-open stop after agent timeout failed");
                        } else {
                            let _ = state.sync_journal(&orch);
                            let _ = state.events.send(DaemonEvent::SeatStateChanged {
                                seat_id: Some(seat_id),
                                state: SeatState::Idle,
                            });
                        }
                    }
                }
            }
        });
    }

    fn spawn_hotplug_poller(state: Arc<Self>) {
        let Ok(handle) = tokio::runtime::Handle::try_current() else {
            return;
        };
        handle.spawn(async move {
            let mut ticker = tokio::time::interval(std::time::Duration::from_millis(250));
            ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
            let mut last_published: Option<u64> = None;
            loop {
                ticker.tick().await;
                let mut orch = state.orchestrator.lock().await;
                // Event → DebounceGate quiet window → invalidate cache → rescan.
                if !orch.hardware_mut().poll_invalidated() {
                    continue;
                }
                match orch.hardware().snapshot() {
                    Ok(tree) => {
                        if last_published == Some(tree.revision) {
                            info!(
                                revision = tree.revision,
                                "hot-plug rescan skipped (revision unchanged)"
                            );
                            continue;
                        }
                        last_published = Some(tree.revision);
                        let remap = orch.apply_hardware_remap(&tree);
                        if remap.changed() {
                            info!(
                                remaps = remap.remaps.len(),
                                unresolved = remap.unresolved.len(),
                                "seat config rematched after hot-plug"
                            );
                            if let Err(err) = state.config_store.save(orch.config()) {
                                warn!(error = %err, "failed to persist rematched configuration");
                            }
                        }
                        info!(
                            revision = tree.revision,
                            gpus = tree.gpus.len(),
                            displays = tree.displays.len(),
                            inputs = tree.input_devices.len(),
                            "hot-plug rescan published"
                        );
                        let _ = state.events.send(DaemonEvent::HardwareUpdated(tree));
                    }
                    Err(err) => warn!(error = %err, "hot-plug rescan failed"),
                }
            }
        });
    }

    /// Persists or clears the journal to match orchestrator state.
    fn sync_journal(&self, orch: &HostOrchestrator) -> Result<(), CoreError> {
        match orch.journal() {
            Some(journal) => self.journal_store.save(journal),
            None => self.journal_store.clear(),
        }
    }

    /// Handles one client request.
    pub async fn handle(&self, request: ClientRequest) -> ServerResponse {
        match request {
            ClientRequest::Hello { client_version } => {
                if client_version != PROTOCOL_VERSION {
                    ServerResponse::Err(ProtocolError::IncompatibleVersion {
                        server: PROTOCOL_VERSION,
                        client: client_version,
                    })
                } else {
                    ServerResponse::HelloAck {
                        server_version: PROTOCOL_VERSION,
                    }
                }
            }
            ClientRequest::GetSnapshot => {
                let orch = self.orchestrator.lock().await;
                match orch.hardware().snapshot() {
                    Ok(hardware) => ServerResponse::Snapshot {
                        hardware: Box::new(hardware),
                        config: Box::new(orch.config().clone()),
                        state: orch.state().clone(),
                    },
                    Err(err) => ServerResponse::Err(ProtocolError::Application {
                        message: err.to_string(),
                    }),
                }
            }
            ClientRequest::GetFeasibilityReport => {
                let orch = self.orchestrator.lock().await;
                match orch.hardware().snapshot() {
                    Ok(hardware) => {
                        let session = probe_dual_sessions();
                        let outputs = hardware.displays.iter().filter(|d| d.attached).count();
                        let display = probe_display(outputs);
                        let keyboards = hardware
                            .input_devices
                            .iter()
                            .filter(|d| d.kind == InputDeviceKind::Keyboard)
                            .count();
                        let pointers = hardware
                            .input_devices
                            .iter()
                            .filter(|d| d.kind == InputDeviceKind::Pointer)
                            .count();
                        let input = probe_input(keyboards, pointers);
                        ServerResponse::Feasibility(FeasibilityReport::from_hardware_and_probes(
                            &hardware,
                            Some(&session),
                            Some(&display),
                            Some(&input),
                        ))
                    }
                    Err(err) => ServerResponse::Err(ProtocolError::Application {
                        message: err.to_string(),
                    }),
                }
            }
            ClientRequest::ValidateConfig(config) => {
                let orch = self.orchestrator.lock().await;
                match orch.validate_config(&config) {
                    Ok(report) => ServerResponse::Validation(report),
                    Err(CoreError::Validation(report)) => ServerResponse::Validation(report),
                    Err(err) => ServerResponse::Err(ProtocolError::Application {
                        message: err.to_string(),
                    }),
                }
            }
            ClientRequest::SaveConfig(config) => {
                let mut orch = self.orchestrator.lock().await;
                match orch.save_config(config) {
                    Ok(()) => {
                        // Persist rematched ids (instance/interface/container from live HW).
                        if let Err(err) = self.config_store.save(orch.config()) {
                            warn!(error = %err, "failed to persist saved configuration");
                            ServerResponse::Err(ProtocolError::Application {
                                message: err.to_string(),
                            })
                        } else {
                            ServerResponse::Ok
                        }
                    }
                    Err(CoreError::Validation(report)) => ServerResponse::Validation(report),
                    Err(err) => ServerResponse::Err(ProtocolError::Application {
                        message: err.to_string(),
                    }),
                }
            }
            ClientRequest::Start(config) => self.start(config).await,
            ClientRequest::Stop => self.stop().await,
            ClientRequest::Subscribe => ServerResponse::Ok,
            ClientRequest::AcknowledgeEmergency => {
                let _ = self.events.send(DaemonEvent::Warning {
                    code: "emergency_acked".into(),
                    message: "emergency fail-open acknowledged".into(),
                });
                ServerResponse::Ok
            }
            ClientRequest::AgentHello {
                seat_id,
                agent_version,
            } => self.agent_hello(seat_id, agent_version).await,
            ClientRequest::AgentHeartbeat { seat_id } => self.agent_heartbeat(seat_id).await,
            ClientRequest::AgentFailOpen { seat_id, reason } => {
                self.agent_fail_open(seat_id, reason).await
            }
        }
    }

    async fn agent_hello(&self, seat_id: SeatId, agent_version: u32) -> ServerResponse {
        let interval_ms = {
            let mut agents = self.agents.lock().await;
            agents.register(seat_id, agent_version, Instant::now());
            agents.heartbeat_interval_ms()
        };
        info!(%seat_id, agent_version, "session agent online");
        let _ = self.events.send(DaemonEvent::AgentOnline { seat_id });
        ServerResponse::AgentAck {
            seat_id,
            heartbeat_interval_ms: interval_ms,
        }
    }

    async fn agent_heartbeat(&self, seat_id: SeatId) -> ServerResponse {
        let ok = {
            let mut agents = self.agents.lock().await;
            agents.heartbeat(seat_id, Instant::now())
        };
        if ok {
            ServerResponse::Ok
        } else {
            ServerResponse::Err(ProtocolError::InvalidCommand {
                message: format!("agent for {seat_id} is not registered; send AgentHello first"),
            })
        }
    }

    async fn agent_fail_open(&self, seat_id: SeatId, reason: String) -> ServerResponse {
        {
            let mut agents = self.agents.lock().await;
            let _ = agents.remove(seat_id);
        }
        warn!(%seat_id, %reason, "session agent requested fail-open");
        let _ = self.events.send(DaemonEvent::AgentOffline {
            seat_id,
            reason: reason.clone(),
        });
        let _ = self.events.send(DaemonEvent::EmergencyFailOpen {
            reason: format!("agent {seat_id}: {reason}"),
        });
        let mut orch = self.orchestrator.lock().await;
        match orch.stop() {
            Ok(()) => {
                let _ = self.sync_journal(&orch);
                let _ = self.events.send(DaemonEvent::SeatStateChanged {
                    seat_id: Some(seat_id),
                    state: SeatState::Idle,
                });
                ServerResponse::Ok
            }
            Err(err) => ServerResponse::Err(ProtocolError::Application {
                message: err.to_string(),
            }),
        }
    }

    async fn start(&self, config: MultiseatConfig) -> ServerResponse {
        let _ = self.events.send(DaemonEvent::OperationProgress {
            stage: "start".into(),
            detail: "validating".into(),
            percent: Some(5),
        });

        let mut orch = self.orchestrator.lock().await;
        match orch.start(config) {
            Ok(()) => {
                if let Err(err) = self.sync_journal(&orch) {
                    warn!(error = %err, "failed to persist journal after start");
                }
                let _ = self.events.send(DaemonEvent::SeatStateChanged {
                    seat_id: None,
                    state: SeatState::Active,
                });
                ServerResponse::Ok
            }
            Err(CoreError::Validation(report)) => ServerResponse::Validation(report),
            Err(err) => {
                let _ = self.sync_journal(&orch);
                let _ = self.events.send(DaemonEvent::SeatStateChanged {
                    seat_id: None,
                    state: SeatState::Failed {
                        error: err.to_string(),
                    },
                });
                ServerResponse::Err(ProtocolError::Application {
                    message: err.to_string(),
                })
            }
        }
    }

    async fn stop(&self) -> ServerResponse {
        let mut orch = self.orchestrator.lock().await;
        match orch.stop() {
            Ok(()) => {
                if let Err(err) = self.sync_journal(&orch) {
                    warn!(error = %err, "failed to clear journal after stop");
                }
                let _ = self.events.send(DaemonEvent::SeatStateChanged {
                    seat_id: None,
                    state: SeatState::Idle,
                });
                ServerResponse::Ok
            }
            Err(err) => ServerResponse::Err(ProtocolError::Application {
                message: err.to_string(),
            }),
        }
    }
}

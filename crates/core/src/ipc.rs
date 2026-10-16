//! Typed IPC messages exchanged between GUI/CLI and the daemon.

use serde::{Deserialize, Serialize};

use crate::feasibility::FeasibilityReport;
use crate::hardware::HardwareTree;
use crate::ids::SeatId;
use crate::seat::{MultiseatConfig, SeatState};
use crate::validation::ValidationReport;

/// Correlation id for request/response pairs.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct RequestId(pub u64);

/// Client → daemon commands.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum ClientRequest {
    /// Protocol handshake with client version.
    Hello {
        /// Client protocol version.
        client_version: u32,
    },
    /// Request current hardware + seat snapshot.
    GetSnapshot,
    /// Build Stage 0 go/no-go feasibility report from current inventory.
    GetFeasibilityReport,
    /// Validate a draft configuration without applying it.
    ValidateConfig(MultiseatConfig),
    /// Persist a validated configuration.
    SaveConfig(MultiseatConfig),
    /// Start multiseat using the saved or provided config.
    Start(MultiseatConfig),
    /// Stop multiseat and restore host control.
    Stop,
    /// Subscribe to push events on this connection.
    Subscribe,
    /// Acknowledge emergency fail-open and clear the alert.
    AcknowledgeEmergency,
    /// Session agent registers for a seat (after protocol Hello).
    AgentHello {
        /// Seat this agent serves.
        seat_id: SeatId,
        /// Agent binary protocol/capability version.
        agent_version: u32,
    },
    /// Session agent liveness pulse.
    AgentHeartbeat {
        /// Seat this agent serves.
        seat_id: SeatId,
    },
    /// Agent requests host fail-open (crash path / local emergency).
    AgentFailOpen {
        /// Seat that initiated fail-open.
        seat_id: SeatId,
        /// Human-readable reason.
        reason: String,
    },
}

/// Daemon → client reply correlated with a request.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum ServerResponse {
    /// Handshake accepted.
    HelloAck {
        /// Server protocol version.
        server_version: u32,
    },
    /// Snapshot payload.
    Snapshot {
        /// Hardware tree.
        hardware: Box<HardwareTree>,
        /// Saved configuration.
        config: Box<MultiseatConfig>,
        /// Aggregated orchestrator state.
        state: SeatState,
    },
    /// Validation outcome.
    Validation(ValidationReport),
    /// Stage 0 feasibility checklist.
    Feasibility(FeasibilityReport),
    /// Agent registration accepted.
    AgentAck {
        /// Seat the agent is bound to.
        seat_id: SeatId,
        /// Suggested heartbeat period in milliseconds.
        heartbeat_interval_ms: u64,
    },
    /// Generic success.
    Ok,
    /// Typed protocol/application error.
    Err(ProtocolError),
}

/// Push notifications from the daemon.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum DaemonEvent {
    /// Hardware inventory changed.
    HardwareUpdated(HardwareTree),
    /// Seat state changed.
    SeatStateChanged {
        /// Seat id when applicable.
        seat_id: Option<SeatId>,
        /// New state.
        state: SeatState,
    },
    /// Progress of a long-running operation.
    OperationProgress {
        /// Short stage name.
        stage: String,
        /// Optional detail.
        detail: String,
        /// Percent complete in `0..=100` when known.
        percent: Option<u8>,
    },
    /// Non-fatal diagnostic warning.
    Warning {
        /// Warning code.
        code: String,
        /// Human-readable message.
        message: String,
    },
    /// Emergency fail-open engaged.
    EmergencyFailOpen {
        /// Reason for fail-open.
        reason: String,
    },
    /// Session agent connected and registered.
    AgentOnline {
        /// Seat the agent serves.
        seat_id: SeatId,
    },
    /// Session agent went offline or timed out (heartbeat miss / FailOpen).
    AgentOffline {
        /// Seat the agent served.
        seat_id: SeatId,
        /// Why the agent is considered offline.
        reason: String,
    },
}

#[cfg(test)]
mod daemon_event_tests {
    use super::*;

    #[test]
    fn agent_online_offline_variants_construct() {
        let online = DaemonEvent::AgentOnline { seat_id: SeatId(1) };
        let offline = DaemonEvent::AgentOffline {
            seat_id: SeatId(1),
            reason: "heartbeat timeout".into(),
        };
        assert!(matches!(online, DaemonEvent::AgentOnline { .. }));
        assert!(matches!(offline, DaemonEvent::AgentOffline { .. }));
    }
}

/// Protocol and authorization failures.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum ProtocolError {
    /// Client/server protocol mismatch.
    IncompatibleVersion {
        /// Server version.
        server: u32,
        /// Client version.
        client: u32,
    },
    /// Frame exceeds configured limits.
    FrameTooLarge {
        /// Observed payload length.
        length: u32,
        /// Configured maximum.
        max: u32,
    },
    /// Client is not authorized for the command.
    Unauthorized,
    /// Command is invalid for the current state.
    InvalidCommand {
        /// Detail.
        message: String,
    },
    /// Payload could not be decoded.
    CorruptPayload {
        /// Detail.
        message: String,
    },
    /// Application-level failure.
    Application {
        /// Detail.
        message: String,
    },
}

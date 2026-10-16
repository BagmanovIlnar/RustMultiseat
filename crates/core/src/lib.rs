//! Shared domain models, IPC message contracts, platform traits, and
//! [`SeatOrchestrator`] for RustMultiseat.
//!
//! This crate is intentionally free of Windows FFI so it can be unit-tested on
//! any host used for development CI.

#![deny(missing_docs)]

pub mod error;
pub mod feasibility;
pub mod hardware;
pub mod ids;
pub mod ipc;
pub mod journal;
pub mod orchestrator;
pub mod protocol;
pub mod seat;
pub mod traits;
pub mod validation;

pub use error::{CoreError, CoreResult};
pub use feasibility::{
    CheckStatus, DisplayProbeResult, DualSessionProbeResult, FeasibilityCheck, FeasibilityReport,
    InputProbeResult, ObservedSession, SessionConnectState,
};
pub use hardware::{
    DisplayInfo, GpuInfo, HardwareTree, InputDeviceInfo, InputDeviceKind, PlatformCapabilities,
    UsbNode,
};
pub use ids::{DeviceId, DisplayId, SeatId, SessionId, UserSid};
pub use ipc::{ClientRequest, DaemonEvent, ProtocolError, RequestId, ServerResponse};
pub use journal::{
    journal_io_err, JournalEntry, JournalStore, MemoryJournalStore, OperationJournal,
    RollbackToken, JOURNAL_SCHEMA_VERSION,
};
pub use orchestrator::SeatOrchestrator;
pub use protocol::{PROTOCOL_MAGIC, PROTOCOL_VERSION};
pub use seat::{MultiseatConfig, SeatConfig, SeatState};
pub use traits::{
    BackendHealth, DisplayBackend, DisplayCapabilities, HardwareEnumerator, InputAssignments,
    InputRouter, SessionBackend, SessionHandle,
};
pub use validation::{
    validate_config, ValidationIssue, ValidationReport, ValidationSeverity,
};

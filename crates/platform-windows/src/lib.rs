//! Windows platform layer and host-agnostic mock backends.
//!
//! Real DXGI/SetupAPI bindings live behind `#[cfg(windows)]`. Mock backends
//! power development hosts and act as a fallback when native inventory fails.

#![deny(missing_docs)]

pub mod debounce;
pub mod display;
pub mod host;
pub mod input;
pub mod inventory;
pub mod mock;
pub mod probe;
pub mod session;

pub use debounce::DebounceGate;
pub use display::{probe_display, HostDisplayBackend, WindowsDisplayBackend};
pub use host::HostHardwareEnumerator;
pub use input::{probe_input, HostInputRouter, WindowsInputRouter};
pub use inventory::{prefer_native, probe_capabilities, WindowsHardwareEnumerator};
pub use mock::{MockDisplayBackend, MockHardwareEnumerator, MockInputRouter, MockSessionBackend};
pub use probe::detect_capabilities;
pub use session::{
    agent_args, probe_dual_sessions, resolve_agent_binary, spawn_session_agent, HostSessionBackend,
    WindowsSessionBackend, AGENT_BIN_ENV, REQUIRE_AGENT_ENV,
};

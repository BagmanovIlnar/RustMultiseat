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
pub mod poc_status;
pub mod probe;
pub mod session;

#[cfg(test)]
mod test_env;

pub use debounce::DebounceGate;
pub use display::{
    display_candidate_for_os_version, probe_display, HostDisplayBackend, WindowsDisplayBackend,
    DISPLAY_ATTACH_ENV, DISPLAY_BACKEND_ENV,
};
pub use host::HostHardwareEnumerator;
pub use input::{
    ctl_code, emergency_fail_open, filter_heartbeat, format_route_table_preview, probe_input,
    route_table_from_assignments, route_table_from_config, HostInputRouter, InputFilterControl,
    InputFilterError, InputFilterHealth, RmsIfFailOpenReq, RmsIfRouteEntry, RmsIfRouteTable,
    RmsIfStats, RmsIfVersionInfo, RouteBuildError, WindowsInputRouter, INPUT_FILTER_ABI_VERSION,
    INPUT_FILTER_CONTROL_DEVICE, INPUT_SOFT_APPLY_ENV, IOCTL_RMS_IF_GET_VERSION,
    IOCTL_RMS_IF_QUERY_STATS, IOCTL_RMS_IF_SET_FAIL_OPEN, IOCTL_RMS_IF_SET_ROUTES,
    KEYBOARD_FILTER_SERVICE, POINTER_FILTER_SERVICE, RMS_IF_ABI_VERSION, RMS_IF_DEVICE_TYPE,
    RMS_IF_INSTANCE_ID_LEN, RMS_IF_MAX_ROUTES, RMS_IF_ROUTE_F_KEYBOARD, RMS_IF_ROUTE_F_POINTER,
    RMS_IF_ROUTE_F_SUPPRESS_HOST, RMS_IF_TABLE_F_REPLACE_ALL,
};
pub use inventory::{prefer_native, probe_capabilities, SnapshotCache, WindowsHardwareEnumerator};
pub use mock::{
    MockDisplayBackend, MockHardwareEnumerator, MockInputRouter, MockSessionBackend,
    MOCK_FAIL_STAGE_ENV,
};
pub use poc_status::{input_soft_apply_enabled, NativePocStatus};
pub use probe::{detect_capabilities, windows_version_numbers};
pub use session::{
    agent_args, launch_agent_in_session, log_native_session_flags, native_agent_launch_enabled,
    native_session_control_enabled, probe_dual_sessions, resolve_agent_binary, resolve_daemon_addr,
    spawn_session_agent, HostSessionBackend, WindowsSessionBackend, AGENT_BIN_ENV, DAEMON_ADDR_ENV,
    NATIVE_AGENT_LAUNCH_ENV, NATIVE_SESSION_CONTROL_ENV, REQUIRE_AGENT_ENV,
};

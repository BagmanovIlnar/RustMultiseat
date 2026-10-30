//! Stage 0 input-router probe and native [`InputRouter`] stub.

mod backend;
mod filter_control;
mod host;
mod ioctl;
mod probe;
mod routes;

#[cfg(windows)]
mod native_ioctl;

pub use backend::WindowsInputRouter;
pub use filter_control::{
    emergency_fail_open, filter_heartbeat, InputFilterControl, InputFilterError, InputFilterHealth,
    INPUT_FILTER_ABI_VERSION, INPUT_FILTER_CONTROL_DEVICE,
};
pub use host::HostInputRouter;
pub use ioctl::{
    ctl_code, RmsIfFailOpenReq, RmsIfRouteEntry, RmsIfRouteTable, RmsIfStats, RmsIfVersionInfo,
    IOCTL_RMS_IF_GET_VERSION, IOCTL_RMS_IF_QUERY_STATS, IOCTL_RMS_IF_SET_FAIL_OPEN,
    IOCTL_RMS_IF_SET_ROUTES, RMS_IF_ABI_VERSION, RMS_IF_DEVICE_TYPE, RMS_IF_INSTANCE_ID_LEN,
    RMS_IF_MAX_ROUTES, RMS_IF_ROUTE_F_KEYBOARD, RMS_IF_ROUTE_F_POINTER,
    RMS_IF_ROUTE_F_SUPPRESS_HOST, RMS_IF_TABLE_F_REPLACE_ALL,
};
pub use probe::{
    probe_input, INPUT_SOFT_APPLY_ENV, KEYBOARD_FILTER_SERVICE, POINTER_FILTER_SERVICE,
};
pub use routes::{
    format_route_table_preview, route_flags_label, route_instance_id, route_table_from_assignments,
    route_table_from_config, RouteBuildError,
};

#[cfg(test)]
mod tests {
    use super::*;
    use multiseat_core::CheckStatus;

    #[test]
    fn probe_returns_structured_result() {
        let result = probe_input(2, 2);
        assert!(!result.detail().is_empty());
        assert!(result.fail_open_ready);
        assert!(matches!(
            result.status,
            CheckStatus::Pass | CheckStatus::Blocked | CheckStatus::Unknown | CheckStatus::Fail
        ));
    }
}

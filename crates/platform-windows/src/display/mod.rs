//! Stage 0 display-model probe and native [`DisplayBackend`] stub.

mod backend;
mod host;
mod probe;

pub use backend::WindowsDisplayBackend;
pub use host::HostDisplayBackend;
pub use probe::{
    display_candidate_for_os_version, probe_display, DISPLAY_ATTACH_ENV, DISPLAY_BACKEND_ENV,
};

#[cfg(test)]
mod tests {
    use super::*;
    use multiseat_core::CheckStatus;

    #[test]
    fn probe_returns_structured_result() {
        let result = probe_display(2);
        assert!(!result.detail().is_empty());
        assert!(matches!(
            result.status,
            CheckStatus::Pass | CheckStatus::Blocked | CheckStatus::Unknown | CheckStatus::Fail
        ));
    }
}

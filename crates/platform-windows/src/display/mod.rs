//! Stage 0 display-model probe and native [`DisplayBackend`] stub.

mod backend;
mod host;
mod probe;

pub use backend::WindowsDisplayBackend;
pub use host::HostDisplayBackend;
pub use probe::probe_display;

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
            CheckStatus::Pass
                | CheckStatus::Blocked
                | CheckStatus::Unknown
                | CheckStatus::Fail
        ));
    }
}

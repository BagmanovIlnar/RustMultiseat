//! Stage 0 input-router probe and native [`InputRouter`] stub.

mod backend;
mod host;
mod probe;

pub use backend::WindowsInputRouter;
pub use host::HostInputRouter;
pub use probe::probe_input;

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
            CheckStatus::Pass
                | CheckStatus::Blocked
                | CheckStatus::Unknown
                | CheckStatus::Fail
        ));
    }
}

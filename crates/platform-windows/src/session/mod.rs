//! Stage 0 dual-session probe and native [`SessionBackend`].

mod backend;
mod host;

#[cfg(windows)]
mod wts;

#[cfg(not(windows))]
mod stub;

use multiseat_core::DualSessionProbeResult;

pub use backend::WindowsSessionBackend;
pub use host::HostSessionBackend;

/// Enumerates local sessions and evaluates the dual-session Stage 0 check.
///
/// Does **not** attempt undocumented session creation. On client Windows the
/// typical result is `Blocked` with a single active console session.
pub fn probe_dual_sessions() -> DualSessionProbeResult {
    #[cfg(windows)]
    {
        wts::probe()
    }
    #[cfg(not(windows))]
    {
        stub::probe()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use multiseat_core::CheckStatus;

    #[test]
    fn probe_returns_structured_result() {
        let result = probe_dual_sessions();
        assert!(!result.detail().is_empty());
        // Dev hosts without Windows WTS land on Blocked; Windows lab may Pass/Blocked.
        assert!(matches!(
            result.status,
            CheckStatus::Pass | CheckStatus::Blocked | CheckStatus::Unknown | CheckStatus::Fail
        ));
    }
}

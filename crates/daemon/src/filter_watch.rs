//! Input-filter health watchdog while multiseat is Active.
//!
//! Armed after a successful `Start`. Periodic [`filter_heartbeat`](multiseat_platform_windows::filter_heartbeat)
//! pings; consecutive unhealthy results trigger EmergencyFailOpen + orchestrator stop
//! (same outcome as a missing session-agent heartbeat).

use std::sync::atomic::{AtomicBool, Ordering};

use multiseat_platform_windows::{filter_heartbeat, InputFilterHealth};

/// Test/lab injection: next pings report unhealthy (cleared manually).
static INJECT_UNHEALTHY: AtomicBool = AtomicBool::new(false);

/// Env: force an unhealthy filter ping (`1`) for lab / IT (non-sticky; clear after test).
pub const FILTER_HEARTBEAT_FAIL_ENV: &str = "MULTISEAT_INPUT_FILTER_HEARTBEAT_FAIL";

/// Env: consecutive unhealthy pings required before fail-open (default `1`).
pub const FILTER_FAIL_THRESHOLD_ENV: &str = "MULTISEAT_INPUT_FILTER_FAIL_THRESHOLD";

/// Default consecutive failures before fail-open.
pub const DEFAULT_FILTER_FAIL_THRESHOLD: u32 = 1;

/// Result of feeding one health sample into the watchdog.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FilterWatchAction {
    /// Watchdog not armed (Idle / stopped).
    Idle,
    /// Armed and healthy (or below failure threshold).
    Ok,
    /// Threshold reached — caller must fail-open and stop.
    FailOpen {
        /// Operator-facing reason.
        detail: String,
    },
}

/// Tracks armed state + consecutive unhealthy filter pings.
#[derive(Debug, Clone)]
pub struct FilterWatchdog {
    armed: bool,
    consecutive_failures: u32,
    fail_threshold: u32,
}

impl Default for FilterWatchdog {
    fn default() -> Self {
        Self::new()
    }
}

impl FilterWatchdog {
    /// Creates a watchdog with [`DEFAULT_FILTER_FAIL_THRESHOLD`].
    pub fn new() -> Self {
        Self {
            armed: false,
            consecutive_failures: 0,
            fail_threshold: DEFAULT_FILTER_FAIL_THRESHOLD,
        }
    }

    /// Creates a watchdog with a custom failure threshold (tests).
    pub fn with_threshold(fail_threshold: u32) -> Self {
        Self {
            armed: false,
            consecutive_failures: 0,
            fail_threshold: fail_threshold.max(1),
        }
    }

    /// Builds from env (`MULTISEAT_INPUT_FILTER_FAIL_THRESHOLD`) or default.
    pub fn from_env() -> Self {
        let threshold = std::env::var(FILTER_FAIL_THRESHOLD_ENV)
            .ok()
            .and_then(|s| s.parse::<u32>().ok())
            .unwrap_or(DEFAULT_FILTER_FAIL_THRESHOLD)
            .max(1);
        Self::with_threshold(threshold)
    }

    /// True after successful Start until Stop / fail-open.
    pub fn is_armed(&self) -> bool {
        self.armed
    }

    /// Configured consecutive-failure threshold.
    pub fn fail_threshold(&self) -> u32 {
        self.fail_threshold
    }

    /// Arm on successful Start (reset failure streak).
    pub fn arm(&mut self) {
        self.armed = true;
        self.consecutive_failures = 0;
    }

    /// Disarm on Stop or after fail-open handling.
    pub fn disarm(&mut self) {
        self.armed = false;
        self.consecutive_failures = 0;
    }

    /// Feeds one health sample; may return [`FilterWatchAction::FailOpen`].
    pub fn observe(&mut self, healthy: bool, detail: impl Into<String>) -> FilterWatchAction {
        if !self.armed {
            return FilterWatchAction::Idle;
        }
        if healthy {
            self.consecutive_failures = 0;
            return FilterWatchAction::Ok;
        }
        self.consecutive_failures = self.consecutive_failures.saturating_add(1);
        if self.consecutive_failures >= self.fail_threshold {
            let detail = detail.into();
            self.disarm();
            FilterWatchAction::FailOpen { detail }
        } else {
            FilterWatchAction::Ok
        }
    }

    /// Convenience: observe a full [`InputFilterHealth`] snapshot.
    pub fn observe_health(&mut self, health: &InputFilterHealth) -> FilterWatchAction {
        self.observe(health.healthy, health.detail.clone())
    }
}

/// Forces unhealthy filter pings until cleared (IT / crash-injection lab).
pub fn set_inject_unhealthy(enabled: bool) {
    INJECT_UNHEALTHY.store(enabled, Ordering::SeqCst);
}

/// Runs one filter control ping, applying lab injection (atomic or env) if set.
pub fn ping_filter_health() -> InputFilterHealth {
    let mut health = match filter_heartbeat() {
        Ok(h) => h,
        Err(err) => InputFilterHealth {
            present: false,
            soft: false,
            abi_version: None,
            fail_open: 1,
            healthy: false,
            forced_fail_open: false,
            detail: format!("filter open/ping error: {err}"),
        },
    };
    let inject = INJECT_UNHEALTHY.load(Ordering::SeqCst)
        || std::env::var_os(FILTER_HEARTBEAT_FAIL_ENV).is_some_and(|v| v == "1");
    if inject {
        health.healthy = false;
        health.detail = format!(
            "injected unhealthy ({FILTER_HEARTBEAT_FAIL_ENV} / test hook): {}",
            health.detail
        );
    }
    health
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn observe_idle_when_disarmed() {
        let mut w = FilterWatchdog::new();
        assert_eq!(w.observe(false, "x"), FilterWatchAction::Idle);
    }

    #[test]
    fn first_failure_triggers_with_threshold_one() {
        let mut w = FilterWatchdog::with_threshold(1);
        w.arm();
        match w.observe(false, "bad ioctl") {
            FilterWatchAction::FailOpen { detail } => assert!(detail.contains("bad ioctl")),
            other => panic!("unexpected {other:?}"),
        }
        assert!(!w.is_armed());
    }

    #[test]
    fn threshold_two_needs_two_failures() {
        let mut w = FilterWatchdog::with_threshold(2);
        w.arm();
        assert_eq!(w.observe(false, "1"), FilterWatchAction::Ok);
        assert!(w.is_armed());
        assert!(matches!(
            w.observe(false, "2"),
            FilterWatchAction::FailOpen { .. }
        ));
    }

    #[test]
    fn healthy_resets_streak() {
        let mut w = FilterWatchdog::with_threshold(2);
        w.arm();
        assert_eq!(w.observe(false, "1"), FilterWatchAction::Ok);
        assert_eq!(w.observe(true, "ok"), FilterWatchAction::Ok);
        assert_eq!(w.observe(false, "again"), FilterWatchAction::Ok);
        assert!(w.is_armed());
    }
}

//! Non-Windows stand-in for the dual-session probe.

use multiseat_core::DualSessionProbeResult;

pub fn probe() -> DualSessionProbeResult {
    DualSessionProbeResult::unavailable(
        "WTS dual-session probe requires a Windows host (dev/CI stub)",
    )
}

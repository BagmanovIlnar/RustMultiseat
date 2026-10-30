//! Documented WTS session lock / disconnect controls.
//!
//! Opt-in via `MULTISEAT_NATIVE_SESSION_CONTROL=1`. Without the flag, callers
//! get an honest error (lock) or local-only unbind (disconnect bookkeeping is
//! handled by [`super::backend::WindowsSessionBackend`]).
//!
//! - **lock:** `CreateProcessAsUser` + `rundll32 user32.dll,LockWorkStation`
//!   in the target session (`LockWorkStation` is interactive-desktop only).
//! - **disconnect:** `WTSDisconnectSession` (session remains, user is detached).

use std::env;

use multiseat_core::{CoreError, CoreResult};
use tracing::info;

#[cfg(windows)]
use tracing::warn;

/// Opt-in for native WTS lock/disconnect side effects (`1` / `true`).
pub const NATIVE_SESSION_CONTROL_ENV: &str = "MULTISEAT_NATIVE_SESSION_CONTROL";

/// Whether native WTS lock/disconnect is enabled.
pub fn native_session_control_enabled() -> bool {
    matches!(
        env::var(NATIVE_SESSION_CONTROL_ENV).as_deref(),
        Ok("1") | Ok("true")
    )
}

/// Locks the target session desktop when the feature flag is set.
pub fn lock_session(session_id: u32) -> CoreResult<()> {
    if !native_session_control_enabled() {
        return Err(CoreError::Backend(format!(
            "native session lock disabled; set {NATIVE_SESSION_CONTROL_ENV}=1 to attempt \
             CreateProcessAsUser(LockWorkStation) in WTS session {session_id} \
             (requires SE_TCB_NAME / Session 0 service)"
        )));
    }

    #[cfg(windows)]
    {
        lock_windows(session_id)
    }
    #[cfg(not(windows))]
    {
        let _ = session_id;
        Err(CoreError::Backend(
            "native session lock is Windows-only".into(),
        ))
    }
}

/// Disconnects the WTS session when the feature flag is set.
///
/// Returns `Ok(())` when the flag is off so callers can still drop local seat
/// bindings without requiring elevation in CI/dev hosts.
pub fn disconnect_session(session_id: u32) -> CoreResult<()> {
    if !native_session_control_enabled() {
        info!(
            session_id,
            "native WTS disconnect skipped ({NATIVE_SESSION_CONTROL_ENV} unset)"
        );
        return Ok(());
    }

    #[cfg(windows)]
    {
        disconnect_windows(session_id)
    }
    #[cfg(not(windows))]
    {
        let _ = session_id;
        Err(CoreError::Backend(
            "native WTS disconnect is Windows-only".into(),
        ))
    }
}

#[cfg(windows)]
fn lock_windows(session_id: u32) -> CoreResult<()> {
    // LockWorkStation is only valid on the interactive desktop of the target
    // session; launch it there via the same CreateProcessAsUser path as agents.
    const LOCK_CMD: &str = "rundll32.exe user32.dll,LockWorkStation";
    info!(
        session_id,
        "attempting session lock via CreateProcessAsUser"
    );
    let pid = super::agent_as_user::spawn_command_in_session(session_id, LOCK_CMD)?;
    info!(
        session_id,
        pid, "LockWorkStation helper launched in session"
    );
    Ok(())
}

#[cfg(windows)]
fn disconnect_windows(session_id: u32) -> CoreResult<()> {
    use windows::Win32::Foundation::HANDLE;
    use windows::Win32::System::RemoteDesktop::{WTSDisconnectSession, WTS_CURRENT_SERVER_HANDLE};

    info!(session_id, "attempting WTSDisconnectSession");
    unsafe {
        WTSDisconnectSession(Some(HANDLE(WTS_CURRENT_SERVER_HANDLE.0)), session_id, false)
            .map_err(|err| {
                warn!(session_id, error = %err, "WTSDisconnectSession failed");
                CoreError::Backend(format!("WTSDisconnectSession({session_id}) failed: {err}"))
            })?;
    }
    info!(session_id, "WTSDisconnectSession succeeded");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lock_refuses_without_feature_flag() {
        let prev = env::var_os(NATIVE_SESSION_CONTROL_ENV);
        env::remove_var(NATIVE_SESSION_CONTROL_ENV);
        let err = lock_session(1).unwrap_err();
        let msg = err.to_string();
        assert!(msg.contains(NATIVE_SESSION_CONTROL_ENV));
        assert!(msg.contains("LockWorkStation"));
        match prev {
            Some(v) => env::set_var(NATIVE_SESSION_CONTROL_ENV, v),
            None => env::remove_var(NATIVE_SESSION_CONTROL_ENV),
        }
    }

    #[test]
    fn disconnect_soft_skips_without_flag() {
        let prev = env::var_os(NATIVE_SESSION_CONTROL_ENV);
        env::remove_var(NATIVE_SESSION_CONTROL_ENV);
        disconnect_session(1).unwrap();
        match prev {
            Some(v) => env::set_var(NATIVE_SESSION_CONTROL_ENV, v),
            None => env::remove_var(NATIVE_SESSION_CONTROL_ENV),
        }
    }

    #[test]
    fn feature_flag_parser() {
        let prev = env::var_os(NATIVE_SESSION_CONTROL_ENV);
        env::remove_var(NATIVE_SESSION_CONTROL_ENV);
        assert!(!native_session_control_enabled());
        env::set_var(NATIVE_SESSION_CONTROL_ENV, "1");
        assert!(native_session_control_enabled());
        match prev {
            Some(v) => env::set_var(NATIVE_SESSION_CONTROL_ENV, v),
            None => env::remove_var(NATIVE_SESSION_CONTROL_ENV),
        }
    }
}

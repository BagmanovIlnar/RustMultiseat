//! Native session-agent launch via documented WTS + `CreateProcessAsUser`.
//!
//! Disabled by default (`MULTISEAT_NATIVE_AGENT_LAUNCH`). When enabled on
//! Windows, attempts `WTSQueryUserToken` + `CreateProcessAsUserW` into the
//! bound session. Failures surface real Win32 errors — never a fake Ok.
//!
//! Requires the caller to hold `SE_TCB_NAME` (typical for a Session 0 service).

use std::env;

use multiseat_core::{CoreError, CoreResult, SeatId};

#[cfg(windows)]
use tracing::{info, warn};

#[cfg(windows)]
use super::agent_launch::{agent_args, resolve_agent_binary, resolve_daemon_addr};

/// Opt-in for the native CreateProcessAsUser path (`1` / `true`).
pub const NATIVE_AGENT_LAUNCH_ENV: &str = "MULTISEAT_NATIVE_AGENT_LAUNCH";

/// Whether native cross-session agent launch is enabled.
pub fn native_agent_launch_enabled() -> bool {
    matches!(
        env::var(NATIVE_AGENT_LAUNCH_ENV).as_deref(),
        Ok("1") | Ok("true")
    )
}

/// Result of a successful native agent launch.
#[derive(Debug, Clone, Copy)]
pub struct NativeAgentLaunch {
    /// Process id of the spawned session agent.
    pub pid: u32,
}

/// Launches `multiseat-session-agent` in `session_id` when the feature flag is set.
pub fn launch_agent_in_session(
    session_id: u32,
    seat_id: SeatId,
    daemon_addr: Option<&str>,
) -> CoreResult<NativeAgentLaunch> {
    if !native_agent_launch_enabled() {
        return Err(CoreError::Backend(format!(
            "native start_agent disabled; set {NATIVE_AGENT_LAUNCH_ENV}=1 to attempt \
             WTSQueryUserToken + CreateProcessAsUser into WTS session {session_id} \
             (requires SE_TCB_NAME; use mock / MULTISEAT_USE_MOCK=1 for same-session spawn)"
        )));
    }

    #[cfg(windows)]
    {
        launch_windows(session_id, seat_id, daemon_addr)
    }
    #[cfg(not(windows))]
    {
        let _ = (session_id, seat_id, daemon_addr);
        Err(CoreError::Backend(
            "native CreateProcessAsUser path is Windows-only".into(),
        ))
    }
}

/// Best-effort terminate of a previously launched native agent.
pub fn stop_native_agent(pid: u32) {
    #[cfg(windows)]
    {
        stop_windows(pid);
    }
    #[cfg(not(windows))]
    {
        let _ = pid;
    }
}

#[cfg(windows)]
fn launch_windows(
    session_id: u32,
    seat_id: SeatId,
    daemon_addr: Option<&str>,
) -> CoreResult<NativeAgentLaunch> {
    use std::path::Path;

    let bin = resolve_agent_binary().ok_or_else(|| {
        CoreError::Backend("session-agent binary path could not be resolved".into())
    })?;
    if !Path::new(&bin).is_file() {
        // PATH-name fallback is not usable for CreateProcessAsUser without SearchPath.
        return Err(CoreError::Backend(format!(
            "session-agent binary not found at {} (set MULTISEAT_SESSION_AGENT to a full path)",
            bin.display()
        )));
    }

    let addr = resolve_daemon_addr(daemon_addr);
    let argv = agent_args(seat_id, addr.as_deref());
    let cmdline = build_command_line(&bin, &argv);
    info!(
        seat = %seat_id,
        session_id,
        bin = %bin.display(),
        %cmdline,
        "attempting CreateProcessAsUser for session-agent"
    );
    let pid = spawn_command_in_session(session_id, &cmdline)?;
    info!(
        seat = %seat_id,
        session_id,
        pid,
        "session-agent launched via CreateProcessAsUser"
    );
    Ok(NativeAgentLaunch { pid })
}

/// Spawns `cmdline` in `session_id` via `WTSQueryUserToken` + `CreateProcessAsUserW`.
///
/// Caller must already hold required privileges (`SE_TCB_NAME`). Used by agent
/// launch and session lock helpers.
#[cfg(windows)]
pub(crate) fn spawn_command_in_session(session_id: u32, cmdline: &str) -> CoreResult<u32> {
    use std::os::windows::ffi::OsStrExt;

    use windows::core::PCWSTR;
    use windows::Win32::Foundation::{CloseHandle, HANDLE};
    use windows::Win32::System::RemoteDesktop::WTSQueryUserToken;
    use windows::Win32::System::Threading::{
        CreateProcessAsUserW, CREATE_NO_WINDOW, CREATE_UNICODE_ENVIRONMENT, PROCESS_INFORMATION,
        STARTUPINFOW,
    };

    let mut cmdline_wide: Vec<u16> = std::ffi::OsString::from(cmdline)
        .encode_wide()
        .chain(std::iter::once(0))
        .collect();
    let desktop: Vec<u16> = OsStrExt::encode_wide(std::ffi::OsStr::new("winsta0\\default"))
        .chain(std::iter::once(0))
        .collect();

    unsafe {
        let mut user_token = HANDLE::default();
        WTSQueryUserToken(session_id, &mut user_token).map_err(|err| {
            CoreError::Backend(format!(
                "WTSQueryUserToken({session_id}) failed: {err} (need SE_TCB_NAME / Session 0 service)"
            ))
        })?;
        let token = HandleGuard(user_token);

        let mut startup = STARTUPINFOW::default();
        startup.cb = std::mem::size_of::<STARTUPINFOW>() as u32;
        startup.lpDesktop = windows::core::PWSTR(desktop.as_ptr() as *mut u16);

        let mut process_info = PROCESS_INFORMATION::default();
        CreateProcessAsUserW(
            Some(token.0),
            PCWSTR::null(),
            Some(windows::core::PWSTR(cmdline_wide.as_mut_ptr())),
            None,
            None,
            false,
            CREATE_UNICODE_ENVIRONMENT | CREATE_NO_WINDOW,
            None,
            None,
            &startup,
            &mut process_info,
        )
        .map_err(|err| {
            CoreError::Backend(format!(
                "CreateProcessAsUserW failed for session {session_id}: {err}"
            ))
        })?;

        let pid = process_info.dwProcessId;
        let _ = CloseHandle(process_info.hThread);
        let _ = CloseHandle(process_info.hProcess);
        Ok(pid)
    }
}

#[cfg(windows)]
fn stop_windows(pid: u32) {
    use windows::Win32::Foundation::CloseHandle;
    use windows::Win32::System::Threading::{
        OpenProcess, TerminateProcess, PROCESS_TERMINATE,
    };

    if pid == 0 {
        return;
    }
    unsafe {
        match OpenProcess(PROCESS_TERMINATE, false, pid) {
            Ok(handle) => {
                if let Err(err) = TerminateProcess(handle, 1) {
                    warn!(pid, error = %err, "TerminateProcess for session-agent failed");
                }
                let _ = CloseHandle(handle);
            }
            Err(err) => warn!(pid, error = %err, "OpenProcess for session-agent failed"),
        }
    }
}

#[cfg(windows)]
fn build_command_line(bin: &std::path::Path, argv: &[String]) -> String {
    let mut parts = Vec::with_capacity(1 + argv.len());
    parts.push(quote_windows_arg(&bin.to_string_lossy()));
    for arg in argv {
        parts.push(quote_windows_arg(arg));
    }
    parts.join(" ")
}

#[cfg(windows)]
fn quote_windows_arg(arg: &str) -> String {
    if arg.is_empty() {
        return "\"\"".into();
    }
    if !arg.contains([' ', '\t', '"']) {
        return arg.to_string();
    }
    let mut out = String::from('"');
    for ch in arg.chars() {
        if ch == '"' {
            out.push('\\');
        }
        out.push(ch);
    }
    out.push('"');
    out
}

#[cfg(windows)]
struct HandleGuard(windows::Win32::Foundation::HANDLE);

#[cfg(windows)]
impl Drop for HandleGuard {
    fn drop(&mut self) {
        unsafe {
            let _ = windows::Win32::Foundation::CloseHandle(self.0);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn launch_refuses_without_feature_flag() {
        let prev = env::var_os(NATIVE_AGENT_LAUNCH_ENV);
        env::remove_var(NATIVE_AGENT_LAUNCH_ENV);
        let err = launch_agent_in_session(1, SeatId(1), Some("127.0.0.1:9")).unwrap_err();
        let msg = err.to_string();
        assert!(msg.contains(NATIVE_AGENT_LAUNCH_ENV));
        assert!(msg.contains("CreateProcessAsUser"));
        match prev {
            Some(v) => env::set_var(NATIVE_AGENT_LAUNCH_ENV, v),
            None => env::remove_var(NATIVE_AGENT_LAUNCH_ENV),
        }
    }

    #[test]
    fn feature_flag_parser() {
        let prev = env::var_os(NATIVE_AGENT_LAUNCH_ENV);
        env::remove_var(NATIVE_AGENT_LAUNCH_ENV);
        assert!(!native_agent_launch_enabled());
        env::set_var(NATIVE_AGENT_LAUNCH_ENV, "1");
        assert!(native_agent_launch_enabled());
        match prev {
            Some(v) => env::set_var(NATIVE_AGENT_LAUNCH_ENV, v),
            None => env::remove_var(NATIVE_AGENT_LAUNCH_ENV),
        }
    }
}

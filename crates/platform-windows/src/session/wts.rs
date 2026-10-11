//! Documented WTS session enumeration for Stage 0 evidence.

use multiseat_core::{
    DualSessionProbeResult, ObservedSession, SessionConnectState,
};
use windows::core::PWSTR;
use windows::Win32::Foundation::HANDLE;
use windows::Win32::System::RemoteDesktop::{
    WTSEnumerateSessionsW, WTSFreeMemory, WTSGetActiveConsoleSessionId, WTS_CONNECTSTATE_CLASS,
    WTS_CURRENT_SERVER_HANDLE, WTS_SESSION_INFOW, WTSActive, WTSConnected, WTSDisconnected,
};

/// Runs the dual-session probe via `WTSEnumerateSessionsW`.
pub fn probe() -> DualSessionProbeResult {
    match enumerate() {
        Ok(sessions) => DualSessionProbeResult::from_sessions(sessions, false),
        Err(err) => DualSessionProbeResult::unavailable(format!("WTS enumerate failed: {err}")),
    }
}

fn enumerate() -> Result<Vec<ObservedSession>, String> {
    unsafe {
        let mut info: *mut WTS_SESSION_INFOW = std::ptr::null_mut();
        let mut count = 0u32;
        WTSEnumerateSessionsW(
            Some(HANDLE(WTS_CURRENT_SERVER_HANDLE.0)),
            0,
            1,
            &mut info,
            &mut count,
        )
        .map_err(|e| e.to_string())?;

        if info.is_null() {
            return Ok(Vec::new());
        }

        let console_id = WTSGetActiveConsoleSessionId();
        let slice = std::slice::from_raw_parts(info, count as usize);
        let mut sessions = Vec::with_capacity(slice.len());
        for entry in slice {
            let station_name = pwstr_to_string(entry.pWinStationName);
            let is_console = entry.SessionId == console_id
                || station_name.eq_ignore_ascii_case("Console");
            sessions.push(ObservedSession {
                session_id: entry.SessionId,
                station_name,
                state: map_state(entry.State),
                is_console,
            });
        }

        WTSFreeMemory(info.cast());
        Ok(sessions)
    }
}

fn map_state(state: WTS_CONNECTSTATE_CLASS) -> SessionConnectState {
    if state == WTSActive {
        SessionConnectState::Active
    } else if state == WTSConnected {
        SessionConnectState::Connected
    } else if state == WTSDisconnected {
        SessionConnectState::Disconnected
    } else {
        SessionConnectState::Other(format!("wts_{}", state.0))
    }
}

unsafe fn pwstr_to_string(ptr: PWSTR) -> String {
    if ptr.is_null() {
        return String::new();
    }
    ptr.to_string().unwrap_or_default()
}

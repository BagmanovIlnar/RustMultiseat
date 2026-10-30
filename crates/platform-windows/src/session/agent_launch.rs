//! Launch helper for `multiseat-session-agent` (same-machine spawn).
//!
//! Mock backends use this to exercise the heartbeat path. Native
//! `WindowsSessionBackend` uses [`super::agent_as_user`] (`CreateProcessAsUser`)
//! when `MULTISEAT_NATIVE_AGENT_LAUNCH=1`.

use std::env;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};

use multiseat_core::{CoreError, CoreResult, SeatId};
use tracing::{info, warn};

/// Environment override for the agent binary path.
pub const AGENT_BIN_ENV: &str = "MULTISEAT_SESSION_AGENT";

/// When set to `1`, missing agent binary is an error even for mock soft-start.
pub const REQUIRE_AGENT_ENV: &str = "MULTISEAT_REQUIRE_AGENT";

/// Optional daemon endpoint for spawned agents (`--addr`).
///
/// Used when the session backend has no explicit bind configured.
pub const DAEMON_ADDR_ENV: &str = "MULTISEAT_DAEMON_ADDR";

/// Resolves the session-agent executable.
///
/// Order: `MULTISEAT_SESSION_AGENT` → `current_exe` directory sibling → `PATH`
/// lookup name `multiseat-session-agent` (`.exe` on Windows).
pub fn resolve_agent_binary() -> Option<PathBuf> {
    if let Some(path) = env::var_os(AGENT_BIN_ENV) {
        let path = PathBuf::from(path);
        if path.is_file() {
            return Some(path);
        }
        warn!(
            path = %path.display(),
            "{AGENT_BIN_ENV} set but file not found"
        );
    }

    if let Ok(exe) = env::current_exe() {
        if let Some(dir) = exe.parent() {
            for name in agent_file_names() {
                let candidate = dir.join(name);
                if candidate.is_file() {
                    return Some(candidate);
                }
            }
        }
    }

    // Last resort: rely on PATH (Command finds it when spawned by name).
    Some(PathBuf::from(agent_command_name()))
}

fn agent_command_name() -> &'static str {
    if cfg!(windows) {
        "multiseat-session-agent.exe"
    } else {
        "multiseat-session-agent"
    }
}

fn agent_file_names() -> &'static [&'static str] {
    if cfg!(windows) {
        &["multiseat-session-agent.exe", "multiseat-session-agent"]
    } else {
        &["multiseat-session-agent"]
    }
}

/// Resolves the daemon endpoint for a spawned agent.
///
/// Order: explicit argument → `MULTISEAT_DAEMON_ADDR` → `None` (agent clap default).
pub fn resolve_daemon_addr(explicit: Option<&str>) -> Option<String> {
    if let Some(addr) = explicit.filter(|s| !s.is_empty()) {
        return Some(addr.to_string());
    }
    env::var(DAEMON_ADDR_ENV).ok().filter(|s| !s.is_empty())
}

/// Builds the argv for a session agent (without resolving the binary).
pub fn agent_args(seat_id: SeatId, daemon_addr: Option<&str>) -> Vec<String> {
    let mut args = vec!["run".into(), "--seat-id".into(), seat_id.0.to_string()];
    if let Some(addr) = resolve_daemon_addr(daemon_addr) {
        args.push("--addr".into());
        args.push(addr);
    }
    args
}

/// Spawns `multiseat-session-agent run --seat-id …`.
pub fn spawn_session_agent(seat_id: SeatId, daemon_addr: Option<&str>) -> CoreResult<Child> {
    let bin = resolve_agent_binary().ok_or_else(|| {
        CoreError::Backend("session-agent binary path could not be resolved".into())
    })?;
    spawn_session_agent_at(&bin, seat_id, daemon_addr)
}

/// Spawns using an explicit binary path (tests).
pub fn spawn_session_agent_at(
    bin: &Path,
    seat_id: SeatId,
    daemon_addr: Option<&str>,
) -> CoreResult<Child> {
    let args = agent_args(seat_id, daemon_addr);
    info!(
        seat = %seat_id,
        bin = %bin.display(),
        ?args,
        "spawning session-agent"
    );
    Command::new(bin)
        .args(&args)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|err| {
            CoreError::Backend(format!(
                "failed to spawn session-agent {}: {err}",
                bin.display()
            ))
        })
}

/// Best-effort stop of a child agent process.
pub fn stop_agent_child(child: &mut Child) {
    match child.try_wait() {
        Ok(Some(_)) => {}
        Ok(None) => {
            let _ = child.kill();
            let _ = child.wait();
        }
        Err(err) => warn!(error = %err, "session-agent try_wait failed"),
    }
}

/// Whether mock start should fail when the binary cannot be spawned.
pub fn require_agent() -> bool {
    matches!(env::var(REQUIRE_AGENT_ENV).as_deref(), Ok("1") | Ok("true"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    /// Serializes env mutation across parallel tests in this module.
    static ENV_LOCK: Mutex<()> = Mutex::new(());

    #[test]
    fn args_include_seat_and_optional_addr() {
        let _guard = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        // Clear env so this test is deterministic regardless of host.
        let prev = env::var_os(DAEMON_ADDR_ENV);
        env::remove_var(DAEMON_ADDR_ENV);
        let args = agent_args(SeatId(3), None);
        assert_eq!(args, vec!["run", "--seat-id", "3"]);
        let args = agent_args(SeatId(3), Some("127.0.0.1:9"));
        assert_eq!(args, vec!["run", "--seat-id", "3", "--addr", "127.0.0.1:9"]);
        match prev {
            Some(v) => env::set_var(DAEMON_ADDR_ENV, v),
            None => env::remove_var(DAEMON_ADDR_ENV),
        }
    }

    #[test]
    fn env_addr_fills_when_explicit_missing() {
        let _guard = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let prev = env::var_os(DAEMON_ADDR_ENV);
        env::set_var(DAEMON_ADDR_ENV, "127.0.0.1:4242");
        let args = agent_args(SeatId(1), None);
        assert_eq!(
            args,
            vec!["run", "--seat-id", "1", "--addr", "127.0.0.1:4242"]
        );
        // Explicit wins over env.
        let args = agent_args(SeatId(1), Some("127.0.0.1:1"));
        assert_eq!(args, vec!["run", "--seat-id", "1", "--addr", "127.0.0.1:1"]);
        match prev {
            Some(v) => env::set_var(DAEMON_ADDR_ENV, v),
            None => env::remove_var(DAEMON_ADDR_ENV),
        }
    }

    #[test]
    fn resolve_returns_something() {
        // At least the PATH name fallback.
        assert!(resolve_agent_binary().is_some());
    }
}

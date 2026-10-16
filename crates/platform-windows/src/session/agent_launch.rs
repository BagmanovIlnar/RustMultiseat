//! Launch helper for `multiseat-session-agent` (same-machine spawn).
//!
//! Mock backends use this to exercise the heartbeat path. Native
//! `WindowsSessionBackend` still refuses until a documented
//! CreateProcessAsUser / session-token path exists (Stage 0).

use std::env;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};

use multiseat_core::{CoreError, CoreResult, SeatId};
use tracing::{info, warn};

/// Environment override for the agent binary path.
pub const AGENT_BIN_ENV: &str = "MULTISEAT_SESSION_AGENT";

/// When set to `1`, missing agent binary is an error even for mock soft-start.
pub const REQUIRE_AGENT_ENV: &str = "MULTISEAT_REQUIRE_AGENT";

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

/// Builds the argv for a session agent (without resolving the binary).
pub fn agent_args(seat_id: SeatId, daemon_addr: Option<&str>) -> Vec<String> {
    let mut args = vec![
        "run".into(),
        "--seat-id".into(),
        seat_id.0.to_string(),
    ];
    if let Some(addr) = daemon_addr {
        args.push("--addr".into());
        args.push(addr.to_string());
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

    #[test]
    fn args_include_seat_and_optional_addr() {
        let args = agent_args(SeatId(3), None);
        assert_eq!(args, vec!["run", "--seat-id", "3"]);
        let args = agent_args(SeatId(3), Some("127.0.0.1:9"));
        assert_eq!(
            args,
            vec!["run", "--seat-id", "3", "--addr", "127.0.0.1:9"]
        );
    }

    #[test]
    fn resolve_returns_something() {
        // At least the PATH name fallback.
        assert!(resolve_agent_binary().is_some());
    }
}

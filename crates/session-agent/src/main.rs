//! Session agent + Stage 0 dual-session probe entrypoint.
//!
//! Runs inside a user session with least privilege. Registers with the daemon,
//! sends heartbeats, and requests fail-open on shutdown/errors so host input
//! is never left suppressed.

use std::time::Duration;

use clap::{Parser, Subcommand};
use multiseat_core::{ClientRequest, SeatId, ServerResponse, PROTOCOL_VERSION};
use multiseat_daemon::{DaemonClient, DEFAULT_BIND};
use tracing::{error, info, warn};
use tracing_subscriber::EnvFilter;

/// Capability version advertised in `AgentHello`.
const AGENT_VERSION: u32 = 1;

#[derive(Debug, Parser)]
#[command(name = "multiseat-session-agent")]
struct Args {
    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    /// Stay resident as a seat session agent (IPC heartbeat to daemon).
    Run {
        /// Seat id this agent belongs to.
        #[arg(long)]
        seat_id: u32,
        /// Daemon endpoint (TCP or Named Pipe).
        #[arg(long, default_value = DEFAULT_BIND)]
        addr: String,
    },
    /// Enumerate WTS sessions and print dual-session Stage 0 evidence as JSON.
    Probe,
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::from_default_env().add_directive("info".parse().unwrap()))
        .init();

    let args = Args::parse();
    match args.command {
        Command::Run { seat_id, addr } => run_agent(SeatId(seat_id), &addr).await?,
        Command::Probe => {
            let result = multiseat_platform_windows::probe_dual_sessions();
            info!(
                status = ?result.status,
                active = result.active_count,
                "dual-session probe complete"
            );
            println!("{}", serde_json::to_string_pretty(&result)?);
        }
    }
    Ok(())
}

async fn run_agent(seat_id: SeatId, addr: &str) -> Result<(), Box<dyn std::error::Error>> {
    info!(%seat_id, %addr, "session agent connecting");
    let mut client = DaemonClient::connect(addr).await?;
    let ack = client
        .request(&ClientRequest::AgentHello {
            seat_id,
            agent_version: AGENT_VERSION,
        })
        .await?;

    let interval_ms = match ack {
        ServerResponse::AgentAck {
            seat_id: ack_seat,
            heartbeat_interval_ms,
        } => {
            if ack_seat != seat_id {
                return Err(format!("daemon ack seat mismatch: {ack_seat} != {seat_id}").into());
            }
            heartbeat_interval_ms.max(200)
        }
        other => return Err(format!("unexpected AgentHello response: {other:?}").into()),
    };

    info!(
        %seat_id,
        interval_ms,
        agent_version = AGENT_VERSION,
        protocol = PROTOCOL_VERSION,
        "session agent registered"
    );

    let mut ticker = tokio::time::interval(Duration::from_millis(interval_ms));
    ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);

    loop {
        tokio::select! {
            _ = ticker.tick() => {
                match client
                    .request(&ClientRequest::AgentHeartbeat { seat_id })
                    .await
                {
                    Ok(ServerResponse::Ok) => {}
                    Ok(other) => warn!(?other, "unexpected heartbeat response"),
                    Err(err) => {
                        error!(error = %err, "heartbeat failed; requesting fail-open then exit");
                        let _ = client
                            .request(&ClientRequest::AgentFailOpen {
                                seat_id,
                                reason: format!("heartbeat transport error: {err}"),
                            })
                            .await;
                        return Err(err.into());
                    }
                }
            }
            _ = tokio::signal::ctrl_c() => {
                info!(%seat_id, "Ctrl+C; fail-open then exit");
                let _ = client
                    .request(&ClientRequest::AgentFailOpen {
                        seat_id,
                        reason: "agent shutdown (Ctrl+C)".into(),
                    })
                    .await;
                return Ok(());
            }
        }
    }
}

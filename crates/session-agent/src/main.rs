//! Session agent + Stage 0 dual-session probe entrypoint.
//!
//! Runs inside a user session with least privilege. Real input/display
//! bridging is gated on Stage 0 feasibility results.

use clap::{Parser, Subcommand};
use tracing_subscriber::EnvFilter;

#[derive(Debug, Parser)]
#[command(name = "multiseat-session-agent")]
struct Args {
    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    /// Stay resident as a seat session agent (stub).
    Run {
        /// Seat id this agent belongs to.
        #[arg(long)]
        seat_id: u32,
    },
    /// Enumerate WTS sessions and print dual-session Stage 0 evidence as JSON.
    Probe,
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::from_default_env().add_directive("info".parse().unwrap()))
        .init();

    let args = Args::parse();
    match args.command {
        Command::Run { seat_id } => {
            tracing::info!(seat_id, "session agent stub started; waiting");
            loop {
                std::thread::sleep(std::time::Duration::from_secs(3600));
            }
        }
        Command::Probe => {
            let result = multiseat_platform_windows::probe_dual_sessions();
            tracing::info!(
                status = ?result.status,
                active = result.active_count,
                "dual-session probe complete"
            );
            println!("{}", serde_json::to_string_pretty(&result)?);
        }
    }
    Ok(())
}

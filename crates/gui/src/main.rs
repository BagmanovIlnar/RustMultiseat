//! Minimal Iced dashboard for RustMultiseat.
//!
//! Stage 6 slice: live IPC to the daemon — inventory, validation, Stage 0
//! feasibility, start/stop. Custom DnD and tray land in a later iteration.

mod app;
mod config_choice;
mod ipc;
mod panels;

use clap::Parser;
use multiseat_core::ServerResponse;
use multiseat_daemon::DEFAULT_BIND;

use app::MultiseatApp;

#[derive(Debug, Parser)]
#[command(name = "multiseat-gui")]
struct Args {
    /// Daemon endpoint: TCP `host:port` or Windows Named Pipe `\\.\pipe\Name`.
    #[arg(long, default_value = DEFAULT_BIND)]
    addr: String,
}

/// UI / IPC messages.
#[derive(Debug, Clone)]
pub enum Message {
    /// Manual snapshot refresh.
    Refresh,
    /// Periodic snapshot while connected.
    TickRefresh,
    /// Fetch Stage 0 feasibility report.
    Feasibility,
    /// Validate saved config (or lab sample if empty).
    Validate,
    /// Persist the built-in lab sample config on the daemon.
    SaveSample,
    /// Start multiseat with saved/lab config.
    Start,
    /// Stop multiseat / fail-open.
    Stop,
    /// Acknowledge emergency fail-open alert.
    AckEmergency,
    /// Result of a background daemon RPC.
    DaemonResult(Result<ServerResponse, String>),
}

fn main() -> iced::Result {
    let args = Args::parse();
    let addr = args.addr;
    iced::application(
        move || MultiseatApp::new(addr.clone()),
        MultiseatApp::update,
        MultiseatApp::view,
    )
    .title("RustMultiseat")
    .theme(MultiseatApp::theme)
    .subscription(MultiseatApp::subscription)
    .run()
}

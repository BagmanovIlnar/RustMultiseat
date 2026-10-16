//! Minimal Iced dashboard for RustMultiseat.
//!
//! Stage 6: live IPC, click-DnD seat assignment with local validation preview,
//! Validate/Start/Save against the draft. Tray lands later.

mod app;
mod assignment;
mod config_choice;
mod ipc;
mod panels;

use clap::Parser;
use multiseat_core::{DeviceId, DisplayId, SeatId, ServerResponse};
use multiseat_daemon::DEFAULT_BIND;

use app::MultiseatApp;
use assignment::Assignable;

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
    /// Validate draft (daemon).
    Validate,
    /// Persist draft config on the daemon.
    SaveDraft,
    /// Start multiseat with draft/saved/lab config.
    Start,
    /// Stop multiseat / fail-open.
    Stop,
    /// Acknowledge emergency fail-open alert.
    AckEmergency,
    /// Pick a hardware item (drag start).
    DragStarted(Assignable),
    /// Cancel current pick.
    CancelDrag,
    /// Drop picked item onto a seat.
    DroppedOnSeat(SeatId),
    /// Drop picked item onto host emergency slots.
    DroppedOnEmergency,
    /// Append an empty seat to the draft.
    AddSeat,
    /// Replace draft with lab sample config.
    LoadSampleDraft,
    /// Reset draft from last saved snapshot.
    ResetDraft,
    /// Clear one display from a seat.
    ClearSeatDisplay(SeatId, DisplayId),
    /// Clear one keyboard from a seat.
    ClearSeatKeyboard(SeatId, DeviceId),
    /// Clear one pointer from a seat.
    ClearSeatPointer(SeatId, DeviceId),
    /// Clear emergency keyboard.
    ClearEmergencyKeyboard,
    /// Clear emergency pointer.
    ClearEmergencyPointer,
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

//! Minimal Iced dashboard for RustMultiseat.
//!
//! Stage 6: live IPC, pointer drag-and-drop seat assignment with canvas
//! preview and Esc-cancel, validation preview, and Windows system tray
//! (hide-on-close + Show/Exit).

mod app;
mod assignment;
mod config_choice;
mod dnd;
mod ipc;
mod panels;
mod tray;

use clap::Parser;
use multiseat_core::{DaemonEvent, DeviceId, DisplayId, SeatId, ServerResponse};
use multiseat_daemon::DEFAULT_BIND;

use app::MultiseatApp;
use assignment::Assignable;
use tray::TrayCommand;

#[derive(Debug, Parser)]
#[command(name = "multiseat-gui")]
struct Args {
    /// Daemon endpoint: TCP `host:port` or Windows Named Pipe `\\.\pipe\Name`.
    #[arg(long, default_value = DEFAULT_BIND)]
    addr: String,

    /// Disable the system tray (window closes exit the app).
    #[arg(long)]
    no_tray: bool,
}

/// UI / IPC messages.
#[derive(Debug, Clone)]
pub enum Message {
    /// Manual snapshot refresh.
    Refresh,
    /// Periodic snapshot while connected.
    TickRefresh,
    /// Push event from a subscribed daemon connection.
    DaemonPush(DaemonEvent),
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
    /// Pick a hardware item (press starts a drag session).
    DragStarted(Assignable),
    /// Cancel current drag (Esc or Cancel button).
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
    /// OS close button (intercepted when tray hide is on).
    WindowCloseRequested(iced::window::Id),
    /// Toggle hide-to-tray preference (Windows tray builds only).
    ToggleHideToTray,
    /// Explicit exit from UI.
    ExitApp,
    /// Tray menu / double-click.
    Tray(TrayCommand),
    /// Internal: apply show to a discovered window id.
    TrayShowWindow(iced::window::Id),
    /// Result of a background daemon RPC.
    DaemonResult(Result<ServerResponse, String>),
}

fn main() -> iced::Result {
    let args = Args::parse();
    let addr = args.addr;
    let tray_enabled = !args.no_tray && tray::is_available();
    let hide_on_close = tray_enabled;

    iced::application(
        move || MultiseatApp::new(addr.clone(), tray_enabled, hide_on_close),
        MultiseatApp::update,
        MultiseatApp::view,
    )
    .title("RustMultiseat")
    .theme(MultiseatApp::theme)
    .subscription(MultiseatApp::subscription)
    // When hide-to-tray is on, we handle CloseRequested ourselves.
    .exit_on_close_request(!hide_on_close)
    .window_size(iced::Size::new(1280.0, 800.0))
    .run()
}

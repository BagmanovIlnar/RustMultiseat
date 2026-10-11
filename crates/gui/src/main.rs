//! Minimal Iced dashboard for RustMultiseat.
//!
//! Stage 6 slice: connect to the mock daemon, show inventory/state, and run
//! validate / start / stop. Custom DnD and tray land in a later iteration.

use clap::Parser;
use iced::widget::{button, column, container, row, scrollable, text, Column, Space};
use iced::{Element, Length, Task, Theme};
use multiseat_core::{
    ClientRequest, HardwareTree, MultiseatConfig, SeatState, ServerResponse, ValidationReport,
};
use multiseat_daemon::{lab_sample_config, DaemonClientSync, DEFAULT_BIND};

#[derive(Debug, Parser)]
#[command(name = "multiseat-gui")]
struct Args {
    /// Daemon TCP address.
    #[arg(long, default_value = DEFAULT_BIND)]
    addr: String,
}

#[derive(Debug, Clone)]
enum Message {
    Refresh,
    ValidateSample,
    StartSample,
    Stop,
    DaemonResult(Result<ServerResponse, String>),
}

#[derive(Debug)]
struct MultiseatApp {
    addr: String,
    connected: bool,
    busy: bool,
    status: String,
    state: SeatState,
    hardware: HardwareTree,
    config: MultiseatConfig,
    last_validation: Option<ValidationReport>,
}

impl MultiseatApp {
    fn new(addr: String) -> (Self, Task<Message>) {
        let boot_addr = addr.clone();
        let app = Self {
            addr,
            connected: false,
            busy: true,
            status: "Connecting…".into(),
            state: SeatState::Idle,
            hardware: HardwareTree::empty(),
            config: MultiseatConfig::default(),
            last_validation: None,
        };
        (
            app,
            Task::perform(snapshot_async(boot_addr), Message::DaemonResult),
        )
    }

    fn update(&mut self, message: Message) -> Task<Message> {
        match message {
            Message::Refresh => self.kick(ClientRequest::GetSnapshot),
            Message::ValidateSample => {
                self.kick(ClientRequest::ValidateConfig(lab_sample_config()))
            }
            Message::StartSample => self.kick(ClientRequest::Start(lab_sample_config())),
            Message::Stop => self.kick(ClientRequest::Stop),
            Message::DaemonResult(result) => {
                self.busy = false;
                let refresh = match result {
                    Ok(response) => {
                        let mutating =
                            matches!(response, ServerResponse::Ok | ServerResponse::Validation(_));
                        self.apply_response(response);
                        mutating && self.connected
                    }
                    Err(err) => {
                        self.connected = false;
                        self.status = format!("Error: {err}");
                        false
                    }
                };
                if refresh {
                    self.kick(ClientRequest::GetSnapshot)
                } else {
                    Task::none()
                }
            }
        }
    }

    fn kick(&mut self, request: ClientRequest) -> Task<Message> {
        if self.busy {
            return Task::none();
        }
        self.busy = true;
        self.status = "Working…".into();
        let addr = self.addr.clone();
        Task::perform(request_async(addr, request), Message::DaemonResult)
    }

    fn apply_response(&mut self, response: ServerResponse) {
        match response {
            ServerResponse::HelloAck { .. } => {
                self.connected = true;
                self.status = "Connected".into();
            }
            ServerResponse::Snapshot {
                hardware,
                config,
                state,
            } => {
                self.connected = true;
                self.hardware = *hardware;
                self.config = *config;
                self.state = state;
                self.status = format!(
                    "Snapshot rev {} · {} devices · {} displays",
                    self.hardware.revision,
                    self.hardware.input_devices.len(),
                    self.hardware.displays.len()
                );
            }
            ServerResponse::Validation(report) => {
                self.connected = true;
                let errors = report
                    .issues
                    .iter()
                    .filter(|i| matches!(i.severity, multiseat_core::ValidationSeverity::Error))
                    .count();
                self.status = if errors == 0 {
                    "Validation OK".into()
                } else {
                    format!("Validation failed ({errors} errors)")
                };
                self.last_validation = Some(report);
            }
            ServerResponse::Feasibility(report) => {
                self.connected = true;
                self.status = report.summary;
            }
            ServerResponse::Ok => {
                self.connected = true;
                self.status = "OK".into();
                // Refresh after mutating commands.
            }
            ServerResponse::Err(err) => {
                self.status = format!("Daemon error: {err:?}");
            }
        }
    }

    fn view(&self) -> Element<'_, Message> {
        let header = column![
            text("RustMultiseat").size(28),
            text(format!("Daemon: {}", self.addr)).size(14),
            text(format!("State: {:?}", self.state)).size(16),
            text(&self.status).size(14),
        ]
        .spacing(4);

        let controls = row![
            button("Refresh").on_press_maybe((!self.busy).then_some(Message::Refresh)),
            button("Validate sample")
                .on_press_maybe((!self.busy).then_some(Message::ValidateSample)),
            button("Start sample").on_press_maybe((!self.busy).then_some(Message::StartSample)),
            button("Stop").on_press_maybe((!self.busy).then_some(Message::Stop)),
        ]
        .spacing(8);

        let devices = device_list(&self.hardware);
        let seats = seat_list(&self.config);
        let validation = validation_list(self.last_validation.as_ref());

        let body = row![
            column![text("Hardware").size(20), scrollable(devices)]
                .spacing(8)
                .width(Length::FillPortion(2)),
            column![text("Seats / config").size(20), scrollable(seats)]
                .spacing(8)
                .width(Length::FillPortion(1)),
            column![text("Validation").size(20), scrollable(validation)]
                .spacing(8)
                .width(Length::FillPortion(1)),
        ]
        .spacing(16)
        .height(Length::Fill);

        container(
            column![header, controls, Space::new().height(12), body]
                .spacing(12)
                .padding(16),
        )
        .width(Length::Fill)
        .height(Length::Fill)
        .into()
    }
}

fn device_list(hardware: &HardwareTree) -> Column<'_, Message> {
    let mut col = column![].spacing(4);
    if hardware.input_devices.is_empty() && hardware.displays.is_empty() {
        col = col.push(text("No hardware loaded yet."));
        return col;
    }
    for gpu in &hardware.gpus {
        col = col.push(text(format!("GPU: {}", gpu.description)).size(14));
    }
    for display in &hardware.displays {
        col = col.push(text(format!(
            "Display {} ({}x{})",
            display.id,
            display.width.unwrap_or(0),
            display.height.unwrap_or(0)
        )));
    }
    for device in &hardware.input_devices {
        col = col.push(text(format!("{:?} · {}", device.kind, device.name)));
    }
    for root in &hardware.usb_roots {
        col = col.push(text(format!("USB root: {}", root.description)));
        for child in &root.children {
            col = col.push(text(format!(
                "  hub port {:?}: {}",
                child.port, child.description
            )));
        }
    }
    col
}

fn seat_list(config: &MultiseatConfig) -> Column<'_, Message> {
    let mut col = column![].spacing(4);
    if let Some(kbd) = &config.emergency_keyboard {
        col = col.push(text(format!("Emergency kbd: {kbd}")));
    }
    if let Some(ptr) = &config.emergency_pointer {
        col = col.push(text(format!("Emergency ptr: {ptr}")));
    }
    if config.seats.is_empty() {
        col = col.push(text("No seats in saved config."));
    }
    for seat in &config.seats {
        col = col.push(text(format!(
            "{} enabled={} user={}",
            seat.id, seat.enabled, seat.user_sid
        )));
    }
    col
}

fn validation_list(report: Option<&ValidationReport>) -> Column<'_, Message> {
    let mut col = column![].spacing(4);
    match report {
        None => col = col.push(text("No validation run yet.")),
        Some(report) if report.issues.is_empty() => col = col.push(text("No issues.")),
        Some(report) => {
            for issue in &report.issues {
                col = col.push(text(format!(
                    "{:?} {}: {}",
                    issue.severity, issue.code, issue.message
                )));
            }
        }
    }
    col
}

async fn snapshot_async(addr: String) -> Result<ServerResponse, String> {
    request_async(addr, ClientRequest::GetSnapshot).await
}

async fn request_async(addr: String, request: ClientRequest) -> Result<ServerResponse, String> {
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let result = (|| {
            let mut client = DaemonClientSync::connect(&addr).map_err(|e| e.to_string())?;
            client.request(&request).map_err(|e| e.to_string())
        })();
        let _ = tx.send(result);
    });
    match rx.recv() {
        Ok(result) => result,
        Err(err) => Err(err.to_string()),
    }
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
    .theme(Theme::Dark)
    .run()
}

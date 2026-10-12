//! Iced application state: live IPC dashboard over the mock/native daemon.

use std::time::Duration;

use iced::widget::{button, column, container, row, scrollable, text, Space};
use iced::{Element, Length, Subscription, Task, Theme};
use multiseat_core::{
    ClientRequest, FeasibilityReport, HardwareTree, MultiseatConfig, SeatState, ServerResponse,
    ValidationReport,
};
use multiseat_daemon::lab_sample_config;

use crate::config_choice::{config_for_action, ConfigSource};
use crate::ipc::request_async;
use crate::panels::{device_list, feasibility_list, seat_list, validation_list};
use crate::Message;

/// Root dashboard application.
pub struct MultiseatApp {
    addr: String,
    connected: bool,
    busy: bool,
    status: String,
    state: SeatState,
    hardware: HardwareTree,
    config: MultiseatConfig,
    last_validation: Option<ValidationReport>,
    last_feasibility: Option<FeasibilityReport>,
    last_config_source: Option<ConfigSource>,
}

impl MultiseatApp {
    /// Boots the app and kicks an initial snapshot fetch.
    pub fn new(addr: String) -> (Self, Task<Message>) {
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
            last_feasibility: None,
            last_config_source: None,
        };
        (
            app,
            Task::perform(
                request_async(boot_addr, ClientRequest::GetSnapshot),
                Message::DaemonResult,
            ),
        )
    }

    /// Window theme.
    pub fn theme(&self) -> Theme {
        Theme::Dark
    }

    /// Periodic snapshot while connected (live inventory / state).
    pub fn subscription(&self) -> Subscription<Message> {
        if self.connected && !self.busy {
            iced::time::every(Duration::from_secs(5)).map(|_| Message::TickRefresh)
        } else {
            Subscription::none()
        }
    }

    /// Handles UI and IPC messages.
    pub fn update(&mut self, message: Message) -> Task<Message> {
        match message {
            Message::Refresh | Message::TickRefresh => self.kick(ClientRequest::GetSnapshot),
            Message::Feasibility => self.kick(ClientRequest::GetFeasibilityReport),
            Message::Validate => {
                let (cfg, src) = config_for_action(&self.config);
                self.last_config_source = Some(src);
                self.kick(ClientRequest::ValidateConfig(cfg))
            }
            Message::SaveSample => self.kick(ClientRequest::SaveConfig(lab_sample_config())),
            Message::Start => {
                let (cfg, src) = config_for_action(&self.config);
                self.last_config_source = Some(src);
                self.kick(ClientRequest::Start(cfg))
            }
            Message::Stop => self.kick(ClientRequest::Stop),
            Message::AckEmergency => self.kick(ClientRequest::AcknowledgeEmergency),
            Message::DaemonResult(result) => {
                self.busy = false;
                let refresh = match result {
                    Ok(response) => {
                        let mutating = matches!(
                            response,
                            ServerResponse::Ok
                                | ServerResponse::Validation(_)
                                | ServerResponse::Feasibility(_)
                        );
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
                    "Snapshot rev {} · {} inputs · {} displays · state={:?}",
                    self.hardware.revision,
                    self.hardware.input_devices.len(),
                    self.hardware.displays.len(),
                    self.state
                );
            }
            ServerResponse::Validation(report) => {
                self.connected = true;
                let errors = report
                    .issues
                    .iter()
                    .filter(|i| matches!(i.severity, multiseat_core::ValidationSeverity::Error))
                    .count();
                let src = self
                    .last_config_source
                    .map(|s| format!(" [{s:?}]"))
                    .unwrap_or_default();
                self.status = if errors == 0 {
                    format!("Validation OK{src}")
                } else {
                    format!("Validation failed ({errors} errors){src}")
                };
                self.last_validation = Some(report);
            }
            ServerResponse::Feasibility(report) => {
                self.connected = true;
                self.status = format!(
                    "Feasibility go={} · {}",
                    report.go, report.summary
                );
                self.last_feasibility = Some(report);
            }
            ServerResponse::Ok => {
                self.connected = true;
                self.status = "OK".into();
            }
            ServerResponse::Err(err) => {
                self.status = format!("Daemon error: {err:?}");
            }
        }
    }

    /// Renders the dashboard.
    pub fn view(&self) -> Element<'_, Message> {
        let link = if self.connected { "online" } else { "offline" };
        let header = column![
            text("RustMultiseat").size(28),
            text(format!("Daemon: {} ({link})", self.addr)).size(14),
            text(format!("Orchestrator: {:?}", self.state)).size(16),
            text(&self.status).size(14),
        ]
        .spacing(4);

        let controls = row![
            button("Refresh").on_press_maybe((!self.busy).then_some(Message::Refresh)),
            button("Feasibility").on_press_maybe((!self.busy).then_some(Message::Feasibility)),
            button("Validate").on_press_maybe((!self.busy).then_some(Message::Validate)),
            button("Save sample").on_press_maybe((!self.busy).then_some(Message::SaveSample)),
            button("Start").on_press_maybe((!self.busy).then_some(Message::Start)),
            button("Stop").on_press_maybe((!self.busy).then_some(Message::Stop)),
            button("Ack emergency")
                .on_press_maybe((!self.busy).then_some(Message::AckEmergency)),
        ]
        .spacing(8);

        let body = row![
            column![text("Hardware").size(20), scrollable(device_list(&self.hardware))]
                .spacing(8)
                .width(Length::FillPortion(2)),
            column![
                text("Seats / config").size(20),
                scrollable(seat_list(&self.config))
            ]
            .spacing(8)
            .width(Length::FillPortion(1)),
            column![
                text("Validation").size(20),
                scrollable(validation_list(self.last_validation.as_ref()))
            ]
            .spacing(8)
            .width(Length::FillPortion(1)),
            column![
                text("Feasibility").size(20),
                scrollable(feasibility_list(self.last_feasibility.as_ref()))
            ]
            .spacing(8)
            .width(Length::FillPortion(2)),
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

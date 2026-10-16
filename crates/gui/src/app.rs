//! Iced application state: live IPC dashboard + draft seat assignment.

use std::time::Duration;

use iced::widget::{button, column, container, row, scrollable, text, Space};
use iced::window::{self, Mode};
use iced::{Element, Length, Subscription, Task, Theme};
use multiseat_core::{
    ClientRequest, FeasibilityReport, HardwareTree, MultiseatConfig, SeatState, ServerResponse,
    ValidationReport,
};
use multiseat_daemon::lab_sample_config;

use crate::assignment::{
    add_seat, assign_emergency, assign_to_seat, clear_seat_display, clear_seat_keyboard,
    clear_seat_pointer, ensure_editable_seat, preview_validation, DragState,
};
use crate::config_choice::{config_for_action, ConfigSource};
use crate::ipc::request_async;
use crate::panels::{device_list, feasibility_list, seat_editor, validation_list};
use crate::tray::{self, TrayCommand};
use crate::Message;

/// Root dashboard application.
pub struct MultiseatApp {
    addr: String,
    connected: bool,
    busy: bool,
    status: String,
    state: SeatState,
    hardware: HardwareTree,
    /// Last config persisted on the daemon.
    saved: MultiseatConfig,
    /// Editable draft (DnD target); sent on Validate/Start/Save.
    draft: MultiseatConfig,
    /// True when draft diverged from `saved` via local edits.
    draft_dirty: bool,
    drag: Option<DragState>,
    last_validation: Option<ValidationReport>,
    /// True when `last_validation` came from local preview (not daemon).
    validation_is_preview: bool,
    last_feasibility: Option<FeasibilityReport>,
    last_config_source: Option<ConfigSource>,
    /// Tray subscription is running (Windows).
    tray_enabled: bool,
    /// Close button hides to tray instead of exiting.
    hide_to_tray: bool,
    /// Last known main window id (for show/hide).
    main_window: Option<window::Id>,
    /// True after a close-to-hide.
    window_hidden: bool,
}

impl MultiseatApp {
    /// Boots the app and kicks an initial snapshot fetch.
    pub fn new(addr: String, tray_enabled: bool, hide_to_tray: bool) -> (Self, Task<Message>) {
        let boot_addr = addr.clone();
        let mut draft = MultiseatConfig::default();
        ensure_editable_seat(&mut draft);
        let app = Self {
            addr,
            connected: false,
            busy: true,
            status: "Connecting…".into(),
            state: SeatState::Idle,
            hardware: HardwareTree::empty(),
            saved: MultiseatConfig::default(),
            draft,
            draft_dirty: false,
            drag: None,
            last_validation: None,
            validation_is_preview: false,
            last_feasibility: None,
            last_config_source: None,
            tray_enabled,
            hide_to_tray: hide_to_tray && tray_enabled,
            main_window: None,
            window_hidden: false,
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

    /// IPC timer + close requests + tray channel.
    pub fn subscription(&self) -> Subscription<Message> {
        let mut batch = vec![window::close_requests().map(Message::WindowCloseRequested)];

        if self.tray_enabled {
            batch.push(tray::subscription(true).map(Message::Tray));
        }

        if self.connected && !self.busy && !self.window_hidden {
            batch.push(iced::time::every(Duration::from_secs(5)).map(|_| Message::TickRefresh));
        }

        Subscription::batch(batch)
    }

    /// Handles UI and IPC messages.
    pub fn update(&mut self, message: Message) -> Task<Message> {
        match message {
            Message::WindowCloseRequested(id) => {
                self.main_window = Some(id);
                if self.hide_to_tray && self.tray_enabled {
                    self.window_hidden = true;
                    self.status = "Hidden to tray (Show / Exit from tray menu)".into();
                    window::set_mode(id, Mode::Hidden)
                } else {
                    iced::exit()
                }
            }
            Message::ToggleHideToTray => {
                if self.tray_enabled {
                    self.hide_to_tray = !self.hide_to_tray;
                    self.status = format!(
                        "Hide to tray on close: {}",
                        if self.hide_to_tray { "on" } else { "off" }
                    );
                } else {
                    self.status = "Tray unavailable on this platform".into();
                }
                Task::none()
            }
            Message::ExitApp => iced::exit(),
            Message::Tray(TrayCommand::Show) => self.show_window(),
            Message::TrayShowWindow(id) => {
                self.main_window = Some(id);
                self.window_hidden = false;
                Task::batch([
                    window::set_mode(id, Mode::Windowed),
                    window::gain_focus(id),
                ])
            }
            Message::Tray(TrayCommand::Exit) => iced::exit(),
            Message::Refresh | Message::TickRefresh => self.kick(ClientRequest::GetSnapshot),
            Message::Feasibility => self.kick(ClientRequest::GetFeasibilityReport),
            Message::Validate => {
                let (cfg, src) = config_for_action(&self.draft, &self.saved);
                self.last_config_source = Some(src);
                self.kick(ClientRequest::ValidateConfig(cfg))
            }
            Message::SaveDraft => {
                let (cfg, src) = config_for_action(&self.draft, &self.saved);
                self.last_config_source = Some(src);
                self.kick(ClientRequest::SaveConfig(cfg))
            }
            Message::Start => {
                let (cfg, src) = config_for_action(&self.draft, &self.saved);
                self.last_config_source = Some(src);
                self.kick(ClientRequest::Start(cfg))
            }
            Message::Stop => self.kick(ClientRequest::Stop),
            Message::AckEmergency => self.kick(ClientRequest::AcknowledgeEmergency),
            Message::DragStarted(item) => {
                self.drag = Some(DragState { item });
                self.status = format!(
                    "Picked {}; click a seat or Emergency",
                    self.drag.as_ref().unwrap().item.label()
                );
                Task::none()
            }
            Message::CancelDrag => {
                self.drag = None;
                self.status = "Pick cancelled".into();
                Task::none()
            }
            Message::DroppedOnSeat(seat_id) => {
                if let Some(drag) = self.drag.take() {
                    match assign_to_seat(&mut self.draft, seat_id, drag.item) {
                        Ok(()) => {
                            self.draft_dirty = true;
                            self.refresh_preview();
                            self.status = format!("Assigned to {seat_id}");
                        }
                        Err(err) => self.status = err,
                    }
                }
                Task::none()
            }
            Message::DroppedOnEmergency => {
                if let Some(drag) = self.drag.take() {
                    match assign_emergency(&mut self.draft, drag.item) {
                        Ok(()) => {
                            self.draft_dirty = true;
                            self.refresh_preview();
                            self.status = "Assigned to emergency".into();
                        }
                        Err(err) => self.status = err,
                    }
                }
                Task::none()
            }
            Message::AddSeat => {
                let id = add_seat(&mut self.draft);
                self.draft_dirty = true;
                self.refresh_preview();
                self.status = format!("Added {id}");
                Task::none()
            }
            Message::LoadSampleDraft => {
                self.draft = lab_sample_config();
                self.draft_dirty = true;
                self.drag = None;
                self.refresh_preview();
                self.status = "Draft loaded from lab sample".into();
                Task::none()
            }
            Message::ResetDraft => {
                self.sync_draft_from_saved();
                self.draft_dirty = false;
                self.drag = None;
                self.refresh_preview();
                self.status = "Draft reset from saved config".into();
                Task::none()
            }
            Message::ClearSeatDisplay(seat_id, id) => {
                clear_seat_display(&mut self.draft, seat_id, &id);
                self.draft_dirty = true;
                self.refresh_preview();
                Task::none()
            }
            Message::ClearSeatKeyboard(seat_id, id) => {
                clear_seat_keyboard(&mut self.draft, seat_id, &id);
                self.draft_dirty = true;
                self.refresh_preview();
                Task::none()
            }
            Message::ClearSeatPointer(seat_id, id) => {
                clear_seat_pointer(&mut self.draft, seat_id, &id);
                self.draft_dirty = true;
                self.refresh_preview();
                Task::none()
            }
            Message::ClearEmergencyKeyboard => {
                self.draft.emergency_keyboard = None;
                self.draft_dirty = true;
                self.refresh_preview();
                Task::none()
            }
            Message::ClearEmergencyPointer => {
                self.draft.emergency_pointer = None;
                self.draft_dirty = true;
                self.refresh_preview();
                Task::none()
            }
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

    fn refresh_preview(&mut self) {
        let report = preview_validation(&self.draft, &self.hardware);
        let errors = report
            .issues
            .iter()
            .filter(|i| matches!(i.severity, multiseat_core::ValidationSeverity::Error))
            .count();
        self.validation_is_preview = true;
        self.last_validation = Some(report);
        if !self.status.starts_with("Assigned")
            && !self.status.starts_with("Added")
            && !self.status.starts_with("Draft")
        {
            self.status = format!("Preview: {errors} error(s)");
        }
    }

    fn sync_draft_from_saved(&mut self) {
        self.draft = self.saved.clone();
        ensure_editable_seat(&mut self.draft);
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

    fn show_window(&mut self) -> Task<Message> {
        self.window_hidden = false;
        self.status = "Window restored from tray".into();
        if let Some(id) = self.main_window {
            return Task::batch([
                window::set_mode(id, Mode::Windowed),
                window::gain_focus(id),
            ]);
        }
        window::latest().then(|id| match id {
            Some(id) => Task::done(Message::TrayShowWindow(id)),
            None => Task::none(),
        })
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
                self.saved = *config;
                self.state = state;
                if !self.draft_dirty {
                    self.sync_draft_from_saved();
                }
                let dirty = if self.draft_dirty { " · draft dirty" } else { "" };
                self.status = format!(
                    "Snapshot rev {} · {} inputs · {} displays · state={:?}{dirty}",
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
                self.validation_is_preview = false;
                self.last_validation = Some(report);
            }
            ServerResponse::Feasibility(report) => {
                self.connected = true;
                self.status = format!("Feasibility go={} · {}", report.go, report.summary);
                self.last_feasibility = Some(report);
            }
            ServerResponse::AgentAck {
                seat_id,
                heartbeat_interval_ms,
            } => {
                self.connected = true;
                self.status = format!(
                    "Agent ack seat={seat_id} heartbeat={heartbeat_interval_ms}ms"
                );
            }
            ServerResponse::Ok => {
                self.connected = true;
                self.status = "OK".into();
                // After successful save, treat draft as clean on next snapshot.
                if matches!(self.last_config_source, Some(ConfigSource::Draft)) {
                    self.draft_dirty = false;
                }
            }
            ServerResponse::Err(err) => {
                self.status = format!("Daemon error: {err:?}");
            }
        }
    }

    /// Renders the dashboard.
    pub fn view(&self) -> Element<'_, Message> {
        let link = if self.connected { "online" } else { "offline" };
        let dirty = if self.draft_dirty { "yes" } else { "no" };
        let tray = if self.tray_enabled {
            format!(
                "tray · hide_on_close={}",
                if self.hide_to_tray { "on" } else { "off" }
            )
        } else {
            "no tray".into()
        };
        let header = column![
            text("RustMultiseat").size(28),
            text(format!("Daemon: {} ({link})", self.addr)).size(14),
            text(format!(
                "Orchestrator: {:?} · draft dirty={dirty} · {tray}",
                self.state
            ))
            .size(16),
            text(&self.status).size(14),
        ]
        .spacing(4);

        let hide_label = if self.hide_to_tray {
            "Hide to tray: ON"
        } else {
            "Hide to tray: OFF"
        };
        let controls = row![
            button("Refresh").on_press_maybe((!self.busy).then_some(Message::Refresh)),
            button("Feasibility").on_press_maybe((!self.busy).then_some(Message::Feasibility)),
            button("Validate").on_press_maybe((!self.busy).then_some(Message::Validate)),
            button("Save draft").on_press_maybe((!self.busy).then_some(Message::SaveDraft)),
            button("Start").on_press_maybe((!self.busy).then_some(Message::Start)),
            button("Stop").on_press_maybe((!self.busy).then_some(Message::Stop)),
            button("Ack emergency")
                .on_press_maybe((!self.busy).then_some(Message::AckEmergency)),
            button(hide_label).on_press_maybe(
                self.tray_enabled
                    .then_some(Message::ToggleHideToTray)
            ),
            button("Exit").on_press(Message::ExitApp),
        ]
        .spacing(8);

        let body = row![
            column![
                text("Hardware (pick)").size(20),
                scrollable(device_list(&self.hardware, self.drag.as_ref()))
            ]
            .spacing(8)
            .width(Length::FillPortion(2)),
            column![
                text("Seats (drop)").size(20),
                scrollable(seat_editor(&self.draft, self.drag.as_ref()))
            ]
            .spacing(8)
            .width(Length::FillPortion(2)),
            column![
                text("Validation").size(20),
                scrollable(validation_list(
                    self.last_validation.as_ref(),
                    self.validation_is_preview
                ))
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

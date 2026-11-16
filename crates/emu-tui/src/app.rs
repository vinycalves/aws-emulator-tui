//! Application state, background work and key handling.

use std::collections::BTreeMap;
use std::time::{Duration, Instant, SystemTime};

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use emu_core::catalog;
use emu_core::client::{ApiError, ApiResponse};
use emu_core::config::{self, ConnectionConfig};
use emu_core::services::{self, CreateKind, Row, Table, ViewId, WriteAction};
use emu_core::{EmulatorClient, Supervisor};
use ratatui::widgets::{ListState, TableState};
use serde_json::Value;
use tokio::sync::mpsc::{unbounded_channel, UnboundedReceiver, UnboundedSender};
use tui_textarea::TextArea;

use crate::theme;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Tab {
    Explorer,
    Console,
    Emulator,
    Requests,
}

impl Tab {
    pub const ALL: [Tab; 4] = [Tab::Explorer, Tab::Console, Tab::Emulator, Tab::Requests];

    pub fn title(self) -> &'static str {
        match self {
            Tab::Explorer => "Explorer",
            Tab::Console => "Console",
            Tab::Emulator => "Emulator",
            Tab::Requests => "Requests",
        }
    }

    fn next(self) -> Tab {
        match self {
            Tab::Explorer => Tab::Console,
            Tab::Console => Tab::Emulator,
            Tab::Emulator => Tab::Requests,
            Tab::Requests => Tab::Explorer,
        }
    }

    fn previous(self) -> Tab {
        match self {
            Tab::Explorer => Tab::Requests,
            Tab::Console => Tab::Explorer,
            Tab::Emulator => Tab::Console,
            Tab::Requests => Tab::Emulator,
        }
    }
}

#[derive(Clone, Debug)]
pub enum Health {
    Unknown,
    Up,
    Down(String),
}

impl Health {
    pub fn is_up(&self) -> bool {
        matches!(self, Health::Up)
    }
}

#[derive(Clone, Debug)]
pub enum ServiceStatus {
    Unknown,
    Loading,
    Ok,
    Err,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Focus {
    Sidebar,
    Table,
}

#[derive(Clone, Debug)]
pub struct Toast {
    pub text: String,
    pub kind: ToastKind,
    pub until: Instant,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ToastKind {
    Info,
    Ok,
    Error,
}

impl ToastKind {
    pub fn style(self) -> ratatui::style::Style {
        match self {
            ToastKind::Info => theme::muted(),
            ToastKind::Ok => theme::ok(),
            ToastKind::Error => theme::err(),
        }
    }
}

#[derive(Clone, Debug)]
pub struct Activity {
    pub time: String,
    pub service: String,
    pub operation: String,
    pub status: String,
    pub duration_ms: u128,
    pub request: String,
    pub response: String,
    pub ok: bool,
}

pub struct ExplorerState {
    pub focus: Focus,
    pub service_index: usize,
    pub view: ViewId,
    pub table: Option<Table>,
    pub table_state: TableState,
    pub loading: bool,
    pub error: Option<String>,
    pub filter: Option<String>,
    pub show_detail: bool,
    pub detail_title: String,
    pub detail_body: String,
    pub detail_scroll: u16,
    pub generation: u64,
    pub subview_index: usize,
}

impl ExplorerState {
    pub fn visible_rows(&self) -> Vec<&Row> {
        let Some(table) = &self.table else {
            return Vec::new();
        };
        match &self.filter {
            None => table.rows.iter().collect(),
            Some(needle) => {
                let needle = needle.to_ascii_lowercase();
                table
                    .rows
                    .iter()
                    .filter(|row| {
                        row.id.to_ascii_lowercase().contains(&needle)
                            || row
                                .cells
                                .iter()
                                .any(|cell| cell.to_ascii_lowercase().contains(&needle))
                    })
                    .collect()
            }
        }
    }

    pub fn selected_row(&self) -> Option<&Row> {
        let rows = self.visible_rows();
        let index = self.table_state.selected().unwrap_or(0);
        rows.get(index).copied()
    }
}

pub struct ConsoleState {
    pub service: String,
    pub operation: String,
    pub textarea: TextArea<'static>,
    pub response: String,
    pub status: Option<(u16, u128)>,
    pub error: Option<String>,
    pub busy: bool,
    pub focus_editor: bool,
    pub scroll: u16,
}

impl ConsoleState {
    pub fn protocol(&self) -> String {
        catalog::catalog()
            .get(&self.service)
            .map(|service| service.protocol().to_string())
            .unwrap_or_default()
    }

    pub fn body_text(&self) -> String {
        self.textarea.lines().join("\n")
    }
}

pub struct EmulatorState {
    pub logs_offset: u16,
}

pub struct App {
    pub config: ConnectionConfig,
    pub client: EmulatorClient,
    pub supervisor: Supervisor,
    pub health: Health,
    pub identity: String,
    pub tab: Tab,
    pub explorer: ExplorerState,
    pub console: ConsoleState,
    pub emulator: EmulatorState,
    pub services: Vec<&'static services::CuratedService>,
    pub service_status: BTreeMap<&'static str, ServiceStatus>,
    pub activities: Vec<Activity>,
    pub activity_state: TableState,
    pub modal: Option<Modal>,
    pub toast: Option<Toast>,
    pub should_quit: bool,
    tx: UnboundedSender<Msg>,
    rx: UnboundedReceiver<Msg>,
    last_health: Instant,
}

impl App {
    pub fn new(config: ConnectionConfig) -> anyhow::Result<Self> {
        let client = EmulatorClient::new(&config)?;
        let (tx, rx) = unbounded_channel();
        let supervisor = Supervisor::new(config.binary.clone(), config.port());

        let mut console = ConsoleState {
            service: "dynamodb".to_string(),
            operation: "ListTables".to_string(),
            textarea: TextArea::new(vec!["{}".to_string()]),
            response: String::new(),
            status: None,
            error: None,
            busy: false,
            focus_editor: false,
            scroll: 0,
        };
        console.textarea.set_block(
            ratatui::widgets::Block::bordered()
                .title(" request ")
                .border_style(theme::muted()),
        );

        let mut app = Self {
            config,
            client,
            supervisor,
            health: Health::Unknown,
            identity: String::new(),
            tab: Tab::Explorer,
            explorer: ExplorerState {
                focus: Focus::Table,
                service_index: 0,
                view: ViewId::S3Buckets,
                table: None,
                table_state: TableState::default(),
                loading: false,
                error: None,
                filter: None,
                show_detail: false,
                detail_title: String::new(),
                detail_body: String::new(),
                detail_scroll: 0,
                generation: 0,
                subview_index: 0,
            },
            console,
            emulator: EmulatorState { logs_offset: 0 },
            services: services::CURATED.iter().collect(),
            service_status: BTreeMap::new(),
            activities: Vec::new(),
            activity_state: TableState::default(),
            modal: None,
            toast: None,
            should_quit: false,
            tx,
            rx,
            last_health: Instant::now() - Duration::from_secs(10),
        };

        app.spawn_health();
        app.spawn_identity();
        app.spawn_probes();
        app.load_table();
        Ok(app)
    }

    pub async fn run(mut self) -> anyhow::Result<()> {
        let mut terminal = ratatui::init();
        let result = self.event_loop(&mut terminal).await;
        ratatui::restore();
        result
    }

    async fn event_loop(
        &mut self,
        terminal: &mut ratatui::Terminal<ratatui::backend::CrosstermBackend<std::io::Stdout>>,
    ) -> anyhow::Result<()> {
        let (event_tx, mut event_rx) = unbounded_channel();
        std::thread::spawn(move || loop {
            match crossterm::event::read() {
                Ok(crossterm::event::Event::Key(key)) => {
                    if event_tx.send(key).is_err() {
                        break;
                    }
                }
                Ok(_) => {}
                Err(_) => break,
            }
        });

        let mut ticker = tokio::time::interval(Duration::from_millis(250));
        ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);

        loop {
            terminal.draw(|frame| crate::ui::draw(frame, self))?;

            tokio::select! {
                _ = ticker.tick() => {
                    self.on_tick();
                }
                Some(message) = self.rx.recv() => {
                    self.on_message(message);
                }
                Some(key) = event_rx.recv() => {
                    self.handle_key(key).await;
                }
            }

            if self.should_quit {
                break;
            }
        }

        if self.supervisor.has_child() {
            let _ = self.supervisor.stop().await;
        }
        Ok(())
    }

    // ---------------------------------------------------------------- background

    fn spawn_health(&self) {
        let client = self.client.clone();
        let tx = self.tx.clone();
        tokio::spawn(async move {
            let result = client.health().await;
            let _ = tx.send(Msg::Health(result));
        });
    }

    fn spawn_identity(&self) {
        let client = self.client.clone();
        let tx = self.tx.clone();
        tokio::spawn(async move {
            let result = services::caller_identity(&client).await;
            let _ = tx.send(Msg::Identity(result));
        });
    }

    fn spawn_probes(&mut self) {
        for service in &self.services {
            self.service_status
                .insert(service.key, ServiceStatus::Loading);
            let client = self.client.clone();
            let tx = self.tx.clone();
            let key = service.key;
            tokio::spawn(async move {
                let result = services::probe(&client, key).await;
                let _ = tx.send(Msg::Probed { key, result });
            });
        }
    }

    pub fn load_table(&mut self) {
        self.explorer.generation += 1;
        let generation = self.explorer.generation;
        let view = self.explorer.view.clone();
        self.explorer.loading = true;
        self.explorer.error = None;
        let client = self.client.clone();
        let tx = self.tx.clone();
        let started = Instant::now();
        tokio::spawn(async move {
            let result = services::fetch(&client, &view).await;
            let _ = tx.send(Msg::Table {
                generation,
                view,
                started,
                result,
            });
        });
    }

    fn spawn_detail(&mut self, view: ViewId, id: String) {
        self.explorer.show_detail = true;
        self.explorer.detail_title = id.clone();
        self.explorer.detail_body = "Loading…".to_string();
        self.explorer.detail_scroll = 0;

        if let Some((service, operation, input)) = services::describe_call(&view, &id) {
            let client = self.client.clone();
            let tx = self.tx.clone();
            let started = Instant::now();
            let request = format!("{service}.{operation}\n{}", pretty_json(&input));
            tokio::spawn(async move {
                let result = client.call(service, operation, input).await;
                let _ = tx.send(Msg::Detail {
                    view,
                    id,
                    operation: operation.to_string(),
                    service: service.to_string(),
                    request,
                    started,
                    result,
                });
            });
        } else if let Some(row) = self.explorer.selected_row() {
            if let Some(detail) = &row.detail {
                self.explorer.detail_body = pretty_json(detail);
            } else {
                self.explorer.detail_body = "No detail available for this resource.".to_string();
            }
        }
    }

    fn spawn_write(&mut self, action: WriteAction, label: String) {
        let client = self.client.clone();
        let tx = self.tx.clone();
        let view = self.explorer.view.clone();
        let started = Instant::now();
        let (service, operation, request) = match &action {
            WriteAction::Call {
                service,
                operation,
                input,
            } => (
                service.to_string(),
                operation.to_string(),
                format!("{service}.{operation}\n{}", pretty_json(input)),
            ),
            WriteAction::S3 {
                method,
                path,
                query,
                ..
            } => {
                let suffix = if query.is_empty() {
                    String::new()
                } else {
                    format!("?{query}")
                };
                (
                    "s3".to_string(),
                    format!("{method} {}", path.trim_start_matches('/')),
                    format!("{method} {path}{suffix}"),
                )
            }
        };
        tokio::spawn(async move {
            let result = services::execute(&client, &action).await;
            let _ = tx.send(Msg::Write {
                label,
                view,
                service,
                operation,
                request,
                started,
                result,
            });
        });
    }

    fn send_console_request(&mut self) {
        if self.console.operation.is_empty() {
            self.toast(ToastKind::Error, "Pick an operation first (o)");
            return;
        }
        let text = self.console.body_text();
        let input: Value = if text.trim().is_empty() {
            Value::Object(Default::default())
        } else {
            match serde_json::from_str(&text) {
                Ok(value) => value,
                Err(error) => {
                    self.toast(ToastKind::Error, format!("Invalid JSON: {error}"));
                    return;
                }
            }
        };

        self.console.busy = true;
        self.console.error = None;
        self.console.scroll = 0;
        let client = self.client.clone();
        let tx = self.tx.clone();
        let service = self.console.service.clone();
        let operation = self.console.operation.clone();
        let request = format!("{service}.{operation}\n{}", pretty_json(&input));
        let started = Instant::now();
        tokio::spawn(async move {
            let result = client.call(&service, &operation, input).await;
            let _ = tx.send(Msg::Console {
                service,
                operation,
                request,
                started,
                result,
            });
        });
    }

    // ------------------------------------------------------------------ messages

    fn on_message(&mut self, message: Msg) {
        match message {
            Msg::Health(result) => {
                let was_up = self.health.is_up();
                self.health = match result {
                    Ok(()) => Health::Up,
                    Err(error) => Health::Down(error),
                };
                if !was_up && self.health.is_up() {
                    self.reload_connection();
                    self.spawn_identity();
                    self.spawn_probes();
                    self.load_table();
                }
            }
            Msg::Identity(result) => match result {
                Ok(identity) => self.identity = identity,
                Err(error) => {
                    self.identity = if error.code.as_deref() == Some("TransportError") {
                        String::new()
                    } else {
                        format!(
                            "identity unavailable ({})",
                            error.code.as_deref().unwrap_or("error")
                        )
                    }
                }
            },
            Msg::Probed { key, result } => {
                let status = match result {
                    Ok(()) => ServiceStatus::Ok,
                    Err(_) => ServiceStatus::Err,
                };
                self.service_status.insert(key, status);
            }
            Msg::Table {
                generation,
                view,
                started,
                result,
            } => {
                if generation != self.explorer.generation || view != self.explorer.view {
                    return;
                }
                self.explorer.loading = false;
                match result {
                    Ok(table) => {
                        let count = table.rows.len();
                        let (service, operation) = services::list_operation(&view);
                        self.explorer.table = Some(table);
                        self.explorer.error = None;
                        self.explorer.table_state = TableState::default();
                        if count > 0 {
                            self.explorer.table_state.select(Some(0));
                        }
                        self.explorer.show_detail = false;
                        self.push_activity(Activity {
                            time: clock(),
                            service: service.to_string(),
                            operation: operation.to_string(),
                            status: "200".to_string(),
                            duration_ms: started.elapsed().as_millis(),
                            request: format!("{service}.{operation}"),
                            response: format!(
                                "{count} {}",
                                if count == 1 { "row" } else { "rows" }
                            ),
                            ok: true,
                        });
                    }
                    Err(error) => {
                        let (service, operation) = services::list_operation(&view);
                        self.explorer.table = None;
                        self.explorer.error = Some(error.message.clone());
                        self.push_activity(activity_from_error(
                            service,
                            operation,
                            format!("{service}.{operation}"),
                            started,
                            &error,
                        ));
                    }
                }
            }
            Msg::Detail {
                view,
                id,
                operation,
                service,
                request,
                started,
                result,
            } => {
                if view == self.explorer.view && id == self.explorer.detail_title {
                    match result {
                        Ok(response) => {
                            self.explorer.detail_body = response.pretty();
                            self.push_activity(activity_from_response(
                                &service,
                                &operation,
                                request.clone(),
                                started,
                                &response,
                            ));
                        }
                        Err(error) => {
                            self.explorer.detail_body = format!(
                                "{} {}\n\n{}",
                                error.status_label(),
                                error.code.clone().unwrap_or_default(),
                                error.message
                            );
                            self.push_activity(activity_from_error(
                                &service, &operation, request, started, &error,
                            ));
                        }
                    }
                }
            }
            Msg::Write {
                label,
                view,
                service,
                operation,
                request,
                started,
                result,
            } => {
                match &result {
                    Ok(response) => {
                        self.push_activity(activity_from_response(
                            &service,
                            &operation,
                            request.clone(),
                            started,
                            response,
                        ));
                        self.toast(ToastKind::Ok, format!("{label} succeeded"));
                    }
                    Err(error) => {
                        self.push_activity(activity_from_error(
                            &service, &operation, request, started, error,
                        ));
                        self.toast(
                            ToastKind::Error,
                            format!("{label} failed: {}", first_line(&error.message)),
                        );
                    }
                }
                if view == self.explorer.view {
                    self.load_table();
                }
            }
            Msg::Console {
                service,
                operation,
                request,
                started,
                result,
            } => {
                self.console.busy = false;
                match result {
                    Ok(response) => {
                        self.console.status = Some((response.status, response.duration_ms));
                        self.console.error = None;
                        self.console.response = response.pretty();
                        self.push_activity(activity_from_response(
                            &service,
                            &operation,
                            request.clone(),
                            started,
                            &response,
                        ));
                    }
                    Err(error) => {
                        self.console.status =
                            error.status.map(|status| (status, error.duration_ms));
                        self.console.error = Some(error.message.clone());
                        self.console.response = if error.body.trim().is_empty() {
                            error.message.clone()
                        } else {
                            emu_core::xml::pretty(&error.body)
                        };
                        self.push_activity(activity_from_error(
                            &service, &operation, request, started, &error,
                        ));
                    }
                }
            }
        }
    }

    fn on_tick(&mut self) {
        if self.last_health.elapsed() >= Duration::from_secs(5) {
            self.last_health = Instant::now();
            self.spawn_health();
        }
        if self
            .toast
            .as_ref()
            .is_some_and(|toast| Instant::now() >= toast.until)
        {
            self.toast = None;
        }
        let _ = self.supervisor.is_alive();
    }

    fn reload_connection(&mut self) {
        let config = config::discover();
        match EmulatorClient::new(&config) {
            Ok(client) => {
                self.client = client;
                self.config = config;
                self.service_status.clear();
                for service in &self.services {
                    self.service_status
                        .insert(service.key, ServiceStatus::Loading);
                }
            }
            Err(error) => self.toast(ToastKind::Error, format!("cannot rebuild client: {error}")),
        }
    }

    fn push_activity(&mut self, activity: Activity) {
        self.activities.push(activity);
        if self.activities.len() > 200 {
            self.activities.remove(0);
        }
        let index = self.activities.len().saturating_sub(1);
        self.activity_state.select(Some(index));
    }

    fn toast(&mut self, kind: ToastKind, text: impl Into<String>) {
        self.toast = Some(Toast {
            text: text.into(),
            kind,
            until: Instant::now() + Duration::from_secs(5),
        });
    }

    // --------------------------------------------------------------------- input

    pub async fn handle_key(&mut self, key: KeyEvent) {
        let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
        if ctrl && key.code == KeyCode::Char('c') {
            if self.supervisor.has_child() {
                self.modal = Some(Modal::Confirm {
                    title: "Quit".to_string(),
                    message: "This TUI started the emulator. Quit and stop it?".to_string(),
                    action: ConfirmAction::Quit,
                });
            } else {
                self.should_quit = true;
            }
            return;
        }

        if let Some(modal) = self.modal.take() {
            self.handle_modal(modal, key).await;
            return;
        }

        match key.code {
            KeyCode::F(1) => {
                self.modal = Some(Modal::Help { scroll: 0 });
                return;
            }
            KeyCode::Char('?') => {
                self.modal = Some(Modal::Help { scroll: 0 });
                return;
            }
            KeyCode::Char('q') => {
                if self.supervisor.has_child() {
                    self.modal = Some(Modal::Confirm {
                        title: "Quit".to_string(),
                        message: "This TUI started the emulator. Quit and stop it?".to_string(),
                        action: ConfirmAction::Quit,
                    });
                } else {
                    self.should_quit = true;
                }
                return;
            }
            KeyCode::Char('1') => {
                self.tab = Tab::Explorer;
                return;
            }
            KeyCode::Char('2') => {
                self.tab = Tab::Console;
                return;
            }
            KeyCode::Char('3') => {
                self.tab = Tab::Emulator;
                return;
            }
            KeyCode::Char('4') => {
                self.tab = Tab::Requests;
                return;
            }
            KeyCode::Tab => {
                self.tab = self.tab.next();
                return;
            }
            KeyCode::BackTab => {
                self.tab = self.tab.previous();
                return;
            }
            _ => {}
        }

        match self.tab {
            Tab::Explorer => self.explorer_key(key).await,
            Tab::Console => self.console_key(key).await,
            Tab::Emulator => self.emulator_key(key).await,
            Tab::Requests => self.requests_key(key),
        }
    }

    async fn explorer_key(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Char('a') | KeyCode::Char('A') => {
                self.tab = Tab::Console;
                self.open_service_picker();
                return;
            }
            KeyCode::Char('h') | KeyCode::Left => {
                self.explorer.focus = Focus::Sidebar;
                return;
            }
            _ => {}
        }

        match self.explorer.focus {
            Focus::Sidebar => match key.code {
                KeyCode::Char('j') | KeyCode::Down => {
                    if self.explorer.service_index + 1 < self.services.len() {
                        self.explorer.service_index += 1;
                        self.select_service();
                    }
                }
                KeyCode::Char('k') | KeyCode::Up => {
                    if self.explorer.service_index > 0 {
                        self.explorer.service_index -= 1;
                        self.select_service();
                    }
                }
                KeyCode::Char('l') | KeyCode::Right | KeyCode::Enter => {
                    self.explorer.focus = Focus::Table;
                }
                KeyCode::Char('g') | KeyCode::Home => {
                    self.explorer.service_index = 0;
                    self.select_service();
                }
                KeyCode::Char('G') | KeyCode::End => {
                    self.explorer.service_index = self.services.len().saturating_sub(1);
                    self.select_service();
                }
                _ => {}
            },
            Focus::Table => match key.code {
                KeyCode::Char('j') | KeyCode::Down => self.move_selection(1),
                KeyCode::Char('k') | KeyCode::Up => self.move_selection(-1),
                KeyCode::Char('g') | KeyCode::Home => {
                    self.explorer.table_state.select(Some(0));
                }
                KeyCode::Char('G') | KeyCode::End => {
                    let last = self.explorer.visible_rows().len().saturating_sub(1);
                    self.explorer.table_state.select(Some(last));
                }
                KeyCode::Enter => self.inspect_selected(),
                KeyCode::Esc => {
                    if self.explorer.show_detail {
                        self.explorer.show_detail = false;
                    } else if let Some(parent) = self.explorer.view.parent() {
                        self.open_view(parent);
                    } else {
                        self.explorer.focus = Focus::Sidebar;
                    }
                }
                KeyCode::Char('r') => self.load_table(),
                KeyCode::Char('c') => self.open_create(),
                KeyCode::Char('d') => self.confirm_delete(),
                KeyCode::Char('/') => {
                    self.modal = Some(Modal::Input {
                        title: "Filter".to_string(),
                        prompt: "Show rows containing".to_string(),
                        input: Input::new(self.explorer.filter.clone().unwrap_or_default()),
                        action: InputAction::Filter,
                    });
                }
                KeyCode::Char('x') => {
                    self.explorer.filter = None;
                    self.explorer.table_state = TableState::default();
                    if self
                        .explorer
                        .table
                        .as_ref()
                        .is_some_and(|table| !table.rows.is_empty())
                    {
                        self.explorer.table_state.select(Some(0));
                    }
                }
                KeyCode::Char('v') => self.cycle_subview(),
                KeyCode::PageDown => {
                    self.explorer.detail_scroll = self.explorer.detail_scroll.saturating_add(8);
                }
                KeyCode::PageUp => {
                    self.explorer.detail_scroll = self.explorer.detail_scroll.saturating_sub(8);
                }
                _ => {}
            },
        }
    }

    async fn console_key(&mut self, key: KeyEvent) {
        if key.modifiers.contains(KeyModifiers::CONTROL) && key.code == KeyCode::Char('s') {
            self.send_console_request();
            return;
        }
        if self.console.focus_editor {
            match key.code {
                KeyCode::Esc => {
                    self.console.focus_editor = false;
                    return;
                }
                KeyCode::Char('s') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                    self.send_console_request();
                    return;
                }
                KeyCode::F(5) => {
                    self.send_console_request();
                    return;
                }
                _ => {
                    self.console.textarea.input(key);
                    return;
                }
            }
        }

        match key.code {
            KeyCode::Char('s') => self.open_service_picker(),
            KeyCode::Char('o') => self.open_operation_picker(),
            KeyCode::Char('e') => self.console.focus_editor = true,
            KeyCode::Char('r') => {
                self.reset_console_body();
            }
            KeyCode::F(5) => self.send_console_request(),
            KeyCode::Char(']') | KeyCode::PageDown => {
                self.console.scroll = self.console.scroll.saturating_add(8)
            }
            KeyCode::Char('[') | KeyCode::PageUp => {
                self.console.scroll = self.console.scroll.saturating_sub(8)
            }
            _ => {}
        }
    }

    async fn emulator_key(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Char('s') => {
                if self.health.is_up() {
                    self.toast(
                        ToastKind::Info,
                        "emulator already running; this TUI is attached",
                    );
                    return;
                }
                match self.supervisor.start().await {
                    Ok(pid) => {
                        self.toast(ToastKind::Ok, format!("emulator started (pid {pid})"));
                        tokio::time::sleep(Duration::from_millis(1_200)).await;
                        self.reload_connection();
                        self.spawn_health();
                    }
                    Err(error) => self.toast(ToastKind::Error, error),
                }
            }
            KeyCode::Char('x') => {
                if !self.supervisor.has_child() {
                    self.toast(
                        ToastKind::Info,
                        "no child process: the emulator was started outside this TUI",
                    );
                    return;
                }
                match self.supervisor.stop().await {
                    Ok(()) => {
                        self.health = Health::Down("stopped by user".to_string());
                        self.toast(ToastKind::Ok, "emulator stopped");
                    }
                    Err(error) => self.toast(ToastKind::Error, error),
                }
            }
            KeyCode::Char('R') => {
                let _ = self.supervisor.stop().await;
                tokio::time::sleep(Duration::from_millis(300)).await;
                match self.supervisor.start().await {
                    Ok(pid) => {
                        self.toast(ToastKind::Ok, format!("emulator restarted (pid {pid})"));
                        tokio::time::sleep(Duration::from_millis(1_200)).await;
                        self.reload_connection();
                        self.spawn_health();
                    }
                    Err(error) => self.toast(ToastKind::Error, error),
                }
            }
            KeyCode::Char('r') => {
                self.reload_connection();
                self.spawn_health();
                self.toast(
                    ToastKind::Info,
                    "reloaded endpoint configuration and credentials",
                );
            }
            KeyCode::Char('g') => {
                self.emulator.logs_offset = 0;
            }
            KeyCode::Char('c') => {
                self.supervisor.clear_logs();
                self.emulator.logs_offset = 0;
            }
            KeyCode::Char('j') | KeyCode::Down => {
                self.emulator.logs_offset = self.emulator.logs_offset.saturating_add(1);
            }
            KeyCode::Char('k') | KeyCode::Up => {
                self.emulator.logs_offset = self.emulator.logs_offset.saturating_sub(1);
            }
            KeyCode::PageDown => {
                self.emulator.logs_offset = self.emulator.logs_offset.saturating_add(10);
            }
            KeyCode::PageUp => {
                self.emulator.logs_offset = self.emulator.logs_offset.saturating_sub(10);
            }
            _ => {}
        }
    }

    fn requests_key(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Char('j') | KeyCode::Down => {
                let next = self
                    .activity_state
                    .selected()
                    .unwrap_or(0)
                    .saturating_add(1)
                    .min(self.activities.len().saturating_sub(1));
                self.activity_state.select(Some(next));
            }
            KeyCode::Char('k') | KeyCode::Up => {
                let next = self
                    .activity_state
                    .selected()
                    .unwrap_or(0)
                    .saturating_sub(1);
                self.activity_state.select(Some(next));
            }
            KeyCode::Enter => {
                if let Some(activity) = self
                    .activity_state
                    .selected()
                    .and_then(|index| self.activities.get(index))
                {
                    let body = format!(
                        "{} {}\n{} · {} ms\n\n── request ─────────────────────────────\n{}\n\n── response ────────────────────────────\n{}",
                        activity.service,
                        activity.operation,
                        activity.time,
                        activity.duration_ms,
                        activity.request,
                        activity.response
                    );
                    self.modal = Some(Modal::Text {
                        title: format!("{} {}", activity.service, activity.operation),
                        body,
                        scroll: 0,
                    });
                }
            }
            KeyCode::Char('c') => {
                self.activities.clear();
                self.activity_state = TableState::default();
            }
            KeyCode::Esc => self.tab = Tab::Explorer,
            _ => {}
        }
    }

    async fn handle_modal(&mut self, modal: Modal, key: KeyEvent) {
        match modal {
            Modal::Help { mut scroll } => match key.code {
                KeyCode::Esc | KeyCode::Char('q') | KeyCode::Char('?') => {}
                KeyCode::Char('j') | KeyCode::Down => {
                    scroll = scroll.saturating_add(1);
                    self.modal = Some(Modal::Help { scroll });
                }
                KeyCode::Char('k') | KeyCode::Up => {
                    scroll = scroll.saturating_sub(1);
                    self.modal = Some(Modal::Help { scroll });
                }
                KeyCode::PageDown => {
                    scroll = scroll.saturating_add(10);
                    self.modal = Some(Modal::Help { scroll });
                }
                KeyCode::PageUp => {
                    scroll = scroll.saturating_sub(10);
                    self.modal = Some(Modal::Help { scroll });
                }
                _ => self.modal = Some(Modal::Help { scroll }),
            },
            Modal::Text {
                title,
                body,
                mut scroll,
            } => match key.code {
                KeyCode::Esc | KeyCode::Char('q') => {}
                KeyCode::Char('j') | KeyCode::Down => {
                    scroll = scroll.saturating_add(1);
                    self.modal = Some(Modal::Text {
                        title,
                        body,
                        scroll,
                    });
                }
                KeyCode::Char('k') | KeyCode::Up => {
                    scroll = scroll.saturating_sub(1);
                    self.modal = Some(Modal::Text {
                        title,
                        body,
                        scroll,
                    });
                }
                KeyCode::PageDown => {
                    scroll = scroll.saturating_add(10);
                    self.modal = Some(Modal::Text {
                        title,
                        body,
                        scroll,
                    });
                }
                KeyCode::PageUp => {
                    scroll = scroll.saturating_sub(10);
                    self.modal = Some(Modal::Text {
                        title,
                        body,
                        scroll,
                    });
                }
                _ => {
                    self.modal = Some(Modal::Text {
                        title,
                        body,
                        scroll,
                    })
                }
            },
            Modal::Input {
                title,
                prompt,
                mut input,
                action,
            } => match key.code {
                KeyCode::Esc => {}
                KeyCode::Enter => {
                    self.modal = None;
                    let value = input.value.trim().to_string();
                    match action {
                        InputAction::Filter => {
                            self.explorer.filter =
                                if value.is_empty() { None } else { Some(value) };
                            self.explorer.table_state = TableState::default();
                            if !self.explorer.visible_rows().is_empty() {
                                self.explorer.table_state.select(Some(0));
                            }
                        }
                        InputAction::CreateName(view) => {
                            self.submit_create(view, serde_json::json!({"name": value}));
                        }
                    }
                }
                _ => {
                    input.handle_key(key);
                    self.modal = Some(Modal::Input {
                        title,
                        prompt,
                        input,
                        action,
                    });
                }
            },
            Modal::Json {
                title,
                mut textarea,
                action,
                error,
            } => match key.code {
                KeyCode::Esc => {}
                KeyCode::Char('s') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                    let text = textarea.lines().join("\n");
                    match serde_json::from_str::<Value>(&text) {
                        Ok(input) => {
                            self.modal = None;
                            match action {
                                JsonAction::CreateJson(view) => self.submit_create(view, input),
                            }
                        }
                        Err(error) => {
                            self.modal = Some(Modal::Json {
                                title,
                                textarea,
                                action,
                                error: Some(format!("Invalid JSON: {error}")),
                            });
                        }
                    }
                }
                _ => {
                    textarea.input(key);
                    self.modal = Some(Modal::Json {
                        title,
                        textarea,
                        action,
                        error,
                    });
                }
            },
            Modal::Confirm {
                title,
                message,
                action,
            } => match key.code {
                KeyCode::Esc | KeyCode::Char('n') => {}
                KeyCode::Enter | KeyCode::Char('y') => {
                    self.modal = None;
                    match action {
                        ConfirmAction::Delete(view, id) => {
                            if let Some(write) = services::build_delete(&view, &id) {
                                self.spawn_write(write, format!("Delete {id}"));
                            } else {
                                self.toast(ToastKind::Error, "delete is not supported here");
                            }
                        }
                        ConfirmAction::Quit => self.should_quit = true,
                    }
                }
                _ => {
                    self.modal = Some(Modal::Confirm {
                        title,
                        message,
                        action,
                    })
                }
            },
            Modal::Picker {
                title,
                mut filter,
                all,
                mut state,
                action,
            } => match key.code {
                KeyCode::Esc => {}
                KeyCode::Up => {
                    let index = state.selected().unwrap_or(0).saturating_sub(1);
                    state.select(Some(index));
                    self.modal = Some(Modal::Picker {
                        title,
                        filter,
                        all,
                        state,
                        action,
                    });
                }
                KeyCode::Down => {
                    let len = filtered_picker(&all, &filter).len();
                    let index = state
                        .selected()
                        .unwrap_or(0)
                        .saturating_add(1)
                        .min(len.saturating_sub(1));
                    state.select(Some(index));
                    self.modal = Some(Modal::Picker {
                        title,
                        filter,
                        all,
                        state,
                        action,
                    });
                }
                KeyCode::Enter => {
                    let filtered = filtered_picker(&all, &filter);
                    let index = state.selected().unwrap_or(0);
                    if let Some((key, _)) = filtered.get(index) {
                        let key = (*key).clone();
                        self.modal = None;
                        match action {
                            PickerAction::ConsoleService => {
                                self.console.service = key.clone();
                                self.console.operation.clear();
                                self.console.response.clear();
                                self.console.error = None;
                                self.console.status = None;
                                self.open_operation_picker();
                            }
                            PickerAction::ConsoleOperation => {
                                self.console.operation = key.clone();
                                self.console.error = None;
                                self.console.status = None;
                                self.reset_console_body();
                            }
                        }
                    }
                }
                _ => {
                    filter.handle_key(key);
                    state.select(Some(0));
                    self.modal = Some(Modal::Picker {
                        title,
                        filter,
                        all,
                        state,
                        action,
                    });
                }
            },
        }
    }

    // ------------------------------------------------------------------- actions

    fn select_service(&mut self) {
        let service = self.services[self.explorer.service_index];
        if let Some(view) = services::base_view(service.key) {
            self.explorer.subview_index = 0;
            self.open_view(view);
        }
    }

    fn open_view(&mut self, view: ViewId) {
        self.explorer.view = view;
        self.explorer.show_detail = false;
        self.explorer.table = None;
        self.explorer.table_state = TableState::default();
        self.explorer.detail_body.clear();
        self.load_table();
    }

    fn cycle_subview(&mut self) {
        if self.explorer.view.service_key() != "iam" {
            return;
        }
        self.explorer.subview_index = (self.explorer.subview_index + 1) % 2;
        let view = if self.explorer.subview_index == 0 {
            ViewId::IamUsers
        } else {
            ViewId::IamRoles
        };
        self.open_view(view);
    }

    fn move_selection(&mut self, delta: i32) {
        let len = self.explorer.visible_rows().len();
        if len == 0 {
            return;
        }
        let current = self.explorer.table_state.selected().unwrap_or(0) as i32;
        let next = (current + delta).clamp(0, len as i32 - 1);
        self.explorer.table_state.select(Some(next as usize));
    }

    fn inspect_selected(&mut self) {
        let Some(row) = self.explorer.selected_row() else {
            return;
        };
        let id = row.id.clone();
        let detail = row.detail.clone();
        let view = self.explorer.view.clone();
        match view {
            ViewId::S3Buckets => {
                self.open_view(ViewId::S3Objects(id));
            }
            _ => {
                if services::describe_call(&view, &id).is_some() {
                    self.spawn_detail(view, id);
                } else {
                    self.explorer.show_detail = true;
                    self.explorer.detail_title = id;
                    self.explorer.detail_scroll = 0;
                    self.explorer.detail_body = detail
                        .map(|value| pretty_json(&value))
                        .unwrap_or_else(|| "No detail available.".to_string());
                }
            }
        }
    }

    fn open_create(&mut self) {
        let view = self.explorer.view.clone();
        match services::create_kind(&view) {
            Some(CreateKind::Name { title, placeholder }) => {
                self.modal = Some(Modal::Input {
                    title: title.to_string(),
                    prompt: placeholder.to_string(),
                    input: Input::default(),
                    action: InputAction::CreateName(view),
                });
            }
            Some(CreateKind::Json { title, template }) => {
                let text = pretty_json(&template);
                let textarea = TextArea::new(text.lines().map(str::to_string).collect());
                self.modal = Some(Modal::Json {
                    title: title.to_string(),
                    textarea: Box::new(textarea),
                    action: JsonAction::CreateJson(view),
                    error: None,
                });
            }
            None => self.toast(
                ToastKind::Info,
                format!("{} does not support create", view.title()),
            ),
        }
    }

    fn confirm_delete(&mut self) {
        let Some(row) = self.explorer.selected_row() else {
            return;
        };
        let view = self.explorer.view.clone();
        let id = row.id.clone();
        if services::build_delete(&view, &id).is_none() {
            self.toast(ToastKind::Info, "delete is not supported here");
            return;
        }
        self.modal = Some(Modal::Confirm {
            title: format!("Delete {}", view.title()),
            message: format!("Delete “{id}”? This cannot be undone."),
            action: ConfirmAction::Delete(view, id),
        });
    }

    fn submit_create(&mut self, view: ViewId, input: Value) {
        match services::build_create(&view, input) {
            Ok(action) => {
                let label = format!("Create {}", view.title());
                self.spawn_write(action, label);
            }
            Err(error) => self.toast(ToastKind::Error, error),
        }
    }

    fn reset_console_body(&mut self) {
        let body = catalog::catalog()
            .get(&self.console.service)
            .and_then(|service| service.operation(&self.console.operation))
            .map(|op| pretty_json(&op.skel))
            .unwrap_or_else(|| "{}".to_string());
        self.console.textarea = TextArea::new(body.lines().map(str::to_string).collect());
        self.console.textarea.set_block(
            ratatui::widgets::Block::bordered()
                .title(" request ")
                .border_style(theme::muted()),
        );
        self.console.scroll = 0;
    }

    fn open_service_picker(&mut self) {
        let catalog = catalog::catalog();
        let all: Vec<(String, String)> = catalog
            .ordered_keys()
            .into_iter()
            .map(|key| {
                let service = &catalog.services[key];
                (
                    key.to_string(),
                    format!("{key}  ·  {}  ·  {}", service.display, service.protocol()),
                )
            })
            .collect();
        let mut state = ListState::default();
        state.select(Some(0));
        self.modal = Some(Modal::Picker {
            title: format!("Services ({})", all.len()),
            filter: Input::default(),
            all,
            state,
            action: PickerAction::ConsoleService,
        });
    }

    fn open_operation_picker(&mut self) {
        let Some(service) = catalog::catalog().get(&self.console.service) else {
            self.toast(ToastKind::Error, "unknown service");
            return;
        };
        let all: Vec<(String, String)> = service
            .ops
            .keys()
            .map(|name| (name.clone(), name.clone()))
            .collect();
        let mut state = ListState::default();
        state.select(Some(0));
        self.modal = Some(Modal::Picker {
            title: format!("{} operations ({})", self.console.service, all.len()),
            filter: Input::default(),
            all,
            state,
            action: PickerAction::ConsoleOperation,
        });
    }
}

// ---------------------------------------------------------------------- support

pub enum Msg {
    Health(Result<(), String>),
    Identity(Result<String, ApiError>),
    Probed {
        key: &'static str,
        result: Result<(), ApiError>,
    },
    Table {
        generation: u64,
        view: ViewId,
        started: Instant,
        result: Result<Table, ApiError>,
    },
    Detail {
        view: ViewId,
        id: String,
        service: String,
        operation: String,
        request: String,
        started: Instant,
        result: Result<ApiResponse, ApiError>,
    },
    Write {
        label: String,
        view: ViewId,
        service: String,
        operation: String,
        request: String,
        started: Instant,
        result: Result<ApiResponse, ApiError>,
    },
    Console {
        service: String,
        operation: String,
        request: String,
        started: Instant,
        result: Result<ApiResponse, ApiError>,
    },
}

pub enum Modal {
    Help {
        scroll: u16,
    },
    Text {
        title: String,
        body: String,
        scroll: u16,
    },
    Input {
        title: String,
        prompt: String,
        input: Input,
        action: InputAction,
    },
    Json {
        title: String,
        textarea: Box<TextArea<'static>>,
        action: JsonAction,
        error: Option<String>,
    },
    Confirm {
        title: String,
        message: String,
        action: ConfirmAction,
    },
    Picker {
        title: String,
        filter: Input,
        all: Vec<(String, String)>,
        state: ListState,
        action: PickerAction,
    },
}

pub enum InputAction {
    Filter,
    CreateName(ViewId),
}

pub enum JsonAction {
    CreateJson(ViewId),
}

pub enum ConfirmAction {
    Delete(ViewId, String),
    Quit,
}

#[derive(Clone, Copy)]
pub enum PickerAction {
    ConsoleService,
    ConsoleOperation,
}

#[derive(Clone, Debug, Default)]
pub struct Input {
    pub value: String,
    pub cursor: usize,
}

impl Input {
    pub fn new(value: String) -> Self {
        let cursor = value.len();
        Self { value, cursor }
    }

    pub fn handle_key(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Char(character) => {
                if !key.modifiers.contains(KeyModifiers::CONTROL) {
                    self.value.insert(self.cursor, character);
                    self.cursor += character.len_utf8();
                }
            }
            KeyCode::Backspace => {
                if self.cursor > 0 {
                    let previous = self.value[..self.cursor]
                        .char_indices()
                        .next_back()
                        .map(|(index, _)| index)
                        .unwrap_or(0);
                    self.value.replace_range(previous..self.cursor, "");
                    self.cursor = previous;
                }
            }
            KeyCode::Delete => {
                if self.cursor < self.value.len() {
                    let next = self.value[self.cursor..]
                        .char_indices()
                        .nth(1)
                        .map(|(index, _)| self.cursor + index)
                        .unwrap_or(self.value.len());
                    self.value.replace_range(self.cursor..next, "");
                }
            }
            KeyCode::Left => {
                if self.cursor > 0 {
                    self.cursor = self.value[..self.cursor]
                        .char_indices()
                        .next_back()
                        .map(|(index, _)| index)
                        .unwrap_or(0);
                }
            }
            KeyCode::Right => {
                if self.cursor < self.value.len() {
                    self.cursor = self.value[self.cursor..]
                        .char_indices()
                        .nth(1)
                        .map(|(index, _)| self.cursor + index)
                        .unwrap_or(self.value.len());
                }
            }
            KeyCode::Home => self.cursor = 0,
            KeyCode::End => self.cursor = self.value.len(),
            _ => {}
        }
    }
}

pub fn filtered_picker<'a>(
    all: &'a [(String, String)],
    filter: &Input,
) -> Vec<&'a (String, String)> {
    if filter.value.trim().is_empty() {
        return all.iter().collect();
    }
    let needle = filter.value.to_ascii_lowercase();
    all.iter()
        .filter(|(key, label)| {
            key.to_ascii_lowercase().contains(&needle)
                || label.to_ascii_lowercase().contains(&needle)
        })
        .collect()
}

pub fn pretty_json(value: &Value) -> String {
    serde_json::to_string_pretty(value).unwrap_or_else(|_| "{}".to_string())
}

fn clock() -> String {
    let stamp = emu_core::sigv4::amz_date(SystemTime::now());
    format!("{}:{}:{}", &stamp[9..11], &stamp[11..13], &stamp[13..15])
}

fn first_line(text: &str) -> &str {
    text.lines().next().unwrap_or(text)
}

fn activity_from_response(
    service: &str,
    operation: &str,
    request: String,
    started: Instant,
    response: &ApiResponse,
) -> Activity {
    Activity {
        time: clock(),
        service: service.to_string(),
        operation: operation.to_string(),
        status: response.status.to_string(),
        duration_ms: response.duration_ms,
        request,
        response: truncate(&response.pretty(), 40_000),
        ok: true,
    }
    .with_fallback_duration(started)
}

fn activity_from_error(
    service: &str,
    operation: &str,
    request: String,
    started: Instant,
    error: &ApiError,
) -> Activity {
    Activity {
        time: clock(),
        service: service.to_string(),
        operation: operation.to_string(),
        status: error
            .status
            .map(|status| status.to_string())
            .unwrap_or_else(|| "ERR".to_string()),
        duration_ms: error.duration_ms,
        request,
        response: truncate(
            &if error.body.trim().is_empty() {
                error.message.clone()
            } else {
                error.body.clone()
            },
            40_000,
        ),
        ok: false,
    }
    .with_fallback_duration(started)
}

impl Activity {
    fn with_fallback_duration(mut self, started: Instant) -> Self {
        if self.duration_ms == 0 {
            self.duration_ms = started.elapsed().as_millis();
        }
        self
    }
}

fn truncate(text: &str, limit: usize) -> String {
    if text.len() <= limit {
        return text.to_string();
    }
    let mut end = limit;
    while end > 0 && !text.is_char_boundary(end) {
        end -= 1;
    }
    format!("{}…", &text[..end])
}

impl Default for ExplorerState {
    fn default() -> Self {
        Self {
            focus: Focus::Table,
            service_index: 0,
            view: ViewId::S3Buckets,
            table: None,
            table_state: TableState::default(),
            loading: false,
            error: None,
            filter: None,
            show_detail: false,
            detail_title: String::new(),
            detail_body: String::new(),
            detail_scroll: 0,
            generation: 0,
            subview_index: 0,
        }
    }
}

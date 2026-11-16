//! Application state, background work and key handling.

use std::collections::BTreeMap;
use std::io::Write;
use std::time::{Duration, Instant, SystemTime};

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use emu_core::catalog;
use emu_core::client::{ApiError, ApiResponse};
use emu_core::config::{self, ConnectionConfig};
use emu_core::generic::{self, GenericTable, Token};
use emu_core::services::{self, CreateKind, Row, Table, ViewId, WriteAction};
use emu_core::sigv4;
use emu_core::{EmulatorClient, Supervisor};
use ratatui::widgets::{ListState, TableState};
use serde_json::{json, Value};
use tokio::sync::mpsc::{unbounded_channel, UnboundedReceiver, UnboundedSender};
use tui_textarea::TextArea;

use crate::theme;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Tab {
    Explorer,
    Browse,
    Console,
    Emulator,
    Requests,
}

impl Tab {
    pub const ALL: [Tab; 5] = [
        Tab::Explorer,
        Tab::Browse,
        Tab::Console,
        Tab::Emulator,
        Tab::Requests,
    ];

    pub fn title(self) -> &'static str {
        match self {
            Tab::Explorer => "Explorer",
            Tab::Browse => "Browse",
            Tab::Console => "Console",
            Tab::Emulator => "Emulator",
            Tab::Requests => "Requests",
        }
    }

    fn next(self) -> Tab {
        match self {
            Tab::Explorer => Tab::Browse,
            Tab::Browse => Tab::Console,
            Tab::Console => Tab::Emulator,
            Tab::Emulator => Tab::Requests,
            Tab::Requests => Tab::Explorer,
        }
    }

    fn previous(self) -> Tab {
        match self {
            Tab::Explorer => Tab::Requests,
            Tab::Browse => Tab::Explorer,
            Tab::Console => Tab::Browse,
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

pub struct BrowseState {
    pub service: String,
    pub operation: String,
    pub input: Value,
    pub table: Option<GenericTable>,
    pub table_state: TableState,
    pub filter: Option<String>,
    pub loading: bool,
    pub error: Option<String>,
    pub status: Option<(u16, u128)>,
    pub token: Option<Token>,
    pub generation: u64,
}

impl BrowseState {
    pub fn visible_rows(&self) -> Vec<&emu_core::generic::GenericRow> {
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
                        row.cells
                            .iter()
                            .any(|cell| cell.to_ascii_lowercase().contains(&needle))
                    })
                    .collect()
            }
        }
    }

    pub fn selected_row(&self) -> Option<&emu_core::generic::GenericRow> {
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
    pub browse: BrowseState,
    pub console: ConsoleState,
    pub emulator: EmulatorState,
    pub services: Vec<&'static services::CuratedService>,
    pub service_status: BTreeMap<&'static str, ServiceStatus>,
    pub activities: Vec<Activity>,
    pub activity_state: TableState,
    pub modal: Option<Modal>,
    pub toast: Option<Toast>,
    pub read_only: bool,
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
            browse: BrowseState {
                service: "dynamodb".to_string(),
                operation: "ListTables".to_string(),
                input: Value::Object(Default::default()),
                table: None,
                table_state: TableState::default(),
                filter: None,
                loading: false,
                error: None,
                status: None,
                token: None,
                generation: 0,
            },
            console,
            emulator: EmulatorState { logs_offset: 0 },
            services: services::CURATED.iter().collect(),
            service_status: BTreeMap::new(),
            activities: Vec::new(),
            activity_state: TableState::default(),
            modal: None,
            toast: None,
            read_only: false,
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

    fn spawn_doctor(&mut self) {
        let client = self.client.clone();
        let config = self.config.clone();
        let statuses = self.service_status.clone();
        let owned_pid = self.supervisor.pid();
        let tx = self.tx.clone();
        tokio::spawn(async move {
            let mut lines = vec!["AWS Emulator TUI — diagnostics".to_string(), String::new()];
            let catalog = catalog::catalog();
            let operations: usize = catalog
                .services
                .values()
                .map(|service| service.ops.len())
                .sum();
            lines.push(format!(
                "catalog        {} services · {} operations ({})",
                catalog.len(),
                operations,
                catalog.source
            ));
            lines.push(format!("endpoint       {}", config.endpoint));
            lines.push(format!("region         {}", config.region));

            match client.health().await {
                Ok(()) => lines.push("health         ok".to_string()),
                Err(error) => lines.push(format!("health         FAILED — {error}")),
            }
            match services::caller_identity(&client).await {
                Ok(identity) => lines.push(format!("identity       {identity}")),
                Err(error) => lines.push(format!(
                    "identity       FAILED — {} {}",
                    error.status_label(),
                    error.message
                )),
            }

            lines.push(format!(
                "credentials    {} ({})",
                if config.paths.credentials.is_file() {
                    "present"
                } else {
                    "missing"
                },
                config.credentials_source.label()
            ));
            lines.push(format!(
                "config         {} ({})",
                config.paths.config.display(),
                exists(&config.paths.config)
            ));
            lines.push(format!(
                "certificate    {} ({})",
                config.paths.ca.display(),
                exists(&config.paths.ca)
            ));
            lines.push(format!(
                "state dir      {} ({})",
                config.paths.directory.display(),
                writable(&config.paths.directory)
            ));
            lines.push(format!(
                "process        {}",
                owned_pid
                    .map(|pid| format!("pid {pid}, started by this TUI"))
                    .unwrap_or_else(|| "external or not attached".to_string())
            ));
            lines.push(format!(
                "binary         {}",
                config
                    .binary
                    .as_ref()
                    .map(|path| path.display().to_string())
                    .unwrap_or_else(|| "not found".to_string())
            ));
            if let Some(binary) = &config.binary {
                match tokio::process::Command::new(binary)
                    .arg("-v")
                    .output()
                    .await
                {
                    Ok(output) => {
                        let version = String::from_utf8_lossy(&output.stdout)
                            .lines()
                            .next()
                            .unwrap_or("")
                            .to_string();
                        let version = if version.is_empty() {
                            "no version output".to_string()
                        } else {
                            version
                        };
                        lines.push(format!("version        {version}"));
                    }
                    Err(error) => lines.push(format!("version        FAILED — {error}")),
                }
            }

            let mut ok = 0;
            let mut loading = 0;
            let mut failed = Vec::new();
            for (key, status) in &statuses {
                match status {
                    ServiceStatus::Ok => ok += 1,
                    ServiceStatus::Loading => loading += 1,
                    ServiceStatus::Err => failed.push(*key),
                    ServiceStatus::Unknown => loading += 1,
                }
            }
            lines.push(format!(
                "probes         {ok} ok, {loading} pending, {} failed",
                failed.len()
            ));
            if !failed.is_empty() {
                lines.push(format!("failed         {}", failed.join(", ")));
            }
            if !config.warnings.is_empty() {
                lines.push(String::new());
                lines.push("warnings".to_string());
                for warning in &config.warnings {
                    lines.push(format!("  · {warning}"));
                }
            }
            lines.push(String::new());
            lines.push(
                "Suggested fixes: start the emulator with `s` on the Emulator tab, or press `r` \
                 there to reload the generated credentials and certificate."
                    .to_string(),
            );

            let _ = tx.send(Msg::Doctor(lines.join("\n")));
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
        if self.read_only && is_write_operation(&self.console.operation) {
            self.toast(ToastKind::Info, "read-only mode blocks write operations");
            return;
        }
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
            Msg::Doctor(report) => {
                self.modal = Some(Modal::Text {
                    title: "Diagnostics".to_string(),
                    body: report,
                    scroll: 0,
                });
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
            Msg::Special {
                service,
                operation,
                label,
                request,
                started,
                result,
            } => match result {
                Ok(response) => {
                    self.push_activity(activity_from_response(
                        &service, &operation, request, started, &response,
                    ));
                    let body = format!(
                        "{} · HTTP {} · {} ms\n\n{}",
                        label,
                        response.status,
                        response.duration_ms,
                        response.pretty()
                    );
                    self.modal = Some(Modal::Text {
                        title: format!("{service}.{operation}"),
                        body,
                        scroll: 0,
                    });
                }
                Err(error) => {
                    self.push_activity(activity_from_error(
                        &service, &operation, request, started, &error,
                    ));
                    self.toast(
                        ToastKind::Error,
                        format!("{label} failed: {}", first_line(&error.message)),
                    );
                }
            },
            Msg::S3Preview {
                bucket,
                key,
                started,
                result,
            } => match result {
                Ok(response) => {
                    let content_type = response
                        .headers
                        .iter()
                        .find(|(name, _)| name.eq_ignore_ascii_case("content-type"))
                        .map(|(_, value)| value.clone())
                        .unwrap_or_default();
                    let printable = !response.body.is_empty()
                        && response.body.chars().all(|character| {
                            character == '\n'
                                || character == '\r'
                                || character == '\t'
                                || !character.is_control()
                        });
                    let text = content_type.contains("text")
                        || content_type.contains("json")
                        || content_type.contains("xml")
                        || content_type.is_empty()
                        || printable;
                    let body = if text {
                        truncate(&response.body, 100_000)
                    } else {
                        format!(
                            "Binary object ({} bytes, {content_type}).\n\nDownload it with the AWS CLI:\n\n  aws --endpoint-url {} s3api get-object --bucket {} --key {} <file>",
                            response.body.len(),
                            self.client.endpoint(),
                            bucket,
                            key
                        )
                    };
                    let title = format!("{bucket}/{key}");
                    self.push_activity(activity_from_response(
                        "s3",
                        "GetObject",
                        format!("s3.GetObject\n{bucket}/{key}"),
                        started,
                        &response,
                    ));
                    self.modal = Some(Modal::Text {
                        title,
                        body,
                        scroll: 0,
                    });
                }
                Err(error) => {
                    self.push_activity(activity_from_error(
                        "s3",
                        "GetObject",
                        format!("s3.GetObject\n{bucket}/{key}"),
                        started,
                        &error,
                    ));
                    self.toast(
                        ToastKind::Error,
                        format!("preview failed: {}", first_line(&error.message)),
                    );
                }
            },
            Msg::Browse {
                generation,
                append,
                service,
                operation,
                request,
                started,
                result,
            } => {
                if generation != self.browse.generation {
                    return;
                }
                self.browse.loading = false;
                match result {
                    Ok((table, response)) => {
                        self.browse.status = Some((response.status, response.duration_ms));
                        self.browse.error = None;
                        self.browse.token = table.token.clone();
                        if append {
                            if let Some(existing) = self.browse.table.as_mut() {
                                for row in table.rows {
                                    if !existing.rows.iter().any(|known| known.detail == row.detail)
                                    {
                                        existing.rows.push(row);
                                    }
                                }
                                existing.token = table.token;
                            } else {
                                self.browse.table = Some(table);
                            }
                        } else {
                            let count = table.rows.len();
                            self.browse.table = Some(table);
                            self.browse.table_state = TableState::default();
                            if count > 0 {
                                self.browse.table_state.select(Some(0));
                            }
                        }
                        self.push_activity(activity_from_response(
                            &service,
                            &operation,
                            request.clone(),
                            started,
                            &response,
                        ));
                    }
                    Err(error) => {
                        if !append {
                            self.browse.table = None;
                        }
                        self.browse.error = Some(error.message.clone());
                        self.push_activity(activity_from_error(
                            &service, &operation, request, started, &error,
                        ));
                    }
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

        if key.modifiers.contains(KeyModifiers::CONTROL) && key.code == KeyCode::Char('p') {
            self.open_palette();
            return;
        }
        if key.modifiers.contains(KeyModifiers::CONTROL) && key.code == KeyCode::Char('e') {
            self.export_current();
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
            KeyCode::Char('D') => {
                self.spawn_doctor();
                return;
            }
            KeyCode::Char(':') => {
                self.open_palette();
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
                self.tab = Tab::Browse;
                self.ensure_browse_loaded();
                return;
            }
            KeyCode::Char('3') => {
                self.tab = Tab::Console;
                return;
            }
            KeyCode::Char('4') => {
                self.tab = Tab::Emulator;
                return;
            }
            KeyCode::Char('5') => {
                self.tab = Tab::Requests;
                return;
            }
            KeyCode::Tab => {
                self.tab = self.tab.next();
                if self.tab == Tab::Browse {
                    self.ensure_browse_loaded();
                }
                return;
            }
            KeyCode::BackTab => {
                self.tab = self.tab.previous();
                if self.tab == Tab::Browse {
                    self.ensure_browse_loaded();
                }
                return;
            }
            _ => {}
        }

        match self.tab {
            Tab::Explorer => self.explorer_key(key).await,
            Tab::Browse => self.browse_key(key).await,
            Tab::Console => self.console_key(key).await,
            Tab::Emulator => self.emulator_key(key).await,
            Tab::Requests => self.requests_key(key),
        }
    }

    async fn explorer_key(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Char('a') | KeyCode::Char('A') => {
                self.tab = Tab::Browse;
                self.open_service_picker(PickerAction::BrowseService);
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
                KeyCode::Char('o') => self.open_children(),
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
                KeyCode::Char('x') => self.open_special(),
                KeyCode::Char('d') => self.confirm_delete(),
                KeyCode::Char('/') => {
                    self.modal = Some(Modal::Input {
                        title: "Filter".to_string(),
                        prompt: "Show rows containing".to_string(),
                        input: Input::new(self.explorer.filter.clone().unwrap_or_default()),
                        action: InputAction::Filter,
                    });
                }
                KeyCode::Char('X') => {
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
                KeyCode::Char('y') => self.copy_selected(),
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
            KeyCode::Char('s') => self.open_service_picker(PickerAction::ConsoleService),
            KeyCode::Char('o') => self.open_operation_picker(
                PickerAction::ConsoleOperation,
                &self.console.service.clone(),
            ),
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

    async fn start_emulator(&mut self) {
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

    async fn stop_emulator(&mut self) {
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

    async fn restart_emulator(&mut self) {
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

    async fn emulator_key(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Char('s') => self.start_emulator().await,
            KeyCode::Char('x') => self.stop_emulator().await,
            KeyCode::Char('R') => self.restart_emulator().await,
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
                        InputAction::BrowseFilter => {
                            self.browse.filter = if value.is_empty() { None } else { Some(value) };
                            self.browse.table_state = TableState::default();
                            if !self.browse.visible_rows().is_empty() {
                                self.browse.table_state.select(Some(0));
                            }
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
                                JsonAction::BrowseRun => {
                                    self.browse.input = input;
                                    self.load_browse(false);
                                }
                                JsonAction::RunSpecial {
                                    service,
                                    operation,
                                    label,
                                } => {
                                    if self.read_only && is_write_operation(&operation) {
                                        self.toast(
                                            ToastKind::Info,
                                            "read-only mode blocks write operations",
                                        );
                                    } else {
                                        self.spawn_special(service, operation, label, input);
                                    }
                                }
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
                                self.open_operation_picker(PickerAction::ConsoleOperation, &key);
                            }
                            PickerAction::ConsoleOperation => {
                                self.console.operation = key.clone();
                                self.console.error = None;
                                self.console.status = None;
                                self.reset_console_body();
                            }
                            PickerAction::BrowseService => {
                                self.browse.service = key.clone();
                                self.browse.operation.clear();
                                self.browse.table = None;
                                self.browse.token = None;
                                self.browse.error = None;
                                self.browse.status = None;
                                self.open_operation_picker(PickerAction::BrowseOperation, &key);
                            }
                            PickerAction::Palette => {
                                self.run_palette(&key).await;
                            }
                            PickerAction::SwitchEndpoint => {
                                if self.supervisor.has_child() {
                                    self.toast(
                                        ToastKind::Info,
                                        "stop the TUI-started emulator before switching endpoints",
                                    );
                                    return;
                                }
                                std::env::set_var("EMULATOR_ENDPOINT", &key);
                                self.reload_connection();
                                self.supervisor =
                                    Supervisor::new(self.config.binary.clone(), self.config.port());
                                self.explorer.table = None;
                                self.explorer.error = None;
                                self.explorer.generation += 1;
                                self.browse.table = None;
                                self.browse.token = None;
                                self.browse.error = None;
                                self.browse.generation += 1;
                                self.spawn_health();
                                self.spawn_identity();
                                self.spawn_probes();
                                self.load_table();
                                if self.tab == Tab::Browse {
                                    self.ensure_browse_loaded();
                                }
                                self.toast(ToastKind::Ok, format!("switched to {key}"));
                            }
                            PickerAction::BrowseOperation => {
                                self.browse.operation = key.clone();
                                self.browse.table = None;
                                self.browse.token = None;
                                self.browse.error = None;
                                self.browse.status = None;
                                let input = catalog::catalog()
                                    .get(&self.browse.service)
                                    .and_then(|service| service.operation(&key))
                                    .map(|op| op.skel.clone())
                                    .unwrap_or_else(|| Value::Object(Default::default()));
                                self.open_browse_editor(input);
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
            ViewId::S3Objects(bucket) => {
                self.spawn_s3_preview(bucket, id);
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

    fn open_children(&mut self) {
        let Some(row) = self.explorer.selected_row() else {
            return;
        };
        let view = self.explorer.view.clone();
        let id = row.id.clone();
        if let ViewId::S3Buckets = view {
            self.open_view(ViewId::S3Objects(id));
            return;
        }
        if let Some((service, operation, input)) = services::drilldown(&view, &id) {
            self.open_browse(service, operation, input);
        } else {
            self.toast(
                ToastKind::Info,
                format!("{} has no child collection for this row", view.title()),
            );
        }
    }

    fn open_special(&mut self) {
        if self.read_only {
            self.toast(ToastKind::Info, "read-only mode is on");
            return;
        }
        let Some(row) = self.explorer.selected_row() else {
            return;
        };
        let view = self.explorer.view.clone();
        let id = row.id.clone();
        let Some(action) = services::special_action(&view, &id) else {
            self.toast(
                ToastKind::Info,
                format!("{} has no extra action for this row", view.title()),
            );
            return;
        };
        let text = pretty_json(&action.template);
        let textarea = TextArea::new(text.lines().map(str::to_string).collect());
        self.modal = Some(Modal::Json {
            title: action.title.to_string(),
            textarea: Box::new(textarea),
            action: JsonAction::RunSpecial {
                service: action.service.to_string(),
                operation: action.operation.to_string(),
                label: action.title.to_string(),
            },
            error: None,
        });
    }

    fn spawn_special(&mut self, service: String, operation: String, label: String, input: Value) {
        let client = self.client.clone();
        let tx = self.tx.clone();
        let started = Instant::now();
        let request = format!("{service}.{operation}\n{}", pretty_json(&input));
        tokio::spawn(async move {
            let result = client.call(&service, &operation, input).await;
            let _ = tx.send(Msg::Special {
                service,
                operation,
                label,
                request,
                started,
                result,
            });
        });
    }

    fn spawn_s3_preview(&mut self, bucket: String, key: String) {
        let client = self.client.clone();
        let tx = self.tx.clone();
        let started = Instant::now();
        let path = format!("/{}/{}", sigv4::encode(&bucket), sigv4::encode_path(&key));
        tokio::spawn(async move {
            let result = client.s3("GET", &path, "", Vec::new()).await;
            let _ = tx.send(Msg::S3Preview {
                bucket,
                key,
                started,
                result,
            });
        });
    }

    fn open_create(&mut self) {
        if self.read_only {
            self.toast(ToastKind::Info, "read-only mode is on");
            return;
        }
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
        if self.read_only {
            self.toast(ToastKind::Info, "read-only mode is on");
            return;
        }
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

    fn open_service_picker(&mut self, action: PickerAction) {
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
            action,
        });
    }

    fn open_operation_picker(&mut self, action: PickerAction, service_key: &str) {
        let Some(service) = catalog::catalog().get(service_key) else {
            self.toast(ToastKind::Error, "unknown service");
            return;
        };
        let mut names: Vec<String> = service.ops.keys().cloned().collect();
        if matches!(action, PickerAction::BrowseOperation) {
            names.sort_by_key(|name| (operation_rank(name), name.clone()));
        }
        let all: Vec<(String, String)> = names
            .into_iter()
            .map(|name| {
                let marker = if operation_rank(&name) == 0 {
                    "▸"
                } else {
                    " "
                };
                let label = format!("{marker} {name}");
                (name, label)
            })
            .collect();
        let mut state = ListState::default();
        state.select(Some(0));
        let title = if matches!(action, PickerAction::BrowseOperation) {
            format!("{service_key} operations ({}) — list-like first", all.len())
        } else {
            format!("{service_key} operations ({})", all.len())
        };
        self.modal = Some(Modal::Picker {
            title,
            filter: Input::default(),
            all,
            state,
            action,
        });
    }

    fn ensure_browse_loaded(&mut self) {
        if self.browse.table.is_none()
            && self.browse.error.is_none()
            && !self.browse.loading
            && self.health.is_up()
            && !self.browse.operation.is_empty()
        {
            self.load_browse(false);
        }
    }

    fn open_browse(&mut self, service: &str, operation: &str, input: Value) {
        self.tab = Tab::Browse;
        self.browse.service = service.to_string();
        self.browse.operation = operation.to_string();
        self.browse.input = input;
        self.browse.table = None;
        self.browse.token = None;
        self.browse.error = None;
        self.browse.status = None;
        self.browse.filter = None;
        self.browse.generation += 1;
        self.load_browse(false);
    }

    fn load_browse(&mut self, append: bool) {
        if self.read_only && !append && is_write_operation(&self.browse.operation) {
            self.toast(ToastKind::Info, "read-only mode blocks write operations");
            return;
        }
        if self.browse.operation.is_empty() {
            self.toast(ToastKind::Info, "Pick an operation first (o)");
            return;
        }
        if append && self.browse.token.is_none() {
            self.toast(ToastKind::Info, "No more pages");
            return;
        }
        let input = if append {
            self.browse
                .token
                .as_ref()
                .map(|token| token.next_input(&self.browse.input))
                .unwrap_or_else(|| self.browse.input.clone())
        } else {
            self.browse.input.clone()
        };
        self.browse.generation += 1;
        let generation = self.browse.generation;
        self.browse.loading = true;
        self.browse.error = None;
        let client = self.client.clone();
        let tx = self.tx.clone();
        let service = self.browse.service.clone();
        let operation = self.browse.operation.clone();
        let request = format!("{service}.{operation}\n{}", pretty_json(&input));
        let started = Instant::now();
        tokio::spawn(async move {
            let result = generic::fetch(&client, &service, &operation, input).await;
            let _ = tx.send(Msg::Browse {
                generation,
                append,
                service,
                operation,
                request,
                started,
                result,
            });
        });
    }

    fn open_browse_editor(&mut self, input: Value) {
        let text = pretty_json(&input);
        let textarea = TextArea::new(text.lines().map(str::to_string).collect());
        self.modal = Some(Modal::Json {
            title: format!("Input — {}.{}", self.browse.service, self.browse.operation),
            textarea: Box::new(textarea),
            action: JsonAction::BrowseRun,
            error: None,
        });
    }

    async fn browse_key(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Char('s') => self.open_service_picker(PickerAction::BrowseService),
            KeyCode::Char('o') => self
                .open_operation_picker(PickerAction::BrowseOperation, &self.browse.service.clone()),
            KeyCode::Char('e') => {
                let input = self.browse.input.clone();
                self.open_browse_editor(input);
            }
            KeyCode::Char('r') => self.load_browse(false),
            KeyCode::Char('m') => self.load_browse(true),
            KeyCode::Enter => self.show_browse_detail(),
            KeyCode::Char('/') => {
                self.modal = Some(Modal::Input {
                    title: "Filter".to_string(),
                    prompt: "Show rows containing".to_string(),
                    input: Input::new(self.browse.filter.clone().unwrap_or_default()),
                    action: InputAction::BrowseFilter,
                });
            }
            KeyCode::Char('x') => {
                self.browse.filter = None;
                self.browse.table_state = TableState::default();
                if !self.browse.visible_rows().is_empty() {
                    self.browse.table_state.select(Some(0));
                }
            }
            KeyCode::Char('j') | KeyCode::Down => {
                let len = self.browse.visible_rows().len();
                if len > 0 {
                    let current = self.browse.table_state.selected().unwrap_or(0);
                    self.browse
                        .table_state
                        .select(Some((current + 1).min(len - 1)));
                }
            }
            KeyCode::Char('k') | KeyCode::Up => {
                let current = self.browse.table_state.selected().unwrap_or(0);
                self.browse
                    .table_state
                    .select(Some(current.saturating_sub(1)));
            }
            KeyCode::Char('g') | KeyCode::Home => {
                self.browse.table_state.select(Some(0));
            }
            KeyCode::Char('G') | KeyCode::End => {
                let last = self.browse.visible_rows().len().saturating_sub(1);
                self.browse.table_state.select(Some(last));
            }
            KeyCode::Char('y') => self.copy_selected(),
            KeyCode::Esc => self.tab = Tab::Explorer,
            _ => {}
        }
    }

    fn open_palette(&mut self) {
        let read_only = if self.read_only { "on" } else { "off" };
        let entries = vec![
            ("tab.explorer", "Go to Explorer".to_string()),
            ("tab.browse", "Browse all services".to_string()),
            ("tab.console", "Open the API console".to_string()),
            ("tab.emulator", "Emulator lifecycle and logs".to_string()),
            ("tab.requests", "Request history".to_string()),
            ("diag", "Run diagnostics".to_string()),
            (
                "readonly",
                format!("Toggle read-only mode (currently {read_only})"),
            ),
            ("export", "Export the current table to JSON".to_string()),
            ("copy", "Copy the selected row (OSC52)".to_string()),
            ("endpoint", "Switch emulator endpoint".to_string()),
            ("start", "Start the emulator".to_string()),
            ("stop", "Stop the emulator".to_string()),
            ("restart", "Restart the emulator".to_string()),
            ("help", "Help".to_string()),
            ("quit", "Quit".to_string()),
        ];
        let all: Vec<(String, String)> = entries
            .into_iter()
            .map(|(key, label)| (key.to_string(), label))
            .collect();
        let mut state = ListState::default();
        state.select(Some(0));
        self.modal = Some(Modal::Picker {
            title: "Command palette — type to filter".to_string(),
            filter: Input::default(),
            all,
            state,
            action: PickerAction::Palette,
        });
    }

    async fn run_palette(&mut self, key: &str) {
        match key {
            "tab.explorer" => self.tab = Tab::Explorer,
            "tab.browse" => {
                self.tab = Tab::Browse;
                self.ensure_browse_loaded();
            }
            "tab.console" => self.tab = Tab::Console,
            "tab.emulator" => self.tab = Tab::Emulator,
            "tab.requests" => self.tab = Tab::Requests,
            "diag" => self.spawn_doctor(),
            "readonly" => {
                self.read_only = !self.read_only;
                let state = if self.read_only { "on" } else { "off" };
                self.toast(ToastKind::Info, format!("read-only mode {state}"));
            }
            "export" => self.export_current(),
            "copy" => self.copy_selected(),
            "endpoint" => self.open_endpoint_picker(),
            "start" => self.start_emulator().await,
            "stop" => self.stop_emulator().await,
            "restart" => self.restart_emulator().await,
            "help" => self.modal = Some(Modal::Help { scroll: 0 }),
            "quit" => {
                if self.supervisor.has_child() {
                    self.modal = Some(Modal::Confirm {
                        title: "Quit".to_string(),
                        message: "This TUI started the emulator. Quit and stop it?".to_string(),
                        action: ConfirmAction::Quit,
                    });
                } else {
                    self.should_quit = true;
                }
            }
            _ => {}
        }
    }

    fn open_endpoint_picker(&mut self) {
        let endpoints = config::discovered_endpoints();
        if endpoints.is_empty() {
            self.toast(ToastKind::Info, "no emulators found under ~/.aws/emulator");
            return;
        }
        let all: Vec<(String, String)> = endpoints
            .iter()
            .map(|endpoint| {
                (
                    endpoint.endpoint.clone(),
                    format!(
                        "port {:<5} {}  ·  {}",
                        endpoint.port, endpoint.endpoint, endpoint.region
                    ),
                )
            })
            .collect();
        let mut state = ListState::default();
        state.select(Some(0));
        self.modal = Some(Modal::Picker {
            title: format!("Emulator endpoints ({})", all.len()),
            filter: Input::default(),
            all,
            state,
            action: PickerAction::SwitchEndpoint,
        });
    }

    fn export_current(&mut self) {
        let (view, columns, rows): (String, Vec<String>, Vec<Value>) = match self.tab {
            Tab::Explorer => {
                let Some(table) = &self.explorer.table else {
                    self.toast(ToastKind::Info, "nothing to export on this screen");
                    return;
                };
                let rows = self
                    .explorer
                    .visible_rows()
                    .iter()
                    .map(|row| {
                        json!({
                            "id": row.id,
                            "cells": row.cells,
                            "detail": row.detail,
                        })
                    })
                    .collect();
                (
                    format!(
                        "{} · {}",
                        self.explorer.view.service_key(),
                        self.explorer.view.title()
                    ),
                    table
                        .columns
                        .iter()
                        .map(|column| column.to_string())
                        .collect(),
                    rows,
                )
            }
            Tab::Browse => {
                let Some(table) = &self.browse.table else {
                    self.toast(ToastKind::Info, "nothing to export on this screen");
                    return;
                };
                let rows = self
                    .browse
                    .visible_rows()
                    .iter()
                    .map(|row| row.detail.clone())
                    .collect();
                (
                    format!("{}.{}", self.browse.service, self.browse.operation),
                    table.columns.clone(),
                    rows,
                )
            }
            _ => {
                self.toast(
                    ToastKind::Info,
                    "open a table in Explorer or Browse before exporting",
                );
                return;
            }
        };
        if rows.is_empty() {
            self.toast(ToastKind::Info, "the current table has no rows");
            return;
        }
        let stamp = emu_core::sigv4::amz_date(SystemTime::now());
        let stamp = &stamp[..15];
        let directory = dirs::data_dir()
            .unwrap_or_else(|| std::path::PathBuf::from("."))
            .join("emutui")
            .join("exports");
        if let Err(error) = std::fs::create_dir_all(&directory) {
            self.toast(
                ToastKind::Error,
                format!("cannot create export dir: {error}"),
            );
            return;
        }
        let safe = view
            .chars()
            .map(|character| {
                if character.is_ascii_alphanumeric() || character == '-' || character == '_' {
                    character
                } else {
                    '-'
                }
            })
            .collect::<String>();
        let path = directory.join(format!("{safe}-{stamp}.json"));
        let count = rows.len();
        let payload = json!({
            "generated_by": "emutui",
            "view": view,
            "columns": columns,
            "rows": rows,
            "row_count": count,
        });
        match std::fs::write(
            &path,
            serde_json::to_string_pretty(&payload).unwrap_or_default(),
        ) {
            Ok(()) => self.toast(ToastKind::Ok, format!("exported to {}", path.display())),
            Err(error) => self.toast(ToastKind::Error, format!("export failed: {error}")),
        }
    }

    fn copy_selected(&mut self) {
        let text = match self.tab {
            Tab::Explorer => self.explorer.selected_row().map(|row| {
                row.detail
                    .as_ref()
                    .map(pretty_json)
                    .unwrap_or_else(|| row.id.clone())
            }),
            Tab::Browse => self
                .browse
                .selected_row()
                .map(|row| pretty_json(&row.detail)),
            _ => None,
        };
        let Some(text) = text else {
            self.toast(ToastKind::Info, "select a row first");
            return;
        };
        copy_osc52(&text);
        self.toast(
            ToastKind::Ok,
            format!("copied {} bytes with OSC52", text.len()),
        );
    }

    fn show_browse_detail(&mut self) {
        let Some(row) = self.browse.selected_row() else {
            return;
        };
        self.modal = Some(Modal::Text {
            title: format!("{}.{}", self.browse.service, self.browse.operation),
            body: pretty_json(&row.detail),
            scroll: 0,
        });
    }
}

fn operation_rank(name: &str) -> u8 {
    if name.starts_with("List") {
        0
    } else if name.starts_with("Describe")
        || name.starts_with("Get")
        || name.starts_with("Search")
        || name.starts_with("Scan")
    {
        1
    } else {
        2
    }
}

// ---------------------------------------------------------------------- support

pub enum Msg {
    Health(Result<(), String>),
    Doctor(String),
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
    Special {
        service: String,
        operation: String,
        label: String,
        request: String,
        started: Instant,
        result: Result<ApiResponse, ApiError>,
    },
    S3Preview {
        bucket: String,
        key: String,
        started: Instant,
        result: Result<ApiResponse, ApiError>,
    },
    Browse {
        generation: u64,
        append: bool,
        service: String,
        operation: String,
        request: String,
        started: Instant,
        result: Result<(GenericTable, ApiResponse), ApiError>,
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
    BrowseFilter,
    CreateName(ViewId),
}

pub enum JsonAction {
    CreateJson(ViewId),
    BrowseRun,
    RunSpecial {
        service: String,
        operation: String,
        label: String,
    },
}

pub enum ConfirmAction {
    Delete(ViewId, String),
    Quit,
}

#[derive(Clone, Copy)]
pub enum PickerAction {
    ConsoleService,
    ConsoleOperation,
    BrowseService,
    BrowseOperation,
    Palette,
    SwitchEndpoint,
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

fn copy_osc52(text: &str) {
    use base64::Engine;
    let encoded = base64::engine::general_purpose::STANDARD.encode(text);
    let mut out = std::io::stdout();
    let _ = write!(out, "\x1b]52;c;{encoded}\x07");
    let _ = out.flush();
}

fn is_write_operation(name: &str) -> bool {
    const PREFIXES: &[&str] = &[
        "Create",
        "Put",
        "Delete",
        "Update",
        "Start",
        "Stop",
        "Terminate",
        "Modify",
        "Attach",
        "Detach",
        "Set",
        "Remove",
        "Revoke",
        "Authorize",
        "Send",
        "Publish",
        "Purge",
        "Execute",
        "Run",
        "Invoke",
        "Import",
        "Restore",
        "Cancel",
        "Register",
        "Deregister",
        "Associate",
        "Disassociate",
        "Enable",
        "Disable",
        "Reboot",
        "Reset",
        "Tag",
        "Untag",
        "Copy",
        "Upload",
        "Complete",
        "Abort",
        "Add",
        "Apply",
        "Accept",
        "Reject",
        "Batch",
        "Provision",
        "Allocate",
        "Release",
        "Replace",
        "Rotate",
        "Schedule",
    ];
    PREFIXES.iter().any(|prefix| name.starts_with(prefix))
}

fn exists(path: &std::path::Path) -> &'static str {
    if path.exists() {
        "exists"
    } else {
        "missing"
    }
}

fn writable(path: &std::path::Path) -> &'static str {
    if !path.is_dir() {
        return "missing";
    }
    let probe = path.join(".emutui-write-test");
    match std::fs::write(&probe, b"ok") {
        Ok(()) => {
            let _ = std::fs::remove_file(&probe);
            "writable"
        }
        Err(_) => "not writable",
    }
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

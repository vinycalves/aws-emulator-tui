//! Rendering.

use ratatui::layout::{Alignment, Constraint, Layout, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span, Text};
use ratatui::widgets::{Block, Cell, Clear, List, ListItem, Paragraph, Row, Table, Wrap};
use ratatui::Frame;

use crate::app::{filtered_picker, App, Focus, Health, Input, Modal, ServiceStatus, Tab};
use crate::theme;

pub fn draw(frame: &mut Frame, app: &mut App) {
    let area = frame.area();
    frame.render_widget(Block::default().style(theme::base()), area);

    let chunks = Layout::vertical([
        Constraint::Length(1),
        Constraint::Length(1),
        Constraint::Min(0),
        Constraint::Length(1),
    ])
    .split(area);

    draw_header(frame, app, chunks[0]);
    draw_tabs(frame, app, chunks[1]);

    match app.tab {
        Tab::Explorer => draw_explorer(frame, app, chunks[2]),
        Tab::Browse => draw_browse(frame, app, chunks[2]),
        Tab::Console => draw_console(frame, app, chunks[2]),
        Tab::Emulator => draw_emulator(frame, app, chunks[2]),
        Tab::Requests => draw_requests(frame, app, chunks[2]),
    }

    draw_status(frame, app, chunks[3]);

    if let Some(modal) = &app.modal {
        draw_modal(frame, app, modal);
    }
}

fn draw_header(frame: &mut Frame, app: &App, area: Rect) {
    let (status_text, status_style) = match &app.health {
        Health::Up => ("● healthy", theme::ok()),
        Health::Unknown => ("● checking", theme::muted()),
        Health::Down(_) => ("● offline", theme::err()),
    };

    let mut left = vec![
        Span::styled(" AWS EMULATOR TUI ", theme::title()),
        Span::styled("│ ", theme::muted()),
        Span::styled(
            app.client.endpoint().to_string(),
            Style::default().fg(theme::BLUE),
        ),
        Span::styled("  ", theme::muted()),
        Span::styled(status_text, status_style),
        Span::styled("  │ ", theme::muted()),
        Span::styled(app.config.region.clone(), theme::muted()),
    ];
    if !app.identity.is_empty() {
        left.push(Span::styled("  │ ", theme::muted()));
        left.push(Span::styled(truncate(&app.identity, 60), theme::muted()));
    }
    if app.read_only {
        left.push(Span::styled("  │ ", theme::muted()));
        left.push(Span::styled(
            "READ-ONLY",
            Style::default().fg(theme::RED).add_modifier(Modifier::BOLD),
        ));
    }

    frame.render_widget(
        Paragraph::new(Line::from(left)).style(theme::base().bg(theme::PANEL)),
        area,
    );
}

fn draw_tabs(frame: &mut Frame, app: &App, area: Rect) {
    let mut spans = Vec::new();
    for (index, tab) in Tab::ALL.iter().enumerate() {
        let style = if app.tab == *tab {
            theme::selected()
        } else {
            theme::muted()
        };
        spans.push(Span::styled(
            format!(" {} {} ", index + 1, tab.title()),
            style,
        ));
        spans.push(Span::raw(" "));
    }
    spans.push(Span::styled(
        "  tab switch · ? help · q quit",
        Style::default().fg(theme::DIM),
    ));
    frame.render_widget(
        Paragraph::new(Line::from(spans)).style(theme::base().bg(theme::PANEL_ALT)),
        area,
    );
}

fn draw_explorer(frame: &mut Frame, app: &mut App, area: Rect) {
    let [sidebar, main] = Layout::horizontal([
        Constraint::Length(if area.width > 100 { 26 } else { 22 }),
        Constraint::Min(0),
    ])
    .split(area)
    .as_ref()
    .try_into()
    .expect("two explorer panes");

    draw_sidebar(frame, app, sidebar);
    draw_resource_pane(frame, app, main);
}

fn draw_sidebar(frame: &mut Frame, app: &mut App, area: Rect) {
    let block = Block::bordered()
        .title(Line::from(vec![
            Span::raw(" "),
            Span::styled("SERVICES", theme::title()),
            Span::raw(" "),
        ]))
        .title_bottom(Line::from(" a  all services ").alignment(Alignment::Right))
        .border_style(if app.explorer.focus == Focus::Sidebar {
            Style::default().fg(theme::BORDER_FOCUS)
        } else {
            Style::default().fg(theme::BORDER)
        })
        .style(theme::panel());
    let inner = block.inner(area);
    frame.render_widget(block, area);

    let mut lines: Vec<Line> = Vec::new();
    let mut selected_line = 0usize;
    let mut last_category = "";
    for (index, service) in app.services.iter().enumerate() {
        if service.category != last_category {
            lines.push(Line::from(Span::styled(
                format!(" {}", service.category.to_ascii_uppercase()),
                Style::default().fg(theme::DIM).add_modifier(Modifier::BOLD),
            )));
            last_category = service.category;
        }
        let status = app
            .service_status
            .get(service.key)
            .cloned()
            .unwrap_or(ServiceStatus::Unknown);
        let (glyph, glyph_style) = match status {
            ServiceStatus::Ok => ("●", theme::ok()),
            ServiceStatus::Err => ("✕", theme::err()),
            ServiceStatus::Loading => ("◌", Style::default().fg(theme::YELLOW)),
            ServiceStatus::Unknown => ("○", theme::muted()),
        };
        let selected = index == app.explorer.service_index;
        let style = if selected {
            theme::selected()
        } else {
            Style::default().fg(theme::TEXT)
        };
        let mut spans = vec![
            Span::styled(format!(" {:<15}", service.title), style),
            Span::raw(" "),
            Span::styled(glyph, if selected { style } else { glyph_style }),
        ];
        if selected {
            spans.insert(0, Span::raw(""));
            selected_line = lines.len();
            lines.push(Line::from(spans));
        } else {
            lines.push(Line::from(spans));
        }
    }

    let height = inner.height as usize;
    let offset = if selected_line >= height {
        selected_line + 1 - height
    } else {
        0
    };
    let visible: Vec<Line> = lines.into_iter().skip(offset).take(height).collect();
    frame.render_widget(Paragraph::new(visible).style(theme::panel()), inner);
}

fn draw_resource_pane(frame: &mut Frame, app: &mut App, area: Rect) {
    let show_detail = app.explorer.show_detail;
    let chunks = Layout::vertical([
        Constraint::Length(1),
        Constraint::Min(5),
        Constraint::Length(if show_detail { detail_height(area) } else { 0 }),
    ])
    .split(area);

    let breadcrumb = breadcrumb_line(app);
    frame.render_widget(Paragraph::new(breadcrumb), chunks[0]);

    draw_resource_table(frame, app, chunks[1]);
    if show_detail {
        draw_detail(frame, app, chunks[2]);
    }
}

fn detail_height(area: Rect) -> u16 {
    (area.height * 2 / 5).clamp(7, 18)
}

fn breadcrumb_line(app: &App) -> Line<'static> {
    let service = app.services[app.explorer.service_index];
    let mut spans = vec![
        Span::styled(service.title.to_string(), theme::title()),
        Span::styled(" › ", theme::muted()),
        Span::styled(app.explorer.view.title(), Style::default().fg(theme::TEXT)),
    ];
    if let Some(table) = &app.explorer.table {
        let count = table.rows.len();
        spans.push(Span::styled(
            format!("  ({count} {})", if count == 1 { "row" } else { "rows" }),
            Style::default().fg(theme::DIM),
        ));
    }
    if let Some(filter) = &app.explorer.filter {
        spans.push(Span::styled(
            format!("  filter: {filter}"),
            Style::default().fg(theme::YELLOW),
        ));
    }
    if app.explorer.loading {
        spans.push(Span::styled(
            "  loading…",
            Style::default().fg(theme::YELLOW),
        ));
    }
    if app.health.is_up() {
        spans.push(Span::styled(
            "   enter inspect · o children · x action · c create · d delete · / filter",
            Style::default().fg(theme::DIM),
        ));
    }
    Line::from(spans)
}

fn draw_resource_table(frame: &mut Frame, app: &mut App, area: Rect) {
    let inner_area = area;
    let focused = app.explorer.focus == Focus::Table;

    if let Some(error) = &app.explorer.error {
        let hint = error_hint(error);
        let message = if error.contains("cannot reach")
            || error.contains("error sending request")
            || error.contains("connection")
        {
            format!(
                "Cannot reach the emulator at {}.\n\nStart it from the Emulator tab (s), or run:\n\n    aws-emulator\n\n{}",
                app.client.endpoint(),
                error
            )
        } else if hint.is_empty() {
            error.clone()
        } else {
            format!("{error}\n\n{hint}")
        };
        let paragraph = Paragraph::new(message)
            .block(
                Block::bordered()
                    .title(" error ")
                    .border_style(Style::default().fg(theme::RED))
                    .style(theme::panel()),
            )
            .wrap(Wrap { trim: false })
            .style(theme::err());
        frame.render_widget(paragraph, inner_area);
        return;
    }

    let Some(table) = &app.explorer.table else {
        if app.explorer.loading {
            frame.render_widget(
                Paragraph::new("Loading…")
                    .block(
                        Block::bordered()
                            .title(" resources ")
                            .border_style(Style::default().fg(theme::BORDER))
                            .style(theme::panel()),
                    )
                    .style(theme::muted()),
                inner_area,
            );
        }
        return;
    };

    let columns: Vec<&str> = table.columns.clone();
    let rows: Vec<Row> = app
        .explorer
        .visible_rows()
        .iter()
        .map(|row| {
            let cells: Vec<Cell> = row
                .cells
                .iter()
                .map(|value| Cell::from(value.clone()))
                .collect();
            Row::new(cells)
        })
        .collect();

    let header = Row::new(
        columns
            .iter()
            .map(|column| Cell::from(column.to_string()))
            .collect::<Vec<_>>(),
    )
    .style(
        Style::default()
            .fg(theme::ORANGE)
            .add_modifier(Modifier::BOLD),
    );

    let mut widths: Vec<Constraint> = Vec::new();
    for (index, column) in columns.iter().enumerate() {
        if index == 0 {
            widths.push(Constraint::Fill(2));
            continue;
        }
        let content = app
            .explorer
            .visible_rows()
            .iter()
            .filter_map(|row| row.cells.get(index))
            .map(|cell| cell.chars().count())
            .max()
            .unwrap_or(0);
        let width = (column.chars().count().max(content) as u16 + 2).clamp(8, 42);
        widths.push(Constraint::Length(width));
    }

    let empty = rows.is_empty();
    let mut block = Block::bordered()
        .title(Line::from(vec![
            Span::raw(" "),
            Span::styled(app.explorer.view.title(), theme::title()),
            Span::raw(" "),
        ]))
        .border_style(if focused {
            Style::default().fg(theme::BORDER_FOCUS)
        } else {
            Style::default().fg(theme::BORDER)
        })
        .style(theme::panel());
    if empty {
        block = block.title_bottom(
            Line::from(" no resources yet · press c to create ").alignment(Alignment::Right),
        );
    }

    let table_widget = Table::new(rows, widths)
        .header(header)
        .column_spacing(1)
        .block(block)
        .row_highlight_style(if focused {
            theme::selected()
        } else {
            Style::default().bg(theme::PANEL_ALT)
        })
        .highlight_symbol("");

    frame.render_stateful_widget(table_widget, inner_area, &mut app.explorer.table_state);
}

fn draw_detail(frame: &mut Frame, app: &mut App, area: Rect) {
    let title = format!(" {} ", app.explorer.detail_title);
    let paragraph = Paragraph::new(app.explorer.detail_body.clone())
        .block(
            Block::bordered()
                .title(Line::from(vec![
                    Span::raw(" detail "),
                    Span::styled(truncate(&title, 60), theme::muted()),
                ]))
                .title_bottom(
                    Line::from(" esc close · PgUp/PgDn scroll ").alignment(Alignment::Right),
                )
                .border_style(Style::default().fg(theme::BORDER))
                .style(theme::panel()),
        )
        .scroll((app.explorer.detail_scroll, 0))
        .wrap(Wrap { trim: false });
    frame.render_widget(paragraph, area);
}

fn draw_console(frame: &mut Frame, app: &mut App, area: Rect) {
    let chunks = Layout::vertical([Constraint::Length(1), Constraint::Min(0)]).split(area);
    let protocol = app.console.protocol();
    let status = app
        .console
        .status
        .map(|(code, ms)| format!("HTTP {code} · {ms} ms"))
        .unwrap_or_else(|| "not sent".to_string());
    let context = Line::from(vec![
        Span::styled(" service ", Style::default().fg(theme::DIM)),
        Span::styled(app.console.service.clone(), theme::title()),
        Span::styled("  op ", Style::default().fg(theme::DIM)),
        Span::styled(
            if app.console.operation.is_empty() {
                "<pick one>".to_string()
            } else {
                app.console.operation.clone()
            },
            Style::default().fg(theme::TEXT),
        ),
        Span::styled("  protocol ", Style::default().fg(theme::DIM)),
        Span::styled(protocol, Style::default().fg(theme::PURPLE)),
        Span::styled(
            "  s service · o operation · e edit · ctrl+s send",
            theme::muted(),
        ),
        Span::raw("  "),
        Span::styled(status, theme::muted()),
    ]);
    frame.render_widget(Paragraph::new(context), chunks[0]);

    let [left, right] =
        Layout::horizontal([Constraint::Percentage(46), Constraint::Percentage(54)])
            .split(chunks[1])
            .as_ref()
            .try_into()
            .expect("two console panes");

    draw_console_editor(frame, app, left);
    draw_console_response(frame, app, right);
}

fn draw_console_editor(frame: &mut Frame, app: &mut App, area: Rect) {
    let focused = app.console.focus_editor;
    app.console.textarea.set_block(
        Block::bordered()
            .title(Line::from(vec![
                Span::raw(" request "),
                Span::styled(
                    if focused {
                        "· editing "
                    } else {
                        "· e to edit "
                    },
                    theme::muted(),
                ),
            ]))
            .border_style(if focused {
                Style::default().fg(theme::BORDER_FOCUS)
            } else {
                Style::default().fg(theme::BORDER)
            })
            .style(theme::panel()),
    );
    frame.render_widget(&app.console.textarea, area);
    if app.console.busy {
        let hint = Rect {
            x: area.x + area.width.saturating_sub(12),
            y: area.y,
            width: 10,
            height: 1,
        };
        frame.render_widget(
            Paragraph::new(" sending… ").style(Style::default().fg(theme::YELLOW)),
            hint,
        );
    }
}

fn draw_console_response(frame: &mut Frame, app: &mut App, area: Rect) {
    let (title, style) = match (&app.console.error, app.console.status) {
        (Some(_), _) => (" response · error ", Style::default().fg(theme::RED)),
        (None, Some((code, _))) if code < 400 => (" response ", theme::ok()),
        _ => (" response ", theme::muted()),
    };
    let body = if let Some(error) = &app.console.error {
        let hint = error_hint(error);
        let mut body = if app.console.response.is_empty() {
            error.clone()
        } else {
            format!("{error}\n\n{}", app.console.response)
        };
        if !hint.is_empty() {
            body.push_str("\n\n");
            body.push_str(hint);
        }
        body
    } else if app.console.response.is_empty() {
        "Send a request with Ctrl+S to see the response.\n\nThe body is JSON. For query-protocol services it is flattened into form parameters automatically.".to_string()
    } else {
        app.console.response.clone()
    };
    let paragraph = Paragraph::new(body)
        .block(
            Block::bordered()
                .title(Line::from(vec![
                    Span::raw(" "),
                    Span::styled(title.trim(), style),
                    Span::raw(" "),
                ]))
                .title_bottom(Line::from(" [ ] scroll · ctrl+s send ").alignment(Alignment::Right))
                .border_style(Style::default().fg(theme::BORDER))
                .style(theme::panel()),
        )
        .scroll((app.console.scroll, 0))
        .wrap(Wrap { trim: false });
    frame.render_widget(paragraph, area);
}

fn draw_browse(frame: &mut Frame, app: &mut App, area: Rect) {
    let chunks = Layout::vertical([Constraint::Length(1), Constraint::Min(0)]).split(area);
    let status = app
        .browse
        .status
        .map(|(code, ms)| format!("HTTP {code} · {ms} ms"))
        .unwrap_or_else(|| "not run".to_string());
    let rows = app
        .browse
        .table
        .as_ref()
        .map(|table| table.rows.len())
        .unwrap_or(0);
    let more = if app.browse.token.is_some() {
        "  more pages (m)"
    } else {
        ""
    };
    let context = Line::from(vec![
        Span::styled(" service ", Style::default().fg(theme::DIM)),
        Span::styled(app.browse.service.clone(), theme::title()),
        Span::styled("  op ", Style::default().fg(theme::DIM)),
        Span::styled(
            if app.browse.operation.is_empty() {
                "<pick one>".to_string()
            } else {
                app.browse.operation.clone()
            },
            Style::default().fg(theme::TEXT),
        ),
        Span::styled("  rows ", Style::default().fg(theme::DIM)),
        Span::styled(rows.to_string(), Style::default().fg(theme::TEXT)),
        Span::styled("  ", theme::muted()),
        Span::styled(status, theme::muted()),
        Span::styled(more, Style::default().fg(theme::YELLOW)),
        Span::styled(
            "   s service · o operation · e input · r run · m more · enter detail",
            Style::default().fg(theme::DIM),
        ),
    ]);
    frame.render_widget(Paragraph::new(context), chunks[0]);

    if let Some(error) = &app.browse.error {
        let hint = error_hint(error);
        let message = if hint.is_empty() {
            error.clone()
        } else {
            format!("{error}\n\n{hint}")
        };
        frame.render_widget(
            Paragraph::new(message)
                .block(
                    Block::bordered()
                        .title(" browse error ")
                        .border_style(Style::default().fg(theme::RED))
                        .style(theme::panel()),
                )
                .wrap(Wrap { trim: false })
                .style(theme::err()),
            chunks[1],
        );
        return;
    }

    if app.browse.operation.is_empty() {
        frame.render_widget(
            Paragraph::new(
                "Browse any service in the catalog.\n\n\n  s   pick a service (436 available)\n  o   pick an operation (list-like operations first)\n  e   edit the JSON input\n  r   run\n  m   next page\n",
            )
            .block(
                Block::bordered()
                    .title(" browse ")
                    .border_style(Style::default().fg(theme::BORDER))
                    .style(theme::panel()),
            )
            .style(theme::muted()),
            chunks[1],
        );
        return;
    }

    let Some(table) = &app.browse.table else {
        if app.browse.loading {
            frame.render_widget(
                Paragraph::new("Loading…")
                    .block(
                        Block::bordered()
                            .title(" browse ")
                            .border_style(Style::default().fg(theme::BORDER))
                            .style(theme::panel()),
                    )
                    .style(theme::muted()),
                chunks[1],
            );
        }
        return;
    };

    let columns = table.columns.clone();
    let visible: Vec<Vec<String>> = app
        .browse
        .visible_rows()
        .iter()
        .map(|row| row.cells.clone())
        .collect();
    let filter = app.browse.filter.clone();
    render_table_panel(
        frame,
        chunks[1],
        &format!("{}.{}", app.browse.service, app.browse.operation),
        &columns,
        &visible,
        &mut app.browse.table_state,
        true,
        filter.as_deref(),
        Some(" no rows · e edit input · r run · s service · o operation "),
    );

    if app.browse.loading {
        let hint = Rect {
            x: chunks[1].x + chunks[1].width.saturating_sub(12),
            y: chunks[1].y,
            width: 10,
            height: 1,
        };
        frame.render_widget(
            Paragraph::new(" loading… ").style(Style::default().fg(theme::YELLOW)),
            hint,
        );
    }
}

#[allow(clippy::too_many_arguments)]
fn render_table_panel(
    frame: &mut Frame,
    area: Rect,
    title: &str,
    columns: &[String],
    rows: &[Vec<String>],
    state: &mut ratatui::widgets::TableState,
    focused: bool,
    filter: Option<&str>,
    empty_hint: Option<&str>,
) {
    let table_rows: Vec<Row> = rows
        .iter()
        .map(|row| {
            Row::new(
                row.iter()
                    .map(|value| Cell::from(value.clone()))
                    .collect::<Vec<_>>(),
            )
        })
        .collect();
    let header = Row::new(
        columns
            .iter()
            .map(|column| Cell::from(column.clone()))
            .collect::<Vec<_>>(),
    )
    .style(
        Style::default()
            .fg(theme::ORANGE)
            .add_modifier(Modifier::BOLD),
    );
    let mut widths: Vec<Constraint> = Vec::new();
    for (index, column) in columns.iter().enumerate() {
        let content = rows
            .iter()
            .filter_map(|row| row.get(index))
            .map(|cell| cell.chars().count())
            .max()
            .unwrap_or(0);
        let width = (content.max(column.chars().count()) as u16 + 2).clamp(8, 42);
        if index == 0 {
            widths.push(Constraint::Length(width.clamp(12, 45)));
        } else {
            widths.push(Constraint::Length(width));
        }
    }
    let mut heading = vec![
        Span::raw(" "),
        Span::styled(title.to_string(), theme::title()),
    ];
    if let Some(filter) = filter {
        heading.push(Span::styled(
            format!("  filter: {filter}"),
            Style::default().fg(theme::YELLOW),
        ));
    }
    heading.push(Span::raw(" "));
    let empty = rows.is_empty();
    let mut block = Block::bordered()
        .title(Line::from(heading))
        .border_style(if focused {
            Style::default().fg(theme::BORDER_FOCUS)
        } else {
            Style::default().fg(theme::BORDER)
        })
        .style(theme::panel());
    if empty {
        if let Some(hint) = empty_hint {
            block = block.title_bottom(Line::from(hint).alignment(Alignment::Right));
        }
    }
    let table = Table::new(table_rows, widths)
        .header(header)
        .column_spacing(1)
        .block(block)
        .row_highlight_style(if focused {
            theme::selected()
        } else {
            Style::default().bg(theme::PANEL_ALT)
        })
        .highlight_symbol("");
    frame.render_stateful_widget(table, area, state);
}

fn error_hint(error: &str) -> &'static str {
    if error.contains("not implemented") || error.contains("NotImplemented") {
        "This operation is not implemented by the emulator. Try a List/Describe operation, or check the emulator's fidelity scores."
    } else if error.contains("403")
        || error.contains("SignatureDoesNotMatch")
        || error.contains("InvalidClientTokenId")
        || error.contains("NoCredentials")
    {
        "The emulator rejected the credentials. Press r on the Emulator tab to reload the generated keys, or start the emulator with --auth off."
    } else if error.contains("uses REST-XML") {
        "This service uses REST-XML, which is not implemented in the generic browser yet. S3 has dedicated screens; other REST-XML services are on the roadmap."
    } else if error.contains("no emulated service is registered") {
        "This signing name is not mounted in this emulator build. The Console uses the signing name from the catalog."
    } else if error.contains("cannot reach") || error.contains("error sending request") {
        "Is the emulator running? Start it from the Emulator tab, or run aws-emulator."
    } else {
        ""
    }
}

fn draw_emulator(frame: &mut Frame, app: &mut App, area: Rect) {
    let health = match &app.health {
        Health::Up => "healthy".to_string(),
        Health::Unknown => "checking…".to_string(),
        Health::Down(error) => format!("offline — {error}"),
    };
    let process = match app.supervisor.pid() {
        Some(pid) => format!("pid {pid} (started by this TUI)"),
        None => "external / not attached".to_string(),
    };
    let binary = app
        .config
        .binary
        .as_ref()
        .map(|path| path.display().to_string())
        .unwrap_or_else(|| "not found — install aws-emulator or set AWS_EMULATOR_BIN".to_string());
    let log_count = app.supervisor.logs(usize::MAX).len();

    let mut lines = vec![
        info_line("endpoint", app.client.endpoint().to_string()),
        info_line("health", health),
        info_line("process", process),
        info_line("binary", binary),
        info_line(
            "credentials",
            app.config.credentials_source.label().to_string(),
        ),
        info_line("config", app.config.paths.config.display().to_string()),
        info_line("ca", app.config.paths.ca.display().to_string()),
    ];
    for warning in &app.config.warnings {
        lines.push(info_line("warning", warning.clone()));
    }

    let info_height = (lines.len() as u16 + 4).clamp(10, 22);
    let chunks = Layout::vertical([
        Constraint::Length(info_height),
        Constraint::Length(1),
        Constraint::Min(5),
    ])
    .split(area);

    let info = Paragraph::new(lines)
        .block(
            Block::bordered()
                .title(" emulator ")
                .border_style(Style::default().fg(theme::BORDER))
                .style(theme::panel()),
        )
        .wrap(Wrap { trim: false });
    frame.render_widget(info, chunks[0]);

    let controls = Line::from(vec![
        Span::styled(" s", theme::key()),
        Span::raw(" start · "),
        Span::styled("x", theme::key()),
        Span::raw(" stop · "),
        Span::styled("R", theme::key()),
        Span::raw(" restart · "),
        Span::styled("g", theme::key()),
        Span::raw(" follow · "),
        Span::styled("c", theme::key()),
        Span::raw(" clear · "),
        Span::styled("r", theme::key()),
        Span::raw(" reload · "),
        Span::styled("j/k", theme::key()),
        Span::raw(" scroll"),
        Span::styled(
            format!("   ({log_count} log lines captured)"),
            Style::default().fg(theme::DIM),
        ),
    ]);
    frame.render_widget(Paragraph::new(controls), chunks[1]);

    let height = chunks[2].height.saturating_sub(2) as usize;
    let all = app.supervisor.logs(2_000);
    let offset = app.emulator.logs_offset as usize;
    let end = all.len().saturating_sub(offset);
    let start = end.saturating_sub(height.max(1));
    let lines: Vec<Line> = all[start..end]
        .iter()
        .map(|line| {
            let style = if line.contains("\"level\":\"ERROR\"") || line.contains("ERROR") {
                Style::default().fg(theme::RED)
            } else if line.contains("\"level\":\"WARN\"") {
                Style::default().fg(theme::YELLOW)
            } else if line.starts_with('[') {
                Style::default().fg(theme::PURPLE)
            } else {
                Style::default().fg(theme::MUTED)
            };
            Line::from(Span::styled(
                truncate_tail(line, chunks[2].width.saturating_sub(2) as usize),
                style,
            ))
        })
        .collect();

    let body = if lines.is_empty() {
        Text::from(vec![
            Line::from(Span::styled(
                "No log lines captured.",
                Style::default().fg(theme::MUTED),
            )),
            Line::from(Span::styled(
                "Logs are only available for emulators started from this tab.",
                Style::default().fg(theme::DIM),
            )),
        ])
    } else {
        Text::from(lines)
    };

    frame.render_widget(
        Paragraph::new(body).block(
            Block::bordered()
                .title(" log ")
                .title_bottom(
                    Line::from(" offline emulator: start it with s ").alignment(Alignment::Right),
                )
                .border_style(Style::default().fg(theme::BORDER))
                .style(theme::panel()),
        ),
        chunks[2],
    );
}

fn info_line(label: &str, value: String) -> Line<'static> {
    Line::from(vec![
        Span::styled(format!(" {label:<12}"), Style::default().fg(theme::DIM)),
        Span::styled(value, Style::default().fg(theme::TEXT)),
    ])
}

fn draw_requests(frame: &mut Frame, app: &mut App, area: Rect) {
    if app.activities.is_empty() {
        frame.render_widget(
            Paragraph::new(
                "No requests yet.\n\nEvery list, inspect, create and delete call is recorded here.\nOpen the Console tab to send an arbitrary operation.",
            )
            .block(
                Block::bordered()
                    .title(" requests ")
                    .border_style(Style::default().fg(theme::BORDER))
                    .style(theme::panel()),
            )
            .style(theme::muted()),
            area,
        );
        return;
    }

    let rows: Vec<Row> = app
        .activities
        .iter()
        .map(|activity| {
            let style = if activity.ok {
                Style::default().fg(theme::TEXT)
            } else {
                Style::default().fg(theme::RED)
            };
            Row::new(vec![
                Cell::from(activity.time.clone()),
                Cell::from(activity.service.clone()),
                Cell::from(activity.operation.clone()),
                Cell::from(activity.status.clone()),
                Cell::from(format!("{} ms", activity.duration_ms)),
            ])
            .style(style)
        })
        .collect();

    let table = Table::new(
        rows,
        [
            Constraint::Length(10),
            Constraint::Length(16),
            Constraint::Fill(1),
            Constraint::Length(8),
            Constraint::Length(10),
        ],
    )
    .header(
        Row::new(vec!["TIME", "SERVICE", "OPERATION", "STATUS", "DURATION"]).style(
            Style::default()
                .fg(theme::ORANGE)
                .add_modifier(Modifier::BOLD),
        ),
    )
    .block(
        Block::bordered()
            .title(format!(" requests ({}) ", app.activities.len()))
            .title_bottom(Line::from(" enter detail · c clear ").alignment(Alignment::Right))
            .border_style(Style::default().fg(theme::BORDER))
            .style(theme::panel()),
    )
    .row_highlight_style(theme::selected())
    .column_spacing(1);
    frame.render_stateful_widget(table, area, &mut app.activity_state);
}

fn draw_status(frame: &mut Frame, app: &App, area: Rect) {
    let hint = match app.tab {
        Tab::Explorer if app.explorer.focus == Focus::Sidebar => {
            " ↑↓ service · g/G first/last · l table · a all services · ? help"
        }
        Tab::Explorer => {
            " ↑↓ row · enter inspect · o children · x action · c create · d delete · y copy · ctrl+p palette"
        }
        Tab::Browse => {
            " s service · o operation · e input · r run · m next page · enter detail · / filter · y copy · esc explorer"
        }
        Tab::Console if app.console.focus_editor => {
            " editing request · esc done · ctrl+s send"
        }
        Tab::Console => " s service · o operation · e edit · ctrl+s send · [ ] scroll · ? help",
        Tab::Emulator => " s start · x stop · R restart · g follow · c clear · j/k scroll",
        Tab::Requests => " ↑↓ select · enter detail · c clear",
    };
    let mut spans = vec![Span::styled(hint, Style::default().fg(theme::DIM))];

    if let Some(toast) = &app.toast {
        let remaining = (area.width as usize).saturating_sub(hint.chars().count() + 7);
        if remaining > 10 {
            spans.push(Span::styled("   │   ", Style::default().fg(theme::DIM)));
            spans.push(Span::styled(
                truncate(&toast.text, remaining),
                toast.kind.style(),
            ));
        }
    }

    frame.render_widget(
        Paragraph::new(Line::from(spans)).style(theme::base().bg(theme::PANEL)),
        area,
    );
}

fn draw_modal(frame: &mut Frame, _app: &App, modal: &Modal) {
    match modal {
        Modal::Help { scroll } => {
            let area = centered(frame.area(), 78, 82);
            frame.render_widget(Clear, area);
            let text = help_text();
            frame.render_widget(
                Paragraph::new(text)
                    .block(
                        Block::bordered()
                            .title(" help ")
                            .title_bottom(
                                Line::from(" esc close · j/k scroll ").alignment(Alignment::Right),
                            )
                            .border_style(Style::default().fg(theme::BORDER_FOCUS))
                            .style(theme::panel()),
                    )
                    .scroll((*scroll, 0))
                    .wrap(Wrap { trim: false }),
                area,
            );
        }
        Modal::Text {
            title,
            body,
            scroll,
        } => {
            let area = centered(frame.area(), 88, 84);
            frame.render_widget(Clear, area);
            frame.render_widget(
                Paragraph::new(body.clone())
                    .block(
                        Block::bordered()
                            .title(format!(" {title} "))
                            .title_bottom(
                                Line::from(" esc close · j/k scroll ").alignment(Alignment::Right),
                            )
                            .border_style(Style::default().fg(theme::BORDER_FOCUS))
                            .style(theme::panel()),
                    )
                    .scroll((*scroll, 0))
                    .wrap(Wrap { trim: false }),
                area,
            );
        }
        Modal::Input {
            title,
            prompt,
            input,
            action: _,
        } => {
            let area = centered_fixed(frame.area(), 64, 10);
            frame.render_widget(Clear, area);
            let (before, after) = input.split();
            let block = Block::bordered()
                .title(format!(" {title} "))
                .title_bottom(
                    Line::from(" enter confirm · esc cancel ").alignment(Alignment::Right),
                )
                .border_style(Style::default().fg(theme::BORDER_FOCUS))
                .style(theme::panel());
            let inner = block.inner(area);
            frame.render_widget(block, area);
            let layout = Layout::vertical([
                Constraint::Length(1),
                Constraint::Length(3),
                Constraint::Min(1),
                Constraint::Length(2),
            ])
            .split(inner);
            frame.render_widget(
                Paragraph::new(prompt.clone()).style(theme::muted()),
                layout[0],
            );
            frame.render_widget(
                Paragraph::new(Line::from(vec![
                    Span::styled(before.to_string(), Style::default().fg(theme::TEXT)),
                    Span::styled("▏", Style::default().fg(theme::ORANGE)),
                    Span::styled(after.to_string(), Style::default().fg(theme::TEXT)),
                ]))
                .style(theme::panel())
                .block(Block::bordered().border_style(Style::default().fg(theme::BORDER))),
                layout[1],
            );
            frame.render_widget(
                Paragraph::new(help_for_input(title))
                    .style(Style::default().fg(theme::DIM))
                    .wrap(Wrap { trim: false }),
                layout[3],
            );
        }
        Modal::Json {
            title,
            textarea,
            action: _,
            error,
        } => {
            let area = centered(frame.area(), 86, 84);
            frame.render_widget(Clear, area);
            let block = Block::bordered()
                .title(format!(" {title} — JSON "))
                .title_bottom(
                    Line::from(" ctrl+s submit · esc cancel ").alignment(Alignment::Right),
                )
                .border_style(Style::default().fg(theme::BORDER_FOCUS))
                .style(theme::panel());
            let inner = block.inner(area);
            frame.render_widget(block, area);
            let chunks = Layout::vertical([
                Constraint::Length(if error.is_some() { 1 } else { 0 }),
                Constraint::Min(0),
            ])
            .split(inner);
            if let Some(error) = error {
                frame.render_widget(Paragraph::new(error.clone()).style(theme::err()), chunks[0]);
            }
            frame.render_widget(&**textarea, chunks[1]);
        }
        Modal::Confirm {
            title,
            message,
            action: _,
        } => {
            let area = centered_fixed(frame.area(), 62, 8);
            frame.render_widget(Clear, area);
            frame.render_widget(
                Paragraph::new(message.clone())
                    .block(
                        Block::bordered()
                            .title(format!(" {title} "))
                            .title_bottom(
                                Line::from(" enter/y confirm · esc/n cancel ")
                                    .alignment(Alignment::Right),
                            )
                            .border_style(Style::default().fg(theme::YELLOW))
                            .style(theme::panel()),
                    )
                    .wrap(Wrap { trim: false })
                    .alignment(Alignment::Center),
                area,
            );
        }
        Modal::Picker {
            title,
            filter,
            all,
            state,
            action: _,
        } => {
            let area = centered(frame.area(), 82, 80);
            frame.render_widget(Clear, area);
            let block = Block::bordered()
                .title(format!(" {title} "))
                .title_bottom(
                    Line::from(" ↑↓ select · enter open · esc cancel ").alignment(Alignment::Right),
                )
                .border_style(Style::default().fg(theme::BORDER_FOCUS))
                .style(theme::panel());
            let inner = block.inner(area);
            frame.render_widget(block, area);
            let chunks = Layout::vertical([Constraint::Length(3), Constraint::Min(0)]).split(inner);
            let (before, after) = filter.split();
            frame.render_widget(
                Paragraph::new(Line::from(vec![
                    Span::styled("filter: ", theme::muted()),
                    Span::styled(before.to_string(), Style::default().fg(theme::TEXT)),
                    Span::styled("▏", Style::default().fg(theme::ORANGE)),
                    Span::styled(after.to_string(), Style::default().fg(theme::TEXT)),
                ]))
                .block(Block::bordered().border_style(Style::default().fg(theme::BORDER))),
                chunks[0],
            );
            let filtered = filtered_picker(all, filter);
            let items: Vec<ListItem> = filtered
                .iter()
                .map(|(_, label)| ListItem::new(label.clone()))
                .collect();
            let mut list_state = state.clone();
            frame.render_stateful_widget(
                List::new(items)
                    .highlight_style(theme::selected())
                    .highlight_symbol("› "),
                chunks[1],
                &mut list_state,
            );
            if filtered.is_empty() {
                frame.render_widget(
                    Paragraph::new("no matches").style(theme::muted()),
                    chunks[1],
                );
            }
        }
    }
}

impl Input {
    fn split(&self) -> (&str, &str) {
        (&self.value[..self.cursor], &self.value[self.cursor..])
    }
}

fn help_for_input(title: &str) -> &'static str {
    if title == "Filter" {
        "Substring match over every cell. Leave empty to clear the filter."
    } else {
        "The name becomes the resource identifier. Enter submits."
    }
}

fn centered_fixed(area: Rect, percent_x: u16, height: u16) -> Rect {
    let height = height.min(area.height.saturating_sub(2)).max(3);
    let vertical = Layout::vertical([
        Constraint::Length((area.height.saturating_sub(height)) / 2),
        Constraint::Length(height),
        Constraint::Min(0),
    ])
    .split(area);
    Layout::horizontal([
        Constraint::Percentage((100 - percent_x) / 2),
        Constraint::Percentage(percent_x),
        Constraint::Percentage((100 - percent_x) / 2),
    ])
    .split(vertical[1])[1]
}

fn centered(area: Rect, percent_x: u16, percent_y: u16) -> Rect {
    let vertical = Layout::vertical([
        Constraint::Percentage((100 - percent_y) / 2),
        Constraint::Percentage(percent_y),
        Constraint::Percentage((100 - percent_y) / 2),
    ])
    .split(area);
    Layout::horizontal([
        Constraint::Percentage((100 - percent_x) / 2),
        Constraint::Percentage(percent_x),
        Constraint::Percentage((100 - percent_x) / 2),
    ])
    .split(vertical[1])[1]
}

fn help_text() -> Text<'static> {
    let mut lines = vec![
        Line::from(Span::styled("AWS Emulator TUI", theme::title())),
        Line::from(""),
        Line::from(Span::styled("Global", Style::default().fg(theme::ORANGE).add_modifier(Modifier::BOLD))),
        help_line("1–5 / Tab", "switch tab"),
        help_line("a", "browse all services in the catalog (436)"),
        help_line("Ctrl+P / :", "command palette"),
        help_line("Ctrl+E", "export the current table to JSON"),
        help_line("D", "run diagnostics"),
        help_line("y", "copy the selected row (OSC52)"),
        help_line("?", "this help"),
        help_line("q / Ctrl+C", "quit (stops the emulator if this TUI started it)"),
        Line::from(""),
        Line::from(Span::styled("Explorer", Style::default().fg(theme::ORANGE).add_modifier(Modifier::BOLD))),
        help_line("h / l", "move focus between services and resources"),
        help_line("g / G", "first / last service"),
        help_line("↑↓ / j k", "move the selection"),
        help_line("enter", "inspect, or preview an S3 object"),
        help_line("o", "open the children of a row (table, queue, stack, …)"),
        help_line("x", "extra action (send, publish, invoke, start)"),
        help_line("esc", "close detail / go back"),
        help_line("c", "create a resource"),
        help_line("d", "delete the selected resource"),
        help_line("r", "refresh"),
        help_line("/", "filter rows"),
        help_line("X", "clear the filter"),
        help_line("v", "cycle IAM users/roles"),
        help_line("PgUp/PgDn", "scroll the detail pane"),
        Line::from(""),
        Line::from(Span::styled("Browse", Style::default().fg(theme::ORANGE).add_modifier(Modifier::BOLD))),
        help_line("s / o", "pick a service / operation"),
        help_line("e / r", "edit the input / run it"),
        help_line("m", "load the next page"),
        help_line("enter", "open the row as JSON"),
        Line::from(""),
        Line::from(Span::styled("Console", Style::default().fg(theme::ORANGE).add_modifier(Modifier::BOLD))),
        help_line("s / o", "pick service / operation"),
        help_line("e", "edit the JSON request"),
        help_line("Ctrl+S / F5", "send"),
        help_line("r", "reset the body to the required fields"),
        help_line("[ / ]", "scroll the response"),
        Line::from(""),
        Line::from(Span::styled("Emulator", Style::default().fg(theme::ORANGE).add_modifier(Modifier::BOLD))),
        help_line("s / x / R", "start / stop / restart"),
        help_line("g / c", "follow logs / clear logs"),
        help_line("r", "reload endpoint, credentials, and certificate"),
        Line::from(""),
        Line::from(Span::styled("Read-only", Style::default().fg(theme::ORANGE).add_modifier(Modifier::BOLD))),
        help_line("Ctrl+P", "toggle read-only mode from the palette"),
        Line::from(""),
        Line::from(Span::styled("Requests", Style::default().fg(theme::ORANGE).add_modifier(Modifier::BOLD))),
        help_line("↑↓ / enter", "select / open a request"),
        help_line("c", "clear the history"),
        Line::from(""),
        Line::from(Span::styled(
            "Notes: the emulator keeps state in memory. Starting it from here always disables telemetry.",
            Style::default().fg(theme::DIM),
        )),
        Line::from(Span::styled(
            "All emulated resources are lost when it stops.",
            Style::default().fg(theme::DIM),
        )),
        Line::from(""),
        Line::from(Span::styled(
            format!(
                "Catalog: {} — {} services, {} operations.",
                emu_core::catalog::catalog().source,
                emu_core::catalog::catalog().len(),
                emu_core::catalog::catalog()
                    .services
                    .values()
                    .map(|service| service.ops.len())
                    .sum::<usize>()
            ),
            Style::default().fg(theme::DIM),
        )),
    ];
    while lines.len() < 45 {
        lines.push(Line::from(""));
    }
    Text::from(lines)
}

fn help_line(key: &str, description: &str) -> Line<'static> {
    Line::from(vec![
        Span::styled(format!("  {key:<16}"), Style::default().fg(theme::YELLOW)),
        Span::styled(description.to_string(), Style::default().fg(theme::TEXT)),
    ])
}

fn truncate(text: &str, limit: usize) -> String {
    if text.chars().count() <= limit {
        return text.to_string();
    }
    let shortened: String = text.chars().take(limit.saturating_sub(1)).collect();
    format!("{shortened}…")
}

fn truncate_tail(text: &str, limit: usize) -> String {
    let count = text.chars().count();
    if count <= limit {
        return text.to_string();
    }
    let skip = count - limit.saturating_sub(1);
    format!("…{}", text.chars().skip(skip).collect::<String>())
}

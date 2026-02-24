/*
   Copyright (C) 2026 l5yth

   Licensed under the Apache License, Version 2.0 (the "License");
   you may not use this file except in compliance with the License.
   You may obtain a copy of the License at

       http://www.apache.org/licenses/LICENSE-2.0

   Unless required by applicable law or agreed to in writing, software
   distributed under the License is distributed on an "AS IS" BASIS,
   WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
   See the License for the specific language governing permissions and
   limitations under the License.
*/

//! Runtime TUI implementation.
//!
//! This module is only compiled for non-test builds. Unit tests target the
//! deterministic helper modules (`cli`, `systemd`, `rows`, `journal`, etc.).

use anyhow::{Context, Result};
use crossterm::{
    event::{self, Event, KeyCode, KeyEventKind},
    execute,
    terminal::{EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode},
};
use ratatui::{
    prelude::*,
    widgets::{Block, Borders, Cell, Paragraph, Row, Table, TableState},
};
use std::{
    collections::HashMap,
    env, io,
    sync::mpsc::{self, Receiver, TryRecvError},
    thread,
    time::{Duration, Instant},
};

use crate::{
    cli::{Config, parse_args, usage},
    journal::{fetch_unit_logs, latest_log_lines_batch},
    rows::{build_rows, preserve_selection, seed_logs_from_previous, sort_rows},
    systemd::{fetch_services, filter_services, is_full_all, should_fetch_all},
    types::{DetailLogEntry, LoadPhase, UnitRow, ViewMode, WorkerMsg},
};

fn setup_terminal() -> Result<Terminal<CrosstermBackend<io::Stdout>>> {
    enable_raw_mode().context("enable_raw_mode failed")?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen).context("EnterAlternateScreen failed")?;
    let backend = CrosstermBackend::new(stdout);
    Ok(Terminal::new(backend)?)
}

fn restore_terminal(mut terminal: Terminal<CrosstermBackend<io::Stdout>>) -> Result<()> {
    disable_raw_mode().ok();
    execute!(terminal.backend_mut(), LeaveAlternateScreen).ok();
    terminal.show_cursor().ok();
    Ok(())
}

fn spawn_refresh_worker(config: Config, previous_rows: Vec<UnitRow>) -> Receiver<WorkerMsg> {
    let (tx, rx) = mpsc::channel();
    thread::spawn(move || {
        let fetch_all = should_fetch_all(&config);
        let units = match fetch_services(fetch_all).map(|u| filter_services(u, &config)) {
            Ok(units) => units,
            Err(e) => {
                let _ = tx.send(WorkerMsg::Error(e.to_string()));
                return;
            }
        };

        let mut rows = build_rows(units);
        seed_logs_from_previous(&mut rows, &previous_rows);
        sort_rows(&mut rows, is_full_all(&config));
        let total = rows.len();

        if tx.send(WorkerMsg::UnitsLoaded(rows.clone())).is_err() {
            return;
        }
        if total == 0 {
            let _ = tx.send(WorkerMsg::Finished);
            return;
        }

        const LOG_BATCH_SIZE: usize = 12;
        let mut done = 0usize;
        while done < rows.len() {
            let end = std::cmp::min(done + LOG_BATCH_SIZE, rows.len());
            let units: Vec<String> = rows[done..end].iter().map(|r| r.unit.clone()).collect();
            let logs = latest_log_lines_batch(&units).into_iter().collect();
            if tx
                .send(WorkerMsg::LogsProgress {
                    done: end,
                    total,
                    logs,
                })
                .is_err()
            {
                return;
            }
            done = end;
        }

        let _ = tx.send(WorkerMsg::Finished);
    });
    rx
}

/// Run the interactive terminal UI.
pub fn run() -> Result<()> {
    let config = parse_args(env::args())?;
    if config.show_help {
        println!("{}", usage());
        return Ok(());
    }

    let mut terminal = setup_terminal()?;

    let refresh_every = if config.refresh_secs == 0 {
        None
    } else {
        Some(Duration::from_secs(config.refresh_secs))
    };
    let mut last_refresh = Instant::now();
    let mut refresh_requested = true;
    let mut phase = LoadPhase::Idle;
    let mut worker_rx: Option<Receiver<WorkerMsg>> = None;

    let mut rows: Vec<UnitRow> = Vec::new();
    let mut row_index_by_unit: HashMap<String, usize> = HashMap::new();
    let mut selected_idx: usize = 0;
    let mut list_table_state = TableState::default();
    let mut view_mode = ViewMode::List;
    let mut detail_unit = String::new();
    let mut detail_logs: Vec<DetailLogEntry> = Vec::new();
    let mut detail_scroll: usize = 0;
    let mode_label = "services";
    let refresh_label = if config.refresh_secs == 0 {
        "off".to_string()
    } else {
        format!("{}s", config.refresh_secs)
    };
    let mut status_line = format!(
        "{mode_label}: 0 | auto-refresh: {refresh_label} | ↑/↓: move | l/enter: inspect logs | q: quit | r: refresh"
    );

    let res = (|| -> Result<()> {
        loop {
            let auto_due = refresh_every
                .map(|every| last_refresh.elapsed() >= every)
                .unwrap_or(false);
            if auto_due {
                refresh_requested = true;
            }

            terminal.draw(|f| {
                let size = f.area();
                let chunks = Layout::default()
                    .direction(Direction::Vertical)
                    .constraints([Constraint::Min(1), Constraint::Length(1)])
                    .split(size);
                match view_mode {
                    ViewMode::List => {
                        let header = Row::new([
                            Cell::from(" "),
                            Cell::from("unit"),
                            Cell::from("load"),
                            Cell::from("active"),
                            Cell::from("sub"),
                            Cell::from("description"),
                            Cell::from("log (last line)"),
                        ])
                        .style(Style::default().add_modifier(Modifier::BOLD));

                        let table_rows = rows.iter().map(|r| {
                            Row::new([
                                Cell::from(r.dot.to_string()).style(r.dot_style),
                                Cell::from(r.unit.clone()),
                                Cell::from(r.load.clone()),
                                Cell::from(r.active.clone()),
                                Cell::from(r.sub.clone()),
                                Cell::from(r.description.clone()),
                                Cell::from(r.last_log.clone()),
                            ])
                        });

                        let widths = [
                            Constraint::Length(2),
                            Constraint::Length(38),
                            Constraint::Length(8),
                            Constraint::Length(10),
                            Constraint::Length(12),
                            Constraint::Length(36),
                            Constraint::Min(20),
                        ];

                        list_table_state.select((!rows.is_empty()).then_some(selected_idx));
                        let t = Table::new(table_rows, widths)
                            .header(header)
                            .block(
                                Block::default()
                                    .borders(Borders::ALL)
                                    .title(format!("systemd {mode_label}")),
                            )
                            .row_highlight_style(Style::default().add_modifier(Modifier::REVERSED))
                            .column_spacing(1);

                        f.render_stateful_widget(t, chunks[0], &mut list_table_state);

                        let footer = Paragraph::new(status_line.clone())
                            .style(Style::default().fg(Color::DarkGray));
                        f.render_widget(footer, chunks[1]);
                    }
                    ViewMode::Detail => {
                        let unit_meta = rows
                            .iter()
                            .find(|r| r.unit == detail_unit)
                            .map(|r| format!("unit: {}", r.unit))
                            .unwrap_or_else(|| format!("unit: {}", detail_unit));

                        let header = Row::new([Cell::from("time"), Cell::from("log")])
                            .style(Style::default().add_modifier(Modifier::BOLD));
                        let log_rows = detail_logs
                            .iter()
                            .skip(detail_scroll)
                            .map(|entry| Row::new([entry.time.clone(), entry.log.clone()]));

                        let table =
                            Table::new(log_rows, [Constraint::Length(25), Constraint::Min(20)])
                                .header(header)
                                .block(
                                    Block::default()
                                        .borders(Borders::ALL)
                                        .title(format!("logs for {}", detail_unit)),
                                )
                                .column_spacing(1);
                        f.render_widget(table, chunks[0]);

                        let footer = Paragraph::new(format!(
                            "{} | logs: {} | ↑/↓: scroll | b/esc: back | q: quit | r: refresh",
                            unit_meta,
                            detail_logs.len()
                        ))
                        .style(Style::default().fg(Color::DarkGray));
                        f.render_widget(footer, chunks[1]);
                    }
                }
            })?;

            if refresh_requested && matches!(phase, LoadPhase::Idle) && worker_rx.is_none() {
                phase = LoadPhase::FetchingUnits;
                status_line = format!(
                    "{mode_label}: loading units... | auto-refresh: {refresh_label} | ↑/↓: move | l/enter: inspect logs | q: quit | r: refresh"
                );
                refresh_requested = false;
                last_refresh = Instant::now();
                worker_rx = Some(spawn_refresh_worker(config.clone(), rows.clone()));
            }

            if let Some(rx) = worker_rx.as_ref() {
                let mut clear_worker = false;
                loop {
                    match rx.try_recv() {
                        Ok(WorkerMsg::UnitsLoaded(new_rows)) => {
                            let previous_selected = rows.get(selected_idx).map(|r| r.unit.clone());
                            rows = new_rows;
                            row_index_by_unit = rows
                                .iter()
                                .enumerate()
                                .map(|(idx, row)| (row.unit.clone(), idx))
                                .collect();
                            preserve_selection(previous_selected, &rows, &mut selected_idx);
                            if rows.is_empty() {
                                status_line = format!(
                                    "{mode_label}: 0 | auto-refresh: {refresh_label} | ↑/↓: move | l/enter: inspect logs | q: quit | r: refresh",
                                );
                                phase = LoadPhase::Idle;
                            } else {
                                status_line = format!(
                                    "{mode_label}: {} | logs: 0/{} | auto-refresh: {refresh_label} | ↑/↓: move | l/enter: inspect logs | q: quit | r: refresh",
                                    rows.len(),
                                    rows.len(),
                                );
                                phase = LoadPhase::FetchingLogs;
                            }
                        }
                        Ok(WorkerMsg::LogsProgress { done, total, logs }) => {
                            for (unit, log) in logs {
                                if let Some(idx) = row_index_by_unit.get(&unit).copied()
                                    && let Some(row) = rows.get_mut(idx)
                                {
                                    row.last_log = log;
                                }
                            }
                            if done >= total {
                                status_line = format!(
                                    "{mode_label}: {} | auto-refresh: {refresh_label} | ↑/↓: move | l/enter: inspect logs | q: quit | r: refresh",
                                    rows.len(),
                                );
                            } else {
                                status_line = format!(
                                    "{mode_label}: {} | logs: {}/{} | auto-refresh: {refresh_label} | ↑/↓: move | l/enter: inspect logs | q: quit | r: refresh",
                                    rows.len(),
                                    done,
                                    total,
                                );
                            }
                            phase = LoadPhase::FetchingLogs;
                        }
                        Ok(WorkerMsg::Finished) => {
                            phase = LoadPhase::Idle;
                            status_line = format!(
                                "{mode_label}: {} | auto-refresh: {refresh_label} | ↑/↓: move | l/enter: inspect logs | q: quit | r: refresh",
                                rows.len(),
                            );
                            clear_worker = true;
                            break;
                        }
                        Ok(WorkerMsg::Error(e)) => {
                            status_line = format!(
                                "error: {e} | auto-refresh: {refresh_label} | q: quit | r: refresh",
                            );
                            phase = LoadPhase::Idle;
                            clear_worker = true;
                            break;
                        }
                        Err(TryRecvError::Empty) => break,
                        Err(TryRecvError::Disconnected) => {
                            phase = LoadPhase::Idle;
                            clear_worker = true;
                            break;
                        }
                    }
                }
                if clear_worker {
                    worker_rx = None;
                }
            }

            if event::poll(Duration::from_millis(50))?
                && let Event::Key(k) = event::read()?
                && k.kind == KeyEventKind::Press
            {
                match view_mode {
                    ViewMode::List => match k.code {
                        KeyCode::Char('q') => break,
                        KeyCode::Char('r') => {
                            refresh_requested = true;
                        }
                        KeyCode::Down => {
                            if !rows.is_empty() {
                                selected_idx = std::cmp::min(selected_idx + 1, rows.len() - 1);
                            }
                        }
                        KeyCode::Up => {
                            selected_idx = selected_idx.saturating_sub(1);
                        }
                        KeyCode::Char('l') | KeyCode::Enter => {
                            if let Some(row) = rows.get(selected_idx) {
                                detail_unit = row.unit.clone();
                                detail_logs =
                                    fetch_unit_logs(&detail_unit, 300).unwrap_or_default();
                                detail_scroll = 0;
                                view_mode = ViewMode::Detail;
                            }
                        }
                        _ => {}
                    },
                    ViewMode::Detail => match k.code {
                        KeyCode::Char('q') => break,
                        KeyCode::Char('r') => {
                            refresh_requested = true;
                            detail_logs = fetch_unit_logs(&detail_unit, 300).unwrap_or_default();
                            detail_scroll = 0;
                        }
                        KeyCode::Down => {
                            if !detail_logs.is_empty() {
                                detail_scroll =
                                    std::cmp::min(detail_scroll + 1, detail_logs.len() - 1);
                            }
                        }
                        KeyCode::Up => {
                            detail_scroll = detail_scroll.saturating_sub(1);
                        }
                        KeyCode::Esc | KeyCode::Char('b') => {
                            view_mode = ViewMode::List;
                        }
                        KeyCode::Char('l') => {
                            detail_logs = fetch_unit_logs(&detail_unit, 300).unwrap_or_default();
                            detail_scroll = 0;
                        }
                        _ => {}
                    },
                }
            }
        }
        Ok(())
    })();

    restore_terminal(terminal)?;
    res
}

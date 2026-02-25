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

//! Frame rendering for list and detail views.

use ratatui::{
    prelude::*,
    widgets::{Block, Borders, Cell, Paragraph, Row, Table, TableState},
};

use crate::{
    cli::Config,
    types::{DetailState, LoadPhase, UnitRow, ViewMode},
};

/// Render one UI frame from runtime state.
#[allow(clippy::too_many_arguments)]
pub fn draw_frame(
    f: &mut Frame<'_>,
    view_mode: ViewMode,
    mode_label: &str,
    rows: &[UnitRow],
    selected_idx: usize,
    list_table_state: &mut TableState,
    detail: &DetailState,
    phase: LoadPhase,
    loaded_once: bool,
    last_load_error: bool,
    last_load_error_message: Option<&str>,
    refresh_requested: bool,
    status_line: &str,
    config: &Config,
) {
    let size = f.area();
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Min(1), Constraint::Length(1)])
        .split(size);
    match view_mode {
        ViewMode::List => {
            if rows.is_empty() {
                let block = Block::default()
                    .borders(Borders::ALL)
                    .title(format!("systemd {mode_label}"));
                let inner = block.inner(chunks[0]);
                f.render_widget(block, chunks[0]);

                let message = if matches!(phase, LoadPhase::Idle)
                    && loaded_once
                    && !last_load_error
                    && !refresh_requested
                {
                    format!(
                        "       .----.   @   @\n     / .-\"-.`.  \\v/\n     | | '\\ \\ \\_/ )\n  ,-\\ `-.' /.'  /\n'---`----'----'\n\nNo units matched filters: load={}, active={}, sub={}.",
                        config.load_filter, config.active_filter, config.sub_filter
                    )
                } else if last_load_error && matches!(phase, LoadPhase::Idle) {
                    match last_load_error_message {
                        Some(err) if !err.trim().is_empty() => {
                            format!("Last refresh failed. Press r to retry.\n\n{err}")
                        }
                        _ => "Last refresh failed. Press r to retry.".to_string(),
                    }
                } else {
                    "Loading units and logs...".to_string()
                };
                let p = Paragraph::new(message)
                    .alignment(Alignment::Center)
                    .style(Style::default().fg(Color::DarkGray));
                f.render_widget(p, inner);
            } else {
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

                f.render_stateful_widget(t, chunks[0], list_table_state);
            }

            let footer_text =
                if !rows.is_empty() && last_load_error && matches!(phase, LoadPhase::Idle) {
                    match last_load_error_message {
                        Some(err) if !err.trim().is_empty() => {
                            format!(
                                "refresh failed (stale data): {} | r: refresh | q: quit",
                                err
                            )
                        }
                        _ => "refresh failed (stale data) | r: refresh | q: quit".to_string(),
                    }
                } else {
                    status_line.to_string()
                };
            let footer = Paragraph::new(footer_text).style(Style::default().fg(Color::DarkGray));
            f.render_widget(footer, chunks[1]);
        }
        ViewMode::Detail => {
            let unit_meta = rows
                .iter()
                .find(|r| r.unit == detail.unit)
                .map(|r| format!("unit: {}", r.unit))
                .unwrap_or_else(|| format!("unit: {}", detail.unit));

            let header = Row::new([Cell::from("time"), Cell::from("log")])
                .style(Style::default().add_modifier(Modifier::BOLD));
            let log_rows = detail
                .logs
                .iter()
                .skip(detail.scroll)
                .map(|entry| Row::new([entry.time.clone(), entry.log.clone()]));

            let table = Table::new(log_rows, [Constraint::Length(25), Constraint::Min(20)])
                .header(header)
                .block(
                    Block::default()
                        .borders(Borders::ALL)
                        .title(format!("logs for {}", detail.unit)),
                )
                .column_spacing(1);
            f.render_widget(table, chunks[0]);

            let detail_status = if detail.loading {
                "loading logs...".to_string()
            } else if let Some(err) = &detail.error {
                format!("error: {err}")
            } else {
                format!("logs: {}", detail.logs.len())
            };
            let footer = Paragraph::new(format!(
                "{} | {} | ↑/↓: scroll | b/esc: back | r: refresh | q: quit",
                unit_meta, detail_status
            ))
            .style(Style::default().fg(Color::DarkGray));
            f.render_widget(footer, chunks[1]);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::{Terminal, backend::TestBackend};

    fn sample_config() -> Config {
        Config {
            load_filter: "loaded".to_string(),
            active_filter: "active".to_string(),
            sub_filter: "running".to_string(),
            show_help: false,
            show_version: false,
            scope: crate::types::Scope::System,
        }
    }

    fn sample_row() -> UnitRow {
        UnitRow {
            dot: '.',
            dot_style: Style::default(),
            unit: "a.service".to_string(),
            load: "loaded".to_string(),
            active: "active".to_string(),
            sub: "running".to_string(),
            description: "A".to_string(),
            last_log: "log".to_string(),
        }
    }

    #[test]
    fn draw_frame_renders_list_mode_with_rows() {
        let backend = TestBackend::new(120, 30);
        let mut terminal = Terminal::new(backend).expect("terminal");
        let mut state = TableState::default();
        let detail = DetailState::default();
        terminal
            .draw(|f| {
                draw_frame(
                    f,
                    ViewMode::List,
                    "services",
                    &[sample_row()],
                    0,
                    &mut state,
                    &detail,
                    LoadPhase::Idle,
                    true,
                    false,
                    None,
                    false,
                    "services: 1",
                    &sample_config(),
                )
            })
            .expect("draw");
    }

    #[test]
    fn draw_frame_renders_detail_mode() {
        let backend = TestBackend::new(120, 30);
        let mut terminal = Terminal::new(backend).expect("terminal");
        let mut state = TableState::default();
        let mut detail = DetailState::default();
        detail.unit = "a.service".to_string();
        detail.logs.push(crate::types::DetailLogEntry {
            time: "t".to_string(),
            log: "line".to_string(),
        });
        terminal
            .draw(|f| {
                draw_frame(
                    f,
                    ViewMode::Detail,
                    "services",
                    &[sample_row()],
                    0,
                    &mut state,
                    &detail,
                    LoadPhase::Idle,
                    true,
                    false,
                    None,
                    false,
                    "services: 1",
                    &sample_config(),
                )
            })
            .expect("draw");
    }

    #[test]
    fn draw_frame_renders_empty_no_match_and_error_states() {
        let backend = TestBackend::new(120, 30);
        let mut terminal = Terminal::new(backend).expect("terminal");
        let mut state = TableState::default();
        let detail = DetailState::default();

        terminal
            .draw(|f| {
                draw_frame(
                    f,
                    ViewMode::List,
                    "services",
                    &[],
                    0,
                    &mut state,
                    &detail,
                    LoadPhase::Idle,
                    true,
                    false,
                    None,
                    false,
                    "services: 0",
                    &sample_config(),
                )
            })
            .expect("draw");

        terminal
            .draw(|f| {
                draw_frame(
                    f,
                    ViewMode::List,
                    "services",
                    &[],
                    0,
                    &mut state,
                    &detail,
                    LoadPhase::Idle,
                    true,
                    true,
                    Some("boom"),
                    false,
                    "services: 0",
                    &sample_config(),
                )
            })
            .expect("draw");
    }

    #[test]
    fn draw_frame_renders_stale_footer_with_rows() {
        let backend = TestBackend::new(120, 30);
        let mut terminal = Terminal::new(backend).expect("terminal");
        let mut state = TableState::default();
        let detail = DetailState::default();
        terminal
            .draw(|f| {
                draw_frame(
                    f,
                    ViewMode::List,
                    "services",
                    &[sample_row()],
                    0,
                    &mut state,
                    &detail,
                    LoadPhase::Idle,
                    true,
                    true,
                    Some("stale"),
                    false,
                    "services: 1",
                    &sample_config(),
                )
            })
            .expect("draw");
    }
}

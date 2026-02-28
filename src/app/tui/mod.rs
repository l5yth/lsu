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

//! Runtime TUI orchestration.
//!
//! Responsibilities are split across submodules:
//! - `workers`: background loading for list/detail data
//! - `render`: frame rendering for list/detail views
//! - `input`: key translation into view-independent commands
//! - `state`: pure status text helpers

#[cfg(feature = "debug_tui")]
mod debug;
mod input;
mod render;
mod state;
mod workers;

#[cfg(not(test))]
use anyhow::Context;
use anyhow::Result;
#[cfg(not(test))]
use crossterm::{
    event::{self, Event, KeyEventKind},
    execute,
    terminal::{EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode},
};
#[cfg(not(test))]
use ratatui::{prelude::*, widgets::TableState};
#[cfg(not(test))]
use std::{
    collections::HashMap,
    env, io,
    sync::mpsc::{Receiver, TryRecvError},
    time::Duration,
};

#[cfg(not(test))]
use crate::{
    cli::{parse_args, usage, version_text},
    rows::preserve_selection,
    systemd::{select_enable_disable_action, select_start_stop_action},
    types::{ConfirmationState, DetailState, LoadPhase, UnitAction, UnitRow, ViewMode, WorkerMsg},
};

#[cfg(not(test))]
use self::{
    input::{UiCommand, map_confirmation_key, map_key},
    render::draw_frame,
    state::{
        MODE_LABEL, action_complete_status_text, action_error_status_text,
        action_resolution_error_status_text, action_status_text, list_status_text,
        loading_units_status_text, stale_status_text,
    },
    workers::{spawn_detail_worker, spawn_refresh_worker, spawn_unit_action_worker},
};

#[cfg(not(test))]
fn setup_terminal() -> Result<Terminal<CrosstermBackend<io::Stdout>>> {
    enable_raw_mode().context("enable_raw_mode failed")?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen).context("EnterAlternateScreen failed")?;
    let backend = CrosstermBackend::new(stdout);
    Ok(Terminal::new(backend)?)
}

#[cfg(not(test))]
fn restore_terminal(mut terminal: Terminal<CrosstermBackend<io::Stdout>>) -> Result<()> {
    disable_raw_mode().ok();
    execute!(terminal.backend_mut(), LeaveAlternateScreen).ok();
    terminal.show_cursor().ok();
    Ok(())
}

/// Run the interactive terminal UI.
#[cfg(not(test))]
pub fn run() -> Result<()> {
    let config = parse_args(env::args())?;
    if config.show_version {
        println!("{}", version_text());
        return Ok(());
    }
    if config.show_help {
        println!("{}", usage());
        return Ok(());
    }

    let mut terminal = setup_terminal()?;

    let mut refresh_requested = true;
    let mut phase = LoadPhase::Idle;
    let mut worker_rx: Option<Receiver<WorkerMsg>> = None;
    let mut detail_worker_rx: Option<Receiver<WorkerMsg>> = None;
    let mut action_worker_rx: Option<Receiver<WorkerMsg>> = None;
    let mut loaded_once = false;
    let mut last_load_error = false;
    let mut last_load_error_message: Option<String> = None;

    let mut rows: Vec<UnitRow> = Vec::new();
    let mut row_index_by_unit: HashMap<String, usize> = HashMap::new();
    let mut selected_idx: usize = 0;
    let mut list_table_state = TableState::default();
    let mut view_mode = ViewMode::List;
    let mut detail = DetailState::default();
    let mut confirmation: Option<ConfirmationState> = None;
    let mut status_line = list_status_text(0, None);

    let res = (|| -> Result<()> {
        loop {
            terminal.draw(|f| {
                draw_frame(
                    f,
                    view_mode,
                    MODE_LABEL,
                    &rows,
                    selected_idx,
                    &mut list_table_state,
                    &detail,
                    phase,
                    loaded_once,
                    last_load_error,
                    last_load_error_message.as_deref(),
                    refresh_requested,
                    &status_line,
                    confirmation.as_ref(),
                    &config,
                );
            })?;

            if refresh_requested && matches!(phase, LoadPhase::Idle) && worker_rx.is_none() {
                phase = LoadPhase::FetchingUnits;
                status_line = loading_units_status_text();
                refresh_requested = false;
                worker_rx = Some(spawn_refresh_worker(config.clone(), rows.clone()));
            }

            if let Some(rx) = worker_rx.as_ref() {
                let mut clear_worker = false;
                loop {
                    match rx.try_recv() {
                        Ok(WorkerMsg::UnitsLoaded(new_rows)) => {
                            loaded_once = true;
                            last_load_error = false;
                            last_load_error_message = None;
                            let previous_selected = rows.get(selected_idx).map(|r| r.unit.clone());
                            rows = new_rows;
                            row_index_by_unit = rows
                                .iter()
                                .enumerate()
                                .map(|(idx, row)| (row.unit.clone(), idx))
                                .collect();
                            preserve_selection(previous_selected, &rows, &mut selected_idx);
                            if rows.is_empty() {
                                status_line = list_status_text(0, None);
                                phase = LoadPhase::Idle;
                            } else {
                                status_line = list_status_text(rows.len(), Some((0, rows.len())));
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
                            status_line = list_status_text(rows.len(), Some((done, total)));
                            phase = LoadPhase::FetchingLogs;
                        }
                        Ok(WorkerMsg::Finished) => {
                            phase = LoadPhase::Idle;
                            status_line = list_status_text(rows.len(), None);
                            clear_worker = true;
                            break;
                        }
                        Ok(WorkerMsg::Error(e)) => {
                            last_load_error = true;
                            last_load_error_message = Some(e);
                            status_line = stale_status_text(rows.len());
                            phase = LoadPhase::Idle;
                            clear_worker = true;
                            break;
                        }
                        Ok(
                            WorkerMsg::DetailLogsLoaded { .. }
                            | WorkerMsg::DetailLogsError { .. }
                            | WorkerMsg::UnitActionComplete { .. }
                            | WorkerMsg::UnitActionError { .. },
                        ) => continue,
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

            if let Some(rx) = detail_worker_rx.as_ref() {
                let mut clear_detail_worker = false;
                loop {
                    match rx.try_recv() {
                        Ok(WorkerMsg::DetailLogsLoaded {
                            unit,
                            request_id,
                            logs,
                        }) => {
                            let _ = detail.apply_loaded(request_id, &unit, logs);
                            clear_detail_worker = true;
                            break;
                        }
                        Ok(WorkerMsg::DetailLogsError {
                            unit,
                            request_id,
                            error,
                        }) => {
                            let _ = detail.apply_error(request_id, &unit, error);
                            clear_detail_worker = true;
                            break;
                        }
                        Ok(_) => continue,
                        Err(TryRecvError::Empty) => break,
                        Err(TryRecvError::Disconnected) => {
                            clear_detail_worker = true;
                            break;
                        }
                    }
                }
                if clear_detail_worker {
                    detail_worker_rx = None;
                }
            }

            if let Some(rx) = action_worker_rx.as_ref() {
                let mut clear_action_worker = false;
                loop {
                    match rx.try_recv() {
                        Ok(WorkerMsg::UnitActionComplete { unit, action }) => {
                            status_line = action_complete_status_text(rows.len(), action, &unit);
                            refresh_requested = true;
                            clear_action_worker = true;
                            break;
                        }
                        Ok(WorkerMsg::UnitActionError {
                            unit,
                            action,
                            error,
                        }) => {
                            status_line =
                                action_error_status_text(rows.len(), action, &unit, &error);
                            clear_action_worker = true;
                            break;
                        }
                        Ok(_) => continue,
                        Err(TryRecvError::Empty) => break,
                        Err(TryRecvError::Disconnected) => {
                            clear_action_worker = true;
                            break;
                        }
                    }
                }
                if clear_action_worker {
                    action_worker_rx = None;
                }
            }

            if event::poll(Duration::from_millis(50))?
                && let Event::Key(k) = event::read()?
                && k.kind == KeyEventKind::Press
            {
                if let Some(cmd) = confirmation
                    .as_ref()
                    .and_then(|pending| map_confirmation_key(pending.kind, k.code))
                {
                    match cmd {
                        UiCommand::Confirm => {
                            if action_worker_rx.is_none()
                                && let Some(pending) = confirmation.take()
                                && let Some(action) = pending.confirmed_action()
                            {
                                let status_confirmation =
                                    ConfirmationState::confirm_action(action, pending.unit.clone());
                                status_line = action_status_text(rows.len(), &status_confirmation);
                                action_worker_rx =
                                    Some(spawn_unit_action_worker(&config, pending.unit, action));
                            }
                        }
                        UiCommand::ChooseRestart => {
                            if action_worker_rx.is_none()
                                && let Some(pending) = confirmation.take()
                            {
                                let status_confirmation = ConfirmationState::confirm_action(
                                    UnitAction::Restart,
                                    pending.unit.clone(),
                                );
                                status_line = action_status_text(rows.len(), &status_confirmation);
                                action_worker_rx = Some(spawn_unit_action_worker(
                                    &config,
                                    pending.unit,
                                    UnitAction::Restart,
                                ));
                            }
                        }
                        UiCommand::ChooseStop => {
                            if action_worker_rx.is_none()
                                && let Some(pending) = confirmation.take()
                            {
                                let status_confirmation = ConfirmationState::confirm_action(
                                    UnitAction::Stop,
                                    pending.unit.clone(),
                                );
                                status_line = action_status_text(rows.len(), &status_confirmation);
                                action_worker_rx = Some(spawn_unit_action_worker(
                                    &config,
                                    pending.unit,
                                    UnitAction::Stop,
                                ));
                            }
                        }
                        UiCommand::Cancel => {
                            confirmation = None;
                            status_line = list_status_text(rows.len(), None);
                        }
                        _ => {}
                    }
                } else if confirmation.is_none()
                    && let Some(cmd) = map_key(view_mode, k.code)
                {
                    match cmd {
                        UiCommand::Quit => break,
                        UiCommand::Refresh => {
                            refresh_requested = true;
                            if matches!(view_mode, ViewMode::Detail)
                                && detail_worker_rx.is_none()
                                && !detail.loading
                                && let Some(request_id) = detail.refresh()
                            {
                                detail_worker_rx = Some(spawn_detail_worker(
                                    &config,
                                    detail.unit.clone(),
                                    request_id,
                                ));
                            }
                        }
                        UiCommand::MoveDown => match view_mode {
                            ViewMode::List => {
                                if !rows.is_empty() {
                                    selected_idx = std::cmp::min(selected_idx + 1, rows.len() - 1);
                                }
                            }
                            ViewMode::Detail => {
                                if !detail.logs.is_empty() {
                                    detail.scroll =
                                        std::cmp::min(detail.scroll + 1, detail.logs.len() - 1);
                                }
                            }
                        },
                        UiCommand::MoveUp => match view_mode {
                            ViewMode::List => selected_idx = selected_idx.saturating_sub(1),
                            ViewMode::Detail => detail.scroll = detail.scroll.saturating_sub(1),
                        },
                        UiCommand::OpenDetail => {
                            if let Some(row) = rows.get(selected_idx) {
                                let request_id = detail.begin_for_unit(row.unit.clone());
                                detail_worker_rx = Some(spawn_detail_worker(
                                    &config,
                                    detail.unit.clone(),
                                    request_id,
                                ));
                                view_mode = ViewMode::Detail;
                            }
                        }
                        UiCommand::BackToList => view_mode = ViewMode::List,
                        UiCommand::RefreshDetail => {
                            if detail_worker_rx.is_none()
                                && !detail.loading
                                && let Some(request_id) = detail.refresh()
                            {
                                detail_worker_rx = Some(spawn_detail_worker(
                                    &config,
                                    detail.unit.clone(),
                                    request_id,
                                ));
                            }
                        }
                        UiCommand::RequestStartStop => {
                            if action_worker_rx.is_none()
                                && let Some(row) = rows.get(selected_idx)
                            {
                                match select_start_stop_action(config.scope, &row.unit) {
                                    Ok(UnitAction::Start) => {
                                        confirmation = Some(ConfirmationState::confirm_action(
                                            UnitAction::Start,
                                            row.unit.clone(),
                                        ));
                                    }
                                    Ok(UnitAction::Stop) => {
                                        confirmation = Some(ConfirmationState::restart_or_stop(
                                            row.unit.clone(),
                                        ));
                                    }
                                    Ok(action) => {
                                        confirmation = Some(ConfirmationState::confirm_action(
                                            action,
                                            row.unit.clone(),
                                        ));
                                    }
                                    Err(e) => {
                                        status_line = action_resolution_error_status_text(
                                            rows.len(),
                                            &row.unit,
                                            &e.to_string(),
                                        );
                                    }
                                }
                            }
                        }
                        UiCommand::RequestEnableDisable => {
                            if action_worker_rx.is_none()
                                && let Some(row) = rows.get(selected_idx)
                            {
                                match select_enable_disable_action(config.scope, &row.unit) {
                                    Ok(action) => {
                                        confirmation = Some(ConfirmationState::confirm_action(
                                            action,
                                            row.unit.clone(),
                                        ));
                                    }
                                    Err(e) => {
                                        status_line = action_resolution_error_status_text(
                                            rows.len(),
                                            &row.unit,
                                            &e.to_string(),
                                        );
                                    }
                                }
                            }
                        }
                        UiCommand::Confirm
                        | UiCommand::Cancel
                        | UiCommand::ChooseRestart
                        | UiCommand::ChooseStop => {}
                    }
                }
            }
        }
        Ok(())
    })();

    restore_terminal(terminal)?;
    res
}

#[cfg(test)]
/// Test-build runner stub for the TUI runtime module.
pub fn run() -> Result<()> {
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::input::UiCommand;
    use super::state::{list_status_text, stale_status_text};
    use crate::rows::preserve_selection;
    use crate::types::{DetailState, LoadPhase, UnitRow, ViewMode, WorkerMsg};
    use ratatui::prelude::Style;
    use std::collections::HashMap;

    struct TestUiState {
        view_mode: ViewMode,
        rows: Vec<UnitRow>,
        selected_idx: usize,
        detail: DetailState,
        detail_worker_active: bool,
        refresh_requested: bool,
    }

    fn row(unit: &str) -> UnitRow {
        UnitRow {
            dot: '.',
            dot_style: Style::default(),
            unit: unit.to_string(),
            load: "loaded".to_string(),
            active: "active".to_string(),
            sub: "running".to_string(),
            description: "x".to_string(),
            last_log: String::new(),
        }
    }

    fn apply_command(state: &mut TestUiState, cmd: UiCommand) -> bool {
        match cmd {
            UiCommand::Quit => return true,
            UiCommand::Refresh => {
                state.refresh_requested = true;
                if matches!(state.view_mode, ViewMode::Detail)
                    && !state.detail_worker_active
                    && !state.detail.loading
                    && state.detail.refresh().is_some()
                {
                    state.detail_worker_active = true;
                }
            }
            UiCommand::MoveDown => match state.view_mode {
                ViewMode::List => {
                    if !state.rows.is_empty() {
                        state.selected_idx =
                            std::cmp::min(state.selected_idx + 1, state.rows.len() - 1);
                    }
                }
                ViewMode::Detail => {
                    if !state.detail.logs.is_empty() {
                        state.detail.scroll =
                            std::cmp::min(state.detail.scroll + 1, state.detail.logs.len() - 1);
                    }
                }
            },
            UiCommand::MoveUp => match state.view_mode {
                ViewMode::List => state.selected_idx = state.selected_idx.saturating_sub(1),
                ViewMode::Detail => state.detail.scroll = state.detail.scroll.saturating_sub(1),
            },
            UiCommand::OpenDetail => {
                if let Some(r) = state.rows.get(state.selected_idx) {
                    let _ = state.detail.begin_for_unit(r.unit.clone());
                    state.detail_worker_active = true;
                    state.view_mode = ViewMode::Detail;
                }
            }
            UiCommand::BackToList => state.view_mode = ViewMode::List,
            UiCommand::RefreshDetail => {
                if !state.detail_worker_active
                    && !state.detail.loading
                    && state.detail.refresh().is_some()
                {
                    state.detail_worker_active = true;
                }
            }
            UiCommand::RequestStartStop
            | UiCommand::RequestEnableDisable
            | UiCommand::Confirm
            | UiCommand::Cancel
            | UiCommand::ChooseRestart
            | UiCommand::ChooseStop => {}
        }
        false
    }

    struct ListWorkerTestState {
        rows: Vec<UnitRow>,
        row_index_by_unit: HashMap<String, usize>,
        selected_idx: usize,
        loaded_once: bool,
        phase: LoadPhase,
        status_line: String,
        last_load_error: bool,
        last_load_error_message: Option<String>,
    }

    fn apply_list_worker_msg(state: &mut ListWorkerTestState, msg: WorkerMsg) -> bool {
        match msg {
            WorkerMsg::UnitsLoaded(new_rows) => {
                state.loaded_once = true;
                state.last_load_error = false;
                state.last_load_error_message = None;
                let previous_selected = state.rows.get(state.selected_idx).map(|r| r.unit.clone());
                state.rows = new_rows;
                state.row_index_by_unit = state
                    .rows
                    .iter()
                    .enumerate()
                    .map(|(idx, row)| (row.unit.clone(), idx))
                    .collect();
                preserve_selection(previous_selected, &state.rows, &mut state.selected_idx);
                if state.rows.is_empty() {
                    state.status_line = list_status_text(0, None);
                    state.phase = LoadPhase::Idle;
                } else {
                    state.status_line =
                        list_status_text(state.rows.len(), Some((0, state.rows.len())));
                    state.phase = LoadPhase::FetchingLogs;
                }
                false
            }
            WorkerMsg::LogsProgress { done, total, logs } => {
                for (unit, log) in logs {
                    if let Some(idx) = state.row_index_by_unit.get(&unit).copied()
                        && let Some(row) = state.rows.get_mut(idx)
                    {
                        row.last_log = log;
                    }
                }
                state.status_line = list_status_text(state.rows.len(), Some((done, total)));
                state.phase = LoadPhase::FetchingLogs;
                false
            }
            WorkerMsg::Finished => {
                state.phase = LoadPhase::Idle;
                state.status_line = list_status_text(state.rows.len(), None);
                true
            }
            WorkerMsg::Error(e) => {
                state.last_load_error = true;
                state.last_load_error_message = Some(e);
                state.status_line = stale_status_text(state.rows.len());
                state.phase = LoadPhase::Idle;
                true
            }
            WorkerMsg::DetailLogsLoaded { .. }
            | WorkerMsg::DetailLogsError { .. }
            | WorkerMsg::UnitActionComplete { .. }
            | WorkerMsg::UnitActionError { .. } => false,
        }
    }

    #[test]
    fn test_run_stub_is_ok() {
        assert!(super::run().is_ok());
    }

    #[test]
    fn apply_command_covers_list_and_detail_transitions() {
        let mut state = TestUiState {
            view_mode: ViewMode::List,
            rows: vec![row("a.service"), row("b.service")],
            selected_idx: 0,
            detail: DetailState::default(),
            detail_worker_active: false,
            refresh_requested: false,
        };
        assert!(!apply_command(&mut state, UiCommand::MoveDown));
        assert_eq!(state.selected_idx, 1);
        assert!(!apply_command(&mut state, UiCommand::OpenDetail));
        assert!(matches!(state.view_mode, ViewMode::Detail));
        state.detail_worker_active = false;
        state.detail.loading = false;
        assert!(!apply_command(&mut state, UiCommand::RefreshDetail));
        assert!(state.detail_worker_active);
        state.detail_worker_active = false;
        state.detail.loading = false;
        assert!(!apply_command(&mut state, UiCommand::Refresh));
        assert!(state.refresh_requested);
        assert!(apply_command(&mut state, UiCommand::Quit));
    }

    #[test]
    fn apply_list_worker_msg_covers_all_variants() {
        let mut state = ListWorkerTestState {
            rows: vec![row("a.service")],
            row_index_by_unit: HashMap::from([(String::from("a.service"), 0usize)]),
            selected_idx: 0,
            loaded_once: false,
            phase: LoadPhase::Idle,
            status_line: String::new(),
            last_load_error: false,
            last_load_error_message: None,
        };

        assert!(!apply_list_worker_msg(
            &mut state,
            WorkerMsg::UnitsLoaded(vec![row("x.service")]),
        ));
        assert!(state.loaded_once);
        assert!(matches!(state.phase, LoadPhase::FetchingLogs));

        assert!(!apply_list_worker_msg(
            &mut state,
            WorkerMsg::LogsProgress {
                done: 1,
                total: 1,
                logs: vec![(String::from("x.service"), String::from("ok"))],
            },
        ));
        assert_eq!(state.rows[0].last_log, "ok");

        assert!(apply_list_worker_msg(&mut state, WorkerMsg::Finished));
        assert!(matches!(state.phase, LoadPhase::Idle));

        assert!(apply_list_worker_msg(
            &mut state,
            WorkerMsg::Error("boom".to_string()),
        ));
        assert!(state.last_load_error);
        assert_eq!(state.last_load_error_message.as_deref(), Some("boom"));
    }
}

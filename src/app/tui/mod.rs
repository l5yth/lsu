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
use std::time::{Duration, Instant};
#[cfg(not(test))]
use std::{
    collections::HashMap,
    env, io,
    sync::mpsc::{Receiver, TryRecvError},
};

#[cfg(not(test))]
use crate::{
    cli::{parse_args, usage, version_text},
    rows::preserve_selection,
    systemd::run_unit_action,
    types::{
        ActionResolutionRequest, ConfirmationState, DetailState, LoadPhase, UnitAction, UnitRow,
        ViewMode, WorkerMsg,
    },
};

#[cfg(not(test))]
use self::{
    input::{UiCommand, map_confirmation_key, map_key},
    render::draw_frame,
    state::{
        MODE_LABEL, action_authenticating_status_text, action_error_status_text,
        action_queued_status_text, action_resolution_status_text, list_status_text,
        loading_units_status_text, stale_status_text,
    },
    workers::{spawn_action_resolution_worker, spawn_detail_worker, spawn_refresh_worker},
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

/// Suspend the TUI so external processes (e.g. polkit auth agents) can use the terminal.
#[cfg(not(test))]
fn suspend_terminal(terminal: &mut Terminal<CrosstermBackend<io::Stdout>>) -> Result<()> {
    disable_raw_mode().context("disable_raw_mode failed")?;
    execute!(terminal.backend_mut(), LeaveAlternateScreen)
        .context("LeaveAlternateScreen failed")?;
    terminal.show_cursor().context("show_cursor failed")?;
    Ok(())
}

/// Resume the TUI after a suspension, clearing any output left by external processes.
#[cfg(not(test))]
fn resume_terminal(terminal: &mut Terminal<CrosstermBackend<io::Stdout>>) -> Result<()> {
    execute!(terminal.backend_mut(), EnterAlternateScreen)
        .context("EnterAlternateScreen failed")?;
    enable_raw_mode().context("enable_raw_mode failed")?;
    terminal.clear().context("terminal clear failed")?;
    Ok(())
}

/// Suspend the terminal, run a unit action with authentication support, resume, and update status.
///
/// Returns `Err` only if terminal suspension or resumption fails; action errors are reported
/// via `status_line` rather than propagated.
#[cfg(not(test))]
#[allow(clippy::too_many_arguments)]
fn run_confirmed_action(
    terminal: &mut Terminal<CrosstermBackend<io::Stdout>>,
    scope: crate::types::Scope,
    unit: &str,
    action: UnitAction,
    rows_len: usize,
    status_line: &mut String,
    status_line_overrides_stale: &mut bool,
    refresh_requested: &mut bool,
    queued_action_refresh_deadline: &mut Option<Instant>,
) -> Result<()> {
    set_status_line(
        status_line,
        status_line_overrides_stale,
        action_authenticating_status_text(rows_len, action, unit),
        true,
    );
    suspend_terminal(terminal)?;
    let result = run_unit_action(scope, unit, action);
    resume_terminal(terminal)?;
    let refresh_was_requested = *refresh_requested;
    match result {
        Ok(()) => {
            *refresh_requested = true;
            set_status_line(
                status_line,
                status_line_overrides_stale,
                action_queued_status_text(rows_len, action, unit),
                true,
            );
        }
        Err(e) => {
            set_status_line(
                status_line,
                status_line_overrides_stale,
                action_error_status_text(rows_len, action, unit, &e.to_string()),
                true,
            );
        }
    }
    defer_queued_action_refresh(
        refresh_requested,
        queued_action_refresh_deadline,
        refresh_was_requested,
        Instant::now(),
    );
    Ok(())
}

fn set_status_line(
    status_line: &mut String,
    status_line_overrides_stale: &mut bool,
    text: String,
    overrides_stale: bool,
) {
    *status_line = text;
    *status_line_overrides_stale = overrides_stale;
}

fn set_list_status_line(
    list_status_line: &mut String,
    list_status_line_overrides_stale: &mut bool,
    status_line: &mut String,
    status_line_overrides_stale: &mut bool,
    text: String,
    overrides_stale: bool,
) {
    *list_status_line = text.clone();
    *list_status_line_overrides_stale = overrides_stale;
    set_status_line(
        status_line,
        status_line_overrides_stale,
        text,
        overrides_stale,
    );
}

fn restore_list_status_line(
    list_status_line: &str,
    list_status_line_overrides_stale: bool,
    status_line: &mut String,
    status_line_overrides_stale: &mut bool,
) {
    set_status_line(
        status_line,
        status_line_overrides_stale,
        list_status_line.to_string(),
        list_status_line_overrides_stale,
    );
}

fn cancel_pending_action_resolution<T>(
    action_resolution_worker: &mut Option<T>,
    list_status_line: &str,
    list_status_line_overrides_stale: bool,
    status_line: &mut String,
    status_line_overrides_stale: &mut bool,
) -> bool {
    if action_resolution_worker.take().is_none() {
        return false;
    }
    restore_list_status_line(
        list_status_line,
        list_status_line_overrides_stale,
        status_line,
        status_line_overrides_stale,
    );
    true
}

struct ActionResolutionUiState<'a> {
    list_status_line: &'a str,
    list_status_line_overrides_stale: bool,
    rows_len: usize,
    view_mode: crate::types::ViewMode,
    selected_unit: Option<&'a str>,
}

fn apply_action_resolution_msg(
    confirmation: &mut Option<crate::types::ConfirmationState>,
    status_line: &mut String,
    status_line_overrides_stale: &mut bool,
    ui: ActionResolutionUiState<'_>,
    msg: crate::types::WorkerMsg,
) -> bool {
    match msg {
        crate::types::WorkerMsg::ActionConfirmationReady {
            unit,
            confirmation: resolved,
        } => {
            if !matches!(ui.view_mode, crate::types::ViewMode::List)
                || ui.selected_unit != Some(unit.as_str())
            {
                restore_list_status_line(
                    ui.list_status_line,
                    ui.list_status_line_overrides_stale,
                    status_line,
                    status_line_overrides_stale,
                );
                return true;
            }
            *confirmation = Some(resolved);
            restore_list_status_line(
                ui.list_status_line,
                ui.list_status_line_overrides_stale,
                status_line,
                status_line_overrides_stale,
            );
            true
        }
        crate::types::WorkerMsg::ActionResolutionError { unit, error } => {
            if !matches!(ui.view_mode, crate::types::ViewMode::List)
                || ui.selected_unit != Some(unit.as_str())
            {
                restore_list_status_line(
                    ui.list_status_line,
                    ui.list_status_line_overrides_stale,
                    status_line,
                    status_line_overrides_stale,
                );
                return true;
            }
            set_status_line(
                status_line,
                status_line_overrides_stale,
                self::state::action_resolution_error_status_text(ui.rows_len, &unit, &error),
                true,
            );
            true
        }
        _ => false,
    }
}

const UNIT_ACTION_REFRESH_DELAY: Duration = Duration::from_millis(500);

fn defer_queued_action_refresh(
    refresh_requested: &mut bool,
    queued_action_refresh_deadline: &mut Option<Instant>,
    refresh_was_requested: bool,
    now: Instant,
) {
    if !refresh_was_requested && *refresh_requested {
        *refresh_requested = false;
        *queued_action_refresh_deadline = Some(now + UNIT_ACTION_REFRESH_DELAY);
    }
}

fn activate_queued_action_refresh(
    refresh_requested: &mut bool,
    queued_action_refresh_deadline: &mut Option<Instant>,
    now: Instant,
) {
    if let Some(deadline) = queued_action_refresh_deadline
        && *deadline <= now
    {
        *refresh_requested = true;
        *queued_action_refresh_deadline = None;
    }
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
    let mut action_resolution_worker_rx: Option<Receiver<WorkerMsg>> = None;
    let mut queued_action_refresh_deadline: Option<Instant> = None;
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
    let mut status_line_overrides_stale = false;
    let mut status_line = list_status_text(0, None);
    let mut list_status_line = status_line.clone();
    let mut list_status_line_overrides_stale = false;

    let res = (|| -> Result<()> {
        loop {
            activate_queued_action_refresh(
                &mut refresh_requested,
                &mut queued_action_refresh_deadline,
                Instant::now(),
            );

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
                    status_line_overrides_stale,
                    confirmation.as_ref(),
                    &config,
                );
            })?;

            if refresh_requested && matches!(phase, LoadPhase::Idle) && worker_rx.is_none() {
                phase = LoadPhase::FetchingUnits;
                set_list_status_line(
                    &mut list_status_line,
                    &mut list_status_line_overrides_stale,
                    &mut status_line,
                    &mut status_line_overrides_stale,
                    loading_units_status_text(),
                    false,
                );
                queued_action_refresh_deadline = None;
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
                                set_list_status_line(
                                    &mut list_status_line,
                                    &mut list_status_line_overrides_stale,
                                    &mut status_line,
                                    &mut status_line_overrides_stale,
                                    list_status_text(0, None),
                                    false,
                                );
                                phase = LoadPhase::Idle;
                            } else {
                                set_list_status_line(
                                    &mut list_status_line,
                                    &mut list_status_line_overrides_stale,
                                    &mut status_line,
                                    &mut status_line_overrides_stale,
                                    list_status_text(rows.len(), Some((0, rows.len()))),
                                    false,
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
                            set_list_status_line(
                                &mut list_status_line,
                                &mut list_status_line_overrides_stale,
                                &mut status_line,
                                &mut status_line_overrides_stale,
                                list_status_text(rows.len(), Some((done, total))),
                                false,
                            );
                            phase = LoadPhase::FetchingLogs;
                        }
                        Ok(WorkerMsg::Finished) => {
                            phase = LoadPhase::Idle;
                            set_list_status_line(
                                &mut list_status_line,
                                &mut list_status_line_overrides_stale,
                                &mut status_line,
                                &mut status_line_overrides_stale,
                                list_status_text(rows.len(), None),
                                false,
                            );
                            clear_worker = true;
                            break;
                        }
                        Ok(WorkerMsg::Error(e)) => {
                            last_load_error = true;
                            last_load_error_message = Some(e);
                            set_list_status_line(
                                &mut list_status_line,
                                &mut list_status_line_overrides_stale,
                                &mut status_line,
                                &mut status_line_overrides_stale,
                                stale_status_text(rows.len()),
                                false,
                            );
                            phase = LoadPhase::Idle;
                            clear_worker = true;
                            break;
                        }
                        Ok(
                            WorkerMsg::DetailLogsLoaded { .. }
                            | WorkerMsg::DetailLogsError { .. }
                            | WorkerMsg::ActionConfirmationReady { .. }
                            | WorkerMsg::ActionResolutionError { .. }
                            | WorkerMsg::UnitActionQueued { .. }
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

            if let Some(rx) = action_resolution_worker_rx.as_ref() {
                let mut clear_action_resolution_worker = false;
                loop {
                    match rx.try_recv() {
                        Ok(msg) => {
                            clear_action_resolution_worker = apply_action_resolution_msg(
                                &mut confirmation,
                                &mut status_line,
                                &mut status_line_overrides_stale,
                                ActionResolutionUiState {
                                    list_status_line: &list_status_line,
                                    list_status_line_overrides_stale,
                                    rows_len: rows.len(),
                                    view_mode,
                                    selected_unit: rows
                                        .get(selected_idx)
                                        .map(|row| row.unit.as_str()),
                                },
                                msg,
                            );
                            if clear_action_resolution_worker {
                                break;
                            }
                        }
                        Err(TryRecvError::Empty) => break,
                        Err(TryRecvError::Disconnected) => {
                            clear_action_resolution_worker = true;
                            break;
                        }
                    }
                }
                if clear_action_resolution_worker {
                    action_resolution_worker_rx = None;
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
                            if let Some(pending) = confirmation.take()
                                && let Some(action) = pending.confirmed_action()
                            {
                                run_confirmed_action(
                                    &mut terminal,
                                    config.scope,
                                    &pending.unit,
                                    action,
                                    rows.len(),
                                    &mut status_line,
                                    &mut status_line_overrides_stale,
                                    &mut refresh_requested,
                                    &mut queued_action_refresh_deadline,
                                )?;
                            }
                        }
                        UiCommand::ChooseRestart => {
                            if let Some(pending) = confirmation.take() {
                                run_confirmed_action(
                                    &mut terminal,
                                    config.scope,
                                    &pending.unit,
                                    UnitAction::Restart,
                                    rows.len(),
                                    &mut status_line,
                                    &mut status_line_overrides_stale,
                                    &mut refresh_requested,
                                    &mut queued_action_refresh_deadline,
                                )?;
                            }
                        }
                        UiCommand::ChooseStop => {
                            if let Some(pending) = confirmation.take() {
                                run_confirmed_action(
                                    &mut terminal,
                                    config.scope,
                                    &pending.unit,
                                    UnitAction::Stop,
                                    rows.len(),
                                    &mut status_line,
                                    &mut status_line_overrides_stale,
                                    &mut refresh_requested,
                                    &mut queued_action_refresh_deadline,
                                )?;
                            }
                        }
                        UiCommand::Cancel => {
                            confirmation = None;
                            restore_list_status_line(
                                &list_status_line,
                                list_status_line_overrides_stale,
                                &mut status_line,
                                &mut status_line_overrides_stale,
                            );
                        }
                        _ => {}
                    }
                } else if confirmation.is_none()
                    && let Some(cmd) = map_key(view_mode, k.code)
                {
                    match cmd {
                        UiCommand::Quit => break,
                        UiCommand::Refresh => {
                            cancel_pending_action_resolution(
                                &mut action_resolution_worker_rx,
                                &list_status_line,
                                list_status_line_overrides_stale,
                                &mut status_line,
                                &mut status_line_overrides_stale,
                            );
                            queued_action_refresh_deadline = None;
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
                                    if selected_idx < rows.len() - 1 {
                                        cancel_pending_action_resolution(
                                            &mut action_resolution_worker_rx,
                                            &list_status_line,
                                            list_status_line_overrides_stale,
                                            &mut status_line,
                                            &mut status_line_overrides_stale,
                                        );
                                    }
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
                            ViewMode::List => {
                                if selected_idx > 0 {
                                    cancel_pending_action_resolution(
                                        &mut action_resolution_worker_rx,
                                        &list_status_line,
                                        list_status_line_overrides_stale,
                                        &mut status_line,
                                        &mut status_line_overrides_stale,
                                    );
                                }
                                selected_idx = selected_idx.saturating_sub(1);
                            }
                            ViewMode::Detail => detail.scroll = detail.scroll.saturating_sub(1),
                        },
                        UiCommand::OpenDetail => {
                            if let Some(row) = rows.get(selected_idx) {
                                cancel_pending_action_resolution(
                                    &mut action_resolution_worker_rx,
                                    &list_status_line,
                                    list_status_line_overrides_stale,
                                    &mut status_line,
                                    &mut status_line_overrides_stale,
                                );
                                let request_id = detail.begin_for_unit(row.unit.clone());
                                detail_worker_rx = Some(spawn_detail_worker(
                                    &config,
                                    detail.unit.clone(),
                                    request_id,
                                ));
                                view_mode = ViewMode::Detail;
                            }
                        }
                        // No need to cancel a pending resolution here: resolution
                        // can only be started from List view, and OpenDetail already
                        // cancels it, so no resolution worker is running when the
                        // user presses BackToList from Detail view.
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
                            if action_resolution_worker_rx.is_none()
                                && let Some(row) = rows.get(selected_idx)
                            {
                                set_status_line(
                                    &mut status_line,
                                    &mut status_line_overrides_stale,
                                    action_resolution_status_text(rows.len(), &row.unit),
                                    true,
                                );
                                action_resolution_worker_rx = Some(spawn_action_resolution_worker(
                                    &config,
                                    ActionResolutionRequest::StartStop {
                                        unit: row.unit.clone(),
                                    },
                                ));
                            }
                        }
                        UiCommand::RequestEnableDisable => {
                            if action_resolution_worker_rx.is_none()
                                && let Some(row) = rows.get(selected_idx)
                            {
                                set_status_line(
                                    &mut status_line,
                                    &mut status_line_overrides_stale,
                                    action_resolution_status_text(rows.len(), &row.unit),
                                    true,
                                );
                                action_resolution_worker_rx = Some(spawn_action_resolution_worker(
                                    &config,
                                    ActionResolutionRequest::EnableDisable {
                                        unit: row.unit.clone(),
                                    },
                                ));
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
    use super::{
        ActionResolutionUiState, UNIT_ACTION_REFRESH_DELAY, activate_queued_action_refresh,
        apply_action_resolution_msg, cancel_pending_action_resolution, defer_queued_action_refresh,
        restore_list_status_line, set_list_status_line, set_status_line,
    };
    use crate::rows::preserve_selection;
    use crate::types::{
        ConfirmationState, DetailState, LoadPhase, UnitAction, UnitRow, ViewMode, WorkerMsg,
    };
    use ratatui::prelude::Style;
    use std::collections::HashMap;
    use std::time::{Duration, Instant};

    struct TestUiState {
        view_mode: ViewMode,
        rows: Vec<UnitRow>,
        selected_idx: usize,
        detail: DetailState,
        detail_worker_active: bool,
        action_resolution_active: Option<()>,
        refresh_requested: bool,
        list_status_line: String,
        list_status_line_overrides_stale: bool,
        status_line: String,
        status_line_overrides_stale: bool,
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
                cancel_pending_action_resolution(
                    &mut state.action_resolution_active,
                    &state.list_status_line,
                    state.list_status_line_overrides_stale,
                    &mut state.status_line,
                    &mut state.status_line_overrides_stale,
                );
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
                        if state.selected_idx < state.rows.len() - 1 {
                            cancel_pending_action_resolution(
                                &mut state.action_resolution_active,
                                &state.list_status_line,
                                state.list_status_line_overrides_stale,
                                &mut state.status_line,
                                &mut state.status_line_overrides_stale,
                            );
                        }
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
                ViewMode::List => {
                    if state.selected_idx > 0 {
                        cancel_pending_action_resolution(
                            &mut state.action_resolution_active,
                            &state.list_status_line,
                            state.list_status_line_overrides_stale,
                            &mut state.status_line,
                            &mut state.status_line_overrides_stale,
                        );
                    }
                    state.selected_idx = state.selected_idx.saturating_sub(1);
                }
                ViewMode::Detail => state.detail.scroll = state.detail.scroll.saturating_sub(1),
            },
            UiCommand::OpenDetail => {
                if let Some(r) = state.rows.get(state.selected_idx) {
                    cancel_pending_action_resolution(
                        &mut state.action_resolution_active,
                        &state.list_status_line,
                        state.list_status_line_overrides_stale,
                        &mut state.status_line,
                        &mut state.status_line_overrides_stale,
                    );
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
            | WorkerMsg::ActionConfirmationReady { .. }
            | WorkerMsg::ActionResolutionError { .. }
            | WorkerMsg::UnitActionQueued { .. }
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
            action_resolution_active: Some(()),
            refresh_requested: false,
            list_status_line: "services: 2 | logs: 1/2 | controls".to_string(),
            list_status_line_overrides_stale: false,
            status_line: "resolving action for a.service".to_string(),
            status_line_overrides_stale: true,
        };
        assert!(!apply_command(&mut state, UiCommand::MoveDown));
        assert_eq!(state.selected_idx, 1);
        assert!(state.action_resolution_active.is_none());
        assert_eq!(state.status_line, "services: 2 | logs: 1/2 | controls");
        assert!(!state.status_line_overrides_stale);
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

    #[test]
    fn set_status_line_tracks_override_flag() {
        let mut line = String::new();
        let mut override_stale = false;

        set_status_line(&mut line, &mut override_stale, "x".to_string(), true);
        assert_eq!(line, "x");
        assert!(override_stale);

        set_status_line(&mut line, &mut override_stale, "y".to_string(), false);
        assert_eq!(line, "y");
        assert!(!override_stale);
    }

    #[test]
    fn set_and_restore_list_status_line_track_snapshot_and_restore_progress() {
        let mut list_line = String::new();
        let mut list_override = false;
        let mut line = String::new();
        let mut override_stale = false;

        set_list_status_line(
            &mut list_line,
            &mut list_override,
            &mut line,
            &mut override_stale,
            "services: 2 | logs: 1/2".to_string(),
            false,
        );
        assert_eq!(list_line, "services: 2 | logs: 1/2");
        assert!(!list_override);
        assert_eq!(line, "services: 2 | logs: 1/2");
        assert!(!override_stale);

        set_status_line(
            &mut line,
            &mut override_stale,
            "resolving".to_string(),
            true,
        );
        restore_list_status_line(&list_line, list_override, &mut line, &mut override_stale);
        assert_eq!(line, "services: 2 | logs: 1/2");
        assert!(!override_stale);
    }

    #[test]
    fn apply_action_resolution_msg_updates_confirmation_and_error_status() {
        let mut confirmation = None;
        let list_status_line = "services: 2 | logs: 1/2".to_string();
        let mut status_line = String::new();
        let mut override_stale = false;

        assert!(apply_action_resolution_msg(
            &mut confirmation,
            &mut status_line,
            &mut override_stale,
            ActionResolutionUiState {
                list_status_line: &list_status_line,
                list_status_line_overrides_stale: false,
                rows_len: 2,
                view_mode: ViewMode::List,
                selected_unit: Some("demo.service"),
            },
            WorkerMsg::ActionConfirmationReady {
                unit: "demo.service".to_string(),
                confirmation: ConfirmationState::restart_or_stop("demo.service".to_string()),
            },
        ));
        assert_eq!(
            confirmation,
            Some(ConfirmationState::restart_or_stop(
                "demo.service".to_string()
            ))
        );
        assert_eq!(status_line, list_status_line);
        assert!(!override_stale);

        assert!(apply_action_resolution_msg(
            &mut confirmation,
            &mut status_line,
            &mut override_stale,
            ActionResolutionUiState {
                list_status_line: &list_status_line,
                list_status_line_overrides_stale: false,
                rows_len: 2,
                view_mode: ViewMode::List,
                selected_unit: Some("demo.service"),
            },
            WorkerMsg::ActionResolutionError {
                unit: "demo.service".to_string(),
                error: "boom".to_string(),
            },
        ));
        assert!(status_line.contains("failed to inspect demo.service: boom"));
        assert!(override_stale);
    }

    #[test]
    fn apply_action_resolution_msg_ignores_stale_selection_and_view() {
        let mut confirmation = None;
        let list_status_line = "services: 2 | logs: 1/2".to_string();
        let mut status_line = "resolving".to_string();
        let mut override_stale = true;

        assert!(apply_action_resolution_msg(
            &mut confirmation,
            &mut status_line,
            &mut override_stale,
            ActionResolutionUiState {
                list_status_line: &list_status_line,
                list_status_line_overrides_stale: false,
                rows_len: 2,
                view_mode: ViewMode::List,
                selected_unit: Some("other.service"),
            },
            WorkerMsg::ActionConfirmationReady {
                unit: "demo.service".to_string(),
                confirmation: ConfirmationState::restart_or_stop("demo.service".to_string()),
            },
        ));
        assert!(confirmation.is_none());
        assert_eq!(status_line, list_status_line);
        assert!(!override_stale);

        status_line = "resolving".to_string();
        override_stale = true;
        assert!(apply_action_resolution_msg(
            &mut confirmation,
            &mut status_line,
            &mut override_stale,
            ActionResolutionUiState {
                list_status_line: &list_status_line,
                list_status_line_overrides_stale: false,
                rows_len: 2,
                view_mode: ViewMode::Detail,
                selected_unit: Some("demo.service"),
            },
            WorkerMsg::ActionResolutionError {
                unit: "demo.service".to_string(),
                error: "boom".to_string(),
            },
        ));
        assert_eq!(status_line, list_status_line);
        assert!(!override_stale);
    }

    #[test]
    fn apply_action_resolution_msg_restores_stale_list_status_when_needed() {
        let mut confirmation = None;
        let list_status_line = stale_status_text(2);
        let mut status_line = "resolving".to_string();
        let mut override_stale = true;

        assert!(apply_action_resolution_msg(
            &mut confirmation,
            &mut status_line,
            &mut override_stale,
            ActionResolutionUiState {
                list_status_line: &list_status_line,
                list_status_line_overrides_stale: true,
                rows_len: 2,
                view_mode: ViewMode::List,
                selected_unit: Some("demo.service"),
            },
            WorkerMsg::ActionConfirmationReady {
                unit: "demo.service".to_string(),
                confirmation: ConfirmationState::confirm_action(
                    UnitAction::Start,
                    "demo.service".to_string(),
                ),
            },
        ));
        assert_eq!(status_line, list_status_line);
        assert!(override_stale);
    }

    #[test]
    fn apply_action_resolution_msg_ignores_unrelated_messages() {
        let mut confirmation = None;
        let mut status_line = "unchanged".to_string();
        let mut override_stale = false;

        assert!(!apply_action_resolution_msg(
            &mut confirmation,
            &mut status_line,
            &mut override_stale,
            ActionResolutionUiState {
                list_status_line: "services: 2",
                list_status_line_overrides_stale: false,
                rows_len: 2,
                view_mode: ViewMode::List,
                selected_unit: Some("demo.service"),
            },
            WorkerMsg::Finished,
        ));
        assert!(confirmation.is_none());
        assert_eq!(status_line, "unchanged");
        assert!(!override_stale);
    }

    #[test]
    fn cancel_pending_action_resolution_resets_status_when_active() {
        let mut worker = Some(());
        let list_status_line = "services: 3 | logs: 2/3".to_string();
        let mut status_line = "resolving".to_string();
        let mut override_stale = true;

        assert!(cancel_pending_action_resolution(
            &mut worker,
            &list_status_line,
            false,
            &mut status_line,
            &mut override_stale,
        ));
        assert!(worker.is_none());
        assert_eq!(status_line, list_status_line);
        assert!(!override_stale);
    }

    #[test]
    fn cancel_pending_action_resolution_is_noop_without_pending_worker() {
        let mut worker = None::<()>;
        let mut status_line = "unchanged".to_string();
        let mut override_stale = true;

        assert!(!cancel_pending_action_resolution(
            &mut worker,
            "services: 3",
            false,
            &mut status_line,
            &mut override_stale,
        ));
        assert_eq!(status_line, "unchanged");
        assert!(override_stale);
    }

    #[test]
    fn apply_command_covers_remaining_navigation_branches() {
        let mut state = TestUiState {
            view_mode: ViewMode::List,
            rows: vec![row("a.service")],
            selected_idx: 0,
            detail: DetailState::default(),
            detail_worker_active: false,
            action_resolution_active: Some(()),
            refresh_requested: false,
            list_status_line: "services: 1 | logs: 1/1".to_string(),
            list_status_line_overrides_stale: false,
            status_line: "resolving".to_string(),
            status_line_overrides_stale: true,
        };

        assert!(!apply_command(&mut state, UiCommand::MoveUp));
        assert_eq!(state.selected_idx, 0);
        assert!(state.action_resolution_active.is_some());
        assert_eq!(state.status_line, "resolving");

        assert!(!apply_command(&mut state, UiCommand::MoveDown));
        assert_eq!(state.selected_idx, 0);
        assert!(state.action_resolution_active.is_some());

        state.view_mode = ViewMode::Detail;
        state.detail.logs = vec![
            crate::types::DetailLogEntry {
                time: "t1".to_string(),
                log: "a".to_string(),
            },
            crate::types::DetailLogEntry {
                time: "t2".to_string(),
                log: "b".to_string(),
            },
        ];
        assert!(!apply_command(&mut state, UiCommand::MoveDown));
        assert_eq!(state.detail.scroll, 1);
        assert!(!apply_command(&mut state, UiCommand::MoveUp));
        assert_eq!(state.detail.scroll, 0);
        assert!(!apply_command(&mut state, UiCommand::BackToList));
        assert!(matches!(state.view_mode, ViewMode::List));
    }

    #[test]
    fn refresh_cancels_pending_action_resolution_and_restores_list_status() {
        let mut state = TestUiState {
            view_mode: ViewMode::List,
            rows: vec![row("a.service")],
            selected_idx: 0,
            detail: DetailState::default(),
            detail_worker_active: false,
            action_resolution_active: Some(()),
            refresh_requested: false,
            list_status_line: "services: 1 | logs: 1/1".to_string(),
            list_status_line_overrides_stale: false,
            status_line: "services: 1 | resolving action for a.service...".to_string(),
            status_line_overrides_stale: true,
        };

        assert!(!apply_command(&mut state, UiCommand::Refresh));
        assert!(state.refresh_requested);
        assert!(state.action_resolution_active.is_none());
        assert_eq!(state.status_line, state.list_status_line);
        assert!(!state.status_line_overrides_stale);
    }

    #[test]
    fn defer_queued_action_refresh_schedules_delayed_list_reload() {
        let mut refresh_requested = true;
        let mut queued_deadline = None;
        let now = Instant::now();

        defer_queued_action_refresh(&mut refresh_requested, &mut queued_deadline, false, now);

        assert!(!refresh_requested);
        assert_eq!(
            queued_deadline.expect("deadline").duration_since(now),
            UNIT_ACTION_REFRESH_DELAY
        );
    }

    #[test]
    fn defer_queued_action_refresh_preserves_existing_refresh_request() {
        let mut refresh_requested = true;
        let mut queued_deadline = None;

        defer_queued_action_refresh(
            &mut refresh_requested,
            &mut queued_deadline,
            true,
            Instant::now(),
        );

        assert!(refresh_requested);
        assert!(queued_deadline.is_none());
    }

    #[test]
    fn activate_queued_action_refresh_promotes_elapsed_deadline() {
        let mut refresh_requested = false;
        let mut queued_deadline = Some(Instant::now() - Duration::from_millis(1));

        activate_queued_action_refresh(
            &mut refresh_requested,
            &mut queued_deadline,
            Instant::now(),
        );

        assert!(refresh_requested);
        assert!(queued_deadline.is_none());
    }
}

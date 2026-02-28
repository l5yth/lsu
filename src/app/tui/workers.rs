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

//! Background worker spawning for list and detail data loading.

use std::{
    sync::mpsc::{self, Receiver},
    thread,
};

#[cfg(feature = "debug_tui")]
use super::debug::{
    spawn_debug_action_resolution_worker, spawn_debug_action_worker, spawn_debug_detail_worker,
    spawn_debug_refresh_worker,
};
#[cfg(test)]
use crate::types::Scope;
use crate::{
    cli::Config,
    journal::{fetch_unit_logs, latest_log_lines_batch},
    rows::{build_rows, seed_logs_from_previous, sort_rows},
    systemd::{
        fetch_services, filter_services, is_full_all, run_unit_action,
        select_enable_disable_action, select_start_stop_action, should_fetch_all,
    },
    types::{ActionResolutionRequest, ConfirmationState, UnitAction, UnitRow, WorkerMsg},
};

/// Spawn a background worker that fetches units and batched log previews.
pub fn spawn_refresh_worker(config: Config, previous_rows: Vec<UnitRow>) -> Receiver<WorkerMsg> {
    #[cfg(feature = "debug_tui")]
    if config.debug_tui {
        return spawn_debug_refresh_worker(previous_rows);
    }

    let (tx, rx) = mpsc::channel();
    thread::spawn(move || {
        let fetch_all = should_fetch_all(&config);
        let units =
            match fetch_services(config.scope, fetch_all).map(|u| filter_services(u, &config)) {
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
            let logs = match latest_log_lines_batch(config.scope, &units) {
                Ok(logs) => logs.into_iter().collect(),
                Err(e) => {
                    let _ = tx.send(WorkerMsg::Error(e.to_string()));
                    return;
                }
            };
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

/// Spawn a background worker that loads detailed logs for one unit.
pub fn spawn_detail_worker(config: &Config, unit: String, request_id: u64) -> Receiver<WorkerMsg> {
    #[cfg(feature = "debug_tui")]
    if config.debug_tui {
        return spawn_debug_detail_worker(unit, request_id);
    }

    let (tx, rx) = mpsc::channel();
    let scope = config.scope;
    thread::spawn(move || match fetch_unit_logs(scope, &unit, 300) {
        Ok(logs) => {
            let _ = tx.send(WorkerMsg::DetailLogsLoaded {
                unit,
                request_id,
                logs,
            });
        }
        Err(e) => {
            let _ = tx.send(WorkerMsg::DetailLogsError {
                unit,
                request_id,
                error: e.to_string(),
            });
        }
    });
    rx
}

/// Spawn a background worker that executes one unit action.
pub fn spawn_unit_action_worker(
    config: &Config,
    unit: String,
    action: UnitAction,
) -> Receiver<WorkerMsg> {
    #[cfg(feature = "debug_tui")]
    if config.debug_tui {
        return spawn_debug_action_worker(unit, action);
    }

    let (tx, rx) = mpsc::channel();
    let scope = config.scope;
    thread::spawn(move || match run_unit_action(scope, &unit, action) {
        Ok(()) => {
            let _ = tx.send(WorkerMsg::UnitActionComplete { unit, action });
        }
        Err(e) => {
            let _ = tx.send(WorkerMsg::UnitActionError {
                unit,
                action,
                error: e.to_string(),
            });
        }
    });
    rx
}

fn resolve_action_confirmation(
    scope: crate::types::Scope,
    request: ActionResolutionRequest,
) -> anyhow::Result<ConfirmationState> {
    match request {
        ActionResolutionRequest::StartStop { unit } => {
            let action = select_start_stop_action(scope, &unit)?;
            Ok(match action {
                UnitAction::Stop => ConfirmationState::restart_or_stop(unit),
                _ => ConfirmationState::confirm_action(action, unit),
            })
        }
        ActionResolutionRequest::EnableDisable { unit } => {
            let action = select_enable_disable_action(scope, &unit)?;
            Ok(ConfirmationState::confirm_action(action, unit))
        }
    }
}

/// Spawn a background worker that resolves which unit-action prompt should be shown.
pub fn spawn_action_resolution_worker(
    config: &Config,
    request: ActionResolutionRequest,
) -> Receiver<WorkerMsg> {
    #[cfg(feature = "debug_tui")]
    if config.debug_tui {
        return spawn_debug_action_resolution_worker(request);
    }

    let (tx, rx) = mpsc::channel();
    let scope = config.scope;
    thread::spawn(move || {
        let unit = request.unit().to_string();
        match resolve_action_confirmation(scope, request) {
            Ok(confirmation) => {
                let _ = tx.send(WorkerMsg::ActionConfirmationReady { unit, confirmation });
            }
            Err(e) => {
                let _ = tx.send(WorkerMsg::ActionResolutionError {
                    unit,
                    error: e.to_string(),
                });
            }
        }
    });
    rx
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[test]
    fn refresh_worker_emits_units_then_finished_with_stubbed_backends() {
        let cfg = Config {
            load_filter: "loaded".to_string(),
            active_filter: "active".to_string(),
            sub_filter: "running".to_string(),
            show_help: false,
            show_version: false,
            debug_tui: false,
            scope: Scope::System,
        };
        let rx = spawn_refresh_worker(cfg, Vec::new());
        match rx
            .recv_timeout(Duration::from_millis(500))
            .expect("first msg")
        {
            WorkerMsg::UnitsLoaded(rows) => assert!(rows.is_empty()),
            other => panic!("expected UnitsLoaded, got {other:?}"),
        }
        match rx
            .recv_timeout(Duration::from_millis(500))
            .expect("second msg")
        {
            WorkerMsg::Finished => {}
            other => panic!("expected Finished, got {other:?}"),
        }
    }

    #[test]
    fn refresh_worker_emits_log_progress_for_non_empty_rows() {
        let cfg = Config {
            load_filter: "loaded".to_string(),
            active_filter: "active".to_string(),
            sub_filter: "all".to_string(),
            show_help: false,
            show_version: false,
            debug_tui: false,
            scope: Scope::System,
        };
        let rx = spawn_refresh_worker(cfg, Vec::new());
        match rx
            .recv_timeout(Duration::from_millis(500))
            .expect("units msg")
        {
            WorkerMsg::UnitsLoaded(rows) => assert_eq!(rows.len(), 1),
            other => panic!("expected UnitsLoaded, got {other:?}"),
        }
        match rx
            .recv_timeout(Duration::from_millis(500))
            .expect("progress msg")
        {
            WorkerMsg::LogsProgress { done, total, logs } => {
                assert_eq!(done, 1);
                assert_eq!(total, 1);
                assert_eq!(logs.len(), 1);
            }
            other => panic!("expected LogsProgress, got {other:?}"),
        }
        match rx
            .recv_timeout(Duration::from_millis(500))
            .expect("finished msg")
        {
            WorkerMsg::Finished => {}
            other => panic!("expected Finished, got {other:?}"),
        }
    }

    #[test]
    fn refresh_worker_emits_error_when_systemd_fetch_fails() {
        let cfg = Config {
            load_filter: "loaded".to_string(),
            active_filter: "active".to_string(),
            sub_filter: "running".to_string(),
            show_help: false,
            show_version: false,
            debug_tui: false,
            scope: Scope::User,
        };
        let rx = spawn_refresh_worker(cfg, Vec::new());
        match rx
            .recv_timeout(Duration::from_millis(500))
            .expect("error msg")
        {
            WorkerMsg::Error(msg) => assert!(msg.contains("systemd test error")),
            other => panic!("expected Error, got {other:?}"),
        }
    }

    #[test]
    fn refresh_worker_emits_error_when_journal_batch_fails() {
        let cfg = Config {
            load_filter: "loaded".to_string(),
            active_filter: "inactive".to_string(),
            sub_filter: "dead".to_string(),
            show_help: false,
            show_version: false,
            debug_tui: false,
            scope: Scope::System,
        };
        let rx = spawn_refresh_worker(cfg, Vec::new());
        match rx
            .recv_timeout(Duration::from_millis(500))
            .expect("units msg")
        {
            WorkerMsg::UnitsLoaded(rows) => assert_eq!(rows.len(), 1),
            other => panic!("expected UnitsLoaded, got {other:?}"),
        }
        match rx
            .recv_timeout(Duration::from_millis(500))
            .expect("error msg")
        {
            WorkerMsg::Error(msg) => assert!(msg.contains("journal test error")),
            other => panic!("expected Error, got {other:?}"),
        }
    }

    #[test]
    fn detail_worker_emits_loaded_with_stubbed_backend() {
        let rx = spawn_detail_worker(
            &Config {
                load_filter: "loaded".to_string(),
                active_filter: "active".to_string(),
                sub_filter: "running".to_string(),
                show_help: false,
                show_version: false,
                debug_tui: false,
                scope: Scope::System,
            },
            "a.service".to_string(),
            7,
        );
        match rx
            .recv_timeout(Duration::from_millis(500))
            .expect("detail msg")
        {
            WorkerMsg::DetailLogsLoaded {
                unit,
                request_id,
                logs,
            } => {
                assert_eq!(unit, "a.service");
                assert_eq!(request_id, 7);
                assert_eq!(logs.len(), 1);
            }
            other => panic!("expected DetailLogsLoaded, got {other:?}"),
        }
    }

    #[test]
    fn detail_worker_emits_error_when_backend_fails() {
        let rx = spawn_detail_worker(
            &Config {
                load_filter: "loaded".to_string(),
                active_filter: "active".to_string(),
                sub_filter: "running".to_string(),
                show_help: false,
                show_version: false,
                debug_tui: false,
                scope: Scope::System,
            },
            "error.service".to_string(),
            9,
        );
        match rx
            .recv_timeout(Duration::from_millis(500))
            .expect("detail error msg")
        {
            WorkerMsg::DetailLogsError {
                unit,
                request_id,
                error,
            } => {
                assert_eq!(unit, "error.service");
                assert_eq!(request_id, 9);
                assert!(error.contains("detail journal test error"));
            }
            other => panic!("expected DetailLogsError, got {other:?}"),
        }
    }

    #[test]
    fn unit_action_worker_emits_completion_on_success() {
        let rx = spawn_unit_action_worker(
            &Config {
                load_filter: "loaded".to_string(),
                active_filter: "active".to_string(),
                sub_filter: "running".to_string(),
                show_help: false,
                show_version: false,
                debug_tui: false,
                scope: Scope::System,
            },
            "demo.service".to_string(),
            UnitAction::Start,
        );
        match rx
            .recv_timeout(Duration::from_millis(500))
            .expect("action msg")
        {
            WorkerMsg::UnitActionComplete { unit, action } => {
                assert_eq!(unit, "demo.service");
                assert_eq!(action, UnitAction::Start);
            }
            other => panic!("expected UnitActionComplete, got {other:?}"),
        }
    }

    #[test]
    fn unit_action_worker_emits_error_on_failure() {
        let rx = spawn_unit_action_worker(
            &Config {
                load_filter: "loaded".to_string(),
                active_filter: "active".to_string(),
                sub_filter: "running".to_string(),
                show_help: false,
                show_version: false,
                debug_tui: false,
                scope: Scope::System,
            },
            "action-error.service".to_string(),
            UnitAction::Stop,
        );
        match rx
            .recv_timeout(Duration::from_millis(500))
            .expect("action error msg")
        {
            WorkerMsg::UnitActionError {
                unit,
                action,
                error,
            } => {
                assert_eq!(unit, "action-error.service");
                assert_eq!(action, UnitAction::Stop);
                assert!(error.contains("unit action test error"));
            }
            other => panic!("expected UnitActionError, got {other:?}"),
        }
    }

    #[test]
    fn action_resolution_worker_resolves_start_stop_from_active_state() {
        let rx = spawn_action_resolution_worker(
            &Config {
                load_filter: "loaded".to_string(),
                active_filter: "active".to_string(),
                sub_filter: "running".to_string(),
                show_help: false,
                show_version: false,
                debug_tui: false,
                scope: Scope::System,
            },
            ActionResolutionRequest::StartStop {
                unit: "running.service".to_string(),
            },
        );
        match rx
            .recv_timeout(Duration::from_millis(500))
            .expect("resolution msg")
        {
            WorkerMsg::ActionConfirmationReady { unit, confirmation } => {
                assert_eq!(unit, "running.service");
                assert_eq!(confirmation, ConfirmationState::restart_or_stop(unit));
            }
            other => panic!("expected ActionConfirmationReady, got {other:?}"),
        }
    }

    #[test]
    fn action_resolution_worker_uses_live_start_stop_lookup() {
        let rx = spawn_action_resolution_worker(
            &Config {
                load_filter: "loaded".to_string(),
                active_filter: "active".to_string(),
                sub_filter: "running".to_string(),
                show_help: false,
                show_version: false,
                debug_tui: false,
                scope: Scope::System,
            },
            ActionResolutionRequest::StartStop {
                unit: "stopped.service".to_string(),
            },
        );
        match rx
            .recv_timeout(Duration::from_millis(500))
            .expect("resolution msg")
        {
            WorkerMsg::ActionConfirmationReady { unit, confirmation } => {
                assert_eq!(unit, "stopped.service");
                assert_eq!(
                    confirmation,
                    ConfirmationState::confirm_action(UnitAction::Start, unit)
                );
            }
            other => panic!("expected ActionConfirmationReady, got {other:?}"),
        }
    }

    #[test]
    fn action_resolution_worker_resolves_enable_disable_and_errors() {
        let cfg = Config {
            load_filter: "loaded".to_string(),
            active_filter: "active".to_string(),
            sub_filter: "running".to_string(),
            show_help: false,
            show_version: false,
            debug_tui: false,
            scope: Scope::System,
        };

        let rx = spawn_action_resolution_worker(
            &cfg,
            ActionResolutionRequest::EnableDisable {
                unit: "enabled.service".to_string(),
            },
        );
        match rx
            .recv_timeout(Duration::from_millis(500))
            .expect("enable/disable resolution msg")
        {
            WorkerMsg::ActionConfirmationReady { unit, confirmation } => {
                assert_eq!(unit, "enabled.service");
                assert_eq!(
                    confirmation,
                    ConfirmationState::confirm_action(UnitAction::Disable, unit)
                );
            }
            other => panic!("expected ActionConfirmationReady, got {other:?}"),
        }

        let err_rx = spawn_action_resolution_worker(
            &cfg,
            ActionResolutionRequest::EnableDisable {
                unit: "static.service".to_string(),
            },
        );
        match err_rx
            .recv_timeout(Duration::from_millis(500))
            .expect("resolution error msg")
        {
            WorkerMsg::ActionResolutionError { unit, error } => {
                assert_eq!(unit, "static.service");
                assert!(error.contains("does not support enable/disable"));
            }
            other => panic!("expected ActionResolutionError, got {other:?}"),
        }
    }

    #[cfg(feature = "debug_tui")]
    #[test]
    fn refresh_worker_uses_debug_source_when_enabled() {
        let cfg = Config {
            load_filter: "loaded".to_string(),
            active_filter: "active".to_string(),
            sub_filter: "running".to_string(),
            show_help: false,
            show_version: false,
            debug_tui: true,
            scope: Scope::User,
        };
        let rx = spawn_refresh_worker(cfg, Vec::new());
        match rx
            .recv_timeout(Duration::from_millis(500))
            .expect("first msg")
        {
            WorkerMsg::UnitsLoaded(rows) => {
                assert!(!rows.is_empty());
                assert!(rows.iter().all(|row| row.unit.starts_with("debug-")));
            }
            other => panic!("expected UnitsLoaded, got {other:?}"),
        }
    }

    #[cfg(feature = "debug_tui")]
    #[test]
    fn action_workers_use_debug_backend_when_enabled() {
        let cfg = Config {
            load_filter: "loaded".to_string(),
            active_filter: "active".to_string(),
            sub_filter: "running".to_string(),
            show_help: false,
            show_version: false,
            debug_tui: true,
            scope: Scope::User,
        };

        let resolution_rx = spawn_action_resolution_worker(
            &cfg,
            ActionResolutionRequest::StartStop {
                unit: "debug-api-gateway.service".to_string(),
            },
        );
        match resolution_rx
            .recv_timeout(Duration::from_millis(500))
            .expect("resolution msg")
        {
            WorkerMsg::ActionConfirmationReady { unit, confirmation } => {
                assert_eq!(unit, "debug-api-gateway.service");
                assert_eq!(confirmation, ConfirmationState::restart_or_stop(unit));
            }
            other => panic!("expected ActionConfirmationReady, got {other:?}"),
        }

        let action_rx = spawn_unit_action_worker(
            &cfg,
            "debug-api-gateway.service".to_string(),
            UnitAction::Stop,
        );
        match action_rx
            .recv_timeout(Duration::from_millis(500))
            .expect("action msg")
        {
            WorkerMsg::UnitActionComplete { unit, action } => {
                assert_eq!(unit, "debug-api-gateway.service");
                assert_eq!(action, UnitAction::Stop);
            }
            other => panic!("expected UnitActionComplete, got {other:?}"),
        }
    }
}

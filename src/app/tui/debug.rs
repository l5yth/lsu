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

//! Self-contained fake data workers for optional TUI debug builds.

use std::{
    sync::mpsc::{self, Receiver},
    thread,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use crate::{
    rows::{seed_logs_from_previous, sort_rows, status_dot},
    types::{
        ActionResolutionRequest, ConfirmationState, DetailLogEntry, UnitAction, UnitRow, WorkerMsg,
    },
};

const MAX_DEBUG_UNITS: usize = 21;
const LOG_BATCH_SIZE: usize = 7;

#[derive(Clone, Copy)]
struct DebugUnitTemplate {
    slug: &'static str,
    load: &'static str,
    active: &'static str,
    sub: &'static str,
    description: &'static str,
    preview: &'static str,
}

const DEBUG_UNIT_TEMPLATES: [DebugUnitTemplate; 15] = [
    DebugUnitTemplate {
        slug: "api-gateway",
        load: "loaded",
        active: "active",
        sub: "running",
        description: "Synthetic API gateway with healthy steady-state status",
        preview: "Accepted synthetic health probe from 10.0.0.17",
    },
    DebugUnitTemplate {
        slug: "asset-compiler",
        load: "loaded",
        active: "active",
        sub: "exited",
        description: "One-shot asset compiler to exercise warm yellow states",
        preview: "Completed sprite atlas rebuild in 38ms",
    },
    DebugUnitTemplate {
        slug: "backup-primer",
        load: "loaded",
        active: "activating",
        sub: "start-pre",
        description: "Backup preparer paused in pre-start checks",
        preview: "Checking snapshot volume pressure before activation",
    },
    DebugUnitTemplate {
        slug: "cache-warmer",
        load: "loaded",
        active: "reloading",
        sub: "reload",
        description: "Cache warmer cycling through a synthetic live reload",
        preview: "Reloaded 42 texture manifests from debug seed",
    },
    DebugUnitTemplate {
        slug: "cleanup-runner",
        load: "loaded",
        active: "deactivating",
        sub: "stop-sigterm",
        description: "Graceful shutdown state for key handling checks",
        preview: "Stopping workers after synthetic quit request",
    },
    DebugUnitTemplate {
        slug: "cold-storage",
        load: "loaded",
        active: "inactive",
        sub: "dead",
        description: "Idle cold-storage worker rendered in gray",
        preview: "No queued restores in the last debug interval",
    },
    DebugUnitTemplate {
        slug: "crash-loop",
        load: "loaded",
        active: "failed",
        sub: "failed",
        description: "Failing unit for saturated red error states",
        preview: "Exited with status=1 after synthetic panic path",
    },
    DebugUnitTemplate {
        slug: "db-migrate",
        load: "loaded",
        active: "activating",
        sub: "start-post",
        description: "Migration worker still in post-start staging",
        preview: "Waiting for synthetic schema lock release",
    },
    DebugUnitTemplate {
        slug: "desktop-sync",
        load: "stub",
        active: "maintenance",
        sub: "condition",
        description: "Condition-blocked user sync service to test blue states",
        preview: "ConditionPathExists failed for /tmp/debug-sync.token",
    },
    DebugUnitTemplate {
        slug: "edge-proxy",
        load: "masked",
        active: "inactive",
        sub: "dead",
        description: "Masked edge proxy with muted gray rows",
        preview: "Unit is masked for the current synthetic profile",
    },
    DebugUnitTemplate {
        slug: "event-fanout",
        load: "loaded",
        active: "refreshing",
        sub: "reload-notify",
        description: "Refreshing fanout worker with notify-based reloads",
        preview: "Broadcasting synthetic cache invalidation wave",
    },
    DebugUnitTemplate {
        slug: "ghost-printer",
        load: "not-found",
        active: "inactive",
        sub: "dead",
        description: "Missing printer backend to exercise not-found load states",
        preview: "Referenced unit file does not exist in this profile",
    },
    DebugUnitTemplate {
        slug: "metrics-rollup",
        load: "merged",
        active: "active",
        sub: "running",
        description: "Merged metrics rollup service for alternate load states",
        preview: "Merged counters from 6 synthetic shards",
    },
    DebugUnitTemplate {
        slug: "notification-drain",
        load: "bad-setting",
        active: "failed",
        sub: "auto-restart",
        description: "Broken configuration with restart churn",
        preview: "Restart backoff engaged after invalid debug endpoint",
    },
    DebugUnitTemplate {
        slug: "orphan-reconciler",
        load: "error",
        active: "maintenance",
        sub: "cleaning",
        description: "Loader error plus maintenance cleanup path",
        preview: "Cleaning temporary state left by synthetic fault injection",
    },
];

fn debug_unit_name(template: DebugUnitTemplate) -> String {
    format!("debug-{}.service", template.slug)
}

fn time_seed() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or(Duration::ZERO)
        .as_nanos() as u64
}

fn next_random(state: &mut u64) -> u64 {
    *state = state
        .wrapping_mul(6_364_136_223_846_793_005)
        .wrapping_add(1_442_695_040_888_963_407);
    *state
}

fn shuffle<T>(items: &mut [T], state: &mut u64) {
    for idx in (1..items.len()).rev() {
        let swap_idx = (next_random(state) as usize) % (idx + 1);
        items.swap(idx, swap_idx);
    }
}

fn build_debug_rows() -> Vec<UnitRow> {
    let mut state = time_seed().max(1);
    let mut templates = DEBUG_UNIT_TEMPLATES;
    shuffle(&mut templates, &mut state);

    templates
        .into_iter()
        .take(MAX_DEBUG_UNITS)
        .map(|template| {
            let (dot, dot_style) = status_dot(template.active, template.sub);
            let variant = (next_random(&mut state) % 900) + 100;
            UnitRow {
                dot,
                dot_style,
                unit: debug_unit_name(template),
                load: template.load.to_string(),
                active: template.active.to_string(),
                sub: template.sub.to_string(),
                description: format!("{} [{variant}]", template.description),
                last_log: String::new(),
            }
        })
        .collect()
}

fn debug_preview(row: &UnitRow, ordinal: usize) -> String {
    let template = template_for_unit(&row.unit).expect("debug unit should map to a template");
    format!(
        "#{:02} {} | {} / {} / {}",
        ordinal + 1,
        template.preview,
        row.load,
        row.active,
        row.sub
    )
}

fn template_for_unit(unit: &str) -> Option<DebugUnitTemplate> {
    DEBUG_UNIT_TEMPLATES
        .iter()
        .copied()
        .find(|template| debug_unit_name(*template) == unit)
}

fn build_detail_logs(unit: &str) -> Vec<DetailLogEntry> {
    let template = template_for_unit(unit).unwrap_or(DEBUG_UNIT_TEMPLATES[0]);
    let mut state = unit
        .bytes()
        .fold(0u64, |acc, byte| {
            acc.wrapping_mul(131).wrapping_add(byte as u64)
        })
        .max(1);

    (0..12)
        .map(|idx| {
            let jitter = next_random(&mut state) % 50;
            DetailLogEntry {
                time: format!("2026-02-27 12:{:02}:{:02}", idx, 10 + jitter),
                log: format!(
                    "{} | synthetic detail {:02} | load={} active={} sub={}",
                    template.preview,
                    idx + 1,
                    template.load,
                    template.active,
                    template.sub
                ),
            }
        })
        .collect()
}

fn resolve_debug_action_confirmation(
    request: ActionResolutionRequest,
) -> anyhow::Result<ConfirmationState> {
    match request {
        ActionResolutionRequest::StartStop { unit } => {
            let template =
                template_for_unit(&unit).ok_or_else(|| anyhow::anyhow!("unknown debug unit"))?;
            Ok(match template.active {
                "active" | "activating" | "deactivating" | "reloading" => {
                    ConfirmationState::restart_or_stop(unit)
                }
                _ => ConfirmationState::confirm_action(UnitAction::Start, unit),
            })
        }
        ActionResolutionRequest::EnableDisable { unit } => {
            let template =
                template_for_unit(&unit).ok_or_else(|| anyhow::anyhow!("unknown debug unit"))?;
            match template.load {
                "masked" | "not-found" | "error" | "merged" | "bad-setting" | "stub" => {
                    Err(anyhow::anyhow!(
                        "unit file state '{}' does not support enable/disable",
                        template.load
                    ))
                }
                _ => Ok(ConfirmationState::confirm_action(
                    debug_enable_disable_action(template),
                    unit,
                )),
            }
        }
    }
}

fn debug_enable_disable_action(template: DebugUnitTemplate) -> UnitAction {
    match template.active {
        "active" | "activating" | "deactivating" | "reloading" | "refreshing" => {
            UnitAction::Disable
        }
        _ => UnitAction::Enable,
    }
}

/// Spawn a background worker that emits fake rows and fake preview logs.
pub(super) fn spawn_debug_refresh_worker(previous_rows: Vec<UnitRow>) -> Receiver<WorkerMsg> {
    let (tx, rx) = mpsc::channel();
    thread::spawn(move || {
        let mut rows = build_debug_rows();
        seed_logs_from_previous(&mut rows, &previous_rows);
        sort_rows(&mut rows, true);
        let total = rows.len();

        if tx.send(WorkerMsg::UnitsLoaded(rows.clone())).is_err() {
            return;
        }

        for (batch_idx, batch) in rows.chunks(LOG_BATCH_SIZE).enumerate() {
            let done = std::cmp::min((batch_idx + 1) * LOG_BATCH_SIZE, total);
            let logs = batch
                .iter()
                .enumerate()
                .map(|(offset, row)| {
                    (
                        row.unit.clone(),
                        debug_preview(row, batch_idx * LOG_BATCH_SIZE + offset),
                    )
                })
                .collect();
            if tx
                .send(WorkerMsg::LogsProgress { done, total, logs })
                .is_err()
            {
                return;
            }
        }

        let _ = tx.send(WorkerMsg::Finished);
    });
    rx
}

/// Spawn a background worker that emits fake detail logs for one debug unit.
pub(super) fn spawn_debug_detail_worker(unit: String, request_id: u64) -> Receiver<WorkerMsg> {
    let (tx, rx) = mpsc::channel();
    thread::spawn(move || {
        let _ = tx.send(WorkerMsg::DetailLogsLoaded {
            unit: unit.clone(),
            request_id,
            logs: build_detail_logs(&unit),
        });
    });
    rx
}

/// Spawn a debug worker that resolves a synthetic confirmation prompt.
pub(super) fn spawn_debug_action_resolution_worker(
    request: ActionResolutionRequest,
) -> Receiver<WorkerMsg> {
    let (tx, rx) = mpsc::channel();
    thread::spawn(move || {
        let unit = request.unit().to_string();
        match resolve_debug_action_confirmation(request) {
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

/// Spawn a debug worker that simulates a unit action.
pub(super) fn spawn_debug_action_worker(unit: String, action: UnitAction) -> Receiver<WorkerMsg> {
    let (tx, rx) = mpsc::channel();
    thread::spawn(move || {
        if template_for_unit(&unit).is_some() {
            let _ = tx.send(WorkerMsg::UnitActionComplete { unit, action });
        } else {
            let _ = tx.send(WorkerMsg::UnitActionError {
                unit,
                action,
                error: "unknown debug unit".to_string(),
            });
        }
    });
    rx
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::prelude::{Color, Style};
    use std::time::Duration;

    #[test]
    fn build_debug_rows_stays_within_limit_and_covers_color_buckets() {
        let rows = build_debug_rows();
        assert!(!rows.is_empty());
        assert!(rows.len() <= MAX_DEBUG_UNITS);
        assert!(
            rows.iter()
                .any(|row| row.dot_style == Style::default().fg(Color::Green))
        );
        assert!(
            rows.iter()
                .any(|row| row.dot_style == Style::default().fg(Color::Yellow))
        );
        assert!(
            rows.iter()
                .any(|row| row.dot_style == Style::default().fg(Color::DarkGray))
        );
        assert!(
            rows.iter()
                .any(|row| row.dot_style == Style::default().fg(Color::Red))
        );
        assert!(
            rows.iter()
                .any(|row| row.dot_style == Style::default().fg(Color::Blue))
        );
    }

    #[test]
    fn build_debug_rows_uses_distinct_unit_names() {
        let rows = build_debug_rows();
        let unique_units: std::collections::HashSet<String> =
            rows.iter().map(|row| row.unit.clone()).collect();
        assert_eq!(unique_units.len(), rows.len());
    }

    #[test]
    fn debug_rows_use_normal_all_mode_sorting_after_generation() {
        let mut rows = build_debug_rows();
        sort_rows(&mut rows, true);

        for pair in rows.windows(2) {
            let left = &pair[0];
            let right = &pair[1];
            let left_key = (
                crate::rows::load_rank(&left.load),
                crate::rows::active_rank(&left.active),
                crate::rows::sub_rank(&left.sub),
                left.unit.as_str(),
            );
            let right_key = (
                crate::rows::load_rank(&right.load),
                crate::rows::active_rank(&right.active),
                crate::rows::sub_rank(&right.sub),
                right.unit.as_str(),
            );
            assert!(left_key <= right_key);
        }
    }

    #[test]
    fn spawn_debug_refresh_worker_emits_units_progress_and_finished() {
        let rx = spawn_debug_refresh_worker(Vec::new());
        let total = match rx
            .recv_timeout(Duration::from_millis(500))
            .expect("units message")
        {
            WorkerMsg::UnitsLoaded(rows) => {
                assert!(rows.len() <= MAX_DEBUG_UNITS);
                rows.len()
            }
            other => panic!("expected UnitsLoaded, got {other:?}"),
        };

        let mut progressed = 0usize;
        loop {
            match rx
                .recv_timeout(Duration::from_millis(500))
                .expect("worker message")
            {
                WorkerMsg::LogsProgress {
                    done,
                    total: t,
                    logs,
                } => {
                    assert_eq!(t, total);
                    assert!(!logs.is_empty());
                    progressed = done;
                }
                WorkerMsg::Finished => {
                    assert_eq!(progressed, total);
                    break;
                }
                other => panic!("unexpected message: {other:?}"),
            }
        }
    }

    #[test]
    fn spawn_debug_detail_worker_emits_fake_logs() {
        let rx = spawn_debug_detail_worker("debug-api-gateway.service".to_string(), 4);
        match rx
            .recv_timeout(Duration::from_millis(500))
            .expect("detail message")
        {
            WorkerMsg::DetailLogsLoaded {
                unit,
                request_id,
                logs,
            } => {
                assert_eq!(unit, "debug-api-gateway.service");
                assert_eq!(request_id, 4);
                assert_eq!(logs.len(), 12);
                assert!(logs[0].log.contains("load="));
            }
            other => panic!("expected DetailLogsLoaded, got {other:?}"),
        }
    }

    #[test]
    fn build_detail_logs_emits_valid_times() {
        let logs = build_detail_logs("debug-api-gateway.service");
        assert!(!logs.is_empty());

        for entry in logs {
            let parts: Vec<&str> = entry.time.split([' ', ':']).collect();
            assert_eq!(parts.len(), 4);

            let hour: u32 = parts[1].parse().expect("hour");
            let minute: u32 = parts[2].parse().expect("minute");
            let second: u32 = parts[3].parse().expect("second");

            assert_eq!(hour, 12);
            assert!(minute < 60);
            assert!(second < 60);
        }
    }

    #[test]
    fn spawn_debug_action_resolution_worker_uses_synthetic_state() {
        let rx = spawn_debug_action_resolution_worker(ActionResolutionRequest::StartStop {
            unit: "debug-api-gateway.service".to_string(),
        });
        match rx
            .recv_timeout(Duration::from_millis(500))
            .expect("resolution message")
        {
            WorkerMsg::ActionConfirmationReady { unit, confirmation } => {
                assert_eq!(unit, "debug-api-gateway.service");
                assert_eq!(confirmation, ConfirmationState::restart_or_stop(unit));
            }
            other => panic!("expected ActionConfirmationReady, got {other:?}"),
        }
    }

    #[test]
    fn debug_enable_disable_action_reaches_disable_and_enable_branches() {
        let active_template = template_for_unit("debug-api-gateway.service").expect("template");
        assert_eq!(
            debug_enable_disable_action(active_template),
            UnitAction::Disable
        );

        let inactive_template = template_for_unit("debug-cold-storage.service").expect("template");
        assert_eq!(
            debug_enable_disable_action(inactive_template),
            UnitAction::Enable
        );
    }

    #[test]
    fn spawn_debug_action_resolution_worker_can_resolve_disable_prompt() {
        let rx = spawn_debug_action_resolution_worker(ActionResolutionRequest::EnableDisable {
            unit: "debug-api-gateway.service".to_string(),
        });
        match rx
            .recv_timeout(Duration::from_millis(500))
            .expect("resolution message")
        {
            WorkerMsg::ActionConfirmationReady { unit, confirmation } => {
                assert_eq!(unit, "debug-api-gateway.service");
                assert_eq!(
                    confirmation,
                    ConfirmationState::confirm_action(UnitAction::Disable, unit)
                );
            }
            other => panic!("expected ActionConfirmationReady, got {other:?}"),
        }
    }

    #[test]
    fn spawn_debug_action_worker_emits_completion_for_known_units() {
        let rx =
            spawn_debug_action_worker("debug-api-gateway.service".to_string(), UnitAction::Restart);
        match rx
            .recv_timeout(Duration::from_millis(500))
            .expect("action message")
        {
            WorkerMsg::UnitActionComplete { unit, action } => {
                assert_eq!(unit, "debug-api-gateway.service");
                assert_eq!(action, UnitAction::Restart);
            }
            other => panic!("expected UnitActionComplete, got {other:?}"),
        }
    }
}

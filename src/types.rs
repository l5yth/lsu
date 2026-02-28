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

//! Shared domain and UI state types.

use ratatui::prelude::Style;
use serde::Deserialize;

/// Systemd unit scope.
#[derive(Debug, Clone, Copy)]
pub enum Scope {
    /// User-manager scope (`systemctl --user` / `journalctl --user`).
    User,
    /// System-manager scope (`systemctl --system` / `journalctl --system`).
    System,
}

impl Scope {
    /// Return the matching `systemctl`/`journalctl` scope flag.
    pub fn as_systemd_arg(&self) -> &'static str {
        match self {
            Self::System => "--system",
            Self::User => "--user",
        }
    }
}

/// JSON row returned by `systemctl list-units --output=json`.
#[derive(Debug, Clone, Deserialize)]
pub struct SystemctlUnit {
    /// Unit name, e.g. `sshd.service`.
    pub unit: String,
    /// Unit load state.
    pub load: String,
    /// Unit active state.
    pub active: String,
    /// Unit sub-state.
    pub sub: String,
    /// Human-readable unit description.
    pub description: String,
}

/// Render-ready row for the list table.
#[derive(Debug, Clone)]
pub struct UnitRow {
    /// Colored state marker rendered in the first table column.
    pub dot: char,
    /// Style for the state marker.
    pub dot_style: Style,
    /// Unit name.
    pub unit: String,
    /// Load state.
    pub load: String,
    /// Active state.
    pub active: String,
    /// Sub-state.
    pub sub: String,
    /// Description text.
    pub description: String,
    /// Last-known log preview line.
    pub last_log: String,
}

/// A single timestamped entry in the detail log view.
#[derive(Debug, Clone)]
pub struct DetailLogEntry {
    /// Timestamp value rendered in the detail view.
    pub time: String,
    /// Log message text.
    pub log: String,
}

/// Background loading phase for the list view.
#[derive(Debug, Clone, Copy)]
pub enum LoadPhase {
    /// No refresh currently running.
    Idle,
    /// Unit list fetch is in progress.
    FetchingUnits,
    /// Log batch fetch is in progress.
    FetchingLogs,
}

/// High-level screen mode.
#[derive(Debug, Clone, Copy)]
pub enum ViewMode {
    /// Service list screen.
    List,
    /// Per-unit detail log screen.
    Detail,
}

/// A systemd unit action that can be confirmed and executed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UnitAction {
    /// Start the unit.
    Start,
    /// Restart the unit.
    Restart,
    /// Stop the unit.
    Stop,
    /// Enable the unit.
    Enable,
    /// Disable the unit.
    Disable,
}

impl UnitAction {
    /// Return the `systemctl` subcommand for this action.
    pub fn as_systemctl_arg(self) -> &'static str {
        match self {
            Self::Start => "start",
            Self::Restart => "restart",
            Self::Stop => "stop",
            Self::Enable => "enable",
            Self::Disable => "disable",
        }
    }

    /// Return the present-participle verb used in confirmation prompts.
    pub fn prompt_verb(self) -> &'static str {
        match self {
            Self::Start => "starting",
            Self::Restart => "restarting",
            Self::Stop => "stopping",
            Self::Enable => "enabling",
            Self::Disable => "disabling",
        }
    }

    /// Return the past-tense verb used for completion messages.
    pub fn past_tense(self) -> &'static str {
        match self {
            Self::Start => "started",
            Self::Restart => "restarted",
            Self::Stop => "stopped",
            Self::Enable => "enabled",
            Self::Disable => "disabled",
        }
    }
}

/// The kind of confirmation prompt currently shown.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConfirmationKind {
    /// A yes/no prompt for one action.
    ConfirmAction(UnitAction),
    /// A running-unit prompt offering restart or stop.
    RestartOrStop,
}

/// A pending confirmation for a unit action.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConfirmationState {
    /// The prompt behavior to render and handle.
    pub kind: ConfirmationKind,
    /// Target unit name.
    pub unit: String,
}

impl ConfirmationState {
    /// Create a yes/no confirmation request for the given action and unit.
    pub fn confirm_action(action: UnitAction, unit: String) -> Self {
        Self {
            kind: ConfirmationKind::ConfirmAction(action),
            unit,
        }
    }

    /// Create a restart-or-stop prompt for a running unit.
    pub fn restart_or_stop(unit: String) -> Self {
        Self {
            kind: ConfirmationKind::RestartOrStop,
            unit,
        }
    }

    /// Return the action to execute when the prompt is a yes/no confirmation.
    pub fn confirmed_action(&self) -> Option<UnitAction> {
        match self.kind {
            ConfirmationKind::ConfirmAction(action) => Some(action),
            ConfirmationKind::RestartOrStop => None,
        }
    }
}

/// A request to resolve which action prompt should be shown for a unit.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ActionResolutionRequest {
    /// Resolve the start/restart/stop workflow from the current `ActiveState`.
    StartStop {
        /// Target unit name.
        unit: String,
    },
    /// Resolve the enable/disable workflow from the current `UnitFileState`.
    EnableDisable {
        /// Target unit name.
        unit: String,
    },
}

impl ActionResolutionRequest {
    /// Return the target unit for this request.
    pub fn unit(&self) -> &str {
        match self {
            Self::StartStop { unit, .. } | Self::EnableDisable { unit } => unit,
        }
    }
}

/// Detail pane state used by async log loading.
#[derive(Debug, Clone, Default)]
pub struct DetailState {
    /// Unit currently shown in detail view.
    pub unit: String,
    /// Loaded detail log entries.
    pub logs: Vec<DetailLogEntry>,
    /// Vertical scroll offset in `logs`.
    pub scroll: usize,
    /// Whether a detail fetch is in progress.
    pub loading: bool,
    /// Last detail fetch error, if any.
    pub error: Option<String>,
    next_request_id: u64,
    active_request_id: Option<u64>,
}

impl DetailState {
    /// Enter detail mode for a unit and start an async fetch request.
    pub fn begin_for_unit(&mut self, unit: String) -> u64 {
        self.unit = unit;
        self.logs.clear();
        self.scroll = 0;
        self.loading = true;
        self.error = None;
        self.next_request_id = self.next_request_id.saturating_add(1);
        self.active_request_id = Some(self.next_request_id);
        self.next_request_id
    }

    /// Trigger an async refresh for the current unit while keeping existing rows visible.
    pub fn refresh(&mut self) -> Option<u64> {
        if self.unit.is_empty() {
            return None;
        }
        self.loading = true;
        self.error = None;
        self.next_request_id = self.next_request_id.saturating_add(1);
        self.active_request_id = Some(self.next_request_id);
        Some(self.next_request_id)
    }

    /// Apply an async detail-log payload when it matches the active request.
    pub fn apply_loaded(&mut self, request_id: u64, unit: &str, logs: Vec<DetailLogEntry>) -> bool {
        if self.active_request_id != Some(request_id) || self.unit != unit {
            return false;
        }
        self.logs = logs;
        if self.logs.is_empty() {
            self.scroll = 0;
        } else {
            self.scroll = std::cmp::min(self.scroll, self.logs.len() - 1);
        }
        self.loading = false;
        self.error = None;
        true
    }

    /// Apply an async error when it matches the active request.
    pub fn apply_error(&mut self, request_id: u64, unit: &str, error: String) -> bool {
        if self.active_request_id != Some(request_id) || self.unit != unit {
            return false;
        }
        self.loading = false;
        self.error = Some(error);
        true
    }
}

/// Messages sent from the background worker thread to the UI thread.
#[derive(Debug)]
pub enum WorkerMsg {
    /// Unit rows were loaded and should replace/initialize the table.
    UnitsLoaded(Vec<UnitRow>),
    /// A partial batch of list log updates is ready.
    LogsProgress {
        /// Number of rows processed so far.
        done: usize,
        /// Total rows targeted for this refresh.
        total: usize,
        /// `(unit, last_log)` pairs for this batch.
        logs: Vec<(String, String)>,
    },
    /// Detail logs loaded for a request id/unit pair.
    DetailLogsLoaded {
        /// Unit for which the logs were requested.
        unit: String,
        /// Monotonic request identifier.
        request_id: u64,
        /// Loaded log entries.
        logs: Vec<DetailLogEntry>,
    },
    /// Detail log request failed for a request id/unit pair.
    DetailLogsError {
        /// Unit for which the logs were requested.
        unit: String,
        /// Monotonic request identifier.
        request_id: u64,
        /// Error text to show in the UI.
        error: String,
    },
    /// A confirmation prompt was resolved and is ready to show.
    ActionConfirmationReady {
        /// Target unit name.
        unit: String,
        /// Resolved confirmation prompt.
        confirmation: ConfirmationState,
    },
    /// Resolving a unit action prompt failed.
    ActionResolutionError {
        /// Unit for which the prompt was being resolved.
        unit: String,
        /// Error text to show in the UI.
        error: String,
    },
    /// A unit action completed successfully.
    UnitActionComplete {
        /// Unit for which the action was executed.
        unit: String,
        /// Executed action.
        action: UnitAction,
    },
    /// A unit action failed.
    UnitActionError {
        /// Unit for which the action was attempted.
        unit: String,
        /// Action that failed.
        action: UnitAction,
        /// Error text to show in the UI.
        error: String,
    },
    /// Refresh worker finished all tasks.
    Finished,
    /// Refresh worker failed with a terminal error.
    Error(String),
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_log(text: &str) -> DetailLogEntry {
        DetailLogEntry {
            time: "t".to_string(),
            log: text.to_string(),
        }
    }

    #[test]
    fn detail_state_begin_sets_loading_and_resets_scroll() {
        let mut state = DetailState {
            scroll: 7,
            ..DetailState::default()
        };
        let id = state.begin_for_unit("a.service".to_string());
        assert_eq!(id, 1);
        assert!(state.loading);
        assert_eq!(state.scroll, 0);
        assert_eq!(state.unit, "a.service");
    }

    #[test]
    fn detail_state_ignores_stale_async_responses() {
        let mut state = DetailState::default();
        let id1 = state.begin_for_unit("a.service".to_string());
        let id2 = state.begin_for_unit("b.service".to_string());
        assert_ne!(id1, id2);
        assert!(!state.apply_loaded(id1, "a.service", vec![sample_log("old")]));
        assert!(state.apply_loaded(id2, "b.service", vec![sample_log("new")]));
        assert_eq!(state.logs.len(), 1);
        assert_eq!(state.logs[0].log, "new");
    }

    #[test]
    fn detail_state_refresh_keeps_logs_and_updates_loading() {
        let mut state = DetailState::default();
        let first = state.begin_for_unit("a.service".to_string());
        assert!(state.apply_loaded(first, "a.service", vec![sample_log("x"), sample_log("y")]));
        state.scroll = 1;
        let refresh_id = state.refresh().expect("refresh id");
        assert!(state.loading);
        assert_eq!(state.scroll, 1);
        assert_eq!(state.logs.len(), 2);
        assert!(state.apply_loaded(refresh_id, "a.service", vec![sample_log("z")]));
        assert_eq!(state.scroll, 0);
        assert_eq!(state.logs[0].log, "z");
    }

    #[test]
    fn detail_state_refresh_returns_none_without_unit() {
        let mut state = DetailState::default();
        assert!(state.refresh().is_none());
    }

    #[test]
    fn detail_state_apply_error_sets_error_and_stops_loading() {
        let mut state = DetailState::default();
        let id = state.begin_for_unit("a.service".to_string());
        assert!(state.apply_error(id, "a.service", "boom".to_string()));
        assert!(!state.loading);
        assert_eq!(state.error.as_deref(), Some("boom"));
    }

    #[test]
    fn detail_state_apply_error_ignores_mismatched_request() {
        let mut state = DetailState::default();
        let id = state.begin_for_unit("a.service".to_string());
        assert!(!state.apply_error(id + 1, "a.service", "boom".to_string()));
        assert!(!state.apply_error(id, "b.service", "boom".to_string()));
    }

    #[test]
    fn detail_state_apply_loaded_empty_logs_resets_scroll() {
        let mut state = DetailState::default();
        let id = state.begin_for_unit("a.service".to_string());
        state.scroll = 10;
        assert!(state.apply_loaded(id, "a.service", Vec::new()));
        assert_eq!(state.scroll, 0);
        assert!(state.logs.is_empty());
    }

    #[test]
    fn detail_state_switching_unit_clears_old_logs_immediately() {
        let mut state = DetailState::default();
        let id = state.begin_for_unit("a.service".to_string());
        assert!(state.apply_loaded(id, "a.service", vec![sample_log("old")]));
        assert_eq!(state.logs.len(), 1);
        let _ = state.begin_for_unit("b.service".to_string());
        assert!(state.logs.is_empty());
        assert_eq!(state.scroll, 0);
        assert!(state.loading);
    }

    #[test]
    fn scope_maps_to_expected_systemd_args() {
        assert_eq!(Scope::System.as_systemd_arg(), "--system");
        assert_eq!(Scope::User.as_systemd_arg(), "--user");
    }

    #[test]
    fn all_view_and_load_phase_variants_are_constructible() {
        let list_mode = ViewMode::List;
        let detail_mode = ViewMode::Detail;
        let idle = LoadPhase::Idle;
        let fetching_units = LoadPhase::FetchingUnits;
        let fetching_logs = LoadPhase::FetchingLogs;
        assert!(matches!(list_mode, ViewMode::List));
        assert!(matches!(detail_mode, ViewMode::Detail));
        assert!(matches!(idle, LoadPhase::Idle));
        assert!(matches!(fetching_units, LoadPhase::FetchingUnits));
        assert!(matches!(fetching_logs, LoadPhase::FetchingLogs));
    }

    #[test]
    fn unit_action_labels_match_expected_systemctl_and_prompt_text() {
        assert_eq!(UnitAction::Start.as_systemctl_arg(), "start");
        assert_eq!(UnitAction::Restart.as_systemctl_arg(), "restart");
        assert_eq!(UnitAction::Stop.prompt_verb(), "stopping");
        assert_eq!(UnitAction::Enable.past_tense(), "enabled");
        assert_eq!(UnitAction::Disable.past_tense(), "disabled");
    }

    #[test]
    fn confirmation_state_builders_capture_kind_and_unit() {
        let confirmation =
            ConfirmationState::confirm_action(UnitAction::Start, "demo.service".to_string());
        assert_eq!(
            confirmation.kind,
            ConfirmationKind::ConfirmAction(UnitAction::Start)
        );
        assert_eq!(confirmation.unit, "demo.service");

        let restart_or_stop = ConfirmationState::restart_or_stop("run.service".to_string());
        assert_eq!(restart_or_stop.kind, ConfirmationKind::RestartOrStop);
        assert_eq!(restart_or_stop.confirmed_action(), None);
    }

    #[test]
    fn action_resolution_request_exposes_target_unit() {
        let start_stop = ActionResolutionRequest::StartStop {
            unit: "demo.service".to_string(),
        };
        assert_eq!(start_stop.unit(), "demo.service");

        let enable_disable = ActionResolutionRequest::EnableDisable {
            unit: "other.service".to_string(),
        };
        assert_eq!(enable_disable.unit(), "other.service");
    }
}

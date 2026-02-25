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
    User,
    System,
}

impl Scope {
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
    pub unit: String,
    pub load: String,
    pub active: String,
    pub sub: String,
    pub description: String,
}

/// Render-ready row for the list table.
#[derive(Debug, Clone)]
pub struct UnitRow {
    pub dot: char,
    pub dot_style: Style,
    pub unit: String,
    pub load: String,
    pub active: String,
    pub sub: String,
    pub description: String,
    pub last_log: String,
}

/// A single timestamped entry in the detail log view.
#[derive(Debug, Clone)]
pub struct DetailLogEntry {
    pub time: String,
    pub log: String,
}

/// Background loading phase for the list view.
#[derive(Debug, Clone, Copy)]
pub enum LoadPhase {
    Idle,
    FetchingUnits,
    FetchingLogs,
}

/// High-level screen mode.
#[derive(Debug, Clone, Copy)]
pub enum ViewMode {
    List,
    Detail,
}

/// Detail pane state used by async log loading.
#[derive(Debug, Clone, Default)]
pub struct DetailState {
    pub unit: String,
    pub logs: Vec<DetailLogEntry>,
    pub scroll: usize,
    pub loading: bool,
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
    UnitsLoaded(Vec<UnitRow>),
    LogsProgress {
        done: usize,
        total: usize,
        logs: Vec<(String, String)>,
    },
    DetailLogsLoaded {
        unit: String,
        request_id: u64,
        logs: Vec<DetailLogEntry>,
    },
    DetailLogsError {
        unit: String,
        request_id: u64,
        error: String,
    },
    Finished,
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
}

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

/// Messages sent from the background worker thread to the UI thread.
pub enum WorkerMsg {
    UnitsLoaded(Vec<UnitRow>),
    LogsProgress {
        done: usize,
        total: usize,
        logs: Vec<(String, String)>,
    },
    Finished,
    Error(String),
}

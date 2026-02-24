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

//! Transform and sort logic for list-table rows.

use ratatui::prelude::{Color, Style};

use crate::types::{SystemctlUnit, UnitRow};

/// Select status indicator glyph and color based on active/sub state.
pub fn status_dot(active: &str, sub: &str) -> (char, Style) {
    match (active, sub) {
        ("active", "running") => ('●', Style::default().fg(Color::Green)),
        ("active", _) => ('●', Style::default().fg(Color::Yellow)),
        ("inactive", _) => ('●', Style::default().fg(Color::DarkGray)),
        ("failed", _) => ('●', Style::default().fg(Color::Red)),
        _ => ('●', Style::default().fg(Color::Blue)),
    }
}

/// Sort rank for `load` in `--all` mode.
pub fn load_rank(load: &str) -> u8 {
    match load {
        "loaded" => 0,
        "not-found" => 1,
        _ => 2,
    }
}

/// Sort rank for `active` in `--all` mode.
pub fn active_rank(active: &str) -> u8 {
    match active {
        "active" => 0,
        "inactive" => 1,
        _ => 2,
    }
}

/// Sort rank for `sub` in `--all` mode.
pub fn sub_rank(sub: &str) -> u8 {
    match sub {
        "running" => 0,
        "exited" => 1,
        "dead" => 2,
        _ => 3,
    }
}

/// Build render rows from raw systemctl units.
pub fn build_rows(units: Vec<SystemctlUnit>) -> Vec<UnitRow> {
    units
        .into_iter()
        .map(|u| {
            let (dot, dot_style) = status_dot(&u.active, &u.sub);
            UnitRow {
                dot,
                dot_style,
                unit: u.unit,
                load: u.load,
                active: u.active,
                sub: u.sub,
                description: u.description,
                last_log: String::new(),
            }
        })
        .collect()
}

/// Sort rows according to mode-specific ordering.
pub fn sort_rows(rows: &mut [UnitRow], show_all: bool) {
    if show_all {
        rows.sort_by(|a, b| {
            (
                load_rank(&a.load),
                active_rank(&a.active),
                sub_rank(&a.sub),
                a.unit.as_str(),
            )
                .cmp(&(
                    load_rank(&b.load),
                    active_rank(&b.active),
                    sub_rank(&b.sub),
                    b.unit.as_str(),
                ))
        });
    } else {
        rows.sort_by(|a, b| a.unit.cmp(&b.unit));
    }
}

/// Carry over previously shown log cells by unit name.
pub fn seed_logs_from_previous(new_rows: &mut [UnitRow], previous_rows: &[UnitRow]) {
    let previous_logs: std::collections::HashMap<&str, &str> = previous_rows
        .iter()
        .map(|r| (r.unit.as_str(), r.last_log.as_str()))
        .collect();
    for row in new_rows.iter_mut() {
        if let Some(old_log) = previous_logs.get(row.unit.as_str()) {
            row.last_log = (*old_log).to_string();
        }
    }
}

/// Keep current row selection stable across refreshes and reorders.
pub fn preserve_selection(prev_unit: Option<String>, rows: &[UnitRow], selected_idx: &mut usize) {
    if rows.is_empty() {
        *selected_idx = 0;
        return;
    }
    if let Some(unit) = prev_unit
        && let Some(idx) = rows.iter().position(|r| r.unit == unit)
    {
        *selected_idx = idx;
        return;
    }
    if *selected_idx >= rows.len() {
        *selected_idx = rows.len().saturating_sub(1);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn status_dot_maps_expected_colors() {
        let (dot, style) = status_dot("active", "running");
        assert_eq!(dot, '●');
        assert_eq!(style, Style::default().fg(Color::Green));

        let (dot, style) = status_dot("failed", "dead");
        assert_eq!(dot, '●');
        assert_eq!(style, Style::default().fg(Color::Red));

        let (dot, style) = status_dot("inactive", "dead");
        assert_eq!(dot, '●');
        assert_eq!(style, Style::default().fg(Color::DarkGray));

        let (_, style) = status_dot("active", "exited");
        assert_eq!(style, Style::default().fg(Color::Yellow));

        let (_, style) = status_dot("reloading", "foo");
        assert_eq!(style, Style::default().fg(Color::Blue));
    }

    #[test]
    fn ranks_for_all_sort_order_match_spec() {
        assert!(load_rank("loaded") < load_rank("not-found"));
        assert!(load_rank("not-found") < load_rank("masked"));

        assert!(active_rank("active") < active_rank("inactive"));
        assert!(active_rank("inactive") < active_rank("failed"));

        assert!(sub_rank("running") < sub_rank("exited"));
        assert!(sub_rank("exited") < sub_rank("dead"));
        assert!(sub_rank("dead") < sub_rank("auto-restart"));
    }

    #[test]
    fn sort_rows_all_mode_respects_priority_order() {
        let mut rows = vec![
            UnitRow {
                dot: '●',
                dot_style: Style::default(),
                unit: "z.service".to_string(),
                load: "not-found".to_string(),
                active: "inactive".to_string(),
                sub: "dead".to_string(),
                description: String::new(),
                last_log: String::new(),
            },
            UnitRow {
                dot: '●',
                dot_style: Style::default(),
                unit: "a.service".to_string(),
                load: "loaded".to_string(),
                active: "active".to_string(),
                sub: "running".to_string(),
                description: String::new(),
                last_log: String::new(),
            },
            UnitRow {
                dot: '●',
                dot_style: Style::default(),
                unit: "m.service".to_string(),
                load: "masked".to_string(),
                active: "failed".to_string(),
                sub: "auto-restart".to_string(),
                description: String::new(),
                last_log: String::new(),
            },
        ];

        sort_rows(&mut rows, true);
        assert_eq!(rows[0].unit, "a.service");
        assert_eq!(rows[1].unit, "z.service");
        assert_eq!(rows[2].unit, "m.service");
    }

    #[test]
    fn sort_rows_running_mode_sorts_by_unit_name_only() {
        let mut rows = vec![
            UnitRow {
                dot: '●',
                dot_style: Style::default(),
                unit: "z.service".to_string(),
                load: "loaded".to_string(),
                active: "active".to_string(),
                sub: "running".to_string(),
                description: String::new(),
                last_log: String::new(),
            },
            UnitRow {
                dot: '●',
                dot_style: Style::default(),
                unit: "a.service".to_string(),
                load: "not-found".to_string(),
                active: "failed".to_string(),
                sub: "dead".to_string(),
                description: String::new(),
                last_log: String::new(),
            },
        ];
        sort_rows(&mut rows, false);
        assert_eq!(rows[0].unit, "a.service");
        assert_eq!(rows[1].unit, "z.service");
    }

    #[test]
    fn seed_logs_from_previous_preserves_known_logs_by_unit() {
        let previous = vec![UnitRow {
            dot: '●',
            dot_style: Style::default(),
            unit: "a.service".to_string(),
            load: "loaded".to_string(),
            active: "active".to_string(),
            sub: "running".to_string(),
            description: String::new(),
            last_log: "old message".to_string(),
        }];

        let mut new_rows = vec![
            UnitRow {
                dot: '●',
                dot_style: Style::default(),
                unit: "a.service".to_string(),
                load: "loaded".to_string(),
                active: "active".to_string(),
                sub: "running".to_string(),
                description: String::new(),
                last_log: String::new(),
            },
            UnitRow {
                dot: '●',
                dot_style: Style::default(),
                unit: "b.service".to_string(),
                load: "loaded".to_string(),
                active: "active".to_string(),
                sub: "running".to_string(),
                description: String::new(),
                last_log: String::new(),
            },
        ];

        seed_logs_from_previous(&mut new_rows, &previous);
        assert_eq!(new_rows[0].last_log, "old message");
        assert_eq!(new_rows[1].last_log, "");
    }

    #[test]
    fn preserve_selection_keeps_same_unit_after_reorder() {
        let rows = vec![
            UnitRow {
                dot: '●',
                dot_style: Style::default(),
                unit: "a.service".to_string(),
                load: "loaded".to_string(),
                active: "active".to_string(),
                sub: "running".to_string(),
                description: String::new(),
                last_log: String::new(),
            },
            UnitRow {
                dot: '●',
                dot_style: Style::default(),
                unit: "b.service".to_string(),
                load: "loaded".to_string(),
                active: "active".to_string(),
                sub: "running".to_string(),
                description: String::new(),
                last_log: String::new(),
            },
        ];
        let mut idx = 0;
        preserve_selection(Some("b.service".to_string()), &rows, &mut idx);
        assert_eq!(idx, 1);
    }

    #[test]
    fn preserve_selection_handles_empty_rows() {
        let mut idx = 5;
        preserve_selection(Some("b.service".to_string()), &[], &mut idx);
        assert_eq!(idx, 0);
    }

    #[test]
    fn preserve_selection_clamps_out_of_range_index() {
        let rows = vec![UnitRow {
            dot: '●',
            dot_style: Style::default(),
            unit: "only.service".to_string(),
            load: "loaded".to_string(),
            active: "active".to_string(),
            sub: "running".to_string(),
            description: String::new(),
            last_log: String::new(),
        }];
        let mut idx = 9;
        preserve_selection(None, &rows, &mut idx);
        assert_eq!(idx, 0);
    }
}

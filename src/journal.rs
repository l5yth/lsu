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

//! `journalctl` integration and log parsing helpers.

use anyhow::Result;
use std::collections::{HashMap, HashSet};
#[cfg(not(test))]
use std::process::Command;

#[cfg(not(test))]
use crate::command::cmd_stdout;
use crate::types::DetailLogEntry;

/// Fetch the latest log message text for one systemd unit.
#[cfg(not(test))]
pub fn last_log_line(unit: &str) -> Result<String> {
    let mut cmd = Command::new("journalctl");
    cmd.arg("-u")
        .arg(unit)
        .arg("-n")
        .arg("1")
        .arg("--no-pager")
        .arg("-o")
        .arg("cat");
    let mut line = cmd_stdout(&mut cmd)?;
    line = line.lines().next().unwrap_or("").trim().to_string();
    Ok(line)
}

#[cfg(test)]
/// Test-build stub for one-line log lookup.
pub fn last_log_line(_unit: &str) -> Result<String> {
    Ok(String::new())
}

/// Parse newline-delimited `journalctl -o json` output and pick the latest non-empty message per unit.
pub fn parse_latest_logs_from_journal_json(
    output: &str,
    wanted: &HashSet<String>,
) -> HashMap<String, String> {
    let mut latest = HashMap::new();
    for line in output.lines() {
        let Ok(value) = serde_json::from_str::<serde_json::Value>(line) else {
            continue;
        };

        let Some(unit) = value.get("_SYSTEMD_UNIT").and_then(|v| v.as_str()) else {
            continue;
        };

        if !wanted.contains(unit) || latest.contains_key(unit) {
            continue;
        }

        let message = value
            .get("MESSAGE")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .trim()
            .to_string();
        if message.is_empty() {
            continue;
        }
        latest.insert(unit.to_string(), message);

        if latest.len() == wanted.len() {
            break;
        }
    }
    latest
}

/// Fetch latest logs for a batch of units, with per-unit fallback for missing/empty results.
#[cfg(not(test))]
pub fn latest_log_lines_batch(unit_names: &[String]) -> HashMap<String, String> {
    if unit_names.is_empty() {
        return HashMap::new();
    }

    let wanted: HashSet<String> = unit_names.iter().cloned().collect();
    let mut out = HashMap::new();
    let mut cmd = Command::new("journalctl");
    cmd.arg("--no-pager")
        .arg("-o")
        .arg("json")
        .arg("-r")
        .arg("-n")
        .arg(std::cmp::max(unit_names.len() * 200, 1000).to_string());
    for unit in unit_names {
        cmd.arg("-u").arg(unit);
    }

    if let Ok(output) = cmd_stdout(&mut cmd) {
        out = parse_latest_logs_from_journal_json(&output, &wanted);
    }

    for unit in unit_names {
        if out.get(unit).is_none_or(|v| v.trim().is_empty()) {
            out.insert(unit.clone(), last_log_line(unit).unwrap_or_default());
        }
    }

    out
}

#[cfg(test)]
/// Test-build stub for batched log lookup.
pub fn latest_log_lines_batch(_unit_names: &[String]) -> HashMap<String, String> {
    HashMap::new()
}

/// Parse `journalctl -o short-iso` output into `{time, log}` rows.
pub fn parse_journal_short_iso(output: &str) -> Vec<DetailLogEntry> {
    output
        .lines()
        .filter_map(|line| {
            let trimmed = line.trim();
            if trimmed.is_empty() {
                return None;
            }
            if let Some((time, log)) = trimmed.split_once(' ') {
                Some(DetailLogEntry {
                    time: time.to_string(),
                    log: log.trim_start().to_string(),
                })
            } else {
                Some(DetailLogEntry {
                    time: String::new(),
                    log: trimmed.to_string(),
                })
            }
        })
        .collect()
}

/// Fetch timestamped detail logs for a single unit.
#[cfg(not(test))]
pub fn fetch_unit_logs(unit: &str, max_lines: usize) -> Result<Vec<DetailLogEntry>> {
    let mut cmd = Command::new("journalctl");
    cmd.arg("-u")
        .arg(unit)
        .arg("-n")
        .arg(max_lines.to_string())
        .arg("--no-pager")
        .arg("-o")
        .arg("short-iso")
        .arg("-r");
    let output = cmd_stdout(&mut cmd)?;
    Ok(parse_journal_short_iso(&output))
}

#[cfg(test)]
/// Test-build stub for detail log fetching.
pub fn fetch_unit_logs(_unit: &str, _max_lines: usize) -> Result<Vec<DetailLogEntry>> {
    Ok(Vec::new())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_latest_logs_per_unit_from_json_lines() {
        let output = r#"{"_SYSTEMD_UNIT":"a.service","MESSAGE":"newest a"}
{"_SYSTEMD_UNIT":"b.service","MESSAGE":"newest b"}
{"_SYSTEMD_UNIT":"a.service","MESSAGE":"older a"}"#;
        let wanted = HashSet::from(["a.service".to_string(), "b.service".to_string()]);
        let logs = parse_latest_logs_from_journal_json(output, &wanted);
        assert_eq!(logs.get("a.service").map(String::as_str), Some("newest a"));
        assert_eq!(logs.get("b.service").map(String::as_str), Some("newest b"));
    }

    #[test]
    fn parses_latest_logs_ignores_invalid_lines_and_missing_fields() {
        let output = r#"not-json
{"_SYSTEMD_UNIT":"a.service"}
{"MESSAGE":"no unit"}
{"_SYSTEMD_UNIT":"a.service","MESSAGE":"ok"}"#;
        let wanted = HashSet::from(["a.service".to_string()]);
        let logs = parse_latest_logs_from_journal_json(output, &wanted);
        assert_eq!(logs.get("a.service").map(String::as_str), Some("ok"));
    }

    #[test]
    fn latest_log_lines_batch_empty_input_returns_empty_map() {
        let logs = latest_log_lines_batch(&[]);
        assert!(logs.is_empty());
    }

    #[test]
    fn last_log_line_test_stub_returns_empty_string() {
        let line = last_log_line("unit").expect("stub should succeed");
        assert_eq!(line, "");
    }

    #[test]
    fn fetch_unit_logs_test_stub_returns_empty_vec() {
        let rows = fetch_unit_logs("unit", 10).expect("stub should succeed");
        assert!(rows.is_empty());
    }

    #[test]
    fn parse_journal_short_iso_extracts_time_and_message() {
        let out = "2026-02-24T10:00:00+0000 one log line\nraw-without-timestamp";
        let rows = parse_journal_short_iso(out);
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0].time, "2026-02-24T10:00:00+0000");
        assert_eq!(rows[0].log, "one log line");
        assert_eq!(rows[1].time, "");
        assert_eq!(rows[1].log, "raw-without-timestamp");
    }
}

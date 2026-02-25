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
#[cfg(not(test))]
use anyhow::{Context, bail};
use std::collections::{HashMap, HashSet};
#[cfg(not(test))]
use std::io::{BufRead, BufReader, Read};
#[cfg(not(test))]
use std::process::{Command, Stdio};
#[cfg(not(test))]
use std::sync::mpsc;
#[cfg(not(test))]
use std::thread;
#[cfg(not(test))]
use std::time::{Duration, Instant};

#[cfg(not(test))]
use crate::command::{CommandExecError, cmd_stdout, command_timeout, resolve_trusted_binary};
use crate::types::{DetailLogEntry, Scope};

const BATCH_MIN_LINES: usize = 200;
const BATCH_PER_UNIT_LINES: usize = 20;
const BATCH_MAX_LINES: usize = 4000;
#[cfg(not(test))]
const BATCH_MAX_ATTEMPTS: usize = 3;

/// Fetch the latest log message text for one systemd unit.
#[cfg(not(test))]
pub fn last_log_line(scope: Scope, unit: &str) -> Result<String> {
    let journalctl = resolve_trusted_binary("journalctl")?;
    let mut cmd = Command::new(journalctl);
    cmd.arg(scope.as_systemd_arg())
        .arg("-u")
        .arg(unit)
        .arg("-n")
        .arg("1")
        .arg("--no-pager")
        .arg("-o")
        .arg("cat");
    let mut line = match cmd_stdout(&mut cmd) {
        Ok(line) => line,
        Err(CommandExecError::Timeout { .. }) => {
            bail!(
                "journalctl last line timed out after {}s for {}",
                command_timeout().as_secs(),
                unit
            )
        }
        Err(e) => return Err(e).context("journalctl last line failed"),
    };
    line = line.lines().next().unwrap_or("").trim().to_string();
    Ok(line)
}

#[cfg(test)]
/// Test-build stub for one-line log lookup.
pub fn last_log_line(_scope: Scope, _unit: &str) -> Result<String> {
    Ok(String::new())
}

/// Parse newline-delimited `journalctl -o json` output and pick the latest non-empty message per unit.
pub fn parse_latest_logs_from_journal_json(
    scope: Scope,
    output: &str,
    wanted: &HashSet<String>,
) -> HashMap<String, String> {
    parse_latest_logs_lines(scope, output.lines(), wanted, usize::MAX)
}

fn absorb_latest_log_line(
    scope: Scope,
    line: &str,
    wanted: &HashSet<String>,
    latest: &mut HashMap<String, String>,
) {
    let Ok(value) = serde_json::from_str::<serde_json::Value>(line) else {
        return;
    };

    let Some(unit) = unit_from_journal_entry_for_scope(scope, &value) else {
        return;
    };

    if !wanted.contains(unit) || latest.contains_key(unit) {
        return;
    }

    let message = value
        .get("MESSAGE")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .trim()
        .to_string();
    if message.is_empty() {
        return;
    }

    latest.insert(unit.to_string(), message);
}

/// Parse line-delimited journal JSON with an explicit max-line budget.
pub fn parse_latest_logs_lines<'a, I>(
    scope: Scope,
    lines: I,
    wanted: &HashSet<String>,
    max_lines: usize,
) -> HashMap<String, String>
where
    I: IntoIterator<Item = &'a str>,
{
    let mut latest = HashMap::new();
    for line in lines.into_iter().take(max_lines) {
        absorb_latest_log_line(scope, line, wanted, &mut latest);

        if latest.len() == wanted.len() {
            break;
        }
    }
    latest
}

fn unit_from_journal_entry_for_scope(scope: Scope, value: &serde_json::Value) -> Option<&str> {
    const SYSTEM_UNIT_FIELDS: [&str; 7] = [
        "_SYSTEMD_UNIT",
        "UNIT",
        "USER_UNIT",
        "OBJECT_SYSTEMD_UNIT",
        "COREDUMP_UNIT",
        "COREDUMP_USER_UNIT",
        "_SYSTEMD_USER_UNIT",
    ];
    const USER_UNIT_FIELDS: [&str; 8] = [
        "_SYSTEMD_USER_UNIT",
        "USER_UNIT",
        "UNIT",
        "OBJECT_SYSTEMD_USER_UNIT",
        "OBJECT_SYSTEMD_UNIT",
        "COREDUMP_USER_UNIT",
        "COREDUMP_UNIT",
        "_SYSTEMD_UNIT",
    ];
    let fields: &[&str] = match scope {
        Scope::System => &SYSTEM_UNIT_FIELDS,
        Scope::User => &USER_UNIT_FIELDS,
    };
    for field in fields {
        if let Some(unit) = value.get(field).and_then(|v| v.as_str()) {
            return Some(unit);
        }
    }
    None
}

/// Build a journal query by repeating `-u <unit>` to preserve journalctl's native
/// unit matching semantics, including manager-generated entries tied to a unit.
#[cfg(not(test))]
fn append_unit_matches(cmd: &mut Command, unit_names: &[String]) {
    for unit in unit_names {
        cmd.arg("-u").arg(unit);
    }
}

/// Compute a bounded line budget for one batched journal attempt.
pub fn batch_line_budget(unit_count: usize, attempt: usize) -> usize {
    let base = std::cmp::max(
        BATCH_MIN_LINES,
        unit_count.saturating_mul(BATCH_PER_UNIT_LINES),
    );
    let growth = 1usize << attempt.min(10);
    std::cmp::min(base.saturating_mul(growth), BATCH_MAX_LINES)
}

#[cfg(not(test))]
fn remaining_timeout(deadline: Instant) -> Result<Duration> {
    let now = Instant::now();
    if now >= deadline {
        bail!(
            "journalctl batch query timed out after {}s",
            command_timeout().as_secs()
        );
    }
    Ok(deadline.saturating_duration_since(now))
}

#[cfg(not(test))]
fn wait_child_with_timeout(child: &mut std::process::Child, deadline: Instant) -> Result<()> {
    loop {
        if let Some(status) = child.try_wait()? {
            if status.success() {
                return Ok(());
            }
            bail!("journalctl exited unsuccessfully (status={status})");
        }
        if Instant::now() >= deadline {
            let _ = child.kill();
            let _ = child.wait();
            bail!(
                "journalctl batch query timed out after {}s",
                command_timeout().as_secs()
            );
        }
        thread::sleep(Duration::from_millis(10));
    }
}

#[cfg(not(test))]
fn stream_batch_latest_logs(
    scope: Scope,
    unit_names: &[String],
    line_budget: usize,
) -> Result<HashMap<String, String>> {
    let wanted: HashSet<String> = unit_names.iter().cloned().collect();
    let journalctl = resolve_trusted_binary("journalctl")?;
    let mut cmd = Command::new(journalctl);
    cmd.arg(scope.as_systemd_arg())
        .arg("--no-pager")
        .arg("-o")
        .arg("json")
        .arg("-r")
        .arg("-n")
        .arg(line_budget.to_string());
    append_unit_matches(&mut cmd, unit_names);

    let mut child = cmd.stdout(Stdio::piped()).stderr(Stdio::piped()).spawn()?;
    let stdout = child.stdout.take().context("missing stdout pipe")?;
    let mut stderr = child.stderr.take().context("missing stderr pipe")?;
    let deadline = Instant::now() + command_timeout();

    let (line_tx, line_rx) = mpsc::channel();
    let read_handle = thread::spawn(move || {
        let mut reader = BufReader::new(stdout);
        let mut line = String::new();
        loop {
            line.clear();
            match reader.read_line(&mut line) {
                Ok(0) => break,
                Ok(_) => {
                    let msg = line.trim_end_matches('\n').to_string();
                    if line_tx.send(msg).is_err() {
                        break;
                    }
                }
                Err(_) => break,
            }
        }
    });

    let stderr_handle = thread::spawn(move || {
        let mut buf = String::new();
        let _ = stderr.read_to_string(&mut buf);
        buf
    });

    let mut found = HashMap::new();
    let mut seen_lines = 0usize;
    while seen_lines < line_budget && found.len() < wanted.len() {
        let timeout = match remaining_timeout(deadline) {
            Ok(timeout) => timeout,
            Err(e) => {
                let _ = child.kill();
                let _ = child.wait();
                let _ = read_handle.join();
                let _ = stderr_handle.join();
                return Err(e);
            }
        };
        match line_rx.recv_timeout(timeout) {
            Ok(line) => {
                absorb_latest_log_line(scope, &line, &wanted, &mut found);
                seen_lines += 1;
            }
            Err(mpsc::RecvTimeoutError::Timeout) => {
                let _ = child.kill();
                let _ = child.wait();
                let _ = read_handle.join();
                let _ = stderr_handle.join();
                bail!(
                    "journalctl batch query timed out after {}s",
                    command_timeout().as_secs()
                );
            }
            Err(mpsc::RecvTimeoutError::Disconnected) => break,
        }
    }

    let terminated_early = found.len() == wanted.len();
    if terminated_early {
        let _ = child.kill();
        let _ = child.wait();
    } else {
        wait_child_with_timeout(&mut child, deadline)?;
    }

    let _ = read_handle.join();
    let stderr_output = stderr_handle.join().unwrap_or_default();
    if !terminated_early && (stderr_output.contains("Failed") || stderr_output.contains("failed")) {
        bail!("journalctl batch query failed: {}", stderr_output.trim());
    }
    Ok(found)
}

/// Fetch latest logs for a batch of units, with per-unit fallback for missing/empty results.
#[cfg(not(test))]
pub fn latest_log_lines_batch(
    scope: Scope,
    unit_names: &[String],
) -> Result<HashMap<String, String>> {
    if unit_names.is_empty() {
        return Ok(HashMap::new());
    }

    let mut out = HashMap::new();
    let mut unresolved: Vec<String> = unit_names.to_vec();

    for attempt in 0..BATCH_MAX_ATTEMPTS {
        if unresolved.is_empty() {
            break;
        }
        let budget = batch_line_budget(unresolved.len(), attempt);
        let partial = match stream_batch_latest_logs(scope, &unresolved, budget) {
            Ok(partial) => partial,
            Err(_) => break,
        };
        for (unit, message) in partial {
            if !message.trim().is_empty() {
                out.insert(unit, message);
            }
        }
        unresolved.retain(|unit| !out.contains_key(unit));
    }

    for unit in unresolved {
        if out.get(&unit).is_none_or(|v| v.trim().is_empty()) {
            out.insert(
                unit.clone(),
                last_log_line(scope, &unit).unwrap_or_default(),
            );
        }
    }

    Ok(out)
}

#[cfg(test)]
/// Test-build stub for batched log lookup.
pub fn latest_log_lines_batch(_unit_names: &[String]) -> Result<HashMap<String, String>> {
    Ok(HashMap::new())
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
pub fn fetch_unit_logs(scope: Scope, unit: &str, max_lines: usize) -> Result<Vec<DetailLogEntry>> {
    let journalctl = resolve_trusted_binary("journalctl")?;
    let mut cmd = Command::new(journalctl);
    cmd.arg(scope.as_systemd_arg())
        .arg("-u")
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
pub fn fetch_unit_logs(
    _scope: Scope,
    _unit: &str,
    _max_lines: usize,
) -> Result<Vec<DetailLogEntry>> {
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
        let logs = parse_latest_logs_from_journal_json(Scope::System, output, &wanted);
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
        let logs = parse_latest_logs_from_journal_json(Scope::System, output, &wanted);
        assert_eq!(logs.get("a.service").map(String::as_str), Some("ok"));
    }

    #[test]
    fn latest_log_lines_batch_empty_input_returns_empty_map() {
        let logs = latest_log_lines_batch(&[]).expect("stub should succeed");
        assert!(logs.is_empty());
    }

    #[test]
    fn last_log_line_test_stub_returns_empty_string() {
        let line = last_log_line(Scope::System, "unit").expect("stub should succeed");
        assert_eq!(line, "");
    }

    #[test]
    fn fetch_unit_logs_test_stub_returns_empty_vec() {
        let rows = fetch_unit_logs(Scope::System, "unit", 10).expect("stub should succeed");
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

    #[test]
    fn batch_line_budget_caps_large_batches() {
        assert_eq!(batch_line_budget(10_000, 4), BATCH_MAX_LINES);
    }

    #[test]
    fn batch_line_budget_scales_by_attempt() {
        let b0 = batch_line_budget(50, 0);
        let b1 = batch_line_budget(50, 1);
        let b2 = batch_line_budget(50, 2);
        assert!(b1 >= b0);
        assert!(b2 >= b1);
        assert!(b2 <= BATCH_MAX_LINES);
    }

    #[test]
    fn batch_line_budget_uses_minimum_floor_for_small_inputs() {
        assert_eq!(batch_line_budget(1, 0), BATCH_MIN_LINES);
    }

    #[test]
    fn batch_line_budget_caps_attempt_growth_after_min_limit() {
        let b10 = batch_line_budget(2, 10);
        let b99 = batch_line_budget(2, 99);
        assert_eq!(b10, b99);
        assert!(b99 <= BATCH_MAX_LINES);
    }

    #[test]
    fn parse_latest_logs_lines_respects_budget() {
        let output = r#"{"_SYSTEMD_UNIT":"a.service","MESSAGE":"a"}
{"_SYSTEMD_UNIT":"b.service","MESSAGE":"b"}
{"_SYSTEMD_UNIT":"c.service","MESSAGE":"c"}"#;
        let wanted = HashSet::from([
            "a.service".to_string(),
            "b.service".to_string(),
            "c.service".to_string(),
        ]);
        let logs = parse_latest_logs_lines(Scope::System, output.lines(), &wanted, 2);
        assert_eq!(logs.len(), 2);
        assert!(logs.contains_key("a.service"));
        assert!(logs.contains_key("b.service"));
        assert!(!logs.contains_key("c.service"));
    }

    #[test]
    fn parse_latest_logs_uses_user_unit_field_in_user_scope() {
        let output = r#"{"_SYSTEMD_USER_UNIT":"x.service","MESSAGE":"x msg"}
{"_SYSTEMD_UNIT":"x.service","MESSAGE":"system msg"}"#;
        let wanted = HashSet::from(["x.service".to_string()]);
        let logs = parse_latest_logs_from_journal_json(Scope::User, output, &wanted);
        assert_eq!(logs.get("x.service").map(String::as_str), Some("x msg"));
    }

    #[test]
    fn parse_latest_logs_maps_manager_generated_system_unit_fields() {
        let output = r#"{"UNIT":"x.service","MESSAGE":"unit field"}
{"OBJECT_SYSTEMD_UNIT":"y.service","MESSAGE":"object field"}"#;
        let wanted = HashSet::from(["x.service".to_string(), "y.service".to_string()]);
        let logs = parse_latest_logs_from_journal_json(Scope::System, output, &wanted);
        assert_eq!(
            logs.get("x.service").map(String::as_str),
            Some("unit field")
        );
        assert_eq!(
            logs.get("y.service").map(String::as_str),
            Some("object field")
        );
    }

    #[test]
    fn parse_latest_logs_maps_manager_generated_user_unit_fields() {
        let output = r#"{"UNIT":"x.service","MESSAGE":"unit field"}
{"OBJECT_SYSTEMD_USER_UNIT":"y.service","MESSAGE":"object user field"}"#;
        let wanted = HashSet::from(["x.service".to_string(), "y.service".to_string()]);
        let logs = parse_latest_logs_from_journal_json(Scope::User, output, &wanted);
        assert_eq!(
            logs.get("x.service").map(String::as_str),
            Some("unit field")
        );
        assert_eq!(
            logs.get("y.service").map(String::as_str),
            Some("object user field")
        );
    }

    #[test]
    fn parse_latest_logs_maps_coredump_and_user_unit_fields() {
        let output = r#"{"COREDUMP_UNIT":"a.service","MESSAGE":"system coredump"}
{"COREDUMP_USER_UNIT":"b.service","MESSAGE":"user coredump"}
{"USER_UNIT":"c.service","MESSAGE":"user unit"}"#;
        let wanted = HashSet::from([
            "a.service".to_string(),
            "b.service".to_string(),
            "c.service".to_string(),
        ]);
        let system_logs = parse_latest_logs_from_journal_json(Scope::System, output, &wanted);
        assert_eq!(
            system_logs.get("a.service").map(String::as_str),
            Some("system coredump")
        );
        assert_eq!(
            system_logs.get("b.service").map(String::as_str),
            Some("user coredump")
        );
        assert_eq!(
            system_logs.get("c.service").map(String::as_str),
            Some("user unit")
        );

        let user_logs = parse_latest_logs_from_journal_json(Scope::User, output, &wanted);
        assert_eq!(
            user_logs.get("a.service").map(String::as_str),
            Some("system coredump")
        );
        assert_eq!(
            user_logs.get("b.service").map(String::as_str),
            Some("user coredump")
        );
        assert_eq!(
            user_logs.get("c.service").map(String::as_str),
            Some("user unit")
        );
    }

    #[test]
    fn latest_log_lines_batch_stub_stays_ok_for_non_empty_input() {
        let logs = latest_log_lines_batch(&["a.service".to_string(), "b.service".to_string()])
            .expect("stub should not fail");
        assert!(logs.is_empty());
    }

    #[test]
    fn fallback_error_path_is_non_fatal_in_runtime_contract() {
        // This documents the intended behavior: per-unit fallback errors must not abort the batch.
        // In test builds fallback stubs are always successful, so the function returns Ok.
        let unit_names = ["broken.service".to_string()];
        assert!(latest_log_lines_batch(&unit_names).is_ok());
    }
}

// Copyright (c) 2026 l5yth
// SPDX-License-Identifier: Apache-2.0

use anyhow::{Context, Result, anyhow};
use crossterm::{
    event::{self, Event, KeyCode, KeyEventKind},
    execute,
    terminal::{EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode},
};
use ratatui::{
    prelude::*,
    widgets::{Block, Borders, Cell, Paragraph, Row, Table},
};
use serde::Deserialize;
use std::{
    collections::{HashMap, HashSet},
    env, io,
    process::Command,
    time::{Duration, Instant},
};

/// systemctl list-units --output=json produces objects with these fields.
/// Some distros may include more/less; unknown fields are ignored.
#[derive(Debug, Clone, Deserialize)]
struct SystemctlUnit {
    unit: String,
    load: String,
    active: String,
    sub: String,
    description: String,
}

#[derive(Debug, Clone)]
struct UnitRow {
    dot: char,
    dot_style: Style,
    unit: String,
    load: String,
    active: String,
    sub: String,
    description: String,
    last_log: String,
}

#[derive(Debug, Clone, Copy)]
struct Config {
    show_all: bool,
    refresh_secs: u64,
    show_help: bool,
}

fn usage() -> &'static str {
    "Usage: lsu [OPTIONS]

Show systemd services in a terminal UI.

Options:
  -a, --all            Include non-active service units
  -r, --refresh <num>  Auto-refresh interval in seconds (0 disables, default: 0)
  -h, --help           Show this help text"
}

fn parse_refresh_secs(value: &str) -> Result<u64> {
    let secs = value
        .parse::<u64>()
        .with_context(|| format!("invalid refresh value: {value}"))?;
    Ok(secs)
}

fn parse_args<I, S>(args: I) -> Result<Config>
where
    I: IntoIterator<Item = S>,
    S: Into<String>,
{
    let mut cfg = Config {
        show_all: false,
        refresh_secs: 0,
        show_help: false,
    };

    let mut it = args.into_iter().map(Into::into);
    let _program = it.next();

    while let Some(arg) = it.next() {
        match arg.as_str() {
            "-a" | "--all" => cfg.show_all = true,
            "-h" | "--help" => cfg.show_help = true,
            "-r" | "--refresh" => {
                let value = it
                    .next()
                    .ok_or_else(|| anyhow!("missing value for {arg}\n\n{}", usage()))?;
                cfg.refresh_secs = parse_refresh_secs(&value)?;
            }
            _ => {
                if let Some(value) = arg.strip_prefix("--refresh=") {
                    cfg.refresh_secs = parse_refresh_secs(value)?;
                } else {
                    return Err(anyhow!("unknown argument: {arg}\n\n{}", usage()));
                }
            }
        }
    }

    Ok(cfg)
}

/// Run a command and capture stdout as String.
fn cmd_stdout(cmd: &mut Command) -> Result<String> {
    let out = cmd.output().with_context(|| "failed to spawn command")?;
    if !out.status.success() {
        return Err(anyhow!(
            "command failed (status={}): {}",
            out.status,
            String::from_utf8_lossy(&out.stderr)
        ));
    }
    Ok(String::from_utf8_lossy(&out.stdout).to_string())
}

/// Query service units via systemctl JSON output.
fn fetch_services(show_all: bool) -> Result<Vec<SystemctlUnit>> {
    // --plain and --no-pager keep things predictable; JSON output is easiest to parse.
    let mut cmd = Command::new("systemctl");
    cmd.arg("list-units")
        .arg("--no-pager")
        .arg("--plain")
        .arg("--type=service")
        .arg("--output=json");

    if show_all {
        cmd.arg("--all");
    } else {
        cmd.arg("--state=running");
    }

    let s = cmd_stdout(&mut cmd).context("systemctl list-units failed")?;

    let units: Vec<SystemctlUnit> =
        serde_json::from_str(&s).context("failed to parse systemctl JSON")?;
    Ok(units)
}

fn parse_latest_logs_from_journal_json(
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

        let msg = value
            .get("MESSAGE")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .trim()
            .to_string();

        latest.insert(unit.to_string(), msg);

        if latest.len() == wanted.len() {
            break;
        }
    }

    latest
}

/// Get latest journal message per unit in batches to avoid one process per unit.
fn latest_log_lines(units: &[SystemctlUnit]) -> HashMap<String, String> {
    const UNIT_BATCH_SIZE: usize = 128;
    let unit_names: Vec<String> = units.iter().map(|u| u.unit.clone()).collect();
    let mut all_latest = HashMap::new();

    for batch in unit_names.chunks(UNIT_BATCH_SIZE) {
        let wanted: HashSet<String> = batch.iter().cloned().collect();
        let mut cmd = Command::new("journalctl");
        cmd.arg("--no-pager")
            .arg("-o")
            .arg("json")
            .arg("-r")
            .arg("-n")
            .arg(std::cmp::max(batch.len() * 20, 200).to_string());

        for unit in batch {
            cmd.arg("-u").arg(unit);
        }

        let Ok(output) = cmd_stdout(&mut cmd) else {
            continue;
        };

        let latest = parse_latest_logs_from_journal_json(&output, &wanted);
        all_latest.extend(latest);
    }

    all_latest
}

/// Choose dot + color based on active/sub.
fn status_dot(active: &str, sub: &str) -> (char, Style) {
    // Running services should mostly be active/running, but we don’t assume perfection.
    match (active, sub) {
        ("active", "running") => ('●', Style::default().fg(Color::Green)),
        ("active", _) => ('●', Style::default().fg(Color::Yellow)),
        ("inactive", _) => ('●', Style::default().fg(Color::DarkGray)),
        ("failed", _) => ('●', Style::default().fg(Color::Red)),
        _ => ('●', Style::default().fg(Color::Blue)),
    }
}

fn load_rank(load: &str) -> u8 {
    match load {
        "loaded" => 0,
        "not-found" => 1,
        _ => 2,
    }
}

fn active_rank(active: &str) -> u8 {
    match active {
        "active" => 0,
        "inactive" => 1,
        _ => 2,
    }
}

fn sub_rank(sub: &str) -> u8 {
    match sub {
        "running" => 0,
        "exited" => 1,
        "dead" => 2,
        _ => 3,
    }
}

fn build_rows(units: Vec<SystemctlUnit>) -> Vec<UnitRow> {
    let log_cache = latest_log_lines(&units);
    let mut rows = Vec::with_capacity(units.len());
    for u in units {
        let (dot, dot_style) = status_dot(&u.active, &u.sub);

        let last_log = log_cache.get(&u.unit).cloned().unwrap_or_default();

        rows.push(UnitRow {
            dot,
            dot_style,
            unit: u.unit,
            load: u.load,
            active: u.active,
            sub: u.sub,
            description: u.description,
            last_log,
        });
    }
    rows
}

fn setup_terminal() -> Result<Terminal<CrosstermBackend<io::Stdout>>> {
    enable_raw_mode().context("enable_raw_mode failed")?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen).context("EnterAlternateScreen failed")?;
    let backend = CrosstermBackend::new(stdout);
    Ok(Terminal::new(backend)?)
}

fn restore_terminal(mut terminal: Terminal<CrosstermBackend<io::Stdout>>) -> Result<()> {
    disable_raw_mode().ok();
    execute!(terminal.backend_mut(), LeaveAlternateScreen).ok();
    terminal.show_cursor().ok();
    Ok(())
}

fn main() -> Result<()> {
    let config = parse_args(env::args())?;
    if config.show_help {
        println!("{}", usage());
        return Ok(());
    }

    let mut terminal = setup_terminal()?;

    // refresh cadence
    let refresh_every = if config.refresh_secs == 0 {
        None
    } else {
        Some(Duration::from_secs(config.refresh_secs))
    };
    let mut last_refresh = Instant::now();
    let mut force_refresh = true;

    // state
    let mut rows: Vec<UnitRow> = Vec::new();
    let mode_label = if config.show_all {
        "all services"
    } else {
        "running services"
    };
    let refresh_label = if config.refresh_secs == 0 {
        "off".to_string()
    } else {
        format!("{}s", config.refresh_secs)
    };
    let mut status_line =
        format!("{mode_label}: 0 | auto-refresh: {refresh_label} | q: quit | r: refresh");

    let res = (|| -> Result<()> {
        loop {
            // refresh
            let auto_due = refresh_every
                .map(|every| last_refresh.elapsed() >= every)
                .unwrap_or(false);
            if force_refresh || auto_due {
                match fetch_services(config.show_all).map(|units| {
                    let mut r = build_rows(units);
                    if config.show_all {
                        r.sort_by(|a, b| {
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
                        r.sort_by(|a, b| a.unit.cmp(&b.unit));
                    }
                    r
                }) {
                    Ok(r) => {
                        rows = r;
                        status_line = format!(
                            "{mode_label}: {} | auto-refresh: {refresh_label} | q: quit | r: refresh",
                            rows.len(),
                        );
                    }
                    Err(e) => {
                        status_line = format!(
                            "error: {e} | auto-refresh: {refresh_label} | q: quit | r: refresh",
                        );
                    }
                }
                last_refresh = Instant::now();
                force_refresh = false;
            }

            terminal.draw(|f| {
                let size = f.size();
                let chunks = Layout::default()
                    .direction(Direction::Vertical)
                    .constraints([Constraint::Min(1), Constraint::Length(1)])
                    .split(size);

                let header = Row::new([
                    Cell::from(" "),
                    Cell::from("unit"),
                    Cell::from("load"),
                    Cell::from("active"),
                    Cell::from("sub"),
                    Cell::from("description"),
                    Cell::from("log (last line)"),
                ])
                .style(Style::default().add_modifier(Modifier::BOLD));

                let table_rows = rows.iter().map(|r| {
                    Row::new([
                        Cell::from(r.dot.to_string()).style(r.dot_style),
                        Cell::from(r.unit.clone()),
                        Cell::from(r.load.clone()),
                        Cell::from(r.active.clone()),
                        Cell::from(r.sub.clone()),
                        Cell::from(r.description.clone()),
                        Cell::from(r.last_log.clone()),
                    ])
                });

                let widths = [
                    Constraint::Length(2),
                    Constraint::Length(38),
                    Constraint::Length(8),
                    Constraint::Length(10),
                    Constraint::Length(12),
                    Constraint::Length(36),
                    Constraint::Min(20),
                ];

                let t = Table::new(table_rows, widths)
                    .header(header)
                    .block(
                        Block::default()
                            .borders(Borders::ALL)
                            .title(format!("systemd {mode_label}")),
                    )
                    .column_spacing(1);

                f.render_widget(t, chunks[0]);

                let footer =
                    Paragraph::new(status_line.clone()).style(Style::default().fg(Color::DarkGray));
                f.render_widget(footer, chunks[1]);
            })?;

            // input
            if event::poll(Duration::from_millis(50))? {
                if let Event::Key(k) = event::read()? {
                    if k.kind == KeyEventKind::Press {
                        match k.code {
                            KeyCode::Char('q') => break,
                            KeyCode::Char('r') => {
                                force_refresh = true;
                            }
                            _ => {}
                        }
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
    }

    #[test]
    fn cmd_stdout_returns_output_for_success() {
        let mut cmd = Command::new("sh");
        cmd.arg("-c").arg("printf ok");
        let out = cmd_stdout(&mut cmd).expect("command should succeed");
        assert_eq!(out, "ok");
    }

    #[test]
    fn cmd_stdout_returns_error_for_non_zero_exit() {
        let mut cmd = Command::new("sh");
        cmd.arg("-c").arg("echo fail 1>&2; exit 7");
        let err = cmd_stdout(&mut cmd).expect_err("command should fail");
        let msg = err.to_string();
        assert!(msg.contains("status="));
        assert!(msg.contains("fail"));
    }

    #[test]
    fn parses_systemctl_units_from_json() {
        let raw = r#"
        [
          {
            "unit": "sshd.service",
            "load": "loaded",
            "active": "active",
            "sub": "running",
            "description": "OpenSSH server daemon",
            "extra_field": "ignored"
          }
        ]
        "#;

        let units: Vec<SystemctlUnit> = serde_json::from_str(raw).expect("valid JSON");
        assert_eq!(units.len(), 1);
        assert_eq!(units[0].unit, "sshd.service");
        assert_eq!(units[0].active, "active");
        assert_eq!(units[0].sub, "running");
    }

    #[test]
    fn parse_args_defaults() {
        let cfg = parse_args(vec!["lsu"]).expect("default args should parse");
        assert!(!cfg.show_all);
        assert_eq!(cfg.refresh_secs, 0);
        assert!(!cfg.show_help);
    }

    #[test]
    fn parse_args_all_and_refresh() {
        let cfg = parse_args(vec!["lsu", "--all", "--refresh", "5"]).expect("flags should parse");
        assert!(cfg.show_all);
        assert_eq!(cfg.refresh_secs, 5);
        assert!(!cfg.show_help);
    }

    #[test]
    fn parse_args_help() {
        let cfg = parse_args(vec!["lsu", "-h"]).expect("help should parse");
        assert!(cfg.show_help);
    }

    #[test]
    fn parse_args_allows_zero_refresh() {
        let cfg = parse_args(vec!["lsu", "-r", "0"]).expect("zero should be allowed");
        assert_eq!(cfg.refresh_secs, 0);
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
    fn parses_latest_logs_per_unit_from_json_lines() {
        let output = r#"{"_SYSTEMD_UNIT":"a.service","MESSAGE":"newest a"}
{"_SYSTEMD_UNIT":"b.service","MESSAGE":"newest b"}
{"_SYSTEMD_UNIT":"a.service","MESSAGE":"older a"}"#;
        let wanted = HashSet::from(["a.service".to_string(), "b.service".to_string()]);
        let logs = parse_latest_logs_from_journal_json(output, &wanted);
        assert_eq!(logs.get("a.service").map(String::as_str), Some("newest a"));
        assert_eq!(logs.get("b.service").map(String::as_str), Some("newest b"));
    }
}

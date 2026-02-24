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
    widgets::{Block, Borders, Cell, Paragraph, Row, Table, TableState},
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

#[derive(Debug, Clone)]
struct DetailLogEntry {
    time: String,
    log: String,
}

#[derive(Debug, Clone)]
struct Config {
    load_filter: String,
    active_filter: String,
    sub_filter: String,
    refresh_secs: u64,
    show_help: bool,
}

#[derive(Debug, Clone, Copy)]
enum LoadPhase {
    Idle,
    FetchingUnits,
    FetchingLogs { next_idx: usize },
}

#[derive(Debug, Clone, Copy)]
enum ViewMode {
    List,
    Detail,
}

fn usage() -> &'static str {
    "Usage: lsu [OPTIONS]

Show systemd services in a terminal UI.

Options:
  -a, --all            Shorthand for --load all --active all --sub all
      --load <value>   Filter by load state (e.g. loaded, not-found, masked, all)
      --active <value> Filter by active state (e.g. active, inactive, failed, all)
      --sub <value>    Filter by sub state (e.g. running, exited, dead, all)
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
        load_filter: "all".to_string(),
        active_filter: "active".to_string(),
        sub_filter: "running".to_string(),
        refresh_secs: 0,
        show_help: false,
    };

    let mut it = args.into_iter().map(Into::into);
    let _program = it.next();

    while let Some(arg) = it.next() {
        match arg.as_str() {
            "-a" | "--all" => {
                cfg.load_filter = "all".to_string();
                cfg.active_filter = "all".to_string();
                cfg.sub_filter = "all".to_string();
            }
            "-h" | "--help" => cfg.show_help = true,
            "--load" => {
                let value = it
                    .next()
                    .ok_or_else(|| anyhow!("missing value for {arg}\n\n{}", usage()))?;
                cfg.load_filter = value;
            }
            "--active" => {
                let value = it
                    .next()
                    .ok_or_else(|| anyhow!("missing value for {arg}\n\n{}", usage()))?;
                cfg.active_filter = value;
            }
            "--sub" => {
                let value = it
                    .next()
                    .ok_or_else(|| anyhow!("missing value for {arg}\n\n{}", usage()))?;
                cfg.sub_filter = value;
            }
            "-r" | "--refresh" => {
                let value = it
                    .next()
                    .ok_or_else(|| anyhow!("missing value for {arg}\n\n{}", usage()))?;
                cfg.refresh_secs = parse_refresh_secs(&value)?;
            }
            _ => {
                if let Some(value) = arg.strip_prefix("--load=") {
                    cfg.load_filter = value.to_string();
                } else if let Some(value) = arg.strip_prefix("--active=") {
                    cfg.active_filter = value.to_string();
                } else if let Some(value) = arg.strip_prefix("--sub=") {
                    cfg.sub_filter = value.to_string();
                } else if let Some(value) = arg.strip_prefix("--refresh=") {
                    cfg.refresh_secs = parse_refresh_secs(value)?;
                } else {
                    return Err(anyhow!("unknown argument: {arg}\n\n{}", usage()));
                }
            }
        }
    }

    Ok(cfg)
}

fn filter_matches(value: &str, wanted: &str) -> bool {
    wanted == "all" || value == wanted
}

fn is_full_all(cfg: &Config) -> bool {
    cfg.load_filter == "all" && cfg.active_filter == "all" && cfg.sub_filter == "all"
}

fn should_fetch_all(cfg: &Config) -> bool {
    // Only the default filter set can be safely satisfied from --state=running.
    !(cfg.load_filter == "all" && cfg.active_filter == "active" && cfg.sub_filter == "running")
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

fn filter_services(units: Vec<SystemctlUnit>, cfg: &Config) -> Vec<SystemctlUnit> {
    units
        .into_iter()
        .filter(|u| {
            filter_matches(&u.load, &cfg.load_filter)
                && filter_matches(&u.active, &cfg.active_filter)
                && filter_matches(&u.sub, &cfg.sub_filter)
        })
        .collect()
}

/// Get the last journal line for one unit.
fn last_log_line(unit: &str) -> Result<String> {
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

fn parse_journal_short_iso(output: &str) -> Vec<DetailLogEntry> {
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

fn fetch_unit_logs(unit: &str, max_lines: usize) -> Result<Vec<DetailLogEntry>> {
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

        let message = value
            .get("MESSAGE")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .trim()
            .to_string();
        latest.insert(unit.to_string(), message);

        if latest.len() == wanted.len() {
            break;
        }
    }
    latest
}

fn latest_log_lines_batch(unit_names: &[String]) -> HashMap<String, String> {
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

    // Ensure each requested unit has fresh data even if the batched query misses it.
    for unit in unit_names {
        if !out.contains_key(unit) {
            out.insert(unit.clone(), last_log_line(unit).unwrap_or_default());
        }
    }

    out
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

fn sort_rows(rows: &mut [UnitRow], show_all: bool) {
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

fn seed_logs_from_previous(new_rows: &mut [UnitRow], previous_rows: &[UnitRow]) {
    let previous_logs: HashMap<&str, &str> = previous_rows
        .iter()
        .map(|r| (r.unit.as_str(), r.last_log.as_str()))
        .collect();
    for row in new_rows.iter_mut() {
        if let Some(old_log) = previous_logs.get(row.unit.as_str()) {
            row.last_log = (*old_log).to_string();
        }
    }
}

fn preserve_selection(prev_unit: Option<String>, rows: &[UnitRow], selected_idx: &mut usize) {
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
    let mut refresh_requested = true;
    let mut phase = LoadPhase::Idle;

    // state
    let mut rows: Vec<UnitRow> = Vec::new();
    let mut selected_idx: usize = 0;
    let mut list_table_state = TableState::default();
    let mut view_mode = ViewMode::List;
    let mut detail_unit = String::new();
    let mut detail_logs: Vec<DetailLogEntry> = Vec::new();
    let mut detail_scroll: usize = 0;
    let mode_label = "services";
    let refresh_label = if config.refresh_secs == 0 {
        "off".to_string()
    } else {
        format!("{}s", config.refresh_secs)
    };
    let mut status_line = format!(
        "{mode_label}: 0 | auto-refresh: {refresh_label} | ↑/↓: move | l/enter: inspect logs | q: quit | r: refresh"
    );

    let res = (|| -> Result<()> {
        loop {
            let auto_due = refresh_every
                .map(|every| last_refresh.elapsed() >= every)
                .unwrap_or(false);
            if auto_due {
                refresh_requested = true;
            }

            terminal.draw(|f| {
                let size = f.area();
                let chunks = Layout::default()
                    .direction(Direction::Vertical)
                    .constraints([Constraint::Min(1), Constraint::Length(1)])
                    .split(size);
                match view_mode {
                    ViewMode::List => {
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

                        list_table_state.select((!rows.is_empty()).then_some(selected_idx));
                        let t = Table::new(table_rows, widths)
                            .header(header)
                            .block(
                                Block::default()
                                    .borders(Borders::ALL)
                                    .title(format!("systemd {mode_label}")),
                            )
                            .row_highlight_style(Style::default().add_modifier(Modifier::REVERSED))
                            .column_spacing(1);

                        f.render_stateful_widget(t, chunks[0], &mut list_table_state);

                        let footer = Paragraph::new(status_line.clone())
                            .style(Style::default().fg(Color::DarkGray));
                        f.render_widget(footer, chunks[1]);
                    }
                    ViewMode::Detail => {
                        let unit_meta = rows
                            .iter()
                            .find(|r| r.unit == detail_unit)
                            .map(|r| format!("unit: {}", r.unit))
                            .unwrap_or_else(|| format!("unit: {}", detail_unit));

                        let header = Row::new([Cell::from("time"), Cell::from("log")])
                            .style(Style::default().add_modifier(Modifier::BOLD));
                        let log_rows = detail_logs
                            .iter()
                            .skip(detail_scroll)
                            .map(|entry| Row::new([entry.time.clone(), entry.log.clone()]));

                        let table =
                            Table::new(log_rows, [Constraint::Length(25), Constraint::Min(20)])
                                .header(header)
                                .block(
                                    Block::default()
                                        .borders(Borders::ALL)
                                        .title(format!("logs for {}", detail_unit)),
                                )
                                .column_spacing(1);
                        f.render_widget(table, chunks[0]);

                        let footer = Paragraph::new(format!(
                            "{} | logs: {} | ↑/↓: scroll | b/esc: back | q: quit | r: refresh",
                            unit_meta,
                            detail_logs.len()
                        ))
                        .style(Style::default().fg(Color::DarkGray));
                        f.render_widget(footer, chunks[1]);
                    }
                }
            })?;

            if refresh_requested && matches!(phase, LoadPhase::Idle) {
                phase = LoadPhase::FetchingUnits;
                status_line = format!(
                    "{mode_label}: loading units... | auto-refresh: {refresh_label} | ↑/↓: move | l/enter: inspect logs | q: quit | r: refresh"
                );
                refresh_requested = false;
                last_refresh = Instant::now();
            }

            match phase {
                LoadPhase::Idle => {}
                LoadPhase::FetchingUnits => {
                    let fetch_all = should_fetch_all(&config);
                    match fetch_services(fetch_all).map(|u| filter_services(u, &config)) {
                        Ok(units) => {
                            let previous_selected = rows.get(selected_idx).map(|r| r.unit.clone());
                            let previous_rows = rows.clone();
                            let mut new_rows = build_rows(units);
                            seed_logs_from_previous(&mut new_rows, &previous_rows);
                            sort_rows(&mut new_rows, is_full_all(&config));
                            let row_count = new_rows.len();
                            rows = new_rows;
                            preserve_selection(previous_selected, &rows, &mut selected_idx);

                            if rows.is_empty() {
                                status_line = format!(
                                    "{mode_label}: 0 | auto-refresh: {refresh_label} | ↑/↓: move | l/enter: inspect logs | q: quit | r: refresh",
                                );
                                phase = LoadPhase::Idle;
                            } else {
                                status_line = format!(
                                    "{mode_label}: {} | logs: 0/{} | auto-refresh: {refresh_label} | ↑/↓: move | l/enter: inspect logs | q: quit | r: refresh",
                                    row_count, row_count,
                                );
                                phase = LoadPhase::FetchingLogs { next_idx: 0 };
                            }
                        }
                        Err(e) => {
                            status_line = format!(
                                "error: {e} | auto-refresh: {refresh_label} | q: quit | r: refresh",
                            );
                            phase = LoadPhase::Idle;
                        }
                    }
                }
                LoadPhase::FetchingLogs { mut next_idx } => {
                    const LOG_BATCH_SIZE: usize = 12;
                    if next_idx < rows.len() {
                        let end = std::cmp::min(next_idx + LOG_BATCH_SIZE, rows.len());
                        let units: Vec<String> =
                            rows[next_idx..end].iter().map(|r| r.unit.clone()).collect();
                        let logs = latest_log_lines_batch(&units);
                        for row in rows.iter_mut().take(end).skip(next_idx) {
                            if let Some(log) = logs.get(row.unit.as_str()) {
                                row.last_log = log.clone();
                            }
                        }
                        next_idx = end;
                    }

                    if next_idx >= rows.len() {
                        status_line = format!(
                            "{mode_label}: {} | auto-refresh: {refresh_label} | ↑/↓: move | l/enter: inspect logs | q: quit | r: refresh",
                            rows.len(),
                        );
                        phase = LoadPhase::Idle;
                    } else {
                        status_line = format!(
                            "{mode_label}: {} | logs: {}/{} | auto-refresh: {refresh_label} | ↑/↓: move | l/enter: inspect logs | q: quit | r: refresh",
                            rows.len(),
                            next_idx,
                            rows.len(),
                        );
                        phase = LoadPhase::FetchingLogs { next_idx };
                    }
                }
            }

            // input
            if event::poll(Duration::from_millis(50))?
                && let Event::Key(k) = event::read()?
                && k.kind == KeyEventKind::Press
            {
                match view_mode {
                    ViewMode::List => match k.code {
                        KeyCode::Char('q') => break,
                        KeyCode::Char('r') => {
                            refresh_requested = true;
                        }
                        KeyCode::Down => {
                            if !rows.is_empty() {
                                selected_idx = std::cmp::min(selected_idx + 1, rows.len() - 1);
                            }
                        }
                        KeyCode::Up => {
                            selected_idx = selected_idx.saturating_sub(1);
                        }
                        KeyCode::Char('l') | KeyCode::Enter => {
                            if let Some(row) = rows.get(selected_idx) {
                                detail_unit = row.unit.clone();
                                detail_logs =
                                    fetch_unit_logs(&detail_unit, 300).unwrap_or_default();
                                detail_scroll = 0;
                                view_mode = ViewMode::Detail;
                            }
                        }
                        _ => {}
                    },
                    ViewMode::Detail => match k.code {
                        KeyCode::Char('q') => break,
                        KeyCode::Char('r') => {
                            refresh_requested = true;
                            detail_logs = fetch_unit_logs(&detail_unit, 300).unwrap_or_default();
                            detail_scroll = 0;
                        }
                        KeyCode::Down => {
                            if !detail_logs.is_empty() {
                                detail_scroll =
                                    std::cmp::min(detail_scroll + 1, detail_logs.len() - 1);
                            }
                        }
                        KeyCode::Up => {
                            detail_scroll = detail_scroll.saturating_sub(1);
                        }
                        KeyCode::Esc | KeyCode::Char('b') => {
                            view_mode = ViewMode::List;
                        }
                        KeyCode::Char('l') => {
                            // reload detail logs
                            detail_logs = fetch_unit_logs(&detail_unit, 300).unwrap_or_default();
                            detail_scroll = 0;
                        }
                        _ => {}
                    },
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
        assert_eq!(cfg.load_filter, "all");
        assert_eq!(cfg.active_filter, "active");
        assert_eq!(cfg.sub_filter, "running");
        assert_eq!(cfg.refresh_secs, 0);
        assert!(!cfg.show_help);
    }

    #[test]
    fn parse_args_all_and_refresh() {
        let cfg = parse_args(vec!["lsu", "--all", "--refresh", "5"]).expect("flags should parse");
        assert_eq!(cfg.load_filter, "all");
        assert_eq!(cfg.active_filter, "all");
        assert_eq!(cfg.sub_filter, "all");
        assert_eq!(cfg.refresh_secs, 5);
        assert!(!cfg.show_help);
    }

    #[test]
    fn parse_args_individual_filters() {
        let cfg = parse_args(vec![
            "lsu",
            "--load",
            "not-found",
            "--active=inactive",
            "--sub",
            "dead",
        ])
        .expect("filter args should parse");
        assert_eq!(cfg.load_filter, "not-found");
        assert_eq!(cfg.active_filter, "inactive");
        assert_eq!(cfg.sub_filter, "dead");
    }

    #[test]
    fn parse_args_help() {
        let cfg = parse_args(vec!["lsu", "-h"]).expect("help should parse");
        assert!(cfg.show_help);
    }

    #[test]
    fn parse_args_rejects_unknown_arg() {
        let err = parse_args(vec!["lsu", "--bogus"]).expect_err("unknown arg should fail");
        assert!(err.to_string().contains("unknown argument"));
    }

    #[test]
    fn parse_args_rejects_missing_filter_values() {
        let err = parse_args(vec!["lsu", "--load"]).expect_err("missing --load value");
        assert!(err.to_string().contains("missing value for --load"));

        let err = parse_args(vec!["lsu", "--active"]).expect_err("missing --active value");
        assert!(err.to_string().contains("missing value for --active"));

        let err = parse_args(vec!["lsu", "--sub"]).expect_err("missing --sub value");
        assert!(err.to_string().contains("missing value for --sub"));
    }

    #[test]
    fn parse_args_rejects_invalid_refresh_value() {
        let err = parse_args(vec!["lsu", "--refresh", "abc"]).expect_err("invalid refresh");
        assert!(err.to_string().contains("invalid refresh value"));
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
    fn filter_services_applies_all_filters() {
        let cfg = Config {
            load_filter: "loaded".to_string(),
            active_filter: "active".to_string(),
            sub_filter: "running".to_string(),
            refresh_secs: 0,
            show_help: false,
        };
        let units = vec![
            SystemctlUnit {
                unit: "a.service".to_string(),
                load: "loaded".to_string(),
                active: "active".to_string(),
                sub: "running".to_string(),
                description: String::new(),
            },
            SystemctlUnit {
                unit: "b.service".to_string(),
                load: "loaded".to_string(),
                active: "inactive".to_string(),
                sub: "dead".to_string(),
                description: String::new(),
            },
        ];
        let out = filter_services(units, &cfg);
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].unit, "a.service");
    }

    #[test]
    fn filter_matches_supports_all_and_exact() {
        assert!(filter_matches("running", "all"));
        assert!(filter_matches("running", "running"));
        assert!(!filter_matches("running", "dead"));
    }

    #[test]
    fn is_full_all_only_true_when_all_three_filters_are_all() {
        let all_cfg = Config {
            load_filter: "all".to_string(),
            active_filter: "all".to_string(),
            sub_filter: "all".to_string(),
            refresh_secs: 0,
            show_help: false,
        };
        assert!(is_full_all(&all_cfg));

        let partial_cfg = Config {
            sub_filter: "running".to_string(),
            ..all_cfg
        };
        assert!(!is_full_all(&partial_cfg));
    }

    #[test]
    fn should_fetch_all_only_false_for_default_running_filter_set() {
        let default_cfg = Config {
            load_filter: "all".to_string(),
            active_filter: "active".to_string(),
            sub_filter: "running".to_string(),
            refresh_secs: 0,
            show_help: false,
        };
        assert!(!should_fetch_all(&default_cfg));

        let sub_all = Config {
            sub_filter: "all".to_string(),
            ..default_cfg.clone()
        };
        assert!(should_fetch_all(&sub_all));

        let sub_exited = Config {
            sub_filter: "exited".to_string(),
            ..default_cfg.clone()
        };
        assert!(should_fetch_all(&sub_exited));

        let active_inactive = Config {
            active_filter: "inactive".to_string(),
            ..default_cfg.clone()
        };
        assert!(should_fetch_all(&active_inactive));

        let load_not_found = Config {
            load_filter: "not-found".to_string(),
            ..default_cfg
        };
        assert!(should_fetch_all(&load_not_found));
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
        assert_eq!(logs.get("a.service").map(String::as_str), Some(""));
    }

    #[test]
    fn latest_log_lines_batch_empty_input_returns_empty_map() {
        let logs = latest_log_lines_batch(&[]);
        assert!(logs.is_empty());
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
}

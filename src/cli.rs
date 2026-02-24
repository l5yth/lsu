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

//! Command-line parsing and usage text.

use anyhow::{Context, Result, anyhow};
use std::str::FromStr;

/// Parsed command-line configuration.
#[derive(Debug, Clone)]
pub struct Config {
    pub load_filter: String,
    pub active_filter: String,
    pub sub_filter: String,
    pub refresh_secs: u64,
    pub show_help: bool,
    pub show_version: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum LoadFilter {
    All,
    Loaded,
    Stub,
    NotFound,
    BadSetting,
    Error,
    Merged,
    Masked,
}

impl LoadFilter {
    fn as_str(self) -> &'static str {
        match self {
            Self::All => "all",
            Self::Loaded => "loaded",
            Self::Stub => "stub",
            Self::NotFound => "not-found",
            Self::BadSetting => "bad-setting",
            Self::Error => "error",
            Self::Merged => "merged",
            Self::Masked => "masked",
        }
    }

    fn allowed_values() -> &'static str {
        "all, loaded, stub, not-found, bad-setting, error, merged, masked"
    }
}

impl FromStr for LoadFilter {
    type Err = anyhow::Error;

    fn from_str(s: &str) -> Result<Self> {
        match s {
            "all" => Ok(Self::All),
            "loaded" => Ok(Self::Loaded),
            "stub" => Ok(Self::Stub),
            "not-found" => Ok(Self::NotFound),
            "bad-setting" => Ok(Self::BadSetting),
            "error" => Ok(Self::Error),
            "merged" => Ok(Self::Merged),
            "masked" => Ok(Self::Masked),
            _ => Err(anyhow!(
                "invalid --load value: {s}; allowed: {}",
                Self::allowed_values()
            )),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ActiveFilter {
    All,
    Active,
    Reloading,
    Inactive,
    Failed,
    Activating,
    Deactivating,
    Maintenance,
    Refreshing,
}

impl ActiveFilter {
    fn as_str(self) -> &'static str {
        match self {
            Self::All => "all",
            Self::Active => "active",
            Self::Reloading => "reloading",
            Self::Inactive => "inactive",
            Self::Failed => "failed",
            Self::Activating => "activating",
            Self::Deactivating => "deactivating",
            Self::Maintenance => "maintenance",
            Self::Refreshing => "refreshing",
        }
    }

    fn allowed_values() -> &'static str {
        "all, active, reloading, inactive, failed, activating, deactivating, maintenance, refreshing"
    }
}

impl FromStr for ActiveFilter {
    type Err = anyhow::Error;

    fn from_str(s: &str) -> Result<Self> {
        match s {
            "all" => Ok(Self::All),
            "active" => Ok(Self::Active),
            "reloading" => Ok(Self::Reloading),
            "inactive" => Ok(Self::Inactive),
            "failed" => Ok(Self::Failed),
            "activating" => Ok(Self::Activating),
            "deactivating" => Ok(Self::Deactivating),
            "maintenance" => Ok(Self::Maintenance),
            "refreshing" => Ok(Self::Refreshing),
            _ => Err(anyhow!(
                "invalid --active value: {s}; allowed: {}",
                Self::allowed_values()
            )),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SubFilter {
    All,
    Running,
    Exited,
    Dead,
    Failed,
    StartPre,
    Start,
    StartPost,
    AutoRestart,
    AutoRestartQueued,
    DeadBeforeAutoRestart,
    Condition,
    Reload,
    ReloadPost,
    ReloadSignal,
    ReloadNotify,
    Stop,
    StopWatchdog,
    StopSigterm,
    StopSigkill,
    StopPost,
    FinalSigterm,
    FinalSigkill,
    FinalWatchdog,
    Cleaning,
}

impl SubFilter {
    fn as_str(self) -> &'static str {
        match self {
            Self::All => "all",
            Self::Running => "running",
            Self::Exited => "exited",
            Self::Dead => "dead",
            Self::Failed => "failed",
            Self::StartPre => "start-pre",
            Self::Start => "start",
            Self::StartPost => "start-post",
            Self::AutoRestart => "auto-restart",
            Self::AutoRestartQueued => "auto-restart-queued",
            Self::DeadBeforeAutoRestart => "dead-before-auto-restart",
            Self::Condition => "condition",
            Self::Reload => "reload",
            Self::ReloadPost => "reload-post",
            Self::ReloadSignal => "reload-signal",
            Self::ReloadNotify => "reload-notify",
            Self::Stop => "stop",
            Self::StopWatchdog => "stop-watchdog",
            Self::StopSigterm => "stop-sigterm",
            Self::StopSigkill => "stop-sigkill",
            Self::StopPost => "stop-post",
            Self::FinalSigterm => "final-sigterm",
            Self::FinalSigkill => "final-sigkill",
            Self::FinalWatchdog => "final-watchdog",
            Self::Cleaning => "cleaning",
        }
    }

    fn allowed_values() -> &'static str {
        "all, running, exited, dead, failed, start-pre, start, start-post, auto-restart, auto-restart-queued, dead-before-auto-restart, condition, reload, reload-post, reload-signal, reload-notify, stop, stop-watchdog, stop-sigterm, stop-sigkill, stop-post, final-sigterm, final-sigkill, final-watchdog, cleaning"
    }
}

impl FromStr for SubFilter {
    type Err = anyhow::Error;

    fn from_str(s: &str) -> Result<Self> {
        match s {
            "all" => Ok(Self::All),
            "running" => Ok(Self::Running),
            "exited" => Ok(Self::Exited),
            "dead" => Ok(Self::Dead),
            "failed" => Ok(Self::Failed),
            "start-pre" => Ok(Self::StartPre),
            "start" => Ok(Self::Start),
            "start-post" => Ok(Self::StartPost),
            "auto-restart" => Ok(Self::AutoRestart),
            "auto-restart-queued" => Ok(Self::AutoRestartQueued),
            "dead-before-auto-restart" => Ok(Self::DeadBeforeAutoRestart),
            "condition" => Ok(Self::Condition),
            "reload" => Ok(Self::Reload),
            "reload-post" => Ok(Self::ReloadPost),
            "reload-signal" => Ok(Self::ReloadSignal),
            "reload-notify" => Ok(Self::ReloadNotify),
            "stop" => Ok(Self::Stop),
            "stop-watchdog" => Ok(Self::StopWatchdog),
            "stop-sigterm" => Ok(Self::StopSigterm),
            "stop-sigkill" => Ok(Self::StopSigkill),
            "stop-post" => Ok(Self::StopPost),
            "final-sigterm" => Ok(Self::FinalSigterm),
            "final-sigkill" => Ok(Self::FinalSigkill),
            "final-watchdog" => Ok(Self::FinalWatchdog),
            "cleaning" => Ok(Self::Cleaning),
            _ => Err(anyhow!(
                "invalid --sub value: {s}; allowed: {}",
                Self::allowed_values()
            )),
        }
    }
}

/// Human-readable CLI usage text.
pub fn usage() -> &'static str {
    concat!(
        "lsu v",
        env!("CARGO_PKG_VERSION"),
        "\napache v2 (c) 2026 l5yth\n\nUsage: lsu [OPTIONS]

Show systemd services in a terminal UI.
By default only loaded and active units are shown.

Options:
  -a, --all            Shorthand for --load all --active all --sub all
      --load <value>   Filter by load state (all, loaded, stub, not-found, bad-setting, error, merged, masked)
      --active <value> Filter by active state (all, active, reloading, inactive, failed, activating, deactivating, maintenance, refreshing)
      --sub <value>    Filter by sub state (all, running, exited, dead, failed, start-pre, start, start-post, auto-restart, auto-restart-queued, dead-before-auto-restart, condition, reload, reload-post, reload-signal, reload-notify, stop, stop-watchdog, stop-sigterm, stop-sigkill, stop-post, final-sigterm, final-sigkill, final-watchdog, cleaning)
  -r, --refresh <num>  Auto-refresh interval in seconds (0 disables, default: 0)
  -h, --help           Show this help text
  -v, --version        Show version and copyright"
    )
}

/// Human-readable version output.
pub fn version_text() -> &'static str {
    concat!(
        "lsu v",
        env!("CARGO_PKG_VERSION"),
        "\napache v2 (c) 2026 l5yth"
    )
}

fn parse_refresh_secs(value: &str) -> Result<u64> {
    let secs = value
        .parse::<u64>()
        .with_context(|| format!("invalid refresh value: {value}"))?;
    Ok(secs)
}

/// Parse command-line arguments into a [`Config`].
pub fn parse_args<I, S>(args: I) -> Result<Config>
where
    I: IntoIterator<Item = S>,
    S: Into<String>,
{
    let mut load_filter: Option<LoadFilter> = None;
    let mut active_filter: Option<ActiveFilter> = None;
    let mut sub_filter: Option<SubFilter> = None;
    let mut refresh_secs = 0u64;
    let mut show_help = false;
    let mut show_version = false;
    let mut saw_all = false;
    let mut saw_specific_filter = false;

    let mut it = args.into_iter().map(Into::into);
    let _program = it.next();

    while let Some(arg) = it.next() {
        match arg.as_str() {
            "-a" | "--all" => {
                saw_all = true;
            }
            "-h" | "--help" => show_help = true,
            "-v" | "--version" => show_version = true,
            "--load" => {
                let value = it
                    .next()
                    .ok_or_else(|| anyhow!("missing value for {arg}\n\n{}", usage()))?;
                load_filter = Some(value.parse()?);
                saw_specific_filter = true;
            }
            "--active" => {
                let value = it
                    .next()
                    .ok_or_else(|| anyhow!("missing value for {arg}\n\n{}", usage()))?;
                active_filter = Some(value.parse()?);
                saw_specific_filter = true;
            }
            "--sub" => {
                let value = it
                    .next()
                    .ok_or_else(|| anyhow!("missing value for {arg}\n\n{}", usage()))?;
                sub_filter = Some(value.parse()?);
                saw_specific_filter = true;
            }
            "-r" | "--refresh" => {
                let value = it
                    .next()
                    .ok_or_else(|| anyhow!("missing value for {arg}\n\n{}", usage()))?;
                refresh_secs = parse_refresh_secs(&value)?;
            }
            _ => {
                if let Some(value) = arg.strip_prefix("--load=") {
                    load_filter = Some(value.parse()?);
                    saw_specific_filter = true;
                } else if let Some(value) = arg.strip_prefix("--active=") {
                    active_filter = Some(value.parse()?);
                    saw_specific_filter = true;
                } else if let Some(value) = arg.strip_prefix("--sub=") {
                    sub_filter = Some(value.parse()?);
                    saw_specific_filter = true;
                } else if let Some(value) = arg.strip_prefix("--refresh=") {
                    refresh_secs = parse_refresh_secs(value)?;
                } else {
                    return Err(anyhow!("unknown argument: {arg}\n\n{}", usage()));
                }
            }
        }
    }

    if saw_all && saw_specific_filter {
        return Err(anyhow!(
            "--all cannot be combined with --load, --active, or --sub\n\n{}",
            usage()
        ));
    }

    let (load, active, sub) = if saw_all {
        (LoadFilter::All, ActiveFilter::All, SubFilter::All)
    } else if saw_specific_filter {
        (
            load_filter.unwrap_or(LoadFilter::All),
            active_filter.unwrap_or(ActiveFilter::All),
            sub_filter.unwrap_or(SubFilter::All),
        )
    } else {
        (LoadFilter::Loaded, ActiveFilter::Active, SubFilter::Running)
    };

    Ok(Config {
        load_filter: load.as_str().to_string(),
        active_filter: active.as_str().to_string(),
        sub_filter: sub.as_str().to_string(),
        refresh_secs,
        show_help,
        show_version,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_args_defaults() {
        let cfg = parse_args(vec!["lsu"]).expect("default args should parse");
        assert_eq!(cfg.load_filter, "loaded");
        assert_eq!(cfg.active_filter, "active");
        assert_eq!(cfg.sub_filter, "running");
        assert_eq!(cfg.refresh_secs, 0);
        assert!(!cfg.show_help);
        assert!(!cfg.show_version);
    }

    #[test]
    fn parse_args_all_and_refresh() {
        let cfg = parse_args(vec!["lsu", "--all", "--refresh", "5"]).expect("flags should parse");
        assert_eq!(cfg.load_filter, "all");
        assert_eq!(cfg.active_filter, "all");
        assert_eq!(cfg.sub_filter, "all");
        assert_eq!(cfg.refresh_secs, 5);
        assert!(!cfg.show_help);
        assert!(!cfg.show_version);
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
    fn parse_args_version_flag() {
        let cfg = parse_args(vec!["lsu", "--version"]).expect("version should parse");
        assert!(cfg.show_version);

        let cfg = parse_args(vec!["lsu", "-v"]).expect("short version should parse");
        assert!(cfg.show_version);
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
    fn parse_args_rejects_missing_refresh_values() {
        let err = parse_args(vec!["lsu", "--refresh"]).expect_err("missing --refresh value");
        assert!(err.to_string().contains("missing value for --refresh"));

        let err = parse_args(vec!["lsu", "-r"]).expect_err("missing -r value");
        assert!(err.to_string().contains("missing value for -r"));
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
    fn parse_args_supports_equals_forms() {
        let cfg = parse_args(vec![
            "lsu",
            "--load=loaded",
            "--active=inactive",
            "--sub=dead",
            "--refresh=3",
        ])
        .expect("equals forms should parse");
        assert_eq!(cfg.load_filter, "loaded");
        assert_eq!(cfg.active_filter, "inactive");
        assert_eq!(cfg.sub_filter, "dead");
        assert_eq!(cfg.refresh_secs, 3);
    }

    #[test]
    fn usage_mentions_help_flag() {
        assert!(usage().contains("--help"));
    }

    #[test]
    fn usage_mentions_version_flag() {
        assert!(usage().contains("--version"));
        assert!(usage().contains(env!("CARGO_PKG_VERSION")));
    }

    #[test]
    fn version_text_contains_required_lines() {
        let v = version_text();
        assert!(v.contains(&format!("lsu v{}", env!("CARGO_PKG_VERSION"))));
        assert!(v.contains("apache v2 (c) 2026 l5yth"));
    }

    #[test]
    fn parse_args_specific_filters_imply_all_for_omitted_ones() {
        let cfg = parse_args(vec!["lsu", "--sub", "dead"]).expect("sub filter should parse");
        assert_eq!(cfg.load_filter, "all");
        assert_eq!(cfg.active_filter, "all");
        assert_eq!(cfg.sub_filter, "dead");

        let cfg = parse_args(vec!["lsu", "--load", "loaded"]).expect("load filter should parse");
        assert_eq!(cfg.load_filter, "loaded");
        assert_eq!(cfg.active_filter, "all");
        assert_eq!(cfg.sub_filter, "all");
    }

    #[test]
    fn parse_args_rejects_all_with_specific_filters() {
        let err = parse_args(vec!["lsu", "--all", "--load", "loaded"])
            .expect_err("must reject mixed all/specific");
        assert!(err.to_string().contains("--all cannot be combined"));
    }

    #[test]
    fn parse_args_rejects_invalid_filter_values() {
        let err = parse_args(vec!["lsu", "--load", "bogus"]).expect_err("invalid load");
        assert!(err.to_string().contains("invalid --load value"));

        let err = parse_args(vec!["lsu", "--active", "bogus"]).expect_err("invalid active");
        assert!(err.to_string().contains("invalid --active value"));

        let err = parse_args(vec!["lsu", "--sub", "bogus"]).expect_err("invalid sub");
        assert!(err.to_string().contains("invalid --sub value"));
    }

    #[test]
    fn parse_args_rejects_invalid_filter_values_in_equals_forms() {
        let err = parse_args(vec!["lsu", "--load=bogus"]).expect_err("invalid load");
        assert!(err.to_string().contains("invalid --load value"));

        let err = parse_args(vec!["lsu", "--active=bogus"]).expect_err("invalid active");
        assert!(err.to_string().contains("invalid --active value"));

        let err = parse_args(vec!["lsu", "--sub=bogus"]).expect_err("invalid sub");
        assert!(err.to_string().contains("invalid --sub value"));
    }

    #[test]
    fn parse_args_rejects_invalid_refresh_value_in_equals_form() {
        let err = parse_args(vec!["lsu", "--refresh=abc"]).expect_err("invalid refresh");
        assert!(err.to_string().contains("invalid refresh value"));
    }

    #[test]
    fn parse_args_accepts_stub_load_state() {
        let cfg = parse_args(vec!["lsu", "--load", "stub"]).expect("stub should parse");
        assert_eq!(cfg.load_filter, "stub");
        assert_eq!(cfg.active_filter, "all");
        assert_eq!(cfg.sub_filter, "all");
    }

    #[test]
    fn parse_args_accepts_extended_service_substates() {
        let cfg = parse_args(vec!["lsu", "--sub", "condition"]).expect("condition should parse");
        assert_eq!(cfg.sub_filter, "condition");

        let cfg =
            parse_args(vec!["lsu", "--sub", "reload-post"]).expect("reload-post should parse");
        assert_eq!(cfg.sub_filter, "reload-post");

        let cfg = parse_args(vec!["lsu", "--sub", "dead-before-auto-restart"])
            .expect("dead-before-auto-restart should parse");
        assert_eq!(cfg.sub_filter, "dead-before-auto-restart");

        let cfg = parse_args(vec!["lsu", "--sub", "auto-restart-queued"])
            .expect("auto-restart-queued should parse");
        assert_eq!(cfg.sub_filter, "auto-restart-queued");
    }

    #[test]
    fn parse_args_accepts_all_load_values() {
        for value in [
            "all",
            "loaded",
            "stub",
            "not-found",
            "bad-setting",
            "error",
            "merged",
            "masked",
        ] {
            let cfg = parse_args(vec!["lsu", "--load", value]).expect("load should parse");
            assert_eq!(cfg.load_filter, value);
        }
    }

    #[test]
    fn parse_args_accepts_all_active_values() {
        for value in [
            "all",
            "active",
            "reloading",
            "inactive",
            "failed",
            "activating",
            "deactivating",
            "maintenance",
            "refreshing",
        ] {
            let cfg = parse_args(vec!["lsu", "--active", value]).expect("active should parse");
            assert_eq!(cfg.active_filter, value);
        }
    }

    #[test]
    fn parse_args_accepts_all_sub_values() {
        for value in [
            "all",
            "running",
            "exited",
            "dead",
            "failed",
            "start-pre",
            "start",
            "start-post",
            "auto-restart",
            "auto-restart-queued",
            "dead-before-auto-restart",
            "condition",
            "reload",
            "reload-post",
            "reload-signal",
            "reload-notify",
            "stop",
            "stop-watchdog",
            "stop-sigterm",
            "stop-sigkill",
            "stop-post",
            "final-sigterm",
            "final-sigkill",
            "final-watchdog",
            "cleaning",
        ] {
            let cfg = parse_args(vec!["lsu", "--sub", value]).expect("sub should parse");
            assert_eq!(cfg.sub_filter, value);
        }
    }

    #[test]
    fn parse_args_rejects_all_mixed_with_equals_filters() {
        let err = parse_args(vec!["lsu", "--all", "--sub=running"])
            .expect_err("must reject mixed all/equal filter");
        assert!(err.to_string().contains("--all cannot be combined"));
    }
}

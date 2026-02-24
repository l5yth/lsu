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

//! `systemctl` integration and service filtering logic.

use anyhow::Result;
#[cfg(not(test))]
use anyhow::{Context, bail};
#[cfg(not(test))]
use std::process::Command;

#[cfg(not(test))]
use crate::command::{CommandExecError, cmd_stdout, command_timeout, resolve_trusted_binary};
use crate::{cli::Config, types::SystemctlUnit};

/// Match one state value against a filter value (`all` means wildcard).
pub fn filter_matches(value: &str, wanted: &str) -> bool {
    wanted == "all" || value == wanted
}

/// Whether all filter dimensions are set to `all`.
pub fn is_full_all(cfg: &Config) -> bool {
    cfg.load_filter == "all" && cfg.active_filter == "all" && cfg.sub_filter == "all"
}

/// Whether the query must fetch the full set instead of `--state=running`.
pub fn should_fetch_all(cfg: &Config) -> bool {
    !(cfg.load_filter == "all" && cfg.active_filter == "active" && cfg.sub_filter == "running")
}

/// Query service units via `systemctl` JSON output.
#[cfg(not(test))]
pub fn fetch_services(show_all: bool) -> Result<Vec<SystemctlUnit>> {
    let systemctl = resolve_trusted_binary("systemctl")?;
    let mut cmd = Command::new(systemctl);
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

    let s = match cmd_stdout(&mut cmd) {
        Ok(s) => s,
        Err(CommandExecError::Timeout { .. }) => {
            bail!(
                "systemctl list-units timed out after {}s",
                command_timeout().as_secs()
            )
        }
        Err(e) => return Err(e).context("systemctl list-units failed"),
    };
    let units: Vec<SystemctlUnit> =
        serde_json::from_str(&s).context("failed to parse systemctl JSON")?;
    Ok(units)
}

#[cfg(test)]
/// Test-build stub for `fetch_services`; runtime I/O path is tested in integration environments.
pub fn fetch_services(_show_all: bool) -> Result<Vec<SystemctlUnit>> {
    Ok(Vec::new())
}

/// Apply CLI load/active/sub filters to fetched units.
pub fn filter_services(units: Vec<SystemctlUnit>, cfg: &Config) -> Vec<SystemctlUnit> {
    units
        .into_iter()
        .filter(|u| {
            filter_matches(&u.load, &cfg.load_filter)
                && filter_matches(&u.active, &cfg.active_filter)
                && filter_matches(&u.sub, &cfg.sub_filter)
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

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
    fn fetch_services_test_stub_returns_empty() {
        let units = fetch_services(false).expect("stub should succeed");
        assert!(units.is_empty());
    }
}

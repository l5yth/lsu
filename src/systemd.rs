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

#[cfg(not(test))]
use anyhow::{Context, bail};
use anyhow::{Result, anyhow};
#[cfg(not(test))]
use std::process::Command;

#[cfg(not(test))]
use crate::command::{CommandExecError, cmd_stdout, command_timeout, resolve_trusted_binary};
use crate::{
    cli::Config,
    types::{Scope, SystemctlUnit, UnitAction},
};

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
    !((cfg.load_filter == "all" || cfg.load_filter == "loaded")
        && cfg.active_filter == "active"
        && cfg.sub_filter == "running")
}

/// Choose the start/stop action for a unit from its current `ActiveState`.
pub fn action_for_active_state(active_state: &str) -> UnitAction {
    match active_state {
        "active" | "activating" | "deactivating" | "reloading" => UnitAction::Stop,
        _ => UnitAction::Start,
    }
}

/// Choose the enable/disable action for a unit from its current `UnitFileState`.
pub fn action_for_unit_file_state(unit_file_state: &str) -> Result<UnitAction> {
    match unit_file_state {
        "enabled" | "enabled-runtime" | "linked" | "linked-runtime" => Ok(UnitAction::Disable),
        "disabled" => Ok(UnitAction::Enable),
        other => Err(anyhow!(
            "unit file state '{other}' does not support enable/disable"
        )),
    }
}

#[cfg(not(test))]
fn fetch_unit_property(scope: Scope, unit: &str, property: &str) -> Result<String> {
    let systemctl = resolve_trusted_binary("systemctl")?;
    let mut cmd = Command::new(systemctl);
    cmd.arg("show")
        .arg(scope.as_systemd_arg())
        .arg("--property")
        .arg(property)
        .arg("--value")
        .arg(unit);
    let output =
        cmd_stdout(&mut cmd).with_context(|| format!("systemctl show {property} failed"))?;
    Ok(output.trim().to_string())
}

/// Determine whether a start or stop action should be offered for a unit.
#[cfg(not(test))]
pub fn select_start_stop_action(scope: Scope, unit: &str) -> Result<UnitAction> {
    let active_state = fetch_unit_property(scope, unit, "ActiveState")?;
    Ok(action_for_active_state(&active_state))
}

/// Determine whether an enable or disable action should be offered for a unit.
#[cfg(not(test))]
pub fn select_enable_disable_action(scope: Scope, unit: &str) -> Result<UnitAction> {
    let unit_file_state = fetch_unit_property(scope, unit, "UnitFileState")?;
    action_for_unit_file_state(&unit_file_state)
}

/// Execute one start/stop/enable/disable action for a unit.
#[cfg(not(test))]
pub fn run_unit_action(scope: Scope, unit: &str, action: UnitAction) -> Result<()> {
    let systemctl = resolve_trusted_binary("systemctl")?;
    let mut cmd = Command::new(systemctl);
    cmd.arg(action.as_systemctl_arg())
        .arg(scope.as_systemd_arg())
        .arg(unit);
    let _ = cmd_stdout(&mut cmd)
        .with_context(|| format!("systemctl {} failed", action.as_systemctl_arg()))?;
    Ok(())
}

/// Query service units via `systemctl` JSON output.
#[cfg(not(test))]
pub fn fetch_services(scope: Scope, show_all: bool) -> Result<Vec<SystemctlUnit>> {
    let systemctl = resolve_trusted_binary("systemctl")?;
    let mut cmd = Command::new(systemctl);
    cmd.arg("list-units")
        .arg(scope.as_systemd_arg())
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
pub fn fetch_services(scope: Scope, show_all: bool) -> Result<Vec<SystemctlUnit>> {
    if matches!(scope, Scope::User) {
        return Err(anyhow!("systemd test error"));
    }
    if show_all {
        return Ok(vec![
            SystemctlUnit {
                unit: "a.service".to_string(),
                load: "loaded".to_string(),
                active: "active".to_string(),
                sub: "running".to_string(),
                description: "A".to_string(),
            },
            SystemctlUnit {
                unit: "journal-error.service".to_string(),
                load: "loaded".to_string(),
                active: "inactive".to_string(),
                sub: "dead".to_string(),
                description: "Err".to_string(),
            },
        ]);
    }
    Ok(Vec::new())
}

/// Determine whether a start or stop action should be offered for a unit.
#[cfg(test)]
pub fn select_start_stop_action(_scope: Scope, unit: &str) -> Result<UnitAction> {
    if unit == "state-error.service" {
        return Err(anyhow!("active state test error"));
    }
    if unit == "running.service" {
        Ok(UnitAction::Stop)
    } else {
        Ok(UnitAction::Start)
    }
}

/// Determine whether an enable or disable action should be offered for a unit.
#[cfg(test)]
pub fn select_enable_disable_action(_scope: Scope, unit: &str) -> Result<UnitAction> {
    if unit == "state-error.service" {
        return Err(anyhow!("unit file state test error"));
    }
    if unit == "enabled.service" {
        Ok(UnitAction::Disable)
    } else if unit == "static.service" {
        Err(anyhow!(
            "unit file state 'static' does not support enable/disable"
        ))
    } else {
        Ok(UnitAction::Enable)
    }
}

/// Execute one start/stop/enable/disable action for a unit.
#[cfg(test)]
pub fn run_unit_action(_scope: Scope, unit: &str, _action: UnitAction) -> Result<()> {
    if unit == "action-error.service" {
        return Err(anyhow!("unit action test error"));
    }
    Ok(())
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
            show_help: false,
            show_version: false,
            debug_tui: false,
            scope: Scope::System,
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
    fn action_for_active_state_toggles_runningish_units_to_stop() {
        assert_eq!(action_for_active_state("active"), UnitAction::Stop);
        assert_eq!(action_for_active_state("reloading"), UnitAction::Stop);
        assert_eq!(action_for_active_state("failed"), UnitAction::Start);
        assert_eq!(action_for_active_state("inactive"), UnitAction::Start);
    }

    #[test]
    fn action_for_unit_file_state_toggles_enabledish_units_to_disable() {
        assert_eq!(
            action_for_unit_file_state("enabled").expect("enabled action"),
            UnitAction::Disable
        );
        assert_eq!(
            action_for_unit_file_state("linked-runtime").expect("linked-runtime action"),
            UnitAction::Disable
        );
        assert_eq!(
            action_for_unit_file_state("disabled").expect("disabled action"),
            UnitAction::Enable
        );
    }

    #[test]
    fn action_for_unit_file_state_rejects_unsupported_states() {
        for state in [
            "static",
            "masked",
            "generated",
            "transient",
            "indirect",
            "alias",
        ] {
            let err = action_for_unit_file_state(state).expect_err("unsupported state");
            assert!(err.to_string().contains(&format!(
                "unit file state '{state}' does not support enable/disable"
            )));
        }
    }

    #[test]
    fn is_full_all_only_true_when_all_three_filters_are_all() {
        let all_cfg = Config {
            load_filter: "all".to_string(),
            active_filter: "all".to_string(),
            sub_filter: "all".to_string(),
            show_help: false,
            show_version: false,
            debug_tui: false,
            scope: Scope::System,
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
            show_help: false,
            show_version: false,
            debug_tui: false,
            scope: Scope::System,
        };
        assert!(!should_fetch_all(&default_cfg));

        let loaded_default = Config {
            load_filter: "loaded".to_string(),
            ..default_cfg.clone()
        };
        assert!(!should_fetch_all(&loaded_default));

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
        let units = fetch_services(Scope::System, false).expect("stub should succeed");
        assert!(units.is_empty());
    }

    #[test]
    fn fetch_services_test_stub_returns_row_for_show_all() {
        let units = fetch_services(Scope::System, true).expect("stub should succeed");
        assert_eq!(units.len(), 2);
    }

    #[test]
    fn select_action_test_stubs_return_expected_values() {
        assert_eq!(
            select_start_stop_action(Scope::System, "running.service").expect("start/stop action"),
            UnitAction::Stop
        );
        assert_eq!(
            select_enable_disable_action(Scope::System, "enabled.service")
                .expect("enable/disable action"),
            UnitAction::Disable
        );
        assert_eq!(
            select_enable_disable_action(Scope::System, "disabled.service")
                .expect("enable/disable action"),
            UnitAction::Enable
        );
    }

    #[test]
    fn select_action_test_stubs_surface_errors() {
        let start_stop = select_start_stop_action(Scope::System, "state-error.service")
            .expect_err("start/stop error");
        assert!(start_stop.to_string().contains("active state test error"));

        let enable_disable = select_enable_disable_action(Scope::System, "state-error.service")
            .expect_err("enable/disable error");
        assert!(
            enable_disable
                .to_string()
                .contains("unit file state test error")
        );

        let unsupported = select_enable_disable_action(Scope::System, "static.service")
            .expect_err("unsupported enable/disable");
        assert!(
            unsupported
                .to_string()
                .contains("unit file state 'static' does not support enable/disable")
        );
    }

    #[test]
    fn run_unit_action_test_stub_supports_success_and_error() {
        run_unit_action(Scope::System, "demo.service", UnitAction::Start).expect("action ok");
        let err = run_unit_action(Scope::System, "action-error.service", UnitAction::Stop)
            .expect_err("action error");
        assert!(err.to_string().contains("unit action test error"));
    }

    #[test]
    fn fetch_services_test_stub_errors_for_user_scope() {
        let err = fetch_services(Scope::User, false).expect_err("stub should fail");
        assert!(err.to_string().contains("systemd test error"));
    }
}

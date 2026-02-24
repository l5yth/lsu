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

/// Parsed command-line configuration.
#[derive(Debug, Clone)]
pub struct Config {
    pub load_filter: String,
    pub active_filter: String,
    pub sub_filter: String,
    pub refresh_secs: u64,
    pub show_help: bool,
}

/// Human-readable CLI usage text.
pub fn usage() -> &'static str {
    "Usage: lsu [OPTIONS]

Show systemd services in a terminal UI.
By default only loaded and active units are shown.

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

/// Parse command-line arguments into a [`Config`].
pub fn parse_args<I, S>(args: I) -> Result<Config>
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

#[cfg(test)]
mod tests {
    use super::*;

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
    fn parse_args_supports_equals_forms() {
        let cfg = parse_args(vec![
            "lsu",
            "--load=loaded",
            "--active=active",
            "--sub=running",
            "--refresh=3",
        ])
        .expect("equals forms should parse");
        assert_eq!(cfg.load_filter, "loaded");
        assert_eq!(cfg.active_filter, "active");
        assert_eq!(cfg.sub_filter, "running");
        assert_eq!(cfg.refresh_secs, 3);
    }

    #[test]
    fn usage_mentions_help_flag() {
        assert!(usage().contains("--help"));
    }
}

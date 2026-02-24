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

//! Process execution helpers.

use anyhow::{Result, anyhow, bail};
use std::collections::HashSet;
use std::env;
use std::path::{Path, PathBuf};
use std::process::{Command, ExitStatus, Stdio};
use std::thread;
use std::time::{Duration, Instant};

const ALLOWED_BINARIES: [&str; 2] = ["systemctl", "journalctl"];
const TRUSTED_DIRS: [&str; 5] = ["/usr/bin", "/bin", "/usr/sbin", "/sbin", "/usr/local/bin"];
const DEFAULT_CMD_TIMEOUT_SECS: u64 = 5;

/// Structured subprocess failure modes.
#[derive(Debug)]
pub enum CommandExecError {
    Io(std::io::Error),
    Timeout {
        command: String,
        timeout: Duration,
    },
    NonZeroExit {
        command: String,
        status: ExitStatus,
        stderr: String,
    },
}

impl std::fmt::Display for CommandExecError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Io(e) => write!(f, "{e}"),
            Self::Timeout { command, timeout } => {
                write!(
                    f,
                    "command timed out after {}s: {}",
                    timeout.as_secs_f32(),
                    command
                )
            }
            Self::NonZeroExit {
                command,
                status,
                stderr,
            } => write!(
                f,
                "command failed (status={}): {}{}",
                status,
                command,
                if stderr.trim().is_empty() {
                    String::new()
                } else {
                    format!(" | {}", stderr.trim())
                }
            ),
        }
    }
}

impl std::error::Error for CommandExecError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Io(e) => Some(e),
            _ => None,
        }
    }
}

impl From<std::io::Error> for CommandExecError {
    fn from(value: std::io::Error) -> Self {
        Self::Io(value)
    }
}

/// Return the global subprocess timeout.
pub fn command_timeout() -> Duration {
    let secs = env::var("LSU_CMD_TIMEOUT_SECS")
        .ok()
        .and_then(|v| v.parse::<u64>().ok())
        .filter(|v| *v > 0)
        .unwrap_or(DEFAULT_CMD_TIMEOUT_SECS);
    Duration::from_secs(secs)
}

/// Run a command and return UTF-8 decoded stdout on success.
pub fn cmd_stdout(cmd: &mut Command) -> std::result::Result<String, CommandExecError> {
    cmd_stdout_with_timeout(cmd, command_timeout())
}

/// Run a command with an explicit timeout and return UTF-8 decoded stdout on success.
pub fn cmd_stdout_with_timeout(
    cmd: &mut Command,
    timeout: Duration,
) -> std::result::Result<String, CommandExecError> {
    let rendered = render_command(cmd);
    cmd.stdout(Stdio::piped());
    cmd.stderr(Stdio::piped());
    let mut child = cmd.spawn()?;
    let start = Instant::now();

    loop {
        if let Some(status) = child.try_wait()? {
            let out = child.wait_with_output()?;
            if !status.success() {
                return Err(CommandExecError::NonZeroExit {
                    command: rendered,
                    status,
                    stderr: String::from_utf8_lossy(&out.stderr).to_string(),
                });
            }
            return Ok(String::from_utf8_lossy(&out.stdout).to_string());
        }

        if start.elapsed() >= timeout {
            let _ = child.kill();
            let _ = child.wait();
            return Err(CommandExecError::Timeout {
                command: rendered,
                timeout,
            });
        }

        thread::sleep(Duration::from_millis(10));
    }
}

fn render_command(cmd: &Command) -> String {
    let prog = cmd.get_program().to_string_lossy();
    let args = cmd
        .get_args()
        .map(|a| a.to_string_lossy().into_owned())
        .collect::<Vec<_>>()
        .join(" ");
    if args.is_empty() {
        prog.into_owned()
    } else {
        format!("{prog} {args}")
    }
}

/// Resolve a trusted absolute path for one allowed external binary.
pub fn resolve_trusted_binary(binary: &str) -> Result<PathBuf> {
    let trusted_dirs: Vec<PathBuf> = TRUSTED_DIRS.iter().map(PathBuf::from).collect();
    resolve_trusted_binary_in(binary, env::var_os("PATH"), &trusted_dirs)
}

fn resolve_trusted_binary_in(
    binary: &str,
    path_env: Option<std::ffi::OsString>,
    trusted_dirs: &[PathBuf],
) -> Result<PathBuf> {
    if !ALLOWED_BINARIES.contains(&binary) {
        bail!(
            "binary '{}' is not in the allowed external command list",
            binary
        );
    }

    let trusted_roots = canonical_trusted_roots(trusted_dirs);
    let path_entries: Vec<PathBuf> = path_env
        .as_deref()
        .map(env::split_paths)
        .map(Iterator::collect)
        .unwrap_or_default();

    let mut checked = HashSet::new();

    for dir in path_entries.iter().chain(trusted_dirs.iter()) {
        let candidate = dir.join(binary);
        if !checked.insert(candidate.clone()) || !candidate.is_file() {
            continue;
        }
        if !is_executable(&candidate) {
            continue;
        }
        if is_under_trusted_roots(&candidate, &trusted_roots) {
            return Ok(candidate);
        }
    }

    Err(anyhow!(
        "trusted '{}' not found in allowlisted directories ({})",
        binary,
        TRUSTED_DIRS.join(", ")
    ))
}

fn canonical_trusted_roots(trusted_dirs: &[PathBuf]) -> Vec<PathBuf> {
    trusted_dirs
        .iter()
        .filter_map(|d| d.canonicalize().ok())
        .collect()
}

fn is_under_trusted_roots(candidate: &Path, trusted_roots: &[PathBuf]) -> bool {
    let Ok(canon) = candidate.canonicalize() else {
        return false;
    };
    trusted_roots.iter().any(|root| canon.starts_with(root))
}

#[cfg(unix)]
fn is_executable(path: &Path) -> bool {
    use std::os::unix::fs::PermissionsExt;
    path.metadata()
        .map(|m| (m.permissions().mode() & 0o111) != 0)
        .unwrap_or(false)
}

#[cfg(not(unix))]
fn is_executable(path: &Path) -> bool {
    path.is_file()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::ffi::OsStr;
    use std::fs;
    #[cfg(unix)]
    use std::os::unix::fs::PermissionsExt;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn unique_temp_dir(label: &str) -> PathBuf {
        let n = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("time should be monotonic")
            .as_nanos();
        let dir = env::temp_dir().join(format!("lsu-{label}-{n}"));
        fs::create_dir_all(&dir).expect("create temp dir");
        dir
    }

    fn make_exec(path: &Path) {
        fs::write(path, b"#!/bin/sh\nexit 0\n").expect("write file");
        #[cfg(unix)]
        {
            let mut perms = fs::metadata(path).expect("metadata").permissions();
            perms.set_mode(0o755);
            fs::set_permissions(path, perms).expect("set perms");
        }
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
        match err {
            CommandExecError::NonZeroExit { status, stderr, .. } => {
                assert_eq!(status.code(), Some(7));
                assert!(stderr.contains("fail"));
            }
            other => panic!("expected non-zero exit error, got {other}"),
        }
    }

    #[test]
    fn cmd_stdout_times_out_and_kills_child() {
        let mut cmd = Command::new("sh");
        cmd.arg("-c").arg("sleep 1; printf never");
        let err = cmd_stdout_with_timeout(&mut cmd, Duration::from_millis(50))
            .expect_err("command should time out");
        match err {
            CommandExecError::Timeout { timeout, .. } => {
                assert_eq!(timeout, Duration::from_millis(50))
            }
            other => panic!("expected timeout error, got {other}"),
        }
    }

    #[test]
    fn resolve_trusted_binary_returns_path_from_trusted_dir() {
        let trusted = unique_temp_dir("trusted");
        let untrusted = unique_temp_dir("untrusted");
        let trusted_bin = trusted.join("systemctl");
        let untrusted_bin = untrusted.join("systemctl");
        make_exec(&trusted_bin);
        make_exec(&untrusted_bin);

        let resolved = resolve_trusted_binary_in(
            "systemctl",
            Some(OsStr::new(&format!("{}:{}", untrusted.display(), trusted.display())).to_owned()),
            std::slice::from_ref(&trusted),
        )
        .expect("trusted binary should resolve");

        assert_eq!(resolved, trusted_bin);
        let _ = fs::remove_dir_all(trusted);
        let _ = fs::remove_dir_all(untrusted);
    }

    #[test]
    fn resolve_trusted_binary_rejects_untrusted_path() {
        let untrusted = unique_temp_dir("untrusted-only");
        let untrusted_bin = untrusted.join("journalctl");
        make_exec(&untrusted_bin);

        let err = resolve_trusted_binary_in(
            "journalctl",
            Some(OsStr::new(untrusted.to_string_lossy().as_ref()).to_owned()),
            &[],
        )
        .expect_err("untrusted binary should be rejected");

        assert!(err.to_string().contains("trusted 'journalctl' not found"));
        let _ = fs::remove_dir_all(untrusted);
    }

    #[test]
    fn resolve_trusted_binary_errors_when_missing() {
        let err = resolve_trusted_binary_in("systemctl", Some(OsStr::new("").to_owned()), &[])
            .expect_err("missing binary should fail");
        assert!(err.to_string().contains("trusted 'systemctl' not found"));
    }
}

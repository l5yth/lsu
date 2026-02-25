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
use std::collections::{HashSet, hash_map::DefaultHasher};
use std::env;
use std::hash::{Hash, Hasher};
use std::io::Read;
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
    let mut stdout = child
        .stdout
        .take()
        .ok_or_else(|| CommandExecError::Io(std::io::Error::other("missing child stdout pipe")))?;
    let mut stderr = child
        .stderr
        .take()
        .ok_or_else(|| CommandExecError::Io(std::io::Error::other("missing child stderr pipe")))?;
    let stdout_handle = thread::spawn(move || {
        let mut out = Vec::new();
        let _ = stdout.read_to_end(&mut out);
        out
    });
    let stderr_handle = thread::spawn(move || {
        let mut out = Vec::new();
        let _ = stderr.read_to_end(&mut out);
        out
    });
    let start = Instant::now();

    let status = loop {
        if let Some(status) = child.try_wait()? {
            break status;
        }

        if start.elapsed() >= timeout {
            let _ = child.kill();
            let _ = child.wait();
            let _ = stdout_handle.join();
            let _ = stderr_handle.join();
            return Err(CommandExecError::Timeout {
                command: rendered,
                timeout,
            });
        }

        thread::sleep(Duration::from_millis(10));
    };

    let stdout = stdout_handle.join().unwrap_or_default();
    let stderr = stderr_handle.join().unwrap_or_default();
    if !status.success() {
        return Err(CommandExecError::NonZeroExit {
            command: rendered,
            status,
            stderr: String::from_utf8_lossy(&stderr).to_string(),
        });
    }
    Ok(String::from_utf8_lossy(&stdout).to_string())
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

    let path_entries: Vec<PathBuf> = path_env
        .as_deref()
        .map(env::split_paths)
        .map(Iterator::collect)
        .unwrap_or_default();
    let trusted_roots = canonical_trusted_roots(trusted_dirs, &path_entries);

    let mut checked = HashSet::new();

    for dir in trusted_dirs.iter().chain(path_entries.iter()) {
        let candidate = dir.join(binary);
        if !checked.insert(candidate.clone()) || !candidate.is_file() {
            continue;
        }
        if !is_executable(&candidate) {
            continue;
        }
        if let Some(canonical) = canonical_trusted_candidate(&candidate, &trusted_roots) {
            return Ok(canonical);
        }
    }

    match binary {
        "systemctl" => Err(anyhow!("no systemctl command found, do use systemd?")),
        "journalctl" => Err(anyhow!("no journalctl command found, do use systemd?")),
        _ => Err(anyhow!(
            "trusted '{}' not found in allowlisted directories ({})",
            binary,
            TRUSTED_DIRS.join(", ")
        )),
    }
}

fn canonical_trusted_roots(trusted_dirs: &[PathBuf], path_entries: &[PathBuf]) -> Vec<PathBuf> {
    let mut out = Vec::new();
    let mut seen = HashSet::new();
    for dir in trusted_dirs.iter().chain(path_entries.iter()) {
        let Ok(canon) = dir.canonicalize() else {
            continue;
        };
        if !is_secure_dir(&canon) {
            continue;
        }
        if !seen.insert(path_key(&canon)) {
            continue;
        }
        out.push(canon);
    }
    out
}

fn path_key(path: &Path) -> u64 {
    let mut hasher = DefaultHasher::new();
    path.hash(&mut hasher);
    hasher.finish()
}

fn canonical_trusted_candidate(candidate: &Path, trusted_roots: &[PathBuf]) -> Option<PathBuf> {
    let Ok(canon) = candidate.canonicalize() else {
        return None;
    };
    let file_name = canon.file_name().and_then(|n| n.to_str())?;
    let requested = candidate.file_name().and_then(|n| n.to_str())?;
    if trusted_roots.iter().any(|root| canon.starts_with(root)) && file_name == requested {
        Some(canon)
    } else {
        None
    }
}

#[cfg(unix)]
fn is_secure_dir(path: &Path) -> bool {
    use std::os::unix::fs::PermissionsExt;
    path.metadata()
        .map(|m| (m.permissions().mode() & 0o022) == 0)
        .unwrap_or(false)
}

#[cfg(not(unix))]
fn is_secure_dir(path: &Path) -> bool {
    path.is_dir()
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
    use std::error::Error;
    use std::ffi::OsStr;
    use std::fs;
    #[cfg(unix)]
    use std::os::unix::fs::PermissionsExt;
    use std::sync::Mutex;
    use std::time::{SystemTime, UNIX_EPOCH};

    static ENV_LOCK: Mutex<()> = Mutex::new(());

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
    fn cmd_stdout_non_zero_with_empty_stderr_omits_separator_suffix() {
        let mut cmd = Command::new("sh");
        cmd.arg("-c").arg("exit 3");
        let err = cmd_stdout(&mut cmd).expect_err("command should fail");
        let msg = err.to_string();
        assert!(msg.contains("command failed (status="));
        assert!(!msg.contains(" | "));
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
    fn cmd_stdout_handles_large_stdout_without_deadlock() {
        let mut cmd = Command::new("sh");
        cmd.arg("-c")
            .arg("head -c 1048576 /dev/zero 2>/dev/null | tr '\\0' x");
        let out = cmd_stdout_with_timeout(&mut cmd, Duration::from_secs(2))
            .expect("large output command should succeed");
        assert_eq!(out.len(), 1_048_576);
    }

    #[test]
    fn command_timeout_defaults_when_env_is_missing_or_invalid() {
        let _guard = ENV_LOCK.lock().expect("lock env");
        let prev = env::var_os("LSU_CMD_TIMEOUT_SECS");
        // SAFETY: test serializes env access with ENV_LOCK.
        unsafe { env::remove_var("LSU_CMD_TIMEOUT_SECS") };
        assert_eq!(command_timeout(), Duration::from_secs(5));
        // SAFETY: test serializes env access with ENV_LOCK.
        unsafe { env::set_var("LSU_CMD_TIMEOUT_SECS", "invalid") };
        assert_eq!(command_timeout(), Duration::from_secs(5));
        // SAFETY: test serializes env access with ENV_LOCK.
        unsafe { env::set_var("LSU_CMD_TIMEOUT_SECS", "0") };
        assert_eq!(command_timeout(), Duration::from_secs(5));
        match prev {
            Some(v) => {
                // SAFETY: test serializes env access with ENV_LOCK.
                unsafe { env::set_var("LSU_CMD_TIMEOUT_SECS", v) }
            }
            None => {
                // SAFETY: test serializes env access with ENV_LOCK.
                unsafe { env::remove_var("LSU_CMD_TIMEOUT_SECS") }
            }
        }
    }

    #[test]
    fn command_timeout_uses_positive_env_value() {
        let _guard = ENV_LOCK.lock().expect("lock env");
        let prev = env::var_os("LSU_CMD_TIMEOUT_SECS");
        // SAFETY: test serializes env access with ENV_LOCK.
        unsafe { env::set_var("LSU_CMD_TIMEOUT_SECS", "13") };
        assert_eq!(command_timeout(), Duration::from_secs(13));
        match prev {
            Some(v) => {
                // SAFETY: test serializes env access with ENV_LOCK.
                unsafe { env::set_var("LSU_CMD_TIMEOUT_SECS", v) }
            }
            None => {
                // SAFETY: test serializes env access with ENV_LOCK.
                unsafe { env::remove_var("LSU_CMD_TIMEOUT_SECS") }
            }
        }
    }

    #[test]
    fn command_exec_error_display_and_source_cover_variants() {
        let io = std::io::Error::other("io");
        let io_err = CommandExecError::from(io);
        assert!(io_err.to_string().contains("io"));
        assert!(io_err.source().is_some());

        let timeout = CommandExecError::Timeout {
            command: "cmd".to_string(),
            timeout: Duration::from_secs(2),
        };
        assert!(timeout.to_string().contains("timed out"));
        assert!(timeout.source().is_none());

        let mut cmd = Command::new("sh");
        cmd.arg("-c").arg("exit 9");
        let err = cmd_stdout(&mut cmd).expect_err("expected non-zero");
        let msg = err.to_string();
        assert!(msg.contains("status="));
        assert!(msg.contains("sh -c exit 9"));
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

        #[cfg(unix)]
        {
            let mut perms = fs::metadata(&untrusted).expect("metadata").permissions();
            perms.set_mode(0o777);
            fs::set_permissions(&untrusted, perms).expect("set perms");
        }

        let err = resolve_trusted_binary_in(
            "journalctl",
            Some(OsStr::new(untrusted.to_string_lossy().as_ref()).to_owned()),
            &[],
        )
        .expect_err("untrusted binary should be rejected");

        assert!(
            err.to_string()
                .contains("no journalctl command found, do use systemd?")
        );
        let _ = fs::remove_dir_all(untrusted);
    }

    #[test]
    fn resolve_trusted_binary_skips_non_executable_files() {
        let trusted = unique_temp_dir("trusted-nonexec");
        let target = trusted.join("systemctl");
        fs::write(&target, b"not executable").expect("write file");
        #[cfg(unix)]
        {
            let mut perms = fs::metadata(&target).expect("metadata").permissions();
            perms.set_mode(0o644);
            fs::set_permissions(&target, perms).expect("set perms");
        }

        let err = resolve_trusted_binary_in(
            "systemctl",
            Some(OsStr::new(trusted.to_string_lossy().as_ref()).to_owned()),
            std::slice::from_ref(&trusted),
        )
        .expect_err("non-executable path must be rejected");
        assert!(
            err.to_string()
                .contains("no systemctl command found, do use systemd?")
        );
        let _ = fs::remove_dir_all(trusted);
    }

    #[test]
    fn resolve_trusted_binary_handles_duplicate_path_entries() {
        let trusted = unique_temp_dir("trusted-dup-path");
        let bin = trusted.join("systemctl");
        make_exec(&bin);
        let dup_path = format!("{}:{}", trusted.display(), trusted.display());
        let resolved = resolve_trusted_binary_in(
            "systemctl",
            Some(OsStr::new(&dup_path).to_owned()),
            std::slice::from_ref(&trusted),
        )
        .expect("duplicate path entries should still resolve");
        assert_eq!(resolved, bin);
        let _ = fs::remove_dir_all(trusted);
    }

    #[test]
    fn resolve_trusted_binary_errors_when_missing() {
        let err = resolve_trusted_binary_in("systemctl", Some(OsStr::new("").to_owned()), &[])
            .expect_err("missing binary should fail");
        assert!(
            err.to_string()
                .contains("no systemctl command found, do use systemd?")
        );
    }

    #[test]
    fn resolve_trusted_binary_rejects_disallowed_binary() {
        let err = resolve_trusted_binary_in("sh", None, &[]).expect_err("must reject");
        assert!(
            err.to_string()
                .contains("is not in the allowed external command list")
        );
    }

    #[test]
    fn resolve_trusted_binary_rejects_symlink_to_different_binary_name() {
        #[cfg(not(unix))]
        {
            return;
        }
        let trusted = unique_temp_dir("trusted-link-target");
        let untrusted = unique_temp_dir("untrusted-link");
        let target = trusted.join("python");
        make_exec(&target);
        let fake = untrusted.join("systemctl");
        std::os::unix::fs::symlink(&target, &fake).expect("symlink should be created");

        let err = resolve_trusted_binary_in(
            "systemctl",
            Some(OsStr::new(untrusted.to_string_lossy().as_ref()).to_owned()),
            std::slice::from_ref(&trusted),
        )
        .expect_err("symlink to wrong binary name must be rejected");
        assert!(
            err.to_string()
                .contains("no systemctl command found, do use systemd?")
        );

        let _ = fs::remove_dir_all(trusted);
        let _ = fs::remove_dir_all(untrusted);
    }

    #[test]
    fn resolve_trusted_binary_returns_canonical_path_for_symlink_candidate() {
        #[cfg(not(unix))]
        {
            return;
        }
        let trusted = unique_temp_dir("trusted-systemctl-target");
        let untrusted = unique_temp_dir("untrusted-systemctl-link");
        let target = trusted.join("systemctl");
        make_exec(&target);
        let link = untrusted.join("systemctl");
        std::os::unix::fs::symlink(&target, &link).expect("symlink should be created");

        let resolved = resolve_trusted_binary_in(
            "systemctl",
            Some(OsStr::new(untrusted.to_string_lossy().as_ref()).to_owned()),
            std::slice::from_ref(&trusted),
        )
        .expect("symlink to trusted same-name binary should resolve");

        assert_eq!(resolved, target.canonicalize().expect("canonical target"));
        let _ = fs::remove_dir_all(trusted);
        let _ = fs::remove_dir_all(untrusted);
    }

    #[test]
    fn resolve_trusted_binary_allows_secure_path_entry_outside_defaults() {
        let secure = unique_temp_dir("secure-outside-default");
        let bin = secure.join("systemctl");
        make_exec(&bin);
        #[cfg(unix)]
        {
            let mut perms = fs::metadata(&secure).expect("metadata").permissions();
            perms.set_mode(0o755);
            fs::set_permissions(&secure, perms).expect("set perms");
        }
        let resolved = resolve_trusted_binary_in(
            "systemctl",
            Some(OsStr::new(secure.to_string_lossy().as_ref()).to_owned()),
            &[],
        )
        .expect("secure PATH entry should be allowed");
        assert_eq!(resolved, bin.canonicalize().expect("canonical path"));
        let _ = fs::remove_dir_all(secure);
    }
}

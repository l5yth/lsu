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

use anyhow::{Result, anyhow};
use std::process::Command;

/// Run a command and return UTF-8 decoded stdout on success.
pub fn cmd_stdout(cmd: &mut Command) -> Result<String> {
    let out = cmd.output()?;
    if !out.status.success() {
        return Err(anyhow!(
            "command failed (status={}): {}",
            out.status,
            String::from_utf8_lossy(&out.stderr)
        ));
    }
    Ok(String::from_utf8_lossy(&out.stdout).to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

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
}

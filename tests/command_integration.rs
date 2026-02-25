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

use lsu::command::{CommandExecError, cmd_stdout, cmd_stdout_with_timeout};
use std::{process::Command, time::Duration};

#[test]
fn command_runtime_success_and_non_zero_contracts() {
    let mut ok = Command::new("sh");
    ok.arg("-c").arg("printf integration-ok");
    let out = cmd_stdout(&mut ok).expect("success command");
    assert_eq!(out, "integration-ok");

    let mut fail = Command::new("sh");
    fail.arg("-c").arg("echo integration-fail 1>&2; exit 12");
    let err = cmd_stdout(&mut fail).expect_err("non-zero command");
    match err {
        CommandExecError::NonZeroExit { status, stderr, .. } => {
            assert_eq!(status.code(), Some(12));
            assert!(stderr.contains("integration-fail"));
        }
        other => panic!("expected non-zero exit error, got {other}"),
    }
}

#[test]
fn command_runtime_timeout_contract() {
    let mut slow = Command::new("sh");
    slow.arg("-c").arg("sleep 1; printf too-late");
    let err =
        cmd_stdout_with_timeout(&mut slow, Duration::from_millis(50)).expect_err("timeout command");
    match err {
        CommandExecError::Timeout { timeout, .. } => {
            assert_eq!(timeout, Duration::from_millis(50));
        }
        other => panic!("expected timeout error, got {other}"),
    }
}

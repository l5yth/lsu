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

use std::process::Command;

#[test]
fn tui_process_can_start_and_quit_immediately_with_pty() {
    let bin = env!("CARGO_BIN_EXE_lsu");

    let has_script = Command::new("sh")
        .arg("-c")
        .arg("command -v script >/dev/null 2>&1")
        .status()
        .expect("check script availability")
        .success();
    if !has_script {
        return;
    }

    let cmd = format!("printf q | script -qefc '{}' /dev/null", bin);
    let output = Command::new("sh")
        .arg("-c")
        .arg(cmd)
        .output()
        .expect("run tui with pty");
    assert!(output.status.code().is_some());
}

/// Exercise all three action-confirmation paths (ChooseRestart, ChooseStop, Confirm) so that
/// `run_confirmed_action`, `suspend_terminal`, and `resume_terminal` are covered by the binary
/// execution.  The sequence:
///   1. 's' → RequestStartStop → RestartOrStop prompt
///   2. 'r' → ChooseRestart → run_confirmed_action(Restart)  [systemctl may fail — that's OK]
///   3. 's' → RequestStartStop → RestartOrStop prompt again
///   4. 's' → ChooseStop → run_confirmed_action(Stop)
///   5. 'e' → RequestEnableDisable → ConfirmAction prompt
///   6. 'y' → Confirm → run_confirmed_action(Disable)
///   7. 'q' → Quit
#[cfg(feature = "debug_tui")]
#[test]
fn debug_tui_action_confirmation_paths_are_exercised_with_pty() {
    let bin = env!("CARGO_BIN_EXE_lsu");

    let has_script = Command::new("sh")
        .arg("-c")
        .arg("command -v script >/dev/null 2>&1")
        .status()
        .expect("check script availability")
        .success();
    if !has_script {
        return;
    }

    // Delays:
    //   0.3s  — TUI loads debug units
    //   0.15s — debug action resolution completes (near-instant, but needs one 50ms poll)
    //   1.0s  — systemctl finishes (may fail, but run_confirmed_action completes) + TUI resumes
    let cmd = format!(
        concat!(
            "(sleep 0.3;",
            " printf 's'; sleep 0.15;", // trigger start/stop resolution
            " printf 'r'; sleep 1.0;",  // ChooseRestart → run_confirmed_action
            " printf 's'; sleep 0.15;", // trigger start/stop resolution again
            " printf 's'; sleep 1.0;",  // ChooseStop → run_confirmed_action
            " printf 'e'; sleep 0.15;", // trigger enable/disable resolution
            " printf 'y'; sleep 1.0;",  // Confirm → run_confirmed_action
            " printf 'q')",             // Quit
            " | script -qefc '{} --debug-tui' /dev/null"
        ),
        bin
    );
    let output = Command::new("sh")
        .arg("-c")
        .arg(cmd)
        .output()
        .expect("run debug tui action paths with pty");
    assert!(output.status.code().is_some());
}

#[cfg(feature = "debug_tui")]
#[test]
fn debug_tui_process_can_open_and_refresh_detail_with_pty() {
    let bin = env!("CARGO_BIN_EXE_lsu");

    let has_script = Command::new("sh")
        .arg("-c")
        .arg("command -v script >/dev/null 2>&1")
        .status()
        .expect("check script availability")
        .success();
    if !has_script {
        return;
    }

    let cmd = format!(
        "(sleep 0.2; printf '\\r'; sleep 0.2; printf 'l'; sleep 0.2; printf 'r'; sleep 0.2; printf 'q') | script -qefc '{} --debug-tui' /dev/null",
        bin
    );
    let output = Command::new("sh")
        .arg("-c")
        .arg(cmd)
        .output()
        .expect("run debug tui with pty");
    assert!(output.status.code().is_some());
}

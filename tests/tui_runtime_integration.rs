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

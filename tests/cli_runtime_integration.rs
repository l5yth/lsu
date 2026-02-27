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
fn binary_help_and_version_exit_successfully() {
    let bin = env!("CARGO_BIN_EXE_lsu");

    let help = Command::new(bin)
        .arg("--help")
        .output()
        .expect("run --help");
    assert!(help.status.success());
    let help_stdout = String::from_utf8_lossy(&help.stdout);
    assert!(help_stdout.contains("Usage: lsu [OPTIONS]"));
    assert!(help_stdout.contains("list systemd units"));
    assert!(help_stdout.contains("--user"));
    assert!(!help_stdout.contains("--debug-tui"));

    let version = Command::new(bin)
        .arg("--version")
        .output()
        .expect("run --version");
    assert!(version.status.success());
    let version_stdout = String::from_utf8_lossy(&version.stdout);
    assert!(version_stdout.contains(&format!("lsu v{}", env!("CARGO_PKG_VERSION"))));
    assert!(version_stdout.contains("list systemd units"));
    assert!(version_stdout.contains("apache v2 (c) 2026 l5yth"));
}

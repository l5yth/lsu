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

//! App entry module.
//!
//! In test builds we expose a lightweight stub to keep unit-test coverage
//! focused on deterministic logic modules rather than terminal runtime I/O.

#[cfg(not(test))]
pub mod tui;

#[cfg(not(test))]
pub use self::tui::run;

#[cfg(test)]
/// Test-only runner stub.
pub fn run() -> anyhow::Result<()> {
    Ok(())
}

#[cfg(test)]
mod tests {
    #[test]
    fn test_run_stub_is_ok() {
        assert!(super::run().is_ok());
    }
}

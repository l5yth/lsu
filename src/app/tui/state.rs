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

//! Small state helpers for list status text generation.

/// Static mode label used by the list view.
pub const MODE_LABEL: &str = "services";

/// Build a list footer status text for idle, loading, and log-progress phases.
pub fn list_status_text(rows: usize, logs_progress: Option<(usize, usize)>) -> String {
    match logs_progress {
        Some((done, total)) if done < total => format!(
            "{MODE_LABEL}: {rows} | logs: {done}/{total} | ↑/↓: select | l/enter: inspect logs | r: refresh | q: quit"
        ),
        Some(_) => format!(
            "{MODE_LABEL}: {rows} | ↑/↓: select | l/enter: inspect logs | r: refresh | q: quit"
        ),
        None => format!(
            "{MODE_LABEL}: {rows} | ↑/↓: select | l/enter: inspect logs | r: refresh | q: quit"
        ),
    }
}

/// Build the stale-data status text after a failed refresh.
pub fn stale_status_text(rows: usize) -> String {
    format!(
        "{MODE_LABEL}: {rows} | refresh failed (stale data) | ↑/↓: select | l/enter: inspect logs | r: refresh | q: quit"
    )
}

/// Build the loading status text shown while units are being fetched.
pub fn loading_units_status_text() -> String {
    format!(
        "{MODE_LABEL}: loading units... | ↑/↓: select | l/enter: inspect logs | r: refresh | q: quit"
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn list_status_text_formats_logs_progress() {
        let s = list_status_text(12, Some((3, 12)));
        assert!(s.contains("services: 12"));
        assert!(s.contains("logs: 3/12"));
    }

    #[test]
    fn stale_status_text_mentions_stale_data() {
        let s = stale_status_text(4);
        assert!(s.contains("refresh failed (stale data)"));
    }

    #[test]
    fn loading_units_status_text_mentions_loading() {
        let s = loading_units_status_text();
        assert!(s.contains("loading units"));
    }
}

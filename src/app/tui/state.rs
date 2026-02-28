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

use crate::types::{ConfirmationKind, ConfirmationState};

/// Static mode label used by the list view.
pub const MODE_LABEL: &str = "services";

fn list_controls_text() -> &'static str {
    "↑/↓: select | l/enter: inspect logs | s: start/restart/stop | e: enable/disable | r: refresh | q: quit"
}

/// Build a list footer status text for idle, loading, and log-progress phases.
pub fn list_status_text(rows: usize, logs_progress: Option<(usize, usize)>) -> String {
    match logs_progress {
        Some((done, total)) if done < total => format!(
            "{MODE_LABEL}: {rows} | logs: {done}/{total} | {}",
            list_controls_text()
        ),
        Some(_) => format!("{MODE_LABEL}: {rows} | {}", list_controls_text()),
        None => format!("{MODE_LABEL}: {rows} | {}", list_controls_text()),
    }
}

/// Build the stale-data status text after a failed refresh.
pub fn stale_status_text(rows: usize) -> String {
    format!(
        "{MODE_LABEL}: {rows} | refresh failed (stale data) | {}",
        list_controls_text()
    )
}

/// Build the loading status text shown while units are being fetched.
pub fn loading_units_status_text() -> String {
    format!("{MODE_LABEL}: loading units... | {}", list_controls_text())
}

/// Build the footer status text shown while a unit action is running.
pub fn action_status_text(rows: usize, confirmation: &ConfirmationState) -> String {
    let verb = confirmation
        .confirmed_action()
        .map(|action| action.prompt_verb())
        .unwrap_or("running action for");
    format!("{MODE_LABEL}: {rows} | {} {}...", verb, confirmation.unit)
}

/// Build the footer status text shown while an action prompt is being resolved.
pub fn action_resolution_status_text(rows: usize, unit: &str) -> String {
    format!("{MODE_LABEL}: {rows} | resolving action for {unit}...")
}

/// Build the footer status text after a unit action completes.
pub fn action_complete_status_text(
    rows: usize,
    action: crate::types::UnitAction,
    unit: &str,
) -> String {
    format!(
        "{MODE_LABEL}: {rows} | {} {} | {}",
        action.past_tense(),
        unit,
        list_controls_text()
    )
}

/// Build the footer status text after a unit action fails.
pub fn action_error_status_text(
    rows: usize,
    action: crate::types::UnitAction,
    unit: &str,
    error: &str,
) -> String {
    format!(
        "{MODE_LABEL}: {rows} | failed to {} {}: {} | {}",
        action.as_systemctl_arg(),
        unit,
        error,
        list_controls_text()
    )
}

/// Build the footer status text after resolving an action target fails.
pub fn action_resolution_error_status_text(rows: usize, unit: &str, error: &str) -> String {
    format!(
        "{MODE_LABEL}: {rows} | failed to inspect {}: {} | {}",
        unit,
        error,
        list_controls_text()
    )
}

/// Build the confirmation prompt shown before a unit action executes.
pub fn confirmation_prompt_text(confirmation: &ConfirmationState) -> String {
    match confirmation.kind {
        ConfirmationKind::ConfirmAction(action) => format!(
            "confirm {} of unit {} (y/n)",
            action.prompt_verb(),
            confirmation.unit
        ),
        ConfirmationKind::RestartOrStop => format!(
            "unit {} is running: (r) restart or (s) stop or (esc) cancel",
            confirmation.unit
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{ConfirmationState, UnitAction};

    #[test]
    fn list_status_text_formats_logs_progress() {
        let s = list_status_text(12, Some((3, 12)));
        assert!(s.contains("services: 12"));
        assert!(s.contains("logs: 3/12"));
        assert!(s.contains("s: start/restart/stop"));
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

    #[test]
    fn confirmation_prompt_text_matches_requested_copy() {
        let s = confirmation_prompt_text(&ConfirmationState::confirm_action(
            UnitAction::Disable,
            "foobar.service".to_string(),
        ));
        assert_eq!(s, "confirm disabling of unit foobar.service (y/n)");
    }

    #[test]
    fn action_status_text_mentions_running_action() {
        let confirmation =
            ConfirmationState::confirm_action(UnitAction::Start, "demo.service".to_string());
        let s = action_status_text(3, &confirmation);
        assert!(s.contains("starting demo.service"));
    }

    #[test]
    fn action_resolution_status_text_mentions_target_unit() {
        let s = action_resolution_status_text(3, "demo.service");
        assert!(s.contains("resolving action for demo.service"));
    }

    #[test]
    fn action_complete_and_error_status_include_controls() {
        let complete = action_complete_status_text(4, UnitAction::Enable, "demo.service");
        assert!(complete.contains("enabled demo.service"));
        assert!(complete.contains("e: enable/disable"));

        let error = action_error_status_text(4, UnitAction::Stop, "demo.service", "boom");
        assert!(error.contains("failed to stop demo.service: boom"));
        assert!(error.contains("s: start/restart/stop"));
    }

    #[test]
    fn action_resolution_error_status_mentions_unit() {
        let s = action_resolution_error_status_text(2, "demo.service", "state error");
        assert!(s.contains("failed to inspect demo.service: state error"));
    }

    #[test]
    fn confirmation_prompt_text_for_running_unit_offers_restart_or_stop() {
        let s = confirmation_prompt_text(&ConfirmationState::restart_or_stop(
            "foobar.service".to_string(),
        ));
        assert_eq!(
            s,
            "unit foobar.service is running: (r) restart or (s) stop or (esc) cancel"
        );
    }
}

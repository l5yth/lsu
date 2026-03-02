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

//! Key-event translation for list and detail modes.

use crossterm::event::KeyCode;

use crate::types::{ConfirmationKind, ViewMode};

/// High-level UI command mapped from a key press.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UiCommand {
    Quit,
    Refresh,
    MoveDown,
    MoveUp,
    OpenDetail,
    BackToList,
    RefreshDetail,
    RequestStartStop,
    RequestEnableDisable,
    Confirm,
    Cancel,
    ChooseRestart,
    ChooseStop,
}

/// Translate a key in the current view mode to a UI command.
pub fn map_key(view_mode: ViewMode, key: KeyCode) -> Option<UiCommand> {
    match view_mode {
        ViewMode::List => match key {
            KeyCode::Char('q') => Some(UiCommand::Quit),
            KeyCode::Char('r') => Some(UiCommand::Refresh),
            KeyCode::Down => Some(UiCommand::MoveDown),
            KeyCode::Up => Some(UiCommand::MoveUp),
            KeyCode::Char('l') | KeyCode::Enter => Some(UiCommand::OpenDetail),
            KeyCode::Char('s') => Some(UiCommand::RequestStartStop),
            KeyCode::Char('e') => Some(UiCommand::RequestEnableDisable),
            _ => None,
        },
        ViewMode::Detail => match key {
            KeyCode::Char('q') => Some(UiCommand::Quit),
            KeyCode::Char('r') => Some(UiCommand::Refresh),
            KeyCode::Down => Some(UiCommand::MoveDown),
            KeyCode::Up => Some(UiCommand::MoveUp),
            KeyCode::Esc | KeyCode::Char('b') => Some(UiCommand::BackToList),
            KeyCode::Char('l') => Some(UiCommand::RefreshDetail),
            _ => None,
        },
    }
}

/// Translate a key while a confirmation prompt is active.
pub fn map_confirmation_key(kind: ConfirmationKind, key: KeyCode) -> Option<UiCommand> {
    match kind {
        ConfirmationKind::ConfirmAction(_) => match key {
            KeyCode::Char('y') | KeyCode::Enter => Some(UiCommand::Confirm),
            KeyCode::Char('n') | KeyCode::Esc => Some(UiCommand::Cancel),
            _ => None,
        },
        ConfirmationKind::RestartOrStop => match key {
            KeyCode::Char('r') => Some(UiCommand::ChooseRestart),
            KeyCode::Char('s') => Some(UiCommand::ChooseStop),
            KeyCode::Esc => Some(UiCommand::Cancel),
            _ => None,
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn map_key_list_mode_maps_navigation_and_opening() {
        assert_eq!(
            map_key(ViewMode::List, KeyCode::Enter),
            Some(UiCommand::OpenDetail)
        );
        assert_eq!(
            map_key(ViewMode::List, KeyCode::Char('l')),
            Some(UiCommand::OpenDetail)
        );
        assert_eq!(
            map_key(ViewMode::List, KeyCode::Down),
            Some(UiCommand::MoveDown)
        );
        assert_eq!(
            map_key(ViewMode::List, KeyCode::Up),
            Some(UiCommand::MoveUp)
        );
        assert_eq!(
            map_key(ViewMode::List, KeyCode::Char('s')),
            Some(UiCommand::RequestStartStop)
        );
        assert_eq!(
            map_key(ViewMode::List, KeyCode::Char('e')),
            Some(UiCommand::RequestEnableDisable)
        );
    }

    #[test]
    fn map_key_detail_mode_maps_back_and_refresh_detail() {
        assert_eq!(
            map_key(ViewMode::Detail, KeyCode::Esc),
            Some(UiCommand::BackToList)
        );
        assert_eq!(
            map_key(ViewMode::Detail, KeyCode::Char('b')),
            Some(UiCommand::BackToList)
        );
        assert_eq!(
            map_key(ViewMode::Detail, KeyCode::Char('l')),
            Some(UiCommand::RefreshDetail)
        );
    }

    #[test]
    fn map_key_maps_quit_refresh_and_unknown_keys() {
        assert_eq!(
            map_key(ViewMode::List, KeyCode::Char('q')),
            Some(UiCommand::Quit)
        );
        assert_eq!(
            map_key(ViewMode::List, KeyCode::Char('r')),
            Some(UiCommand::Refresh)
        );
        assert_eq!(
            map_key(ViewMode::Detail, KeyCode::Char('r')),
            Some(UiCommand::Refresh)
        );
        assert_eq!(map_key(ViewMode::Detail, KeyCode::Enter), None);
        assert_eq!(map_key(ViewMode::Detail, KeyCode::Char('s')), None);
    }

    #[test]
    fn map_confirmation_key_maps_accept_and_decline() {
        assert_eq!(
            map_confirmation_key(
                ConfirmationKind::ConfirmAction(crate::types::UnitAction::Start),
                KeyCode::Char('y')
            ),
            Some(UiCommand::Confirm)
        );
        assert_eq!(
            map_confirmation_key(
                ConfirmationKind::ConfirmAction(crate::types::UnitAction::Start),
                KeyCode::Enter
            ),
            Some(UiCommand::Confirm)
        );
        assert_eq!(
            map_confirmation_key(
                ConfirmationKind::ConfirmAction(crate::types::UnitAction::Start),
                KeyCode::Char('n')
            ),
            Some(UiCommand::Cancel)
        );
        assert_eq!(
            map_confirmation_key(
                ConfirmationKind::ConfirmAction(crate::types::UnitAction::Start),
                KeyCode::Esc
            ),
            Some(UiCommand::Cancel)
        );
        assert_eq!(
            map_confirmation_key(
                ConfirmationKind::ConfirmAction(crate::types::UnitAction::Start),
                KeyCode::Char('x')
            ),
            None
        );
    }

    #[test]
    fn map_confirmation_key_maps_restart_or_stop_prompt() {
        assert_eq!(
            map_confirmation_key(ConfirmationKind::RestartOrStop, KeyCode::Char('r')),
            Some(UiCommand::ChooseRestart)
        );
        assert_eq!(
            map_confirmation_key(ConfirmationKind::RestartOrStop, KeyCode::Char('s')),
            Some(UiCommand::ChooseStop)
        );
        assert_eq!(
            map_confirmation_key(ConfirmationKind::RestartOrStop, KeyCode::Esc),
            Some(UiCommand::Cancel)
        );
        assert_eq!(
            map_confirmation_key(ConfirmationKind::RestartOrStop, KeyCode::Enter),
            None
        );
    }
}

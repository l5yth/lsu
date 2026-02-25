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

use crate::types::ViewMode;

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
    }
}

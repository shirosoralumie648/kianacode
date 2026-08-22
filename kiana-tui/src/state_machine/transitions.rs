//! Application-specific transition definitions

use super::{AppEvent, AppState, TransitionDefinition};

/// Application transition builder and registry.
pub struct AppTransitions;

impl AppTransitions {
    /// Check if command palette can be opened from current state.
    pub fn can_open_command_palette(state: &AppState) -> bool {
        matches!(state, AppState::Normal)
    }

    /// Check if config editor can be opened from current state.
    pub fn can_open_config_editor(state: &AppState) -> bool {
        matches!(state, AppState::Normal)
    }

    /// Check if session list can be opened from current state.
    pub fn can_open_session_list(state: &AppState) -> bool {
        matches!(state, AppState::Normal)
    }

    /// Check if help can be opened from current state.
    pub fn can_open_help(state: &AppState) -> bool {
        !matches!(state, AppState::Dialog(_))
    }

    /// Check if search history overlay can be opened from current state.
    pub fn can_open_search_history(state: &AppState) -> bool {
        matches!(state, AppState::Normal)
    }

    /// Check if escape is valid for current state.
    pub fn can_escape(state: &AppState) -> bool {
        state.is_escapable()
    }

    /// Create a transition to command palette.
    pub fn open_command_palette<C>() -> TransitionDefinition<AppState, AppEvent, C> {
        TransitionDefinition::new(
            AppState::CommandPalette,
            "open_command_palette".to_string(),
        )
    }

    /// Create a transition to config editor.
    pub fn open_config_editor<C>() -> TransitionDefinition<AppState, AppEvent, C> {
        TransitionDefinition::new(AppState::ConfigEditing, "open_config_editor".to_string())
    }

    /// Create a transition to session list.
    pub fn open_session_list<C>() -> TransitionDefinition<AppState, AppEvent, C> {
        TransitionDefinition::new(AppState::SessionList, "open_session_list".to_string())
    }

    /// Create a transition to help.
    pub fn open_help<C>() -> TransitionDefinition<AppState, AppEvent, C> {
        TransitionDefinition::new(AppState::Help, "open_help".to_string())
    }

    /// Create a transition to search history overlay.
    pub fn open_search_history<C>() -> TransitionDefinition<AppState, AppEvent, C> {
        TransitionDefinition::new(
            AppState::Overlay(super::OverlayState::SearchHistory),
            "open_search_history".to_string(),
        )
    }

    /// Create a transition back to normal state.
    pub fn return_to_normal<C>() -> TransitionDefinition<AppState, AppEvent, C> {
        TransitionDefinition::new(AppState::Normal, "return_to_normal".to_string())
    }

    /// Create an escape transition that returns to parent state.
    pub fn escape<C>(current: &AppState) -> TransitionDefinition<AppState, AppEvent, C> {
        let target = current.escape_target();
        TransitionDefinition::new(target, "escape".to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state_machine::DialogKind;

    #[test]
    fn test_can_open_command_palette() {
        assert!(AppTransitions::can_open_command_palette(&AppState::Normal));
        assert!(!AppTransitions::can_open_command_palette(
            &AppState::CommandPalette
        ));
        assert!(!AppTransitions::can_open_command_palette(&AppState::Help));
    }

    #[test]
    fn test_can_open_config_editor() {
        assert!(AppTransitions::can_open_config_editor(&AppState::Normal));
        assert!(!AppTransitions::can_open_config_editor(
            &AppState::ConfigEditing
        ));
    }

    #[test]
    fn test_can_open_session_list() {
        assert!(AppTransitions::can_open_session_list(&AppState::Normal));
        assert!(!AppTransitions::can_open_session_list(
            &AppState::SessionList
        ));
    }

    #[test]
    fn test_can_open_help() {
        assert!(AppTransitions::can_open_help(&AppState::Normal));
        assert!(AppTransitions::can_open_help(&AppState::CommandPalette));
        assert!(!AppTransitions::can_open_help(&AppState::Dialog(
            DialogKind::Blocking
        )));
    }

    #[test]
    fn test_can_open_search_history() {
        assert!(AppTransitions::can_open_search_history(&AppState::Normal));
        assert!(!AppTransitions::can_open_search_history(
            &AppState::CommandPalette
        ));
    }

    #[test]
    fn test_can_escape() {
        assert!(!AppTransitions::can_escape(&AppState::Normal));
        assert!(AppTransitions::can_escape(&AppState::CommandPalette));
        assert!(AppTransitions::can_escape(&AppState::Help));
        assert!(!AppTransitions::can_escape(&AppState::Dialog(
            DialogKind::Blocking
        )));
    }

    struct TestContext;

    #[test]
    fn test_open_command_palette_transition() {
        let transition: TransitionDefinition<AppState, AppEvent, TestContext> =
            AppTransitions::open_command_palette();
        assert_eq!(transition.target, AppState::CommandPalette);
        assert_eq!(transition.name, "open_command_palette");
    }

    #[test]
    fn test_escape_transition_targets() {
        let transition: TransitionDefinition<AppState, AppEvent, TestContext> =
            AppTransitions::escape(&AppState::CommandPalette);
        assert_eq!(transition.target, AppState::Normal);

        let transition: TransitionDefinition<AppState, AppEvent, TestContext> =
            AppTransitions::escape(&AppState::Help);
        assert_eq!(transition.target, AppState::Normal);

        let transition: TransitionDefinition<AppState, AppEvent, TestContext> =
            AppTransitions::escape(&AppState::Normal);
        assert_eq!(transition.target, AppState::Normal);
    }
}

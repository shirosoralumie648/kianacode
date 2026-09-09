//! Application-specific state and event definitions

use super::{Event, State};

/// Top-level application state.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum AppState {
    /// Normal operation mode
    Normal,
    /// Command palette is open
    CommandPalette,
    /// Configuration editing mode
    ConfigEditing,
    /// Session list is displayed
    SessionList,
    /// An overlay is active
    Overlay(OverlayState),
    /// Help screen
    Help,
    /// Confirmation dialog
    Dialog(DialogKind),
}

impl State for AppState {}

impl Default for AppState {
    fn default() -> Self {
        Self::Normal
    }
}

impl AppState {
    /// Returns true if user input should be captured for text entry.
    pub fn is_text_input_mode(&self) -> bool {
        matches!(
            self,
            Self::CommandPalette | Self::ConfigEditing | Self::Overlay(OverlayState::SearchHistory)
        )
    }

    /// Returns true if this state blocks normal keybindings.
    pub fn blocks_normal_keys(&self) -> bool {
        !matches!(self, Self::Normal)
    }

    /// Returns true if this state can be escaped from.
    pub fn is_escapable(&self) -> bool {
        !matches!(self, Self::Normal | Self::Dialog(DialogKind::Blocking))
    }

    /// Get the parent state after escaping.
    pub fn escape_target(&self) -> Self {
        match self {
            Self::Normal => Self::Normal,
            Self::CommandPalette
            | Self::ConfigEditing
            | Self::SessionList
            | Self::Help
            | Self::Overlay(_)
            | Self::Dialog(_) => Self::Normal,
        }
    }
}

/// Overlay states for modal UI elements.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum OverlayState {
    /// Search history overlay (Ctrl+R style)
    SearchHistory,
    /// Help overlay
    Help,
    /// Custom dialog
    Dialog(DialogKind),
}

impl State for OverlayState {}

/// Types of dialogs.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum DialogKind {
    /// Non-blocking informational dialog
    Info,
    /// Confirmation dialog
    Confirm,
    /// Blocking modal dialog
    Blocking,
    /// Error dialog
    Error,
}

/// Application events that trigger state transitions.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum AppEvent {
    /// Command palette requested (Ctrl+P)
    CommandPaletteRequested,
    /// Config editing requested
    ConfigEditRequested,
    /// Session list requested
    SessionListRequested,
    /// Help requested
    HelpRequested,
    /// Search history requested (Ctrl+R)
    SearchHistoryRequested,
    /// Escape key pressed
    Escape,
    /// Enter key pressed
    Enter,
    /// Dialog requested
    ShowDialog(DialogKind),
    /// Dialog confirmed
    DialogConfirmed,
    /// Dialog cancelled
    DialogCancelled,
    /// Return to normal mode
    ReturnToNormal,
}

impl Event for AppEvent {}

impl AppEvent {
    /// Get a human-readable name for this event.
    pub fn name(&self) -> &str {
        match self {
            Self::CommandPaletteRequested => "command_palette_requested",
            Self::ConfigEditRequested => "config_edit_requested",
            Self::SessionListRequested => "session_list_requested",
            Self::HelpRequested => "help_requested",
            Self::SearchHistoryRequested => "search_history_requested",
            Self::Escape => "escape",
            Self::Enter => "enter",
            Self::ShowDialog(_) => "show_dialog",
            Self::DialogConfirmed => "dialog_confirmed",
            Self::DialogCancelled => "dialog_cancelled",
            Self::ReturnToNormal => "return_to_normal",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_app_state_text_input_mode() {
        assert!(AppState::CommandPalette.is_text_input_mode());
        assert!(AppState::ConfigEditing.is_text_input_mode());
        assert!(!AppState::Normal.is_text_input_mode());
        assert!(!AppState::Help.is_text_input_mode());
    }

    #[test]
    fn test_app_state_blocks_normal_keys() {
        assert!(!AppState::Normal.blocks_normal_keys());
        assert!(AppState::CommandPalette.blocks_normal_keys());
        assert!(AppState::Help.blocks_normal_keys());
    }

    #[test]
    fn test_app_state_escapable() {
        assert!(!AppState::Normal.is_escapable());
        assert!(AppState::CommandPalette.is_escapable());
        assert!(AppState::Help.is_escapable());
        assert!(!AppState::Dialog(DialogKind::Blocking).is_escapable());
    }

    #[test]
    fn test_app_state_escape_target() {
        assert_eq!(AppState::CommandPalette.escape_target(), AppState::Normal);
        assert_eq!(AppState::Help.escape_target(), AppState::Normal);
        assert_eq!(AppState::Normal.escape_target(), AppState::Normal);
    }

    #[test]
    fn test_app_event_name() {
        assert_eq!(AppEvent::Escape.name(), "escape");
        assert_eq!(
            AppEvent::CommandPaletteRequested.name(),
            "command_palette_requested"
        );
    }
}

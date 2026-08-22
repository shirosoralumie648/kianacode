use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

/// Vim navigation modes
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VimMode {
    Normal,
    Insert,
    Search,
    Command,
}

impl VimMode {
    pub fn display_name(&self) -> &'static str {
        match self {
            Self::Normal => "NORMAL",
            Self::Insert => "INSERT",
            Self::Search => "SEARCH",
            Self::Command => "COMMAND",
        }
    }
}

/// Actions that can be performed via vim navigation
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VimAction {
    MoveUp(usize),
    MoveDown(usize),
    MoveLeft(usize),
    MoveRight(usize),
    JumpToStart,
    JumpToEnd,
    EnterSearchMode,
    EnterCommandMode,
    EnterInsertMode,
    ExitToNormal,
    SearchNext,
    SearchPrev,
    None,
}

/// Handles vim-style navigation key bindings
pub struct VimNavigationHandler {
    mode: VimMode,
    pending_count: Option<usize>,
    last_key: Option<KeyCode>,
}

impl VimNavigationHandler {
    pub fn new() -> Self {
        Self {
            mode: VimMode::Normal,
            pending_count: None,
            last_key: None,
        }
    }

    /// Get current vim mode
    pub fn mode(&self) -> VimMode {
        self.mode
    }

    /// Set vim mode and reset state
    pub fn set_mode(&mut self, mode: VimMode) {
        self.mode = mode;
        self.pending_count = None;
        self.last_key = None;
    }

    /// Check if currently in a mode that accepts vim navigation
    pub fn accepts_vim_navigation(&self) -> bool {
        matches!(self.mode, VimMode::Normal)
    }

    /// Process a key event and return the vim action to perform
    pub fn handle_key(&mut self, key: KeyEvent) -> VimAction {
        match self.mode {
            VimMode::Normal => self.handle_normal_mode(key),
            VimMode::Insert => self.handle_insert_mode(key),
            VimMode::Search => self.handle_search_mode(key),
            VimMode::Command => self.handle_command_mode(key),
        }
    }

    fn handle_normal_mode(&mut self, key: KeyEvent) -> VimAction {
        // Handle Esc to clear pending state
        if key.code == KeyCode::Esc {
            self.pending_count = None;
            self.last_key = None;
            return VimAction::None;
        }

        // Handle number prefixes (0-9)
        if let KeyCode::Char(c) = key.code {
            if c.is_ascii_digit() {
                let digit = c.to_digit(10).unwrap() as usize;

                // Special case: 0 is not a count prefix, it means "go to start of line"
                if digit == 0 && self.pending_count.is_none() {
                    return VimAction::MoveLeft(usize::MAX); // Signal to move to start
                }

                self.pending_count = Some(self.pending_count.unwrap_or(0) * 10 + digit);
                return VimAction::None;
            }
        }

        let count = self.pending_count.unwrap_or(1);

        let action = match key.code {
            // hjkl navigation
            KeyCode::Char('h') => VimAction::MoveLeft(count),
            KeyCode::Char('j') => VimAction::MoveDown(count),
            KeyCode::Char('k') => VimAction::MoveUp(count),
            KeyCode::Char('l') => VimAction::MoveRight(count),

            // Arrow keys also work
            KeyCode::Left => VimAction::MoveLeft(count),
            KeyCode::Down => VimAction::MoveDown(count),
            KeyCode::Up => VimAction::MoveUp(count),
            KeyCode::Right => VimAction::MoveRight(count),

            // gg - jump to start (requires two 'g' presses)
            KeyCode::Char('g') => {
                if self.last_key == Some(KeyCode::Char('g')) {
                    self.last_key = None;
                    self.pending_count = None;
                    return VimAction::JumpToStart;
                }
                self.last_key = Some(KeyCode::Char('g'));
                return VimAction::None;
            }

            // G - jump to end
            KeyCode::Char('G') => VimAction::JumpToEnd,

            // / - enter search mode
            KeyCode::Char('/') => {
                self.pending_count = None;
                VimAction::EnterSearchMode
            }

            // : - enter command mode
            KeyCode::Char(':') => {
                self.pending_count = None;
                VimAction::EnterCommandMode
            }

            // i - enter insert mode
            KeyCode::Char('i') => {
                self.pending_count = None;
                VimAction::EnterInsertMode
            }

            // n - search next
            KeyCode::Char('n') => VimAction::SearchNext,

            // N - search previous
            KeyCode::Char('N') => VimAction::SearchPrev,

            _ => {
                // Unknown key, clear state
                self.last_key = None;
                self.pending_count = None;
                return VimAction::None;
            }
        };

        // Clear state after processing (except for 'g' which needs to wait for second 'g')
        if !matches!(action, VimAction::None) {
            self.pending_count = None;
            if self.last_key != Some(KeyCode::Char('g')) {
                self.last_key = None;
            }
        }

        action
    }

    fn handle_insert_mode(&mut self, key: KeyEvent) -> VimAction {
        // In insert mode, only Esc returns to normal
        if key.code == KeyCode::Esc {
            VimAction::ExitToNormal
        } else {
            VimAction::None
        }
    }

    fn handle_search_mode(&mut self, key: KeyEvent) -> VimAction {
        // In search mode, Esc returns to normal
        if key.code == KeyCode::Esc {
            VimAction::ExitToNormal
        } else {
            VimAction::None
        }
    }

    fn handle_command_mode(&mut self, key: KeyEvent) -> VimAction {
        // In command mode, Esc returns to normal
        if key.code == KeyCode::Esc {
            VimAction::ExitToNormal
        } else {
            VimAction::None
        }
    }

    /// Reset all internal state
    pub fn reset(&mut self) {
        self.pending_count = None;
        self.last_key = None;
    }

    /// Get pending count if any
    pub fn pending_count(&self) -> Option<usize> {
        self.pending_count
    }
}

impl Default for VimNavigationHandler {
    fn default() -> Self {
        Self::new()
    }
}

/// Convert VimMode to StatusBar AppMode
pub fn vim_mode_to_app_mode(vim_mode: VimMode) -> crate::components::AppMode {
    match vim_mode {
        VimMode::Normal => crate::components::AppMode::Normal,
        VimMode::Insert => crate::components::AppMode::Insert,
        VimMode::Search => crate::components::AppMode::Search,
        VimMode::Command => crate::components::AppMode::Config,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key_event(c: char) -> KeyEvent {
        KeyEvent::new(KeyCode::Char(c), KeyModifiers::NONE)
    }

    fn key_event_with_shift(c: char) -> KeyEvent {
        KeyEvent::new(KeyCode::Char(c), KeyModifiers::SHIFT)
    }

    fn key_event_esc() -> KeyEvent {
        KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE)
    }

    fn key_event_code(code: KeyCode) -> KeyEvent {
        KeyEvent::new(code, KeyModifiers::NONE)
    }

    #[test]
    fn hjkl_basic_navigation() {
        let mut handler = VimNavigationHandler::new();

        // h: left
        let action = handler.handle_key(key_event('h'));
        assert_eq!(action, VimAction::MoveLeft(1));

        // j: down
        let action = handler.handle_key(key_event('j'));
        assert_eq!(action, VimAction::MoveDown(1));

        // k: up
        let action = handler.handle_key(key_event('k'));
        assert_eq!(action, VimAction::MoveUp(1));

        // l: right
        let action = handler.handle_key(key_event('l'));
        assert_eq!(action, VimAction::MoveRight(1));
    }

    #[test]
    fn arrow_keys_work() {
        let mut handler = VimNavigationHandler::new();

        assert_eq!(
            handler.handle_key(key_event_code(KeyCode::Left)),
            VimAction::MoveLeft(1)
        );
        assert_eq!(
            handler.handle_key(key_event_code(KeyCode::Down)),
            VimAction::MoveDown(1)
        );
        assert_eq!(
            handler.handle_key(key_event_code(KeyCode::Up)),
            VimAction::MoveUp(1)
        );
        assert_eq!(
            handler.handle_key(key_event_code(KeyCode::Right)),
            VimAction::MoveRight(1)
        );
    }

    #[test]
    fn count_prefix_navigation() {
        let mut handler = VimNavigationHandler::new();

        // 5j: down 5 times
        assert_eq!(handler.handle_key(key_event('5')), VimAction::None);
        let action = handler.handle_key(key_event('j'));
        assert_eq!(action, VimAction::MoveDown(5));
    }

    #[test]
    fn multi_digit_count() {
        let mut handler = VimNavigationHandler::new();

        // 15k: up 15 times
        assert_eq!(handler.handle_key(key_event('1')), VimAction::None);
        assert_eq!(handler.handle_key(key_event('5')), VimAction::None);
        let action = handler.handle_key(key_event('k'));
        assert_eq!(action, VimAction::MoveUp(15));
    }

    #[test]
    fn gg_jump_to_start() {
        let mut handler = VimNavigationHandler::new();

        // First 'g' should return None
        let action = handler.handle_key(key_event('g'));
        assert_eq!(action, VimAction::None);

        // Second 'g' should jump to start
        let action = handler.handle_key(key_event('g'));
        assert_eq!(action, VimAction::JumpToStart);
    }

    #[test]
    fn gg_timeout_with_different_key() {
        let mut handler = VimNavigationHandler::new();

        // First 'g'
        handler.handle_key(key_event('g'));

        // Different key should reset and do normal action
        let action = handler.handle_key(key_event('j'));
        assert_eq!(action, VimAction::MoveDown(1));
    }

    #[test]
    fn shift_g_jump_to_end() {
        let mut handler = VimNavigationHandler::new();

        let action = handler.handle_key(key_event_with_shift('G'));
        assert_eq!(action, VimAction::JumpToEnd);
    }

    #[test]
    fn slash_enters_search_mode() {
        let mut handler = VimNavigationHandler::new();

        let action = handler.handle_key(key_event('/'));
        assert_eq!(action, VimAction::EnterSearchMode);
    }

    #[test]
    fn colon_enters_command_mode() {
        let mut handler = VimNavigationHandler::new();

        let action = handler.handle_key(key_event(':'));
        assert_eq!(action, VimAction::EnterCommandMode);
    }

    #[test]
    fn i_enters_insert_mode() {
        let mut handler = VimNavigationHandler::new();

        let action = handler.handle_key(key_event('i'));
        assert_eq!(action, VimAction::EnterInsertMode);
    }

    #[test]
    fn mode_transitions() {
        let mut handler = VimNavigationHandler::new();
        assert_eq!(handler.mode(), VimMode::Normal);

        // Enter search mode
        let action = handler.handle_key(key_event('/'));
        assert_eq!(action, VimAction::EnterSearchMode);
        handler.set_mode(VimMode::Search);
        assert_eq!(handler.mode(), VimMode::Search);

        // Esc returns to normal
        let action = handler.handle_key(key_event_esc());
        assert_eq!(action, VimAction::ExitToNormal);
        handler.set_mode(VimMode::Normal);
        assert_eq!(handler.mode(), VimMode::Normal);
    }

    #[test]
    fn esc_clears_pending_count_in_normal_mode() {
        let mut handler = VimNavigationHandler::new();

        // Start typing a count
        handler.handle_key(key_event('5'));
        assert_eq!(handler.pending_count(), Some(5));

        // Esc should clear it
        handler.handle_key(key_event_esc());
        assert_eq!(handler.pending_count(), None);
    }

    #[test]
    fn search_next_and_prev() {
        let mut handler = VimNavigationHandler::new();

        let action = handler.handle_key(key_event('n'));
        assert_eq!(action, VimAction::SearchNext);

        let action = handler.handle_key(key_event_with_shift('N'));
        assert_eq!(action, VimAction::SearchPrev);
    }

    #[test]
    fn zero_moves_to_line_start() {
        let mut handler = VimNavigationHandler::new();

        let action = handler.handle_key(key_event('0'));
        assert_eq!(action, VimAction::MoveLeft(usize::MAX));
    }

    #[test]
    fn zero_in_count_works() {
        let mut handler = VimNavigationHandler::new();

        // 10j: down 10 times
        handler.handle_key(key_event('1'));
        handler.handle_key(key_event('0'));
        let action = handler.handle_key(key_event('j'));
        assert_eq!(action, VimAction::MoveDown(10));
    }

    #[test]
    fn insert_mode_only_exits_with_esc() {
        let mut handler = VimNavigationHandler::new();
        handler.set_mode(VimMode::Insert);

        // Regular keys should do nothing
        assert_eq!(handler.handle_key(key_event('j')), VimAction::None);
        assert_eq!(handler.handle_key(key_event('k')), VimAction::None);

        // Esc should exit
        let action = handler.handle_key(key_event_esc());
        assert_eq!(action, VimAction::ExitToNormal);
    }

    #[test]
    fn command_mode_only_exits_with_esc() {
        let mut handler = VimNavigationHandler::new();
        handler.set_mode(VimMode::Command);

        // Regular keys should do nothing
        assert_eq!(handler.handle_key(key_event('j')), VimAction::None);

        // Esc should exit
        let action = handler.handle_key(key_event_esc());
        assert_eq!(action, VimAction::ExitToNormal);
    }

    #[test]
    fn accepts_vim_navigation_only_in_normal_mode() {
        let mut handler = VimNavigationHandler::new();

        assert!(handler.accepts_vim_navigation());

        handler.set_mode(VimMode::Insert);
        assert!(!handler.accepts_vim_navigation());

        handler.set_mode(VimMode::Search);
        assert!(!handler.accepts_vim_navigation());

        handler.set_mode(VimMode::Command);
        assert!(!handler.accepts_vim_navigation());

        handler.set_mode(VimMode::Normal);
        assert!(handler.accepts_vim_navigation());
    }

    #[test]
    fn reset_clears_state() {
        let mut handler = VimNavigationHandler::new();

        handler.handle_key(key_event('5'));
        handler.handle_key(key_event('g'));
        assert!(handler.pending_count().is_some() || handler.last_key.is_some());

        handler.reset();
        assert_eq!(handler.pending_count(), None);
        assert_eq!(handler.last_key, None);
    }
}

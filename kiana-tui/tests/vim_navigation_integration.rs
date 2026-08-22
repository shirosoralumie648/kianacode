use kiana_tui::components::{
    vim_mode_to_app_mode, AppMode, VimAction, VimMode, VimNavigationHandler,
};
use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

fn key_event(c: char) -> KeyEvent {
    KeyEvent::new(KeyCode::Char(c), KeyModifiers::NONE)
}

fn key_event_with_shift(c: char) -> KeyEvent {
    KeyEvent::new(KeyCode::Char(c), KeyModifiers::SHIFT)
}

fn key_event_esc() -> KeyEvent {
    KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE)
}

#[test]
fn vim_navigation_full_workflow() {
    let mut handler = VimNavigationHandler::new();
    assert_eq!(handler.mode(), VimMode::Normal);

    // Navigate down with count prefix
    handler.handle_key(key_event('5'));
    let action = handler.handle_key(key_event('j'));
    assert_eq!(action, VimAction::MoveDown(5));

    // Jump to start with gg
    handler.handle_key(key_event('g'));
    let action = handler.handle_key(key_event('g'));
    assert_eq!(action, VimAction::JumpToStart);

    // Jump to end with G
    let action = handler.handle_key(key_event_with_shift('G'));
    assert_eq!(action, VimAction::JumpToEnd);

    // Enter search mode
    let action = handler.handle_key(key_event('/'));
    assert_eq!(action, VimAction::EnterSearchMode);
    handler.set_mode(VimMode::Search);

    // Exit back to normal
    let action = handler.handle_key(key_event_esc());
    assert_eq!(action, VimAction::ExitToNormal);
    handler.set_mode(VimMode::Normal);

    // Enter command mode
    let action = handler.handle_key(key_event(':'));
    assert_eq!(action, VimAction::EnterCommandMode);
    handler.set_mode(VimMode::Command);

    // Exit back to normal
    let action = handler.handle_key(key_event_esc());
    assert_eq!(action, VimAction::ExitToNormal);
    handler.set_mode(VimMode::Normal);
}

#[test]
fn vim_mode_to_app_mode_mapping() {
    assert_eq!(vim_mode_to_app_mode(VimMode::Normal), AppMode::Normal);
    assert_eq!(vim_mode_to_app_mode(VimMode::Insert), AppMode::Insert);
    assert_eq!(vim_mode_to_app_mode(VimMode::Search), AppMode::Search);
    assert_eq!(vim_mode_to_app_mode(VimMode::Command), AppMode::Config);
}

#[test]
fn vim_navigation_with_status_bar_integration() {
    use kiana_tui::components::StatusBar;

    let mut handler = VimNavigationHandler::new();
    let mut status_bar = StatusBar::new();

    // Start in normal mode
    status_bar.set_mode(vim_mode_to_app_mode(handler.mode()));
    assert_eq!(status_bar.mode(), AppMode::Normal);

    // Enter search mode
    let action = handler.handle_key(key_event('/'));
    if matches!(action, VimAction::EnterSearchMode) {
        handler.set_mode(VimMode::Search);
        status_bar.set_mode(vim_mode_to_app_mode(handler.mode()));
    }
    assert_eq!(status_bar.mode(), AppMode::Search);

    // Return to normal
    let action = handler.handle_key(key_event_esc());
    if matches!(action, VimAction::ExitToNormal) {
        handler.set_mode(VimMode::Normal);
        status_bar.set_mode(vim_mode_to_app_mode(handler.mode()));
    }
    assert_eq!(status_bar.mode(), AppMode::Normal);
}

#[test]
fn complex_navigation_sequence() {
    let mut handler = VimNavigationHandler::new();

    // Complex sequence: 10j, 5k, gg, G, 3h, 2l
    let movements = vec![
        (
            vec![key_event('1'), key_event('0'), key_event('j')],
            VimAction::MoveDown(10),
        ),
        (vec![key_event('5'), key_event('k')], VimAction::MoveUp(5)),
        (vec![key_event('g'), key_event('g')], VimAction::JumpToStart),
        (vec![key_event_with_shift('G')], VimAction::JumpToEnd),
        (vec![key_event('3'), key_event('h')], VimAction::MoveLeft(3)),
        (
            vec![key_event('2'), key_event('l')],
            VimAction::MoveRight(2),
        ),
    ];

    for (keys, expected_action) in movements {
        let mut last_action = VimAction::None;
        for key in keys {
            last_action = handler.handle_key(key);
        }
        assert_eq!(last_action, expected_action);
    }
}

#[test]
fn vim_navigation_state_isolation() {
    let mut handler = VimNavigationHandler::new();

    // Start a count
    handler.handle_key(key_event('5'));
    assert!(handler.pending_count().is_some());

    // Reset should clear it
    handler.reset();
    assert!(handler.pending_count().is_none());

    // Navigation should work normally after reset
    let action = handler.handle_key(key_event('j'));
    assert_eq!(action, VimAction::MoveDown(1));
}

#[test]
fn esc_behavior_in_different_modes() {
    let mut handler = VimNavigationHandler::new();

    // In Normal mode, Esc just clears state
    handler.handle_key(key_event('5'));
    let action = handler.handle_key(key_event_esc());
    assert_eq!(action, VimAction::None);
    assert_eq!(handler.mode(), VimMode::Normal);

    // In Insert mode, Esc exits to Normal
    handler.set_mode(VimMode::Insert);
    let action = handler.handle_key(key_event_esc());
    assert_eq!(action, VimAction::ExitToNormal);

    // In Search mode, Esc exits to Normal
    handler.set_mode(VimMode::Search);
    let action = handler.handle_key(key_event_esc());
    assert_eq!(action, VimAction::ExitToNormal);

    // In Command mode, Esc exits to Normal
    handler.set_mode(VimMode::Command);
    let action = handler.handle_key(key_event_esc());
    assert_eq!(action, VimAction::ExitToNormal);
}

use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use super::error::MacroError;
use super::manager::{MacroAction, MacroRecorder};

fn key_event(c: char) -> KeyEvent {
    KeyEvent::new(KeyCode::Char(c), KeyModifiers::empty())
}

fn key_event_with_mods(code: KeyCode, modifiers: KeyModifiers) -> KeyEvent {
    KeyEvent::new(code, modifiers)
}

#[test]
fn test_start_stop_recording() {
    let mut recorder = MacroRecorder::new();

    // Start recording to register 'a'
    recorder.start_recording('a').unwrap();
    assert!(recorder.is_recording());
    assert_eq!(recorder.recording_register(), Some('a'));

    // Record some events
    recorder.record_event(key_event('h')).unwrap();
    recorder.record_event(key_event('j')).unwrap();
    recorder.record_event(key_event('k')).unwrap();

    // Stop recording
    let reg = recorder.stop_recording().unwrap();
    assert_eq!(reg, 'a');
    assert!(!recorder.is_recording());

    // Macro should be stored
    let macro_def = recorder.get_macro('a').unwrap();
    assert_eq!(macro_def.actions.len(), 3);
    assert_eq!(macro_def.register, 'a');
}

#[test]
fn test_invalid_register() {
    let mut recorder = MacroRecorder::new();

    // Uppercase not allowed
    assert!(matches!(
        recorder.start_recording('A'),
        Err(MacroError::InvalidRegister('A'))
    ));

    // Numbers not allowed
    assert!(matches!(
        recorder.start_recording('1'),
        Err(MacroError::InvalidRegister('1'))
    ));

    // Special characters not allowed
    assert!(matches!(
        recorder.start_recording('@'),
        Err(MacroError::InvalidRegister('@'))
    ));
}

#[test]
fn test_already_recording_error() {
    let mut recorder = MacroRecorder::new();

    recorder.start_recording('a').unwrap();

    let result = recorder.start_recording('b');
    assert!(matches!(result, Err(MacroError::AlreadyRecording('a'))));
}

#[test]
fn test_stop_without_recording() {
    let mut recorder = MacroRecorder::new();

    let result = recorder.stop_recording();
    assert!(matches!(result, Err(MacroError::NotRecording)));
}

#[test]
fn test_play_macro() {
    let mut recorder = MacroRecorder::new();

    // Record a macro
    recorder.start_recording('a').unwrap();
    recorder.record_event(key_event('j')).unwrap();
    recorder.record_event(key_event('j')).unwrap();
    recorder.stop_recording().unwrap();

    // Play it back
    recorder.play_macro('a', 1).unwrap();
    assert!(recorder.is_playing());
    assert_eq!(recorder.playback_register(), Some('a'));

    // Get actions
    let action1 = recorder.next_playback_action().unwrap();
    let action2 = recorder.next_playback_action().unwrap();
    let action3 = recorder.next_playback_action();

    assert!(matches!(action1, MacroAction::KeyPress { .. }));
    assert!(matches!(action2, MacroAction::KeyPress { .. }));
    assert!(action3.is_none()); // Should be done
    assert!(!recorder.is_playing());
}

#[test]
fn test_loop_playback() {
    let mut recorder = MacroRecorder::new();

    // Record a simple macro
    recorder.start_recording('b').unwrap();
    recorder.record_event(key_event('x')).unwrap();
    recorder.stop_recording().unwrap();

    // Play it 3 times
    recorder.play_macro('b', 3).unwrap();

    for i in 0..3 {
        let action = recorder.next_playback_action();
        assert!(action.is_some(), "Action {} should exist", i + 1);
    }

    let last = recorder.next_playback_action();
    assert!(last.is_none(), "Should be done after 3 iterations");
}

#[test]
fn test_repeat_last_macro() {
    let mut recorder = MacroRecorder::new();

    // Record and play
    recorder.start_recording('c').unwrap();
    recorder.record_event(key_event('k')).unwrap();
    recorder.stop_recording().unwrap();

    recorder.play_macro('c', 1).unwrap();
    recorder.next_playback_action(); // consume

    // Repeat last
    recorder.repeat_last(1).unwrap();
    assert!(recorder.is_playing());
    assert_eq!(recorder.playback_register(), Some('c'));
}

#[test]
fn test_repeat_without_previous() {
    let mut recorder = MacroRecorder::new();

    let result = recorder.repeat_last(1);
    assert!(matches!(result, Err(MacroError::NoLastMacro)));
}

#[test]
fn test_max_actions_limit() {
    let mut recorder = MacroRecorder::with_max_actions(10);

    recorder.start_recording('d').unwrap();

    // Record 10 actions (should succeed)
    for _ in 0..10 {
        recorder.record_event(key_event('j')).unwrap();
    }

    // 11th should fail
    let result = recorder.record_event(key_event('j'));
    assert!(matches!(result, Err(MacroError::MaxActionsExceeded(10))));
}

#[test]
fn test_play_nonexistent_macro() {
    let mut recorder = MacroRecorder::new();

    let result = recorder.play_macro('z', 1);
    assert!(matches!(result, Err(MacroError::MacroNotFound('z'))));
}

#[test]
fn test_play_empty_macro() {
    let mut recorder = MacroRecorder::new();

    // Create empty macro
    recorder.start_recording('e').unwrap();
    recorder.stop_recording().unwrap();

    let result = recorder.play_macro('e', 1);
    assert!(matches!(result, Err(MacroError::EmptyMacro('e'))));
}

#[test]
fn test_delete_macro() {
    let mut recorder = MacroRecorder::new();

    recorder.start_recording('f').unwrap();
    recorder.record_event(key_event('x')).unwrap();
    recorder.stop_recording().unwrap();

    assert!(recorder.get_macro('f').is_some());

    recorder.delete_macro('f').unwrap();
    assert!(recorder.get_macro('f').is_none());
}

#[test]
fn test_set_description() {
    let mut recorder = MacroRecorder::new();

    recorder.start_recording('g').unwrap();
    recorder.record_event(key_event('h')).unwrap();
    recorder.stop_recording().unwrap();

    recorder
        .set_description('g', "Test macro".to_string())
        .unwrap();

    let macro_def = recorder.get_macro('g').unwrap();
    assert_eq!(macro_def.description, Some("Test macro".to_string()));
}

#[test]
fn test_stop_playback() {
    let mut recorder = MacroRecorder::new();

    recorder.start_recording('h').unwrap();
    recorder.record_event(key_event('j')).unwrap();
    recorder.record_event(key_event('j')).unwrap();
    recorder.stop_recording().unwrap();

    recorder.play_macro('h', 10).unwrap();
    assert!(recorder.is_playing());

    recorder.stop_playback();
    assert!(!recorder.is_playing());
}

#[test]
fn test_recording_stats() {
    let mut recorder = MacroRecorder::new();

    assert!(recorder.recording_stats().is_none());

    recorder.start_recording('i').unwrap();
    recorder.record_event(key_event('a')).unwrap();
    recorder.record_event(key_event('b')).unwrap();

    let stats = recorder.recording_stats().unwrap();
    assert_eq!(stats.register, 'i');
    assert_eq!(stats.action_count, 2);
}

#[test]
fn test_prevent_recursive_macro() {
    let mut recorder = MacroRecorder::new();

    recorder.start_recording('j').unwrap();

    // Try to record a 'q' (stop recording command)
    let result = recorder.record_event(key_event('q'));
    assert!(matches!(result, Err(MacroError::RecursiveMacro)));

    // Try to record '@' (play macro command)
    let result = recorder.record_event(key_event('@'));
    assert!(matches!(result, Err(MacroError::RecursiveMacro)));
}

#[test]
fn test_play_zero_times() {
    let mut recorder = MacroRecorder::new();

    recorder.start_recording('k').unwrap();
    recorder.record_event(key_event('x')).unwrap();
    recorder.stop_recording().unwrap();

    // Play 0 times should be no-op
    recorder.play_macro('k', 0).unwrap();
    assert!(!recorder.is_playing());
}

#[test]
fn test_multiple_macros() {
    let mut recorder = MacroRecorder::new();

    // Record macro 'a'
    recorder.start_recording('a').unwrap();
    recorder.record_event(key_event('1')).unwrap();
    recorder.stop_recording().unwrap();

    // Record macro 'b'
    recorder.start_recording('b').unwrap();
    recorder.record_event(key_event('2')).unwrap();
    recorder.record_event(key_event('2')).unwrap();
    recorder.stop_recording().unwrap();

    // Record macro 'c'
    recorder.start_recording('c').unwrap();
    recorder.record_event(key_event('3')).unwrap();
    recorder.record_event(key_event('3')).unwrap();
    recorder.record_event(key_event('3')).unwrap();
    recorder.stop_recording().unwrap();

    assert_eq!(recorder.macros().len(), 3);
    assert_eq!(recorder.get_macro('a').unwrap().actions.len(), 1);
    assert_eq!(recorder.get_macro('b').unwrap().actions.len(), 2);
    assert_eq!(recorder.get_macro('c').unwrap().actions.len(), 3);
}

#[test]
fn test_overwrite_macro() {
    let mut recorder = MacroRecorder::new();

    // Record first version
    recorder.start_recording('m').unwrap();
    recorder.record_event(key_event('x')).unwrap();
    recorder.stop_recording().unwrap();

    // Record second version (overwrite)
    recorder.start_recording('m').unwrap();
    recorder.record_event(key_event('y')).unwrap();
    recorder.record_event(key_event('y')).unwrap();
    recorder.stop_recording().unwrap();

    let macro_def = recorder.get_macro('m').unwrap();
    assert_eq!(macro_def.actions.len(), 2);
}

#[test]
fn test_key_with_modifiers() {
    let mut recorder = MacroRecorder::new();

    recorder.start_recording('n').unwrap();
    recorder
        .record_event(key_event_with_mods(
            KeyCode::Char('c'),
            KeyModifiers::CONTROL,
        ))
        .unwrap();
    recorder.stop_recording().unwrap();

    let macro_def = recorder.get_macro('n').unwrap();
    match &macro_def.actions[0] {
        MacroAction::KeyPress { code, modifiers } => {
            assert_eq!(*code, KeyCode::Char('c'));
            assert!(modifiers.contains(KeyModifiers::CONTROL));
        }
        _ => panic!("Expected KeyPress"),
    }
}

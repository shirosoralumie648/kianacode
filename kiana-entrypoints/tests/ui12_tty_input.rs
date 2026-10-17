use crossterm::event::{Event, KeyCode, KeyEvent, KeyModifiers};
use kiana_entrypoints::tty_input::{
    CommittedInput, InputMode, InputTransition, PtyChunkDecoder, TtyInputState,
    MAX_TTY_HISTORY_ENTRIES, MAX_TTY_INPUT_BYTES, MAX_TTY_INPUT_CHARS,
};
use serde_json::Value;

const PTY_FIXTURE: &str = include_str!("fixtures/ui12-pty-events.json");

fn key(code: KeyCode, modifiers: KeyModifiers) -> Event {
    Event::Key(KeyEvent::new(code, modifiers))
}

fn event_from_fixture(value: &Value) -> Event {
    match value["type"].as_str().unwrap() {
        "char" => Event::Key(KeyEvent::from(KeyCode::Char(
            value["value"].as_str().unwrap().chars().next().unwrap(),
        ))),
        "paste" => Event::Paste(
            value["value"]
                .as_str()
                .unwrap()
                .repeat(value["repeat"].as_u64().unwrap_or(1) as usize),
        ),
        "resize" => Event::Resize(
            value["width"].as_u64().unwrap() as u16,
            value["height"].as_u64().unwrap() as u16,
        ),
        "up" => Event::Key(KeyEvent::from(KeyCode::Up)),
        "down" => Event::Key(KeyEvent::from(KeyCode::Down)),
        "backspace" => Event::Key(KeyEvent::from(KeyCode::Backspace)),
        "delete" => Event::Key(KeyEvent::from(KeyCode::Delete)),
        "esc" => Event::Key(KeyEvent::from(KeyCode::Esc)),
        "ctrl_j" => key(KeyCode::Char('j'), KeyModifiers::CONTROL),
        "ctrl_c" => key(KeyCode::Char('c'), KeyModifiers::CONTROL),
        "ctrl_d" => key(KeyCode::Char('d'), KeyModifiers::CONTROL),
        "enter" => Event::Key(KeyEvent::from(KeyCode::Enter)),
        event => panic!("unsupported UI-12 PTY fixture event: {event}"),
    }
}

fn transition_name(transition: &InputTransition) -> &'static str {
    match transition {
        InputTransition::Changed => "changed",
        InputTransition::Committed(_) => "committed",
        InputTransition::Cancel => "cancel",
        InputTransition::Quit => "quit",
        InputTransition::Resized(_) => "resized",
        InputTransition::Rejected(_) => "rejected",
        InputTransition::Ignored => "ignored",
    }
}

#[test]
fn decoded_pty_event_fixture_preserves_unicode_paste_resize_and_commit_boundary() {
    let fixture: Value = serde_json::from_str(PTY_FIXTURE).unwrap();
    for stream in fixture["streams"].as_array().unwrap() {
        let mut state = TtyInputState::default();
        let running = stream["running"].as_bool().unwrap_or(false);
        let mut committed: Vec<CommittedInput> = Vec::new();
        let mut transitions: Vec<InputTransition> = Vec::new();
        for event in stream["events"].as_array().unwrap() {
            let transition = state.apply_event(event_from_fixture(event), running);
            if let InputTransition::Committed(input) = &transition {
                committed.push(input.clone());
            }
            transitions.push(transition);
        }
        if let Some(expected) = stream.get("transitions") {
            let actual: Vec<&str> = transitions.iter().map(transition_name).collect();
            let expected: Vec<&str> = expected
                .as_array()
                .unwrap()
                .iter()
                .filter_map(Value::as_str)
                .collect();
            assert_eq!(actual, expected, "{}", stream["name"]);
        }
        if let Some(expected) = stream.get("draft") {
            assert_eq!(
                state.draft(),
                expected.as_str().unwrap(),
                "{}",
                stream["name"]
            );
        }
        if let Some(expected) = stream.get("committed") {
            let actual: Vec<_> = committed.iter().map(|item| item.text.as_str()).collect();
            let expected: Vec<_> = expected
                .as_array()
                .unwrap()
                .iter()
                .filter_map(Value::as_str)
                .collect();
            assert_eq!(actual, expected, "{}", stream["name"]);
        }
        assert_eq!(
            state.last_committed().map(|input| input.text.as_str()),
            committed.last().map(|input| input.text.as_str()),
            "{} keeps the last commit readable after later cancellation or editing",
            stream["name"]
        );
        if let Some(size) = stream.get("size") {
            let actual = state.terminal_size().unwrap();
            assert_eq!(actual.width, size[0].as_u64().unwrap() as u16);
            assert_eq!(actual.height, size[1].as_u64().unwrap() as u16);
        }
    }
}

#[test]
fn deny_first_rejects_terminal_controls_and_oversized_paste_without_partial_mutation() {
    let fixture: Value = serde_json::from_str(PTY_FIXTURE).unwrap();
    for denial in fixture["denials"].as_array().unwrap() {
        let mut state = TtyInputState::default();
        let transition = state.apply_event(event_from_fixture(&denial["event"]), false);
        assert!(
            matches!(&transition, InputTransition::Rejected(code) if *code == denial["rejection"].as_str().unwrap()),
            "unexpected transition for {}: {transition:?}",
            denial["name"]
        );
        assert_eq!(state.draft(), denial["draft"].as_str().unwrap());
    }

    let mut state = TtyInputState::default();
    let oversized = "🧪".repeat(MAX_TTY_INPUT_CHARS + 1);
    assert_eq!(
        state.apply_event(Event::Paste(oversized), false),
        InputTransition::Rejected("tty_input_limit_exceeded")
    );
    assert!(state.draft().is_empty(), "oversized paste must be atomic");

    let bounded = "🧪".repeat(MAX_TTY_INPUT_BYTES / 4);
    assert_eq!(bounded.len(), MAX_TTY_INPUT_BYTES);
    assert_eq!(bounded.chars().count(), MAX_TTY_INPUT_CHARS);
    assert_eq!(
        state.apply_event(Event::Paste(bounded), false),
        InputTransition::Changed
    );
    assert_eq!(
        state.apply_event(Event::Paste("🧪".to_owned()), false),
        InputTransition::Rejected("tty_input_limit_exceeded")
    );
}

#[test]
fn eof_sigint_escape_and_resize_do_not_rewrite_a_committed_command() {
    let mut state = TtyInputState::default();
    state.apply_event(Event::Paste("已提交命令".to_owned()), false);
    let InputTransition::Committed(committed) =
        state.apply_event(Event::Key(KeyEvent::from(KeyCode::Enter)), false)
    else {
        panic!("Enter must create a committed command")
    };

    assert_eq!(
        state.apply_event(key(KeyCode::Char('c'), KeyModifiers::CONTROL), true),
        InputTransition::Cancel
    );
    assert_eq!(
        state.apply_event(Event::Key(KeyEvent::from(KeyCode::Esc)), true),
        InputTransition::Cancel
    );
    assert_eq!(
        state.apply_event(Event::Resize(80, 24), false),
        InputTransition::Resized(kiana_entrypoints::tty_input::TerminalSize {
            width: 80,
            height: 24
        })
    );
    assert_eq!(committed.text, "已提交命令");
    assert_eq!(state.last_committed().unwrap().text, "已提交命令");
    assert_eq!(
        state.apply_event(key(KeyCode::Char('d'), KeyModifiers::CONTROL), false),
        InputTransition::Quit
    );

    state.apply_event(Event::Paste("draft".to_owned()), false);
    assert_eq!(
        state.apply_event(key(KeyCode::Char('d'), KeyModifiers::CONTROL), false),
        InputTransition::Ignored,
        "Ctrl-D with a nonempty draft must not discard it"
    );
    assert_eq!(state.draft(), "draft");
    assert_eq!(
        state.apply_event(key(KeyCode::Char('c'), KeyModifiers::CONTROL), false),
        InputTransition::Quit
    );
    assert_eq!(
        state.draft(),
        "draft",
        "quit signal does not mutate the draft"
    );
    assert_eq!(state.last_committed().unwrap().text, "已提交命令");
}

#[test]
fn pty_chunks_hold_partial_escape_and_utf8_until_complete() {
    let mut decoder = PtyChunkDecoder::default();
    assert!(decoder.feed(b"\x1b[").unwrap().is_empty());
    assert!(decoder
        .feed(b"200~pasted\n/quit\x1b[201~")
        .unwrap()
        .iter()
        .any(|event| { matches!(event, Event::Paste(text) if text == "pasted\n/quit") }));

    let mut decoder = PtyChunkDecoder::default();
    assert!(decoder.feed(&"你".as_bytes()[..1]).unwrap().is_empty());
    let events = decoder.feed(&"你".as_bytes()[1..]).unwrap();
    assert!(events.iter().any(|event| {
        matches!(
            event,
            Event::Key(KeyEvent {
                code: KeyCode::Char('你'),
                ..
            })
        )
    }));
    assert_eq!(decoder.finish(), Ok(()));

    let mut decoder = PtyChunkDecoder::default();
    assert!(decoder.feed(b"\x1b").unwrap().is_empty());
    assert_eq!(decoder.finish(), Err("tty_input_partial_escape_rejected"));
}

#[test]
fn bracketed_paste_start_marker_can_be_split_across_chunks() {
    let mut decoder = PtyChunkDecoder::default();
    assert!(decoder.feed(b"\x1b[200").unwrap().is_empty());
    let events = decoder.feed(b"~payload\n/quit\x1b[201~").unwrap();
    assert!(events
        .iter()
        .any(|event| matches!(event, Event::Paste(text) if text == "payload\n/quit")));
    assert_eq!(decoder.finish(), Ok(()));
}

#[test]
fn chunk_adapter_applies_only_complete_events_and_explicit_enter_commits() {
    let mut decoder = PtyChunkDecoder::default();
    let mut state = TtyInputState::default();
    assert!(state
        .apply_chunk(&mut decoder, b"\x1b[200~/quit\n", false)
        .unwrap()
        .is_empty());
    assert!(state
        .apply_chunk(&mut decoder, b"\x1b[201~", false)
        .unwrap()
        .iter()
        .all(|transition| transition == &InputTransition::Changed));
    assert_eq!(state.draft(), "/quit\n");
    let transitions = state.apply_chunk(&mut decoder, b"\n", false).unwrap();
    assert!(transitions.iter().any(|transition| {
        matches!(transition, InputTransition::Committed(input) if input.text == "/quit\n")
    }));
}

#[test]
fn non_tty_mode_rejects_interactive_events_before_any_draft_change() {
    let mut state = TtyInputState::new(InputMode::NonTty);
    assert_eq!(
        state.apply_event(Event::Paste("one-shot".to_owned()), false),
        InputTransition::Rejected("tty_input_requires_tty")
    );
    assert!(state.draft().is_empty());
}

#[test]
fn eof_only_exits_an_empty_editor_and_never_submits_a_partial_draft() {
    let mut state = TtyInputState::default();
    assert_eq!(state.eof(), InputTransition::Quit);
    state.apply_event(Event::Paste("unfinished".to_owned()), false);
    assert_eq!(
        state.eof(),
        InputTransition::Rejected("tty_input_eof_with_draft")
    );
    assert_eq!(state.draft(), "unfinished");
}

#[test]
fn ime_candidate_is_separate_and_ctrl_c_cannot_cancel_a_running_turn() {
    let mut state = TtyInputState::default();
    assert_eq!(state.begin_ime(), InputTransition::Changed);
    assert_eq!(state.update_ime("中"), InputTransition::Changed);
    assert!(state.is_composing());
    assert_eq!(
        state.apply_event(key(KeyCode::Char('c'), KeyModifiers::CONTROL), true),
        InputTransition::Changed
    );
    assert!(!state.is_composing());
    assert!(state.draft().is_empty());

    state.begin_ime();
    state.update_ime("文");
    assert_eq!(state.commit_ime(), InputTransition::Changed);
    assert_eq!(state.draft(), "文");
    assert_eq!(
        state.apply_event(Event::Key(KeyEvent::from(KeyCode::Enter)), false),
        InputTransition::Committed(CommittedInput {
            text: "文".to_owned()
        })
    );
}

#[test]
fn history_is_bounded_and_restores_the_uncommitted_draft() {
    let mut state = TtyInputState::default();
    for index in 0..(MAX_TTY_HISTORY_ENTRIES + 2) {
        state.apply_event(Event::Paste(format!("command-{index}")), false);
        state.apply_event(Event::Key(KeyEvent::from(KeyCode::Enter)), false);
    }
    assert_eq!(state.history().len(), MAX_TTY_HISTORY_ENTRIES);
    assert_eq!(state.history().first().unwrap(), "command-2");
    state.apply_event(Event::Paste("editing".to_owned()), false);
    state.apply_event(Event::Key(KeyEvent::from(KeyCode::Up)), false);
    assert_eq!(state.draft(), "command-65");
    state.apply_event(Event::Key(KeyEvent::from(KeyCode::Down)), false);
    assert_eq!(state.draft(), "editing");
}

#[test]
fn whitespace_enter_is_a_noop_and_preserves_the_draft() {
    let mut state = TtyInputState::default();
    state.apply_event(Event::Paste("  \t".to_owned()), false);
    assert_eq!(
        state.apply_event(Event::Key(KeyEvent::from(KeyCode::Enter)), false),
        InputTransition::Ignored
    );
    assert_eq!(state.draft(), "  \t");
}

#[test]
fn partial_escape_sequences_never_synthesize_enter_across_chunks() {
    // A complete arrow sequence split across chunks is still exactly one key event.
    let mut decoder = PtyChunkDecoder::default();
    assert!(decoder.feed(b"\x1b[").unwrap().is_empty());
    assert!(decoder.feed(b"A").unwrap().iter().any(|event| {
        matches!(
            event,
            Event::Key(KeyEvent {
                code: KeyCode::Up,
                ..
            })
        )
    }));
    assert_eq!(decoder.finish(), Ok(()));

    // A dangling escape followed by a carriage return in the next chunk is a malformed
    // sequence, never an Enter and never a command.
    let mut decoder = PtyChunkDecoder::default();
    assert!(decoder.feed(b"\x1b").unwrap().is_empty());
    assert_eq!(
        decoder.feed(b"\r"),
        Err("tty_input_partial_escape_rejected")
    );
    assert_eq!(decoder.finish(), Ok(()));

    // A partial delete sequence is only completed by its own terminator.
    let mut decoder = PtyChunkDecoder::default();
    assert!(decoder.feed(b"\x1b[3").unwrap().is_empty());
    assert!(decoder.feed(b"~").unwrap().iter().any(|event| {
        matches!(
            event,
            Event::Key(KeyEvent {
                code: KeyCode::Delete,
                ..
            })
        )
    }));
    assert_eq!(decoder.finish(), Ok(()));
}

#[test]
fn a_dangling_escape_across_chunks_is_rejected_and_never_submits_the_draft() {
    let mut decoder = PtyChunkDecoder::default();
    let mut state = TtyInputState::default();
    let transitions = state.apply_chunk(&mut decoder, b"ok\x1b[", false).unwrap();
    assert!(transitions
        .iter()
        .all(|transition| *transition == InputTransition::Changed));
    assert_eq!(state.draft(), "ok");
    assert_eq!(
        state.apply_chunk(&mut decoder, b"\r", false),
        Err("tty_input_partial_escape_rejected")
    );
    assert_eq!(
        state.draft(),
        "ok",
        "a dangling escape must never become an Enter or a command"
    );
    assert_eq!(state.last_committed(), None);

    // The decoder is clean after the rejection, so a real Enter still commits exactly once.
    let transitions = state.apply_chunk(&mut decoder, b"\r", false).unwrap();
    assert!(transitions.iter().any(|transition| {
        matches!(transition, InputTransition::Committed(input) if input.text == "ok")
    }));
    assert_eq!(state.draft(), "");
}

#[test]
fn bracketed_paste_payload_controls_stay_data_until_the_closing_marker() {
    let mut decoder = PtyChunkDecoder::default();
    let events = decoder
        .feed(b"\x1b[200~a\x1b[200~b\r/quits\x1b[201~")
        .unwrap();
    assert!(events
        .iter()
        .any(|event| { matches!(event, Event::Paste(text) if text == "a\x1b[200~b\r/quits") }));
    assert_eq!(decoder.finish(), Ok(()));

    // The embedded marker and carriage return stay one payload, and a payload carrying a
    // terminal control is refused as a whole instead of being executed or split.
    let mut state = TtyInputState::default();
    assert_eq!(
        state.apply_event(Event::Paste("a\x1b[200~b\r/quits".to_owned()), false),
        InputTransition::Rejected("tty_input_control_character_rejected")
    );
    assert!(state.draft().is_empty());
    assert_eq!(state.last_committed(), None);
}

#[test]
fn oversized_bracketed_paste_is_rejected_at_the_decoder_boundary_and_clears_pending() {
    let mut decoder = PtyChunkDecoder::default();
    let mut chunk = Vec::from(&b"\x1b[200~"[..]);
    chunk.extend(vec![b'x'; MAX_TTY_INPUT_BYTES + 8]);
    assert_eq!(decoder.feed(&chunk), Err("tty_input_limit_exceeded"));
    assert_eq!(
        decoder.finish(),
        Ok(()),
        "the rejected paste must leave no pending bytes behind"
    );

    // The same decoder still accepts a complete, bounded paste afterwards.
    let mut chunk = Vec::from(&b"\x1b[200~"[..]);
    chunk.extend_from_slice(b"ok\x1b[201~");
    assert!(decoder
        .feed(&chunk)
        .unwrap()
        .iter()
        .any(|event| matches!(event, Event::Paste(text) if text == "ok")));
    assert_eq!(decoder.finish(), Ok(()));
}

#[test]
fn scalar_safe_editing_never_splits_a_character_and_combining_marks_stay_data() {
    let mut state = TtyInputState::default();
    state.apply_event(Event::Paste("e\u{301}🧪".to_owned()), false);
    assert_eq!(state.draft(), "e\u{301}🧪");
    assert_eq!(
        state.apply_event(Event::Key(KeyEvent::from(KeyCode::Backspace)), false),
        InputTransition::Changed
    );
    assert_eq!(state.draft(), "e\u{301}", "backspace removes one scalar");
    state.apply_event(Event::Key(KeyEvent::from(KeyCode::Backspace)), false);
    assert_eq!(state.draft(), "e");
    state.apply_event(Event::Key(KeyEvent::from(KeyCode::Backspace)), false);
    assert_eq!(state.draft(), "");
    assert_eq!(
        state.apply_event(Event::Key(KeyEvent::from(KeyCode::Backspace)), false),
        InputTransition::Ignored
    );
}

#[test]
fn non_tty_mode_rejects_ime_and_resize_before_any_state_change() {
    let mut state = TtyInputState::new(InputMode::NonTty);
    assert_eq!(
        state.begin_ime(),
        InputTransition::Rejected("tty_input_requires_tty")
    );
    assert_eq!(
        state.update_ime("中"),
        InputTransition::Rejected("tty_input_requires_tty")
    );
    assert_eq!(
        state.commit_ime(),
        InputTransition::Rejected("tty_input_requires_tty")
    );
    assert_eq!(
        state.cancel_ime(),
        InputTransition::Rejected("tty_input_requires_tty")
    );
    assert_eq!(
        state.apply_event(Event::Resize(120, 40), false),
        InputTransition::Rejected("tty_input_requires_tty")
    );
    assert_eq!(
        state.eof(),
        InputTransition::Rejected("tty_input_requires_tty")
    );
    assert!(!state.is_composing());
    assert_eq!(state.terminal_size(), None);
    assert!(state.draft().is_empty());
    assert!(state.history().is_empty());
    assert_eq!(state.last_committed(), None);
}

#[test]
fn ime_rejects_paste_enter_and_oversized_candidates_without_touching_the_draft() {
    let mut state = TtyInputState::default();
    assert_eq!(state.begin_ime(), InputTransition::Changed);
    assert_eq!(state.update_ime("中"), InputTransition::Changed);
    assert_eq!(
        state.apply_event(Event::Paste("candidate".to_owned()), false),
        InputTransition::Rejected("tty_input_ime_active")
    );
    assert_eq!(
        state.apply_event(Event::Key(KeyEvent::from(KeyCode::Enter)), false),
        InputTransition::Rejected("tty_input_ime_active")
    );
    assert!(state.draft().is_empty());
    assert_eq!(state.last_committed(), None);

    let oversized = "🧪".repeat(MAX_TTY_INPUT_BYTES / 4 + 1);
    assert_eq!(
        state.update_ime(&oversized),
        InputTransition::Rejected("tty_input_limit_exceeded")
    );
    assert_eq!(state.commit_ime(), InputTransition::Changed);
    assert_eq!(
        state.draft(),
        "中",
        "a rejected candidate must not replace or append to the composition"
    );
}

#[test]
fn history_navigation_is_bounded_and_editing_resets_the_cursor() {
    let mut state = TtyInputState::default();
    for text in ["one", "two"] {
        state.apply_event(Event::Paste(text.to_owned()), false);
        state.apply_event(Event::Key(KeyEvent::from(KeyCode::Enter)), false);
    }
    state.apply_event(Event::Paste("editing".to_owned()), false);
    assert_eq!(state.draft(), "editing");
    assert_eq!(
        state.apply_event(Event::Key(KeyEvent::from(KeyCode::Up)), false),
        InputTransition::Changed
    );
    assert_eq!(state.draft(), "two");
    state.apply_event(Event::Key(KeyEvent::from(KeyCode::Up)), false);
    assert_eq!(state.draft(), "one");
    assert_eq!(
        state.apply_event(Event::Key(KeyEvent::from(KeyCode::Up)), false),
        InputTransition::Changed,
        "recall stops at the oldest history entry"
    );
    assert_eq!(state.draft(), "one");

    state.apply_event(Event::Key(KeyEvent::from(KeyCode::Char('!'))), false);
    assert_eq!(state.draft(), "one!");
    assert_eq!(
        state.apply_event(Event::Key(KeyEvent::from(KeyCode::Down)), false),
        InputTransition::Ignored,
        "editing after a recall drops the cursor instead of restoring the saved draft"
    );
    assert_eq!(state.draft(), "one!");
    assert_eq!(state.history().len(), 2);
    assert_eq!(state.history()[0], "one");
    assert_eq!(state.history()[1], "two");
}

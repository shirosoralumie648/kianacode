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
        "paste" => Event::Paste(value["value"].as_str().unwrap().to_owned()),
        "resize" => Event::Resize(
            value["width"].as_u64().unwrap() as u16,
            value["height"].as_u64().unwrap() as u16,
        ),
        "ctrl_j" => key(KeyCode::Char('j'), KeyModifiers::CONTROL),
        "enter" => Event::Key(KeyEvent::from(KeyCode::Enter)),
        event => panic!("unsupported UI-12 PTY fixture event: {event}"),
    }
}

#[test]
fn decoded_pty_event_fixture_preserves_unicode_paste_resize_and_commit_boundary() {
    let fixture: Value = serde_json::from_str(PTY_FIXTURE).unwrap();
    for stream in fixture["streams"].as_array().unwrap() {
        let mut state = TtyInputState::default();
        let mut committed: Vec<CommittedInput> = Vec::new();
        for event in stream["events"].as_array().unwrap() {
            if let InputTransition::Committed(input) =
                state.apply_event(event_from_fixture(event), false)
            {
                committed.push(input);
            }
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

#[test]
fn tty_input_is_a_bounded_event_adapter_without_execution_authority() {
    let input = include_str!("../src/tty_input.rs");
    let workbench = include_str!("../src/workbench_chat.rs");
    let workbench_entry = include_str!("../src/workbench.rs");
    let cli = include_str!("../src/cli.rs");

    for marker in [
        "MAX_TTY_INPUT_BYTES",
        "MAX_TTY_INPUT_CHARS",
        "MAX_TTY_HISTORY_ENTRIES",
        "PtyChunkDecoder",
        "PASTE_START.starts_with",
        "apply_chunk",
        "pub fn eof(&mut self)",
        "InputMode::NonTty",
        "begin_ime",
        "commit_ime",
        "Event::Paste(text) => self.insert_text(&text)",
        "Event::Resize(width, height)",
        "Committed(CommittedInput)",
        "tty_input_control_character_rejected",
        "tty_input_limit_exceeded",
    ] {
        assert!(
            input.contains(marker),
            "UI-12 input contract marker missing: {marker}"
        );
    }
    assert!(workbench.contains("input.apply_event(event, view.running)"));
    assert!(workbench.contains("view.interpret_line(&command.text)"));
    assert!(workbench_entry.contains("workbench_chat::run"));
    assert!(cli.contains("workbench::"));

    for forbidden in [
        "std::process::Command",
        "tokio::spawn",
        "DaemonHost",
        "ControlPlane",
        "CapabilityBroker",
        "KianaHarness",
        "std::fs::",
        "reqwest::",
    ] {
        assert!(
            !input.contains(forbidden),
            "TTY input contract gained an execution or transport authority: {forbidden}"
        );
    }
    assert!(!workbench.contains("std::process::Command"));
}

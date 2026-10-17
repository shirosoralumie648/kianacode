//! UI-12 boundary guard: the TTY input layer must stay a bounded, pure event adapter between
//! Crossterm and the existing Workbench command path.  It owns a draft, composition and bounded
//! history; it must never own a daemon, broker, event store, process spawn or model loop.

use kiana_entrypoints::tty_input::{
    MAX_TTY_HISTORY_ENTRIES, MAX_TTY_INPUT_BYTES, MAX_TTY_INPUT_CHARS,
};

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
    assert!(!input.contains("#[ignore"));
}

/// Completion slice (2026-10-17): the declarative PTY fixture, the behaviour fixtures, the manual
/// lane, the baseline document and the whole-crate shard must stay wired together, and the
/// Workbench adapter must keep committing only through the existing command path.
#[test]
fn tty_input_completion_slice_stays_wired_to_the_fixture_lane_and_shard() {
    let input = include_str!("../src/tty_input.rs");
    let fixtures = include_str!("ui12_tty_input.rs");
    let fixture_raw = include_str!("fixtures/ui12-pty-events.json");
    let baseline = include_str!("../../docs/roadmap/ui12-tty-input-baseline.md");
    let card = include_str!("../../docs/roadmap/ui-entrypoints.md");
    let lane = include_str!("../../.github/workflows/ui12-tty-input.yml");
    let shards_raw = include_str!("../../scripts/ci/test-shards.json");
    let workbench = include_str!("../src/workbench_chat.rs");

    // 1. the declared budgets match the source constants, and every declared fixture is real.
    let fixture: serde_json::Value =
        serde_json::from_str(fixture_raw).expect("the UI-12 fixture must stay valid JSON");
    assert_eq!(fixture["schema"], "kiana.ui12-pty-events.v1");
    assert_eq!(fixture["step"], "UI-12");
    assert_eq!(
        fixture["budgets"]["max_input_bytes"].as_u64(),
        Some(MAX_TTY_INPUT_BYTES as u64)
    );
    assert_eq!(
        fixture["budgets"]["max_input_chars"].as_u64(),
        Some(MAX_TTY_INPUT_CHARS as u64)
    );
    assert_eq!(
        fixture["budgets"]["max_history_entries"].as_u64(),
        Some(MAX_TTY_HISTORY_ENTRIES as u64)
    );
    let declared: Vec<&str> = fixture["fixtures"]
        .as_array()
        .expect("fixtures")
        .iter()
        .map(|value| value.as_str().expect("fixture name"))
        .collect();
    assert!(
        declared.len() >= 19,
        "the completion slice keeps every named fixture"
    );
    for name in &declared {
        assert!(
            fixtures.contains(&format!("fn {name}(")),
            "UI-12 fixture is not defined in kiana-entrypoints/tests/ui12_tty_input.rs: {name}"
        );
    }
    for name in [
        "partial_escape_sequences_never_synthesize_enter_across_chunks",
        "a_dangling_escape_across_chunks_is_rejected_and_never_submits_the_draft",
        "bracketed_paste_payload_controls_stay_data_until_the_closing_marker",
        "oversized_bracketed_paste_is_rejected_at_the_decoder_boundary_and_clears_pending",
        "scalar_safe_editing_never_splits_a_character_and_combining_marks_stay_data",
        "non_tty_mode_rejects_ime_and_resize_before_any_state_change",
        "ime_rejects_paste_enter_and_oversized_candidates_without_touching_the_draft",
        "history_navigation_is_bounded_and_editing_resets_the_cursor",
    ] {
        assert!(
            declared.contains(&name),
            "the fixture must keep the completion-slice test: {name}"
        );
    }
    assert!(!fixtures.contains("#[ignore"));
    assert!(!fixture_raw.contains("--ignored"));

    // 2. the required markers are documented in the baseline and the denial codes stay in source.
    let required = fixture["required"].as_array().expect("required");
    assert!(!required.is_empty());
    for marker in required {
        let marker = marker.as_str().expect("required marker");
        assert!(
            baseline.contains(marker),
            "UI-12 baseline is missing the required marker: {marker}"
        );
    }
    let denied = fixture["denied"].as_array().expect("denied");
    assert!(!denied.is_empty());
    for code in denied {
        let code = code.as_str().expect("denial code");
        assert!(
            input.contains(code),
            "UI-12 source is missing the denial code: {code}"
        );
    }
    assert!(!fixture["streams"].as_array().expect("streams").is_empty());
    assert!(!fixture["denials"].as_array().expect("denials").is_empty());
    assert!(!fixture["limitations"]
        .as_array()
        .expect("limitations")
        .is_empty());

    // 3. the card keeps the same acceptance and the baseline keeps the lane/test anchors.
    assert!(card.contains("UI-12"));
    for marker in [
        "ui12-pty-events.json",
        "ui12_tty_input",
        "ui12_tty_input_guard",
        "ui12-tty-input.yml",
        "proof_level",
        "limitations",
    ] {
        assert!(
            baseline.contains(marker),
            "UI-12 baseline is missing: {marker}"
        );
    }

    // 4. the Workbench adapter still commits only through the existing command path.
    assert!(workbench.contains("InputTransition::Committed(command) =>"));
    assert!(workbench.contains("view.interpret_line(&command.text)"));
    for forbidden in [
        "interpret_line(input.draft())",
        "interpret_line(&input.draft",
        "std::process::Command",
    ] {
        assert!(
            !workbench.contains(forbidden),
            "UI-12 adapter gained a second command path: {forbidden}"
        );
    }

    // 5. the manual lane stays dispatch-only, read-only and bound to the same targets.
    assert!(lane.contains("workflow_dispatch:"));
    assert!(lane.contains("permissions:"));
    assert!(lane.contains("contents: read"));
    for forbidden in [
        "pull_request",
        "  push:",
        "schedule:",
        "continue-on-error",
        "|| true",
        "--ignored",
    ] {
        assert!(
            !lane.contains(forbidden),
            "UI-12 lane must not use {forbidden}"
        );
    }
    for command in [
        "cargo fetch --locked",
        "cargo fmt --all --check",
        "cargo test -p kiana-entrypoints --test ui12_tty_input --locked -- --test-threads=1",
        "cargo test -p kiana-entrypoints --test ui12_tty_input_guard --locked -- --test-threads=1",
        "cargo test -p kiana-entrypoints --no-run --locked",
    ] {
        assert!(lane.contains(command), "UI-12 lane is missing: {command}");
    }

    // 6. the automatic shard book keeps the whole-crate shard that discovers both targets.
    let shards: serde_json::Value =
        serde_json::from_str(shards_raw).expect("test shards must stay valid JSON");
    assert!(
        shards["kiana-entrypoints"]["targets"].is_null(),
        "kiana-entrypoints must stay a whole-crate shard so new UI-12 targets are discovered"
    );
}

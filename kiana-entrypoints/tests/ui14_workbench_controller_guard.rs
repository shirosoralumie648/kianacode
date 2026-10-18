//! UI-14 boundary guards: the Workbench controller is an intent-only adapter.
//!
//! It may own a keymap, a command palette, a session switcher and a bounded audit trace, but it
//! must never own a daemon, client transport, broker, event store, process, filesystem or model
//! loop. The completion-slice guard keeps the declarative fixture, the behaviour fixtures, the
//! manual lane, the baseline document and the whole-crate shard wired together.

use kiana_entrypoints::workbench_controller::{
    KeyChord, Keymap, WorkbenchCommand, MAX_COMMAND_PALETTE_ENTRIES, MAX_CONTROLLER_AUDIT_ENTRIES,
    MAX_CONTROLLER_TEXT_BYTES, MAX_WORKBENCH_SESSIONS, MAX_WORKBENCH_TARGET_BYTES,
    MAX_WORKBENCH_TITLE_BYTES,
};

#[test]
fn ui14_controller_is_intent_only_and_keeps_authority_in_the_client_daemon_path() {
    let controller = include_str!("../src/workbench_controller.rs");
    let entrypoints = include_str!("../src/lib.rs");

    for marker in [
        "WorkbenchIntent",
        "WorkbenchUiAction",
        "UiActionV1",
        "into_protocol",
        "Keymap",
        "CommandPalette",
        "SessionSwitcher",
        "ActionAuditEntry",
        "expected_epoch",
        "expected_cursor",
        "expected_revision",
        "UiActionV1.target_id is bounded to 256 bytes",
        "duplicate_shortcut",
        "duplicate_command_binding",
        "duplicate_submission",
        "stale_draft",
        "capability_unavailable",
        "WindowClose",
        "SubmissionStatus",
        "RunStatus",
        "WORKBENCH_CONTROLLER_SCHEMA",
        "SubmissionIdentity",
        "InFlightDisposition",
        "ConnectionTransition",
        "TerminationSignal",
        "RecoveryObservation",
        "WorkbenchRecovery",
        "fn recover",
        "fn pending_identity",
        "fn note_termination_signal",
        "fn set_connection",
        "AuditDisposition::Recovered",
        "fn accepts_submissions",
        "workbench_controller_offline",
        "workbench_controller_terminating",
        "workbench_recovery_offline",
        "workbench_recovery_scope_changed",
        "workbench_recovery_epoch_mismatch",
        "workbench_recovery_cursor_stale",
        "workbench_recovery_revision_stale",
        "workbench_recovery_session_changed",
        "action_result_without_submission",
        "action_result_command_mismatch",
    ] {
        assert!(
            controller.contains(marker),
            "UI-14 controller marker missing: {marker}"
        );
    }
    assert!(entrypoints.contains("pub mod workbench_controller;"));

    let code = controller
        .lines()
        .filter(|line| !line.trim_start().starts_with("//"))
        .collect::<Vec<_>>()
        .join("\n");
    for forbidden in [
        "DaemonHost",
        "ControlPlane",
        "CapabilityBroker",
        "KianaHarness",
        "harness_run",
        "tokio::spawn",
        "std::process::Command",
        "std::process::exit",
        "std::fs::",
        "std::net",
        "std::thread",
        "reqwest::",
        "ureq",
        "hyper",
        "kiana_daemon",
        "kiana_client",
        "async fn",
        "unsafe",
        "cancel_envelope_on_host",
        "resume_envelope_on_host",
    ] {
        assert!(
            !code.contains(forbidden),
            "UI-14 controller gained execution or transport authority: {forbidden}"
        );
    }
    // The controller structurally cannot replay a prepared action: it keeps the submission
    // identity and never the payload.
    assert!(!code.contains("in_flight: Option<WorkbenchUiAction>"));
    assert!(code.contains("in_flight: Option<SubmissionIdentity>"));
}

/// Completion slice: the declarative fixture, the behaviour fixtures, the manual lane, the baseline
/// document and the automatic shard must stay wired together.
#[test]
fn ui14_completion_slice_stays_wired_to_the_fixture_lane_and_shard() {
    let controller = include_str!("../src/workbench_controller.rs");
    let fixtures = include_str!("ui14_workbench_controller.rs");
    let fixture_raw = include_str!("fixtures/ui14-controller-actions.json");
    let baseline = include_str!("../../docs/roadmap/ui14-workbench-controller-baseline.md");
    let card = include_str!("../../docs/roadmap/ui-entrypoints.md");
    let lane = include_str!("../../.github/workflows/ui14-workbench-controller.yml");
    let shards_raw = include_str!("../../scripts/ci/test-shards.json");
    let protocol = include_str!("../../kiana-protocol/src/ui_contracts.rs");

    // 1. the declared budgets match the source constants.
    let fixture: serde_json::Value =
        serde_json::from_str(fixture_raw).expect("the UI-14 fixture must stay valid JSON");
    assert_eq!(fixture["schema"], "kiana.ui14-workbench-controller.v1");
    assert_eq!(fixture["step"], "UI-14");
    for (budget, expected) in [
        ("max_sessions", MAX_WORKBENCH_SESSIONS as u64),
        ("max_palette_entries", MAX_COMMAND_PALETTE_ENTRIES as u64),
        ("max_audit_entries", MAX_CONTROLLER_AUDIT_ENTRIES as u64),
        ("max_text_bytes", MAX_CONTROLLER_TEXT_BYTES as u64),
        ("max_target_bytes", MAX_WORKBENCH_TARGET_BYTES as u64),
        ("max_title_bytes", MAX_WORKBENCH_TITLE_BYTES as u64),
        ("commands", WorkbenchCommand::ALL.len() as u64),
    ] {
        assert_eq!(
            fixture["budgets"][budget].as_u64(),
            Some(expected),
            "UI-14 fixture budget {budget} drifted from the source constant"
        );
    }

    // 2. the commands and keymap are the ones the source declares.
    let declared_commands: Vec<&str> = fixture["commands"]
        .as_array()
        .expect("commands")
        .iter()
        .map(|value| value.as_str().expect("command name"))
        .collect();
    assert_eq!(
        declared_commands,
        WorkbenchCommand::ALL
            .into_iter()
            .map(WorkbenchCommand::as_str)
            .collect::<Vec<_>>()
    );
    let keymap = Keymap::default_workbench();
    let mut declared_bindings: Vec<(String, String)> = fixture["keymap"]
        .as_array()
        .expect("keymap")
        .iter()
        .map(|value| {
            (
                value["shortcut"].as_str().expect("shortcut").to_owned(),
                value["command"].as_str().expect("command").to_owned(),
            )
        })
        .collect();
    declared_bindings.sort();
    let mut expected_bindings: Vec<(String, String)> = keymap
        .bindings()
        .map(|binding| (binding.chord.label(), binding.command.as_str().to_owned()))
        .collect();
    expected_bindings.sort();
    assert_eq!(declared_bindings, expected_bindings);
    assert_eq!(
        fixture["budgets"]["shortcuts"].as_u64(),
        Some(keymap.bindings().count() as u64)
    );
    assert_eq!(
        controller.matches("KeyChord::new(").count(),
        keymap.bindings().count(),
        "the default keymap must stay one chord per command"
    );
    assert_eq!(
        keymap.resolve(KeyChord::new('r', true, false, false)),
        Some(WorkbenchCommand::Run)
    );

    // 3. every declared fixture is a real behaviour test, and every scenario points at one.
    let declared: Vec<&str> = fixture["fixtures"]
        .as_array()
        .expect("fixture list")
        .iter()
        .map(|value| value.as_str().expect("fixture name"))
        .collect();
    assert!(
        declared.len() >= 20,
        "the completion slice keeps every named fixture"
    );
    for name in &declared {
        assert_eq!(
            fixtures.matches(&format!("fn {name}(")).count(),
            1,
            "UI-14 fixture is not defined exactly once in kiana-entrypoints/tests/ui14_workbench_controller.rs: {name}"
        );
    }
    for name in [
        "session_switch_cannot_submit_the_previous_draft_and_targets_the_new_session",
        "connection_loss_marks_the_submission_unknown_with_its_original_identity",
        "response_loss_never_mints_a_second_command_and_survives_a_terminal_run",
        "termination_signals_are_not_window_close_cancel_or_resume",
        "restart_recovery_refuses_stale_widened_or_foreign_coordinates",
        "restart_recovery_carries_only_server_coordinates_and_the_unreconciled_identity",
        "hostile_control_traffic_keeps_the_controller_bounded_and_audit_append_only",
    ] {
        assert!(
            declared.contains(&name),
            "the fixture must keep the completion-slice test: {name}"
        );
    }
    assert!(!fixtures.contains("#[ignore"));
    assert!(!fixture_raw.contains("--ignored"));
    let scenarios = fixture["scenarios"].as_array().expect("scenarios");
    assert!(scenarios.len() >= 10);
    for scenario in scenarios {
        let name = scenario["fixture"].as_str().expect("scenario fixture");
        assert!(
            declared.contains(&name),
            "UI-14 fixture file does not exercise the declared scenario fixture: {name}"
        );
        for refusal in scenario["refusals"].as_array().expect("scenario refusals") {
            let refusal = refusal.as_str().expect("scenario refusal");
            assert!(
                controller.contains(refusal),
                "UI-14 scenario references an undeclared refusal code: {refusal}"
            );
        }
    }

    // 4. required markers are documented in the baseline, denial codes stay in source, the
    //    parameterized family is composed from a real template, and the protocol-propagated
    //    markers are real.
    let required = fixture["required"].as_array().expect("required");
    assert!(!required.is_empty());
    for marker in required {
        let marker = marker.as_str().expect("required marker");
        assert!(
            baseline.contains(marker),
            "UI-14 baseline is missing the required marker: {marker}"
        );
    }
    let denied = fixture["denied"].as_array().expect("denied");
    assert!(!denied.is_empty());
    for code in denied {
        let code = code.as_str().expect("denial code");
        assert!(
            controller.contains(code),
            "UI-14 controller is missing the denial code or marker: {code}"
        );
    }
    let templates = fixture["denial_templates"]
        .as_object()
        .expect("denial templates");
    assert!(!templates.is_empty());
    for (template, fields) in templates {
        assert!(
            controller.contains(template),
            "UI-14 controller is missing the denial template: {template}"
        );
        for field in fields.as_array().expect("template fields") {
            let field = field.as_str().expect("template field");
            assert!(
                controller.contains(&format!("\"{field}\"")),
                "UI-14 controller never validates the {field} field"
            );
            assert!(
                fixtures.contains(&format!("workbench_{field}_invalid")),
                "UI-14 fixtures never assert the composed {field} denial code"
            );
        }
    }
    let propagated = fixture["propagated_denials"]
        .as_array()
        .expect("propagated denials");
    assert!(!propagated.is_empty());
    for code in propagated {
        let code = code.as_str().expect("propagated code");
        assert!(
            protocol.contains(code),
            "the propagated wire bound must exist in kiana-protocol: {code}"
        );
    }
    assert!(!fixture["limitations"]
        .as_array()
        .expect("limitations")
        .is_empty());
    let recovery = fixture["recovery"].as_object().expect("recovery section");
    assert!(recovery.contains_key("observation_fields"));
    let not_carried: Vec<&str> = fixture["recovery"]["not_carried"]
        .as_array()
        .expect("not carried")
        .iter()
        .map(|value| value.as_str().expect("not-carried field"))
        .collect();
    for field in [
        "draft",
        "submission_status",
        "in_flight_action",
        "run_status",
    ] {
        assert!(not_carried.contains(&field));
    }

    // 5. the card and baseline keep the lane/test anchors.
    assert!(card.contains("UI-14"));
    for marker in [
        "ui14-controller-actions.json",
        "ui14_workbench_controller",
        "ui14_workbench_controller_guard",
        "ui14-workbench-controller.yml",
        "proof_level",
        "limitations",
    ] {
        assert!(
            baseline.contains(marker),
            "UI-14 baseline is missing: {marker}"
        );
    }

    // 6. the manual lane stays dispatch-only, read-only and bound to the same targets.
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
            "UI-14 lane must not use {forbidden}"
        );
    }
    for command in [
        "cargo fetch --locked",
        "cargo fmt --all --check",
        "cargo test -p kiana-entrypoints --test ui14_workbench_controller --locked -- --test-threads=1",
        "cargo test -p kiana-entrypoints --test ui14_workbench_controller_guard --locked -- --test-threads=1",
        "cargo test -p kiana-entrypoints --no-run --locked",
    ] {
        assert!(lane.contains(command), "UI-14 lane is missing: {command}");
    }

    // 7. the automatic shard book keeps the whole-crate shard that discovers both targets.
    let shards: serde_json::Value =
        serde_json::from_str(shards_raw).expect("test shards must stay valid JSON");
    assert!(
        shards["kiana-entrypoints"]["targets"].is_null(),
        "kiana-entrypoints must stay a whole-crate shard so new UI-14 targets are discovered"
    );
}

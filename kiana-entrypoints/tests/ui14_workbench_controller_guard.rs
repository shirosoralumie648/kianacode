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
        "duplicate_submission",
        "stale_draft",
        "capability_unavailable",
        "WindowClose",
        "SubmissionStatus",
        "RunStatus",
    ] {
        assert!(
            controller.contains(marker),
            "UI-14 controller marker missing: {marker}"
        );
    }
    assert!(entrypoints.contains("pub mod workbench_controller;"));

    for forbidden in [
        "DaemonHost",
        "ControlPlane",
        "CapabilityBroker",
        "KianaHarness",
        "harness_run",
        "tokio::spawn",
        "std::process::Command",
        "std::fs::",
        "reqwest::",
        "cancel_envelope_on_host",
        "resume_envelope_on_host",
    ] {
        assert!(
            !controller.contains(forbidden),
            "UI-14 controller gained execution or transport authority: {forbidden}"
        );
    }
}

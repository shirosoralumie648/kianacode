#[test]
fn automation_events_keep_one_aggregate_cas_idempotency_and_cursor_boundary() {
    let domain = include_str!("../../kiana-domain/src/automation.rs");
    let ports = include_str!("../../kiana-ports/src/lib.rs");
    let core = include_str!("../src/automation.rs");
    for marker in [
        "AUTOMATION_EVENT_ENVELOPE_SCHEMA",
        "AutomationEventEnvelope",
        "validate_against",
        "command_digest",
        "payload_digest",
        "source_cursor",
        "deny_unknown_fields",
    ] {
        assert!(
            domain.contains(marker),
            "domain envelope marker missing: {marker}"
        );
    }
    for marker in [
        "read_stream_after",
        "event_stream_page_limit_invalid",
        "append_idempotent_expected",
        "commit_transition",
    ] {
        assert!(
            ports.contains(marker),
            "event port marker missing: {marker}"
        );
    }
    for marker in [
        "event.envelope",
        "workflow_event_envelope_mismatch",
        "commit_workflow",
        "with_stream_metadata",
        "with_idempotency_key",
        "workflow_replay_conflict",
    ] {
        assert!(core.contains(marker), "core event marker missing: {marker}");
    }
    assert!(core.contains("event.stream_version"));
    assert!(!core.contains("transcript"));
}

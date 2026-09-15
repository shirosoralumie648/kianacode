#[test]
fn event_redaction_boundary_and_artifact_reference_policy_are_source_owned() {
    let events = include_str!("../src/events.rs");
    let core_redaction = include_str!("../src/redaction.rs");
    let domain_redaction = include_str!("../../kiana-domain/src/redaction.rs");
    let states = include_str!("../../kiana-domain/src/states.rs");
    let artifacts = include_str!("../src/artifacts.rs");
    let receipts = include_str!("../src/receipts.rs");
    let baseline = include_str!("../../docs/roadmap/event-receipt-redaction-baseline.md");

    for marker in [
        "prepare_event_payload",
        "redact_event_value",
        "payload_depth",
        "event_payload_size_limit",
        "event_redaction_not_stable",
        "event_data_epoch_invalid",
        "event_artifact_refs",
        "with_redaction_metadata",
        "validate_redaction_metadata",
    ] {
        assert!(
            events.contains(marker) || states.contains(marker),
            "missing redaction marker {marker}"
        );
    }
    assert!(core_redaction.contains("redact_capability_result"));
    assert!(domain_redaction.contains("encode_bounded_value"));
    assert!(domain_redaction.contains("redaction_secret_sentinel_detected"));
    assert!(domain_redaction.contains("StreamingRedactor"));
    assert!(artifacts.contains("artifact"));
    assert!(receipts.contains("redact_event_value"));
    assert!(baseline.contains("secret_never_enters_event_or_receipt"));
    assert!(baseline.contains("redaction_changes_snapshot_marks_non_resumable"));
    assert!(baseline.contains("oversize_payload_is_rejected"));
    assert!(baseline.contains("artifact"));
    assert!(baseline.contains("result_unknown"));
}

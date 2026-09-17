#[test]
fn er11_receipts_are_typed_redacted_and_source_bound() {
    let domain = include_str!("../../kiana-domain/src/receipt_contracts.rs");
    let receipts = include_str!("../src/receipts.rs");
    let attempts = include_str!("../src/capability_attempt_projection.rs");
    for marker in [
        "RunReceipt",
        "ExecutionReceipt",
        "RUN_RECEIPT_SCHEMA",
        "EXECUTION_RECEIPT_SCHEMA",
        "source_cursor",
        "source_event_ids",
        "redaction_profile",
        "feature_status",
        "proof_level",
        "receipt_digest",
        "typed_run_receipt",
        "typed_execution_receipts",
        "receipt_from_events",
        "project_capability_attempts",
        "redact_event_value",
    ] {
        assert!(
            domain.contains(marker) || receipts.contains(marker) || attempts.contains(marker),
            "ER-11 marker missing: {marker}"
        );
    }
    assert!(receipts.contains("invocation_projection_error"));
    assert!(receipts.contains("result_unknown"));
    for forbidden in [
        "raw_output",
        "raw_prompt",
        "secret_payload",
        "broker.execute",
        "RunnerCommand",
        "issue_permit",
    ] {
        assert!(
            !domain.contains(forbidden),
            "receipt contract must not carry {forbidden}"
        );
    }
}

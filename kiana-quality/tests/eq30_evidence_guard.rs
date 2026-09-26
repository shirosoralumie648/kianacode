#[test]
fn evidence_evaluator_stays_pure_and_fail_closed() {
    let source = include_str!("../src/evidence.rs");
    for forbidden in [
        "std::fs",
        "tokio::",
        "reqwest::",
        "Provider",
        "ArtifactStore",
        "EventStore",
        "DaemonHost",
        "KianaHarness",
        "CapabilityBroker",
        "Command::new",
    ] {
        assert!(
            !source.contains(forbidden),
            "forbidden evidence evaluator dependency: {forbidden}"
        );
    }
    for required in [
        "EVIDENCE_RECEIPT_INPUT_SCHEMA",
        "EvidenceReceiptEvaluator",
        "evidence.artifact_hash_mismatch",
        "evidence.receipt_assertion_missing",
        "evidence.redaction_secret_detected",
        "evidence.provenance_mismatch",
        "evidence.source_cursor_invalid",
        "evidence.artifact_missing",
        "sha256:",
        "MAX_FINDINGS",
    ] {
        assert!(
            source.contains(required),
            "missing EQ-30 boundary marker: {required}"
        );
    }
}

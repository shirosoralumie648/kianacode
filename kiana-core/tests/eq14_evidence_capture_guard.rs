#[test]
fn evaluation_evidence_capture_is_reference_only_and_flush_fail_closed() {
    let runtime = include_str!("../../kiana-daemon/src/eval_runtime.rs");
    for marker in [
        "EvalEvidenceCapture",
        "EvalCaptureReceipt",
        "EVAL_EVIDENCE_CAPTURE_SCHEMA",
        "RuntimeEvent",
        "CommandReceipt",
        "invocation_refs",
        "artifact_refs",
        "receipt_refs",
        "infra_flush_unknown",
        "EvalCaptureStatus::InfraUnknown",
    ] {
        assert!(
            runtime.contains(marker),
            "evidence marker missing: {marker}"
        );
    }
    for forbidden in [
        "EventStorePort for EvalEvidenceCapture",
        "append_transition",
        "commit_transition",
        "CapabilityBroker",
        "KianaHarness",
        "tokio::spawn",
        "reqwest",
        "std::net",
        "fs::write",
    ] {
        assert!(
            !runtime.contains(forbidden),
            "second fact source marker found: {forbidden}"
        );
    }
}

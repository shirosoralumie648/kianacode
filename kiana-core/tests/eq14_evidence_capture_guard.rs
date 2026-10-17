#[path = "support/source_guard.rs"]
mod source_guard;

#[test]
fn evaluation_evidence_capture_is_reference_only_and_flush_fail_closed() {
    let runtime =
        source_guard::production_source(include_str!("../../kiana-daemon/src/eval_runtime.rs"));
    let boundary = [
        source_guard::item_source(&runtime, "pub struct EvalCaptureReceipt"),
        source_guard::item_source(&runtime, "impl EvalCaptureReceipt"),
        source_guard::item_source(&runtime, "pub struct EvalEvidenceCapture"),
        source_guard::item_source(&runtime, "impl EvalEvidenceCapture"),
    ]
    .join("\n");
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
        "blocks_eval_pass",
        "apply_flush_invariant",
        "EvalVerdict::Blocked",
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
            !boundary.contains(forbidden),
            "second fact source marker found: {forbidden}"
        );
    }
}

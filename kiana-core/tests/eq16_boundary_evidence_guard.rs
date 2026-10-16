#[path = "support/source_guard.rs"]
mod source_guard;

#[test]
fn evaluation_boundary_evidence_is_scrubbed_and_eval_only() {
    let runtime =
        source_guard::production_source(include_str!("../../kiana-daemon/src/eval_runtime.rs"));
    let boundary = [
        source_guard::item_source(&runtime, "pub struct EvalProcessObservation"),
        source_guard::item_source(&runtime, "pub struct EvalFileDiff"),
        source_guard::item_source(&runtime, "pub struct EvalBoundaryEvidence"),
        source_guard::item_source(&runtime, "impl EvalBoundaryEvidence"),
        source_guard::item_source(&runtime, "pub struct EvalEvidenceCapture"),
        source_guard::item_source(&runtime, "impl EvalEvidenceCapture"),
    ]
    .join("\n");
    for marker in [
        "EvalBoundaryEvidence",
        "EvalProcessObservation",
        "EvalFileDiff",
        "network_syscalls",
        "secret_patterns",
        "SafetyViolation",
        "safety_violation",
        "attach_safety_evidence",
    ] {
        assert!(
            runtime.contains(marker),
            "boundary evidence marker missing: {marker}"
        );
    }
    for forbidden in [
        "std::process::Command",
        "std::process::id",
        "read_proc",
        "TcpStream",
        "reqwest",
        "tokio::spawn",
        "KianaHarness",
        "CapabilityBroker",
        "secret_value",
        "raw_secret",
    ] {
        assert!(
            !boundary.contains(forbidden),
            "uncontrolled evidence marker found: {forbidden}"
        );
    }
}

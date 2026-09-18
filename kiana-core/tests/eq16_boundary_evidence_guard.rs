#[test]
fn evaluation_boundary_evidence_is_scrubbed_and_eval_only() {
    let runtime = include_str!("../../kiana-daemon/src/eval_runtime.rs");
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
            !runtime.contains(forbidden),
            "uncontrolled evidence marker found: {forbidden}"
        );
    }
}

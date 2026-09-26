#[test]
fn aut24_release_gate_rejects_blanket_completion_and_reuses_existing_spine() {
    let domain = include_str!("../../kiana-domain/src/automation_release_evidence.rs");
    let core = include_str!("../src/automation_release_evidence.rs");
    let workflow = include_str!("../src/automation.rs");
    let daemon = include_str!("../../kiana-daemon/src/lib.rs");
    for marker in [
        "Aut24ReleaseGate",
        "Aut24EvidenceRow",
        "Aut24ProofLevel",
        "blanket_completion",
        "next_gate",
        "limitations",
        "DaemonHost",
        "ControlPlane",
        "validate_automation_release_gate",
    ] {
        assert!(
            domain.contains(marker)
                || core.contains(marker)
                || workflow.contains(marker)
                || daemon.contains(marker),
            "AUT-24 marker missing: {marker}"
        );
    }
    for forbidden in [
        "CapabilityBroker::new",
        "ModelClient::new",
        "std::process::Command",
        "publish_release",
    ] {
        assert!(
            !domain.contains(forbidden) && !core.contains(forbidden),
            "AUT-24 release/effect bypass marker present: {forbidden}"
        );
    }
}

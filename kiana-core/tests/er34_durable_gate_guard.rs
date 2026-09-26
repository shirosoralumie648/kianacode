#[test]
fn er34_durable_gate_keeps_ci_source_and_durable_physical_boundaries_explicit() {
    let domain = include_str!("../../kiana-domain/src/er34_durable_gate.rs");
    let core = include_str!("../src/er34_durable_gate.rs");
    let smoke = include_str!("../../scripts/release-smoke.sh");
    let golden = include_str!("../../kiana-daemon/tests/p3_i06_company_golden.rs");
    let baseline = include_str!("../../docs/roadmap/er34-durable-gate-baseline.md");
    for marker in [
        "Er34DurableGateEvidence",
        "Er34GateOrigin",
        "Er34ProofLevel",
        "ci_focused_tests_executed",
        "durable_facts_observed",
        "cache_deleted_and_rebuilt",
        "event_frame_hash",
        "artifact_hash",
        "process_fact_digest",
        "release-smoke",
        "fake_model_coding_project_produces_closing_receipt",
        "validate_er34_durable_gate_evidence",
    ] {
        assert!(
            domain.contains(marker)
                || core.contains(marker)
                || smoke.contains(marker)
                || golden.contains(marker)
                || baseline.contains(marker),
            "ER-34 marker missing: {marker}"
        );
    }
    for forbidden in [
        "std::process::Command",
        "CapabilityBroker::new",
        "ModelClient::new",
        "auto_pass",
        "proof_level = Er34ProofLevel::Durable",
    ] {
        assert!(
            !domain.contains(forbidden) && !core.contains(forbidden),
            "ER-34 proof bypass marker present: {forbidden}"
        );
    }
}

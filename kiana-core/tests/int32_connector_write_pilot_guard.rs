#[test]
fn connector_write_pilot_gate_is_evidence_only() {
    let domain = include_str!("../../kiana-domain/src/connector_write_pilot.rs");
    let core = include_str!("../src/connector_write_pilot.rs");
    for marker in [
        "ReadyForExplicitRun",
        "approval_ref",
        "permit_digest",
        "idempotency_policy_digest",
        "provider_receipt_digest",
        "cancellation_fence_digest",
        "compensation_plan_digest",
        "reconciliation_case_digest",
        "validate_connector_write_pilot_gate",
    ] {
        assert!(
            domain.contains(marker) || core.contains(marker),
            "INT-32 marker missing: {marker}"
        );
    }
    for forbidden in [
        "CapabilityBroker::new",
        "ModelClient::new",
        "std::process::Command",
        "tokio::spawn",
        "EventStore::append",
        "invoke_external",
        "payment",
        "refund",
    ] {
        assert!(
            !domain.contains(forbidden) && !core.contains(forbidden),
            "INT-32 write pilot widened effect boundary: {forbidden}"
        );
    }
}

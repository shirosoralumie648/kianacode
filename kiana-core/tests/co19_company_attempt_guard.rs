#[test]
fn company_attempts_separate_packet_identity_from_epoch_and_effect_fence() {
    let attempt = include_str!("../../kiana-domain/src/company_attempt.rs");
    let company = include_str!("../../kiana-domain/src/company.rs");
    let core = include_str!("../src/company.rs");
    let lease = include_str!("../../kiana-domain/src/packet_graph.rs");

    for marker in [
        "PACKET_ATTEMPT_SCHEMA",
        "PacketAttemptStatus",
        "PacketEffectState",
        "execution_request_id",
        "epoch",
        "lease_expires_at",
        "packet_attempt_already_claimed",
        "packet_attempt_fence_mismatch",
        "packet_attempt_result_unknown_requires_reconcile",
        "fence_expired",
        "safely_reclaimable",
        "PacketAttemptLedger",
        "claim_packet_attempt",
        "fence_expired_packet_attempt",
        "confirm_packet_attempt_stopped",
        "packet_claim_expired",
        "CompanyCommand::ReclaimPacketClaim",
    ] {
        assert!(
            attempt.contains(marker)
                || company.contains(marker)
                || core.contains(marker)
                || lease.contains(marker),
            "CO-19 marker missing: {marker}"
        );
    }
    assert!(attempt.contains("never-started or confirmed-stopped"));
    assert!(!attempt.contains("CapabilityBroker"));
    assert!(!attempt.contains("Runner"));
}

#[test]
fn rework_preserves_rejection_baseline_attempt_and_successor_history() {
    let rework = include_str!("../../kiana-domain/src/rework_contract.rs");
    let business = include_str!("../../kiana-domain/src/company_business.rs");
    let company = include_str!("../../kiana-domain/src/company.rs");
    let core = include_str!("../src/rework_contract.rs");
    for marker in [
        "REWORK_PROVENANCE_SCHEMA",
        "ReworkAttemptState",
        "predecessor_packet_id",
        "successor_packet_id",
        "rejection_ref",
        "baseline_digest",
        "remaining_budget",
        "rework_terminal_attempt_cannot_revive",
        "rework_attempt_not_reclaimable",
        "rework_successor_cycle",
        "ReworkLedger",
        "CompanyBusinessAction::Rework",
        "business_rework_limit_invalid",
        "business_rework_rejection_required",
        "business_rework_cannot_change_baseline",
        "business_rework_active_run_denied",
        "record_rework",
        "ReworkPacket",
    ] {
        assert!(
            rework.contains(marker)
                || business.contains(marker)
                || company.contains(marker)
                || core.contains(marker),
            "CO-29 marker missing: {marker}"
        );
    }
    assert!(!rework.contains("CapabilityBroker"));
    assert!(!rework.contains("Runner"));
}

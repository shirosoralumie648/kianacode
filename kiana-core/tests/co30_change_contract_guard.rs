#[test]
fn change_publication_fences_old_baselines_and_partial_updates() {
    let change = include_str!("../../kiana-domain/src/change_contract.rs");
    let business = include_str!("../../kiana-domain/src/company_business.rs");
    let company = include_str!("../../kiana-domain/src/company.rs");
    let core = include_str!("../src/change_contract.rs");
    for marker in [
        "CHANGE_IMPACT_SCHEMA",
        "BASELINE_PUBLICATION_SCHEMA",
        "ChangeImpact",
        "affected_milestones",
        "affected_packets",
        "invalidated_reviews",
        "invalidated_acceptances",
        "invalidated_deliveries",
        "BaselinePublication",
        "baseline_publication_complete_set_required",
        "baseline_publication_version_conflict",
        "ChangePublicationLedger",
        "CompanyCommand::DecideChange",
        "change_baseline_version_stale",
        "business_change_baseline_stale",
        "publish_change",
    ] {
        assert!(
            change.contains(marker)
                || business.contains(marker)
                || company.contains(marker)
                || core.contains(marker),
            "CO-30 marker missing: {marker}"
        );
    }
    assert!(!change.contains("CapabilityBroker"));
    assert!(!change.contains("Runner"));
}

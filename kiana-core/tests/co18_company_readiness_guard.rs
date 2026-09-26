#[test]
fn company_views_use_one_read_only_readiness_projection() {
    let domain = include_str!("../../kiana-domain/src/company_readiness.rs");
    let company = include_str!("../../kiana-domain/src/company.rs");
    let core = include_str!("../src/company.rs");
    let queue = include_str!("../src/workflow_queue.rs");

    for marker in [
        "COMPANY_READINESS_SCHEMA",
        "company_ready_packets",
        "CompanyReadinessContext",
        "DependencyRequirement::RequiresAcceptance",
        "DependencyOutcome::Succeeded",
        "dependency_acceptance_required",
        "dependency_result_unknown",
        "input_version_missing",
        "input_version_stale",
        "handoff_ack_required",
        "assignment_expired",
        "project_paused",
        "change_pending",
        "ReadinessBlocker",
        "canonical_digest",
        "project_company_readiness",
        "kiana_domain::ready_packets",
    ] {
        assert!(
            domain.contains(marker)
                || company.contains(marker)
                || core.contains(marker)
                || queue.contains(marker),
            "CO-18 marker missing: {marker}"
        );
    }
    assert!(core.contains("project_company_readiness"));
    assert!(domain.contains("never claims a packet"));
    assert!(!domain.contains("CapabilityBroker"));
    assert!(!domain.contains("EventStorePort"));
}

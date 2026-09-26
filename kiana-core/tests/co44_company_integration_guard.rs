#[test]
fn company_integration_keeps_review_conflict_and_publish_boundaries() {
    let integration = include_str!("../../kiana-domain/src/company_integration.rs");
    let swarm = include_str!("../../kiana-domain/src/swarm.rs");
    let receipt = include_str!("../../kiana-domain/src/work_packets.rs");
    let core = include_str!("../src/company_integration.rs");
    for marker in [
        "COMPANY_INTEGRATION_SCHEMA",
        "CompanyIntegrationPlan",
        "CompanyConflictDecision",
        "CompanyMergeReceipt",
        "base_revision",
        "child_output_digests",
        "conflict_decisions",
        "revalidated",
        "pushed_or_published",
        "company_merge_conflict_coverage_required",
        "company_merge_receipt_binding_invalid",
        "MergeReceipt",
        "SwarmCommand::Merge",
        "validate_company_merge_receipt",
    ] {
        assert!(
            integration.contains(marker)
                || swarm.contains(marker)
                || receipt.contains(marker)
                || core.contains(marker),
            "CO-44 marker missing: {marker}"
        );
    }
    for forbidden in [
        "git push",
        "std::process::Command",
        "ModelClient::new",
        "auto_accept_project",
    ] {
        assert!(
            !integration.contains(forbidden),
            "CO-44 bypass marker present: {forbidden}"
        );
    }
}

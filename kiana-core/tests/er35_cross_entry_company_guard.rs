//! ER-35 source guard for cross-entry and CompanyOS fact/reference parity.

#[test]
fn cross_entry_company_gate_reuses_one_spine_and_keeps_unknown_visible() {
    let parity = include_str!("oa24_entrypoint_parity.rs");
    let governance = include_str!("oa27_company_governance.rs");
    let daemon = include_str!("../../kiana-daemon/src/lib.rs");
    let harness_run = include_str!("../../kiana-entrypoints/src/harness_run.rs");
    let client = include_str!("../../kiana-client/src/lib.rs");
    let protocol = include_str!("../../kiana-protocol/src/lib.rs");
    let fixture = include_str!("fixtures/er35-cross-entry-company.json");
    let baseline = include_str!("../../docs/roadmap/er35-cross-entry-company-baseline.md");

    for marker in [
        "project_entrypoint_parity",
        "EntryPointParitySnapshot",
        "CompanyGovernanceSnapshot",
        "CompanyClosingReceipt",
        "runtime_completed_not_business_outcome",
        "closing_chain_incomplete",
        "result_unknown",
        "reconcile",
        "DaemonHost",
        "ControlPlane",
        "KianaHarness",
        "company_governance",
        "entrypoint_parity",
    ] {
        assert!(
            parity.contains(marker)
                || governance.contains(marker)
                || daemon.contains(marker)
                || harness_run.contains(marker)
                || client.contains(marker)
                || protocol.contains(marker)
                || fixture.contains(marker)
                || baseline.contains(marker),
            "ER-35 marker missing: {marker}"
        );
    }
    for source in [parity, governance, harness_run] {
        assert!(!source.contains("CapabilityBroker::new"));
    }
    assert!(fixture.contains("four_entrypoint_same_facts"));
    assert!(fixture.contains("company_close_requires_review_delivery_receipt"));
    assert!(fixture.contains("unknown_requires_reconcile"));
}

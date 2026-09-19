//! PD-16 source guard for receipt recomputation, evidence graph and delivery/closing refs.

#[test]
fn receipt_rebuild_uses_facts_and_retains_evidence_source_boundaries() {
    let receipts = include_str!("../src/receipts.rs");
    let artifacts = include_str!("../src/artifacts.rs");
    let company = include_str!("../src/company.rs");
    let business = include_str!("../src/company_business.rs");
    for marker in [
        "aggregate_receipt_facts",
        "source_event_ids",
        "evidence_ref_digests",
        "provider_receipt_refs",
        "artifact_refs",
        "company_evidence_not_found",
        "write_closing_artifact",
    ] {
        assert!(
            receipts.contains(marker)
                || artifacts.contains(marker)
                || company.contains(marker)
                || business.contains(marker),
            "PD-16 marker missing: {marker}"
        );
    }
    assert!(!receipts.contains("transcript_as_receipt_authority"));
    assert!(!receipts.contains("CapabilityBrokerPort::execute"));
    assert!(!company.contains("model_self_report_as_evidence"));
}

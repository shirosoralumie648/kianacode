#[test]
fn packet_acceptance_requires_evidence_review_and_declared_dependents() {
    let acceptance = include_str!("../../kiana-domain/src/packet_acceptance.rs");
    let review = include_str!("../../kiana-domain/src/company_review.rs");
    let evidence = include_str!("../../kiana-domain/src/company_evidence.rs");
    let company = include_str!("../../kiana-domain/src/company.rs");
    let core = include_str!("../src/packet_acceptance.rs");

    for marker in [
        "PACKET_ACCEPTANCE_SCHEMA",
        "PacketAcceptanceRequest",
        "packet_version",
        "attempt_ref",
        "IndependentReview",
        "CompanyEvidenceReady",
        "packet_acceptance_review_binding_mismatch",
        "packet_acceptance_evidence_binding_mismatch",
        "packet_acceptance_criteria_failed",
        "packet_acceptance_waiver_requires_sponsor",
        "declared_dependents",
        "accepted_dependents",
        "RequestAcceptance",
        "DecideAcceptance",
        "record_packet_acceptance",
        "AcknowledgeHandoff",
    ] {
        assert!(
            acceptance.contains(marker)
                || review.contains(marker)
                || evidence.contains(marker)
                || company.contains(marker)
                || core.contains(marker),
            "CO-26 marker missing: {marker}"
        );
    }
    assert!(!acceptance.contains("PacketHandoff::acknowledge"));
    assert!(!acceptance.contains("CapabilityBroker"));
}

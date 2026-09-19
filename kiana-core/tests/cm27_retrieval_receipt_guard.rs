#[test]
fn retrieval_receipt_keeps_stages_and_provenance_in_one_boundary() {
    let domain = include_str!("../../kiana-domain/src/retrieval_receipt.rs");
    let receipts = include_str!("../src/receipts.rs");

    for marker in [
        "RetrievalReceiptStage",
        "Retrieved",
        "Selected",
        "Sent",
        "Cited",
        "RETRIEVAL_RECEIPT_SCHEMA",
        "REVIEWER_CITATION_SCHEMA",
        "source_revision",
        "permission_scope_digest",
        "algorithm_version",
        "degraded_reasons",
        "omissions",
        "retrieval_citation_provenance_unverifiable",
        "retrieval_receipt_selected_without_retrieved",
        "retrieval_receipt_sent_without_selected",
        "retrieval_receipt_cited_without_sent",
        "retrieval_receipts_from_events",
        "stage",
        "retrieved",
    ] {
        assert!(
            domain.contains(marker) || receipts.contains(marker),
            "CM-27 source marker missing: {marker}"
        );
    }

    assert!(receipts.contains("RetrievalReceiptStage::Retrieved"));
    assert!(domain.contains("validate_against"));
    assert!(!receipts.contains("ModelClient"));
    assert!(!receipts.contains("CapabilityBroker"));
}

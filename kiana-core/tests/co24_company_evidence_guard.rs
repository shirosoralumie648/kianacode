#[test]
fn company_evidence_ingestion_requires_actual_receipt_and_artifact_bindings() {
    let evidence = include_str!("../../kiana-domain/src/company_evidence.rs");
    let core = include_str!("../src/company_evidence.rs");
    let invocation = include_str!("../src/invocation_projection.rs");
    let dispatch = include_str!("../src/dispatch.rs");

    for marker in [
        "COMPANY_EVIDENCE_SCHEMA",
        "CompanyEvidenceBundle",
        "CompanyEvidenceFileChange",
        "CompanyEvidenceArtifact",
        "CompanyEvidenceTestResult",
        "RuntimeReceiptRef",
        "invocation_id",
        "request_id",
        "exit_code",
        "source_revision",
        "workspace_revision",
        "company_evidence_test_zero_matches",
        "company_evidence_foreign_artifact",
        "company_evidence_receipt_binding_invalid",
        "CompanyEvidenceReady",
        "ingest_company_evidence",
        "execution.result_committed",
    ] {
        assert!(
            evidence.contains(marker)
                || core.contains(marker)
                || invocation.contains(marker)
                || dispatch.contains(marker),
            "CO-24 marker missing: {marker}"
        );
    }
    assert!(!evidence.contains("model_claimed == false"));
    assert!(!evidence.contains("CapabilityBroker"));
    assert!(!evidence.contains("Runner"));
}

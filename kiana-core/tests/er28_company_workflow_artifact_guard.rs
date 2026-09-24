//! ER-28 source guard. Runtime terminals and business decisions stay on separate evidence edges.

#[test]
fn er28_keeps_runtime_receipts_incidents_and_business_closeout_separate() {
    let domain = include_str!("../../kiana-domain/src/runtime_evidence.rs");
    let automation = include_str!("../src/automation.rs");
    let workflow = include_str!("../../kiana-workflow/src/durable.rs");
    let company = include_str!("../src/company.rs");
    let company_domain = include_str!("../../kiana-domain/src/company.rs");
    let company_business = include_str!("../../kiana-domain/src/company_business.rs");
    let company_closeout = include_str!("../../kiana-domain/src/company_closeout.rs");
    let baseline = include_str!("../../docs/roadmap/er28-company-workflow-artifact-baseline.md");

    for marker in [
        "RUNTIME_RECEIPT_REF_SCHEMA",
        "RUNTIME_EVIDENCE_BUNDLE_SCHEMA",
        "WORKFLOW_INCIDENT_SCHEMA",
        "RuntimeReceiptRef",
        "RuntimeEvidenceBundle",
        "WorkflowIncident",
        "runtime_evidence",
        "workflow_runtime_receipt_required",
        "workflow_runtime_receipt_binding_mismatch",
        "workflow_incident_required",
        "workflow_incident_binding_mismatch",
        "WorkflowNodeStatus::ResultUnknown",
        "next.incidents",
        "workflow_runtime_proof",
    ] {
        assert!(
            domain.contains(marker) || automation.contains(marker) || workflow.contains(marker),
            "ER-28 runtime marker missing: {marker}"
        );
    }

    for marker in [
        "CompanyCommandReceipt",
        "CompanyClosingReceipt",
        "business_runtime_evidence_missing",
        "business_runtime_evidence_invalid",
        "company_evidence_not_found",
        "company_artifact_changed",
        "company_run_reconciliation_required",
        "acceptance_runtime_receipt_required",
        "review_runtime_receipt_required",
        "delivery_runtime_receipt_required",
        "closing_runtime_receipt_required",
        "CompanyBusinessAction::Closeout",
        "evidence_refs",
    ] {
        assert!(
            company.contains(marker)
                || company_domain.contains(marker)
                || company_business.contains(marker)
                || company_closeout.contains(marker),
            "ER-28 company marker missing: {marker}"
        );
    }

    for marker in [
        "runtime receipt",
        "EvidenceBundle",
        "business acceptance",
        "ResultUnknown",
        "Incident",
        "replay",
        "close",
        "proof_level",
        "source",
    ] {
        assert!(
            baseline.contains(marker),
            "ER-28 baseline marker missing: {marker}"
        );
    }

    assert!(automation.contains("plan_command(&state"));
    assert!(automation.contains("record_workflow_observation"));
    assert!(!workflow.contains("KianaHarness::new"));
    assert!(!workflow.contains("CapabilityBroker::new"));
}

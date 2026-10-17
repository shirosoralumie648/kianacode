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
        "business_runtime_receipt_missing",
        "business_runtime_evidence_invalid",
        "company_evidence_not_found",
        "company_artifact_changed",
        "company_run_reconciliation_required",
        "acceptance_runtime_receipt_required",
        "review_runtime_receipt_required",
        "delivery_runtime_receipt_required",
        "closing_runtime_receipt_required",
        "business_closing_required_acceptance_missing",
        "business_closing_runtime_evidence_missing",
        "business_closing_unresolved_effects",
        "business_evidence_unverified",
        "runtime_bundle_valid",
        "BusinessDeliveryStatus::Confirmed",
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

#[test]
fn er28_behavioural_fixtures_are_wired_into_the_ci_shards() {
    let domain_fixture = include_str!("../../kiana-domain/tests/er28_runtime_evidence.rs");
    let workflow_fixture = include_str!("../../kiana-workflow/tests/er28_workflow_observation.rs");
    let shards = include_str!("../../scripts/ci/test-shards.json");

    for marker in [
        "company_success_cannot_be_inferred_from_run_completed",
        "closing_without_evidence_is_denied",
        "er28_builder_review_acceptance_delivery_close_refs_rebuild_from_events_and_artifacts",
        "RuntimeReceiptRef::new",
        "RuntimeEvidenceBundle::new",
        "business_closing_required_acceptance_missing",
        "business_closing_runtime_evidence_missing",
        "business_closing_unresolved_effects",
        "business_evidence_unverified",
        "business_accepted",
        "BusinessDeliveryStatus::Confirmed",
        "transition(",
    ] {
        assert!(
            domain_fixture.contains(marker),
            "ER-28 domain fixture marker missing: {marker}"
        );
    }
    assert!(!domain_fixture.contains("#[ignore"));

    for marker in [
        "workflow_replay_does_not_execute_effect",
        "workflow_execution_in_flight",
        "WorkflowEffect::Dispatch",
        "WorkflowNodeStatus::Reserved",
        "runtime_receipt.is_none()",
        "tokio::spawn",
    ] {
        assert!(
            workflow_fixture.contains(marker),
            "ER-28 workflow fixture marker missing: {marker}"
        );
    }
    assert!(!workflow_fixture.contains("#[ignore"));

    // The unified CI gate runs the domain/core ER-28 targets through their shards and
    // auto-discovers every kiana-workflow test target (targets: null).
    assert!(shards.contains("\"er28_runtime_evidence\""));
    assert!(shards.contains("\"er28_company_workflow_artifact_guard\""));
    let start = shards
        .find("\"kiana-workflow\": {")
        .expect("kiana-workflow shard entry");
    let entry = &shards[start..(start + 160).min(shards.len())];
    assert!(
        entry.contains("\"targets\": null"),
        "kiana-workflow shard must auto-discover test targets"
    );
}

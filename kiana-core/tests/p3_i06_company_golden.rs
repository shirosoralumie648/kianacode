#[test]
fn company_golden_loop_keeps_runtime_and_business_evidence_separate() {
    let domain = include_str!("../../kiana-domain/src/company.rs");
    let replay = include_str!("../../kiana-domain/src/company_replay.rs");
    let closeout = include_str!("../../kiana-domain/src/company_closeout.rs");
    let business = include_str!("../src/company_business.rs");
    let company = include_str!("../src/company.rs");
    let collaboration = include_str!("../src/collaboration.rs");
    let daemon = include_str!("../../kiana-daemon/src/lib.rs");
    let model = include_str!("../../kiana-daemon/src/model_client.rs");
    let fixture = include_str!("../../kiana-daemon/tests/p3_i06_company_golden.rs");

    for marker in [
        "Objective",
        "Project",
        "CreateMilestone",
        "ApprovePacket",
        "StartRun",
        "RecordRunStarted",
        "RequestAcceptance",
        "RecordReview",
        "DecideAcceptance",
        "PrepareDelivery",
        "ApproveDelivery",
        "Deliver",
        "ConfirmDelivery",
        "CloseProject",
        "RecordOutcome",
        "CompanyClosingReceipt",
        "CompanyCommandReceipt",
        "CompanyReplayReducer",
        "source_event_ids",
        "artifact_refs",
        "evidence_refs",
        "ReworkPacket",
        "PauseProject",
        "RequestCancelProject",
        "ConfirmCancelProject",
        "FailProject",
        "ArchiveProject",
        "MarkDeliveryUnknown",
        "ReconcileDelivery",
        "ResultUnknown",
        "idempotency_key",
        "spawn_from_packet",
        "record_company_run_observation",
        "ScriptedModel",
        "with_harness_and_project_authority",
        "RequestEnvelope::company_command",
        "fake_model_coding_project_produces_closing_receipt",
    ] {
        assert!(
            domain.contains(marker)
                || replay.contains(marker)
                || closeout.contains(marker)
                || business.contains(marker)
                || company.contains(marker)
                || collaboration.contains(marker)
                || daemon.contains(marker)
                || model.contains(marker)
                || fixture.contains(marker),
            "company golden-loop marker missing: {marker}"
        );
    }

    assert!(company.contains("spawn_from_packet(run_context, packet, sandbox.clone())"));
    assert!(company.contains("record_company_run_observation(&context, packet_id)"));
    assert!(fixture.contains("ScriptedModel::from_json(&builder_outputs())"));
    assert!(fixture.contains("fs::read_to_string(root.join(\"OUTPUT.txt\"))"));
    assert!(fixture.contains("closing.output[\"state\"][\"closing_receipts\"]"));
    assert!(closeout.contains("BusinessCloseKind::Success"));
    assert!(closeout.contains("BusinessCloseKind::Failure"));
    assert!(closeout.contains("BusinessCloseKind::Cancelled"));
    assert!(closeout.contains("BusinessCloseKind::Waived"));
    assert!(domain.contains("CompanyCommand::MarkDeliveryUnknown"));
    assert!(domain.contains("CompanyCommand::ReconcileDelivery"));
    assert!(!business.contains("CapabilityBroker"));
    assert!(!business.contains("ModelClient"));
    assert!(!closeout.contains("CapabilityBroker"));
    assert!(!closeout.contains("ModelClient"));
}

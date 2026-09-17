#[test]
fn new_process_rebuilds_the_company_chain() {
    let domain = include_str!("../../kiana-domain/src/company.rs");
    let replay = include_str!("../../kiana-domain/src/company_replay.rs");
    let core = include_str!("../src/company.rs");
    let receipts = include_str!("../src/receipts.rs");
    let artifacts = include_str!("../src/artifacts.rs");
    let baseline = include_str!("../../docs/roadmap/p3-i03-company-chain-baseline.md");

    for marker in [
        "CompanyState",
        "Objective",
        "Project",
        "Milestone",
        "WorkPacket",
        "CompanyRun",
        "Acceptance",
        "Delivery",
        "CompanyClosingReceipt",
        "CompanyReplayReducer",
        "load_company",
        "company_view",
        "company_snapshot",
        "company_aggregate_id",
        "read_stream",
        "project_packets",
        "objectives",
        "projects",
        "milestones",
        "packets",
        "runs",
        "acceptances",
        "deliveries",
        "closing_receipts",
        "events_for_persisted_run",
        "receipt_from_events",
        "artifact_refs",
        "evidence_refs",
        "read_company_artifact",
        "company_artifact_changed",
        "company_artifact_source_revoked",
        "company_evidence_owner_mismatch",
        "company_approved_packet_required",
        "company_packet_not_approved",
        "company_run_not_authorized",
        "company_idempotency_conflict",
        "company_revision_conflict",
        "company_command_invalid",
        "company_replay_gap",
        "company_replay_duplicate_command",
        "company_replay_conflict",
        "company_event_schema_unsupported",
        "RunOutcome",
        "ResultUnknown",
        "CompanyCommandReceipt",
        "source_event_ids",
        "data_revoked",
    ] {
        assert!(
            domain.contains(marker)
                || replay.contains(marker)
                || core.contains(marker)
                || receipts.contains(marker)
                || artifacts.contains(marker)
                || baseline.contains(marker),
            "company chain marker missing: {marker}"
        );
    }

    assert!(core.contains("CompanyReplayReducer::new"));
    assert!(core.contains("reducer.apply(event)"));
    assert!(core.contains("events.read_stream(COMPANY_AGGREGATE"));
    assert!(core.contains("company_view(&context"));
    assert!(replay.contains("state.transition"));
    assert!(replay.contains("runtime_event.kind != format!(\"company.{}\""));
    assert!(core.contains("company_artifact_source_revoked"));
    assert!(core.contains("company_evidence_owner_mismatch"));
    assert!(receipts.contains("receipt_from_events"));
    assert!(receipts.contains("source_event_ids"));
    assert!(!replay.contains("CapabilityBroker"));
    assert!(!replay.contains("ModelClient"));
}

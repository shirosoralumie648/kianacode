#[test]
fn receipt_artifact_and_evidence_cross_locate() {
    let protocol = include_str!("../../kiana-protocol/src/ui_contracts.rs");
    let receipts = include_str!("../src/receipts.rs");
    let invocations = include_str!("../src/invocation_projection.rs");
    let artifacts = include_str!("../src/artifacts.rs");
    let company = include_str!("../src/company_business.rs");
    let daemon = include_str!("../../kiana-daemon/src/lib.rs");
    let web = include_str!("../../kiana-entrypoints/src/web.rs");
    let thread = include_str!("../../kiana-entrypoints/src/web_thread.rs");
    let workbench = include_str!("../../kiana-entrypoints/src/workbench_chat.rs");
    let cli = include_str!("../../kiana-entrypoints/src/cli.rs");
    let baseline = include_str!("../../docs/roadmap/p2-m4-01-run-artifact-detail-baseline.md");

    for marker in [
        "TurnView",
        "ItemView",
        "commandExecution",
        "fileChange",
        "agentMessage",
        "run_id",
        "event_id",
        "invocation_id",
        "invocations",
        "execution_receipts",
        "receipt_from_events",
        "aggregate_receipt_facts",
        "evidence_refs",
        "evidence_ref_digests",
        "artifact_refs",
        "read_project_artifact",
        "files_changed",
        "source_event_ids",
        "receipt",
        "redact_event_value",
        "run_owner_mismatch",
        "diff",
        "receipt_artifact_and_evidence_cross_locate",
    ] {
        assert!(
            protocol.contains(marker)
                || receipts.contains(marker)
                || invocations.contains(marker)
                || artifacts.contains(marker)
                || company.contains(marker)
                || daemon.contains(marker)
                || web.contains(marker)
                || thread.contains(marker)
                || workbench.contains(marker)
                || cli.contains(marker)
                || baseline.contains(marker),
            "run detail marker missing: {marker}"
        );
    }

    assert!(receipts.contains("\"invocations\":invocations"));
    assert!(receipts.contains("\"execution_receipts\": typed_execution_receipts"));
    assert!(receipts.contains("aggregate_receipt_facts"));
    assert!(receipts.contains("evidence_ref_digests"));
    assert!(receipts.contains("source_event_ids"));
    assert!(web.contains("/api/receipt"));
    assert!(web.contains("read_receipt"));
    assert!(thread.contains("items_from_turn"));
    assert!(thread.contains("fileChange"));
    assert!(thread.contains("commandExecution"));
    assert!(daemon.contains("run_owner_mismatch"));
    assert!(!receipts.contains("CapabilityBroker"));
    assert!(!thread.contains("ModelClient"));
}

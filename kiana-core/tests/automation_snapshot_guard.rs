#[test]
fn aut22_snapshot_is_read_only_and_does_not_consume_scheduler_or_receipt_authority() {
    let domain = include_str!("../../kiana-domain/src/automation_snapshot.rs");
    let core = include_str!("../src/automation_snapshot.rs");
    let automation = include_str!("../src/automation.rs");
    let queue = include_str!("../src/workflow_queue.rs");
    let receipts = include_str!("../src/receipts.rs");
    let incident = include_str!("../src/incident_projection.rs");
    for marker in [
        "AutomationSnapshot",
        "AutomationWorkflowView",
        "AutomationTriggerView",
        "AutomationReceiptView",
        "AutomationIncidentView",
        "Due",
        "Blocked",
        "Unknown",
        "business_outcome_confirmed",
        "reconcile_required",
        "source_cursor",
        "limitations",
        "Trigger",
        "Receipt",
        "Incident",
        "validate_automation_snapshot",
    ] {
        assert!(
            domain.contains(marker)
                || core.contains(marker)
                || automation.contains(marker)
                || queue.contains(marker)
                || receipts.contains(marker)
                || incident.contains(marker),
            "AUT-22 marker missing: {marker}"
        );
    }
    for forbidden in [
        "CapabilityBroker::new",
        "ModelClient::new",
        "consume_claim",
        "approve_automation",
        "std::process::Command",
    ] {
        assert!(
            !domain.contains(forbidden) && !core.contains(forbidden),
            "AUT-22 query/effect bypass marker present: {forbidden}"
        );
    }
}

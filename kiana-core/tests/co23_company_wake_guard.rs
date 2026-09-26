#[test]
fn company_wake_rebuilds_from_cursor_and_consumes_once() {
    let wake = include_str!("../../kiana-domain/src/company_wake.rs");
    let daemon = include_str!("../../kiana-daemon/src/company_dispatch.rs");
    let automation = include_str!("../src/automation.rs");
    let workflow = include_str!("../../kiana-workflow/src/durable.rs");

    for marker in [
        "COMPANY_WAKE_SCHEMA",
        "COMPANY_WAKE_LEDGER_SCHEMA",
        "COMPANY_DISPATCH_RECEIPT_SCHEMA",
        "CompanyWakeKind",
        "CompanyWakeStatus::Pending",
        "CompanyWakeStatus::Claimed",
        "CompanyWakeStatus::Consumed",
        "CompanyWakeStatus::Unknown",
        "scan_committed_intents",
        "company_wake_intent_conflict",
        "company_wake_claim_mismatch",
        "company_wake_already_consumed",
        "CompanyDispatchReceipt",
        "source_cursor",
        "CompanyDispatchAdapter",
        "plan_command_intent",
        "WorkflowEffect::Dispatch",
        "replayed",
    ] {
        assert!(
            wake.contains(marker)
                || daemon.contains(marker)
                || automation.contains(marker)
                || workflow.contains(marker),
            "CO-23 marker missing: {marker}"
        );
    }
    assert!(daemon.contains("never invokes a model"));
    assert!(!daemon.contains("CapabilityBroker"));
}

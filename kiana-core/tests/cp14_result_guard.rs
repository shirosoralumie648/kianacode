#[test]
fn cp14_result_finalizer_keeps_effect_unknown_and_delivery_after_commit() {
    let domain = include_str!("../../kiana-domain/src/capabilities.rs");
    let core = include_str!("../src/capabilities.rs");
    let dispatch = include_str!("../src/dispatch.rs");
    let events = include_str!("../src/events.rs");
    let ports = include_str!("../../kiana-ports/src/lib.rs");
    for marker in [
        "CapabilityResultReceipt",
        "CAPABILITY_RESULT_RECEIPT_SCHEMA",
        "effect_started",
        "effect_known",
        "zero_effect",
        "stop_confirmed",
        "result_unknown",
        "result_receipt",
        "execution.result_committed",
        "result.delivery_claimed",
        "finish_cell_capability",
        "CapabilityOutcome::Unknown",
    ] {
        assert!(
            domain.contains(marker)
                || core.contains(marker)
                || dispatch.contains(marker)
                || events.contains(marker)
                || ports.contains(marker),
            "CP-14 marker missing: {marker}"
        );
    }
    assert!(core.contains("result_event_persistence_failed"));
    assert!(core.contains("cell_capability_settlement_failed"));
    assert!(dispatch.contains("result_unknown:result_commit_failed"));
    assert!(ports.contains("CapabilityOutcome::Unknown"));
    for forbidden in [
        "result_unknown_is_success",
        "refund_unknown_usage",
        "reexecute_result",
    ] {
        assert!(
            !domain.contains(forbidden) && !core.contains(forbidden),
            "result finalizer must not enable {forbidden}"
        );
    }
}

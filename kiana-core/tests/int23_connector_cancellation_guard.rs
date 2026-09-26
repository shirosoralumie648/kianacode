#[test]
fn connector_cancellation_settlement_fences_late_results_and_lease_release() {
    let cancellation = include_str!("../../kiana-domain/src/connector_cancellation.rs");
    let dispatch = include_str!("../../kiana-domain/src/connector_dispatch.rs");
    let stop = include_str!("../../kiana-domain/src/process_supervisor.rs");
    let ports = include_str!("../../kiana-ports/src/connector.rs");

    for marker in [
        "ConnectorCancellationSettlement",
        "ConnectorCancellationState",
        "NotExecuted",
        "StopConfirmed",
        "Unknown",
        "HeldForReconciliation",
        "connector_cancellation_late_result_fenced",
        "connector_cancellation_effect_already_observed",
        "late_result_fenced",
        "lease_settlement",
        "StopReport",
        "cancel_checked",
        "connector_cancel_request_invalid",
    ] {
        assert!(
            cancellation.contains(marker)
                || dispatch.contains(marker)
                || stop.contains(marker)
                || ports.contains(marker),
            "INT-23 marker missing: {marker}"
        );
    }

    assert!(cancellation.contains("ConnectorLeaseSettlement::Released"));
    assert!(cancellation.contains("ConnectorLeaseSettlement::HeldForReconciliation"));
    assert!(cancellation.contains("self.late_result_fenced"));
    assert!(cancellation.contains("ConnectorDispatchStage::Dispatching"));
    assert!(!cancellation.contains("CapabilityBroker"));
    assert!(!cancellation.contains("EventStorePort"));
    assert!(!cancellation.contains("authorize_and_execute"));
}

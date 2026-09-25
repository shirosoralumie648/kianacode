#[test]
fn notification_parity_is_read_only_and_reuses_one_source_contract() {
    let source = include_str!("../src/notification_parity.rs");
    let domain = include_str!("../../kiana-domain/src/notification_parity.rs");
    for marker in [
        "compare_notification_entrypoints",
        "NotificationEntrypointSnapshot",
        "NotificationEntrypointParity",
        "source_cursor",
        "authority_epoch",
        "fresh_process_rebuilt",
        "query_original_source_and_rebuild_snapshot",
        "no_entrypoint_drift",
    ] {
        assert!(
            source.contains(marker) || domain.contains(marker),
            "NM-21 marker missing: {marker}"
        );
    }
    for forbidden in [
        "RunStreamBus",
        "NotificationDeliveryWorker",
        "CapabilityBroker",
        "KianaHarness",
        "tokio::spawn",
        "send(",
        "run_envelope_on_host",
        "cancel_envelope_on_host",
        "approve",
        "retry",
        "close",
    ] {
        assert!(
            !source.contains(forbidden),
            "NM-21 parity authority widened: {forbidden}"
        );
        assert!(
            !domain.contains(forbidden),
            "NM-21 domain authority widened: {forbidden}"
        );
    }
}

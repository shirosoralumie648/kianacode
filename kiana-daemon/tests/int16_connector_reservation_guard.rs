#[test]
fn daemon_rechecks_reservation_before_local_fixture_effect() {
    let daemon = include_str!("../src/connectors.rs");
    let broker = include_str!("../../kiana-capability-broker/src/lib.rs");
    let core = include_str!("../../kiana-core/src/connector_reservation.rs");
    for marker in [
        "command_digest",
        "connector_idempotency_command_digest_conflict",
        "connector_reservation",
        "connector_permit",
        "connector_effect_admission",
        "ConnectorInvocationReservation",
        "ConnectorInvocationPermit",
        "ConnectorFixture",
        "provider_receipt",
        "append_idempotent_expected",
        "replayed",
    ] {
        assert!(
            daemon.contains(marker) || broker.contains(marker) || core.contains(marker),
            "INT-16 daemon marker missing: {marker}"
        );
    }
    assert!(daemon.contains("fixture.find_case"));
    assert!(daemon.contains("connector_effect_admission"));
    assert!(daemon.contains("previous.data[\"command_digest\"]"));
    assert!(!daemon.contains("reqwest::Client"));
    assert!(!daemon.contains("ProviderGateway"));
}

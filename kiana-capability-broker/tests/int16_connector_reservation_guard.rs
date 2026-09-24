#[test]
fn broker_consumes_only_server_owned_connector_reservations() {
    let broker = include_str!("../src/lib.rs");
    let domain = include_str!("../../kiana-domain/src/connector_reservation.rs");
    for marker in [
        "validate_connector_effect_boundary",
        "connector_effect_admission",
        "ConnectorInvocationReservation",
        "ConnectorInvocationPermit",
        "connector_reservation_invalid",
        "connector_permit_required",
        "connector_reservation_not_committed",
        "connector_permit_expired",
        "ExecutionPermitVerifierPort",
        "verify_and_consume",
    ] {
        assert!(
            broker.contains(marker) || domain.contains(marker),
            "INT-16 Broker marker missing: {marker}"
        );
    }
    assert!(broker.contains("validate_connector_effect_boundary(&request"));
    assert!(broker.contains("self.permit_verifier"));
    assert!(!broker.contains("EventStore::append"));
    assert!(!broker.contains("tokio::spawn(connector"));
}

#[test]
fn connector_ingress_verifier_stops_at_authentication_and_dedupe() {
    let domain = include_str!("../../kiana-domain/src/connector_ingress.rs");
    let daemon = include_str!("../src/connector_ingress.rs");
    for marker in [
        "ConnectorIngressProtocol",
        "tenant_id",
        "nonce",
        "signature_digest",
        "connector_ingress_dedupe_payload_conflict",
        "ConnectorIngressVerifier",
        "hmac::verify",
    ] {
        assert!(
            domain.contains(marker) || daemon.contains(marker),
            "INT-24 marker missing: {marker}"
        );
    }
    for forbidden in [
        "CapabilityBroker::new",
        "ModelClient::new",
        "std::process::Command",
        "tokio::spawn",
        "EventStore::append",
        "ConnectorAdapter::invoke",
        "auto_success",
    ] {
        assert!(
            !domain.contains(forbidden) && !daemon.contains(forbidden),
            "INT-24 ingress widened effect boundary: {forbidden}"
        );
    }
}

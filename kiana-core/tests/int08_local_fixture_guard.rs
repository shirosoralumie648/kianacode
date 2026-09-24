#[test]
fn local_fixture_is_bounded_deterministic_and_stays_on_the_control_plane_path() {
    let domain = include_str!("../../kiana-domain/src/connector_fixture.rs");
    let connectors = include_str!("../../kiana-domain/src/connectors.rs");
    let core = include_str!("../src/connectors.rs");
    let daemon = include_str!("../../kiana-daemon/src/connectors.rs");

    for marker in [
        "CONNECTOR_FIXTURE_SCHEMA",
        "CONNECTOR_FIXTURE_MAX_BYTES",
        "CONNECTOR_FIXTURE_MAX_OPERATIONS",
        "CONNECTOR_FIXTURE_MAX_CASES_PER_OPERATION",
        "ConnectorFixtureCase",
        "connector_fixture_hash_matches",
        "connector_fixture_payload_duplicate",
        "connector_fixture_external_effect_denied",
        "connector_fixture_operation_missing",
        "connector_fixture_payload_mismatch",
        "connector_payload_sha256",
        "provider_receipt",
        "external_effect_performed",
        "\"bind\" =>",
        "\"invoke\" =>",
        "\"reconcile\" =>",
        "previous.data[\"output\"]",
        "replayed",
        "connector.invoked",
        "connector.reconciled",
        "append_idempotent_expected",
        "authorize_and_execute",
    ] {
        assert!(
            domain.contains(marker)
                || connectors.contains(marker)
                || core.contains(marker)
                || daemon.contains(marker),
            "INT-08 marker missing: {marker}"
        );
    }

    assert!(daemon.contains("ConnectorFixture::from_bytes"));
    assert!(daemon.contains("validate_for_binding"));
    assert!(daemon.contains(".provider_receipt"));
    assert!(daemon.contains("replayed"));
    assert!(daemon.contains("connector_fixture_hash_matches"));
    assert!(!daemon.contains("reqwest::Client"));
    assert!(!daemon.contains("ProviderGateway"));
    assert!(!daemon.contains("external_effect: true"));
    assert!(!domain.contains("EventStorePort"));
    assert!(!domain.contains("CapabilityBroker"));
}

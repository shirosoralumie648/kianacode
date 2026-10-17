#[test]
fn local_fixture_is_bounded_deterministic_and_stays_on_the_control_plane_path() {
    let domain = include_str!("../../kiana-domain/src/connector_fixture.rs");
    let domain_fixtures = include_str!("../../kiana-domain/tests/int08_local_fixture.rs");
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
        // INT-08 deny vocabulary: bounds, replacement, unknown operation and duplicate payload.
        "connector_fixture_size_exceeded",
        "connector_fixture_operation_limit",
        "connector_fixture_case_limit",
        "connector_fixture_schema_unsupported",
        "connector_fixture_identity_mismatch",
        "connector_fixture_operation_unregistered",
        "connector_fixture_hash_mismatch",
        "read_project_file",
    ] {
        assert!(
            domain.contains(marker)
                || connectors.contains(marker)
                || core.contains(marker)
                || daemon.contains(marker),
            "INT-08 marker missing: {marker}"
        );
    }

    // The acceptance fixtures must keep pinning the deny matrix and the deterministic receipt
    // contract; removing one of these obligations has to break this guard.
    for fixture_marker in [
        "fixture_schema_bounds_and_unknown_fields_fail_closed",
        "fixture_replacement_and_hash_spelling_are_pinned",
        "fixture_receipts_are_byte_stable_for_every_outcome",
        "duplicate_payload_and_external_effect_are_rejected",
        "connector_fixture_payload_duplicate",
        "connector_fixture_external_effect_denied",
        "connector_fixture_operation_missing",
        "connector_fixture_operation_unregistered",
        "connector_fixture_hash_matches",
    ] {
        assert!(
            domain_fixtures.contains(fixture_marker),
            "INT-08 fixture obligation missing: {fixture_marker}"
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

    // The fixture contract is local-only: no transport, no credential or event authority and no
    // unsafe code may enter the module that decides what a fixture may claim.
    for forbidden in [
        "reqwest",
        "TcpStream",
        "std::net",
        "ProviderGateway",
        "SecretRef",
        "EventStorePort",
        "CapabilityBroker",
        "unsafe",
    ] {
        assert!(
            !domain.contains(forbidden),
            "INT-08 fixture module must not contain: {forbidden}"
        );
    }

    // A fixture can never assert a physical effect, and the daemon may only reach fixture bytes
    // through the hash-checked project file reader.
    assert!(!domain.contains("external_effect = true"));
    assert!(!domain.contains("external_effect: true"));
    assert!(daemon.contains("connector_fixture_hash_mismatch"));
    assert!(daemon.contains("CONNECTOR_FIXTURE_MAX_BYTES"));
}

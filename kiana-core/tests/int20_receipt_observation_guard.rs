#[test]
fn receipt_observation_contract_is_shared_and_secret_free() {
    let domain = include_str!("../../kiana-domain/src/connectors.rs");
    let observation = include_str!("../../kiana-domain/src/effect_observation.rs");
    let ports = include_str!("../../kiana-ports/src/connector.rs");
    let fixture = include_str!("../../kiana-domain/src/connector_fixture.rs");
    let daemon = include_str!("../../kiana-daemon/src/connectors.rs");

    for marker in [
        "PROVIDER_RECEIPT_SCHEMA",
        "pub fn validate(&self)",
        "provider_payload_hash_valid",
        "provider_receipt_raw_response_forbidden",
        "payload_sha256",
        "validate_for_receipt",
        "effect_observation_receipt_binding_mismatch",
        "EffectObservationState::ConfirmedSuccess",
        "EffectObservationState::ConfirmedFailure",
        "EffectObservationState::Unknown",
        "validate_receipt_for_permit",
        "scan_secret_value",
        "receipt.validate()",
    ] {
        assert!(
            domain.contains(marker)
                || observation.contains(marker)
                || ports.contains(marker)
                || fixture.contains(marker)
                || daemon.contains(marker),
            "INT-20 marker missing: {marker}"
        );
    }

    assert!(observation.contains("#[serde(deny_unknown_fields)]"));
    assert!(domain.contains("final_payload_sha256"));
    assert!(observation.contains("owner_digest"));
    assert!(observation.contains("audience_digest"));
    assert!(ports.contains("receipt.validate()"));
    assert!(daemon.contains("validate_for_receipt"));
    assert!(!ports.contains("raw_provider_response"));
    assert!(!domain.contains("raw_provider_response"));
    assert!(!domain.contains("raw_secret"));
    assert!(!domain.contains("EventStorePort"));
}

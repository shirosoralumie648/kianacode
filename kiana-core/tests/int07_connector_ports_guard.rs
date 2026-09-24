#[test]
fn connector_ports_are_narrow_and_deny_missing_capabilities() {
    let ports = include_str!("../../kiana-ports/src/connector.rs");
    for marker in [
        "pub trait ConnectorAdapter",
        "pub trait EffectObserver",
        "pub trait CredentialProbe",
        "pub trait WebhookVerifier",
        "ConnectorPreparedPermit",
        "CredentialLease",
        "CanonicalConnectorPayload",
        "validate_receipt_for_permit",
        "connector_capability_missing",
        "connector_invoke_unsupported",
        "connector_effect_observation_unsupported",
        "connector_credential_probe_unsupported",
        "connector_webhook_verification_unsupported",
    ] {
        assert!(
            ports.contains(marker),
            "INT-07 port marker missing: {marker}"
        );
    }

    for forbidden in [
        "EventStorePort",
        "ApprovalStorePort",
        "RuntimeEvent",
        "raw_secret",
        "secret_value",
    ] {
        assert!(
            !ports.contains(forbidden),
            "INT-07 port must not depend on authority or raw material: {forbidden}"
        );
    }

    assert!(ports.contains("pub struct EffectObservationRequest"));
    assert!(ports.contains("pub struct CredentialProbeResult"));
    assert!(ports.contains("pub struct VerifiedWebhook"));
    assert!(ports.contains("WorkflowEventOccurrence"));
}

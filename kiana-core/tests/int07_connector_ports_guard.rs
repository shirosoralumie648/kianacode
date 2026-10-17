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
        "connector_prepared_permit_invalid_or_expired",
        "connector_prepared_permit_binding_mismatch",
        "connector_provider_receipt_binding_invalid",
        "connector_probe_credential_binding_mismatch",
        "connector_observation_attempt_mismatch",
        "connector_webhook_occurrence_binding_mismatch",
        "fn invoke_checked",
        "fn cancel_checked",
        "fn observe_checked",
        "fn probe_checked",
        "fn verify_checked",
        "CONNECTOR_PAYLOAD_MAX_BYTES",
        "CONNECTOR_LIMITATIONS_MAX",
    ] {
        assert!(
            ports.contains(marker),
            "INT-07 port marker missing: {marker}"
        );
    }

    // The ports must not gain an authority channel: no EventStore/approval handle, no EventLog
    // append, no dispatch-lifecycle ownership and no raw-secret field name of its own.
    for forbidden in [
        "EventStorePort",
        "ApprovalStorePort",
        "ApprovalRequirement",
        "RuntimeEvent",
        "EventLog",
        "ConnectorDispatchLifecycle",
        "raw_secret",
        "secret_value",
        "#[ignore]",
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

#[test]
fn connector_port_deny_fixtures_and_lane_are_wired() {
    let fixture = include_str!("../../kiana-ports/tests/int07_connector_ports.rs");
    for marker in [
        "fake_adapter_covers_known_unknown_and_stop_results",
        "probe_and_observer_return_redacted_health_and_effect_projection",
        "webhook_verifier_returns_occurrence_without_payload_or_secret",
        "every_checked_port_entry_point_denies_a_missing_capability_before_calling_the_implementation",
        "registered_capabilities_are_not_authority_over_the_prepared_permit",
        "ports_reject_receipts_and_observations_not_bound_to_the_permit",
        "port_results_have_no_raw_secret_channel_and_reject_unknown_fields",
        "descriptor_and_capability_registration_are_pinned_and_default_to_deny",
        "connector_capability_missing:discover",
        "connector_capability_missing:read_only_probe",
        "connector_capability_missing:invoke",
        "connector_capability_missing:cancel",
        "connector_capability_missing:capability_handshake",
        "connector_capability_missing:observe_receipt",
        "connector_capability_missing:webhook_verify",
        "connector_prepared_permit_binding_mismatch",
        "connector_probe_credential_binding_mismatch",
        "connector_provider_receipt_binding_invalid",
        "effect_observation_receipt_binding_mismatch",
        "connector_observation_attempt_mismatch",
        "INT07_RAW_SECRET_SENTINEL",
        "SealedAdapter",
        "PermissiveAdapter",
        "ReceiptAdapter",
        "ForeignReceiptObserver",
        "LateAttemptObserver",
    ] {
        assert!(
            fixture.contains(marker),
            "INT-07 fixture marker missing: {marker}"
        );
    }
    assert!(
        !fixture.contains("#[ignore]"),
        "INT-07 fixtures must not be ignored"
    );
    assert!(
        fixture.contains("load(Ordering::SeqCst), 0"),
        "INT-07 fixtures must prove the adapter implementation was never called"
    );

    let workflow = include_str!("../../.github/workflows/int07-connector-ports.yml");
    for marker in [
        "workflow_dispatch:",
        "permissions:",
        "cargo fetch --locked",
        "cargo fmt --all --check",
        "cargo test -p kiana-ports --test int07_connector_ports --locked -- --test-threads=1",
        "cargo test -p kiana-core --test int07_connector_ports_guard --locked -- --test-threads=1",
        "cargo test -p kiana-ports --test int07_connector_ports --no-run --locked",
        "cargo test -p kiana-core --test int07_connector_ports_guard --no-run --locked",
    ] {
        assert!(
            workflow.contains(marker),
            "INT-07 lane marker missing: {marker}"
        );
    }
    for forbidden in ["continue-on-error", "#[ignore]"] {
        assert!(
            !workflow.contains(forbidden),
            "INT-07 lane must not contain {forbidden}"
        );
    }
}

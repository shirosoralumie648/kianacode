//! INT-18 Broker guard: the effect fence must be checked at the effect boundary, before the
//! one-shot execution permit and the connector handler, and its fixtures must stay wired.
//! `include_str!` assertions are source-level evidence only; they are not runtime receipts.

const BROKER_SOURCE: &str = include_str!("../src/lib.rs");
const DOMAIN_SOURCE: &str = include_str!("../../kiana-domain/src/connector_effect_permit.rs");
const BROKER_FIXTURES: &str = include_str!("int18_effect_permit.rs");

#[test]
fn broker_rechecks_effect_permit_before_execution_permit_and_handler() {
    for marker in [
        "connector_effect_permit_required",
        "connector_effect_permit_invalid",
        "ConnectorEffectPermit",
        "ConnectorEffectFence",
        "connector_configuration_epoch",
        "connector_policy_epoch",
        "connector_credential_epoch",
        "execution_scope",
        "validate_for_effect",
        "connector_effect_scope_required",
        "connector_effect_scope_digest",
        "connector_effect_permit_expired",
        "connector_effect_binding_revoked",
        "connector_effect_binding_snapshot_stale",
        "connector_effect_authority_epoch_stale",
        "connector_effect_data_epoch_stale",
        "connector_binding_snapshot_required",
        "connector_configuration_epoch_required",
        "connector_policy_epoch_required",
        "verify_and_consume",
    ] {
        assert!(
            BROKER_SOURCE.contains(marker) || DOMAIN_SOURCE.contains(marker),
            "INT-18 Broker marker missing: {marker}"
        );
    }
    let fence = BROKER_SOURCE
        .find("validate_connector_effect_boundary(&request, connector_now_unix_ms()?)?;")
        .expect("effect fence");
    let verifier = BROKER_SOURCE
        .find("verify_and_consume")
        .expect("execution permit");
    assert!(
        fence < verifier,
        "effect fence must precede the one-shot execution permit"
    );
    assert!(!BROKER_SOURCE.contains("EventStore::append"));
    assert!(!BROKER_SOURCE.contains("tokio::spawn(connector"));
}

#[test]
fn broker_effect_permit_fixtures_keep_the_handler_and_verifier_at_zero_calls() {
    for fixture in [
        "current_effect_permit_reaches_the_handler_exactly_once",
        "stale_revoked_rotated_and_expired_effect_material_never_reaches_the_handler",
    ] {
        assert!(
            BROKER_FIXTURES.contains(fixture),
            "INT-18 Broker fixture missing: {fixture}"
        );
    }
    // The deny matrix drives the real sealed Broker with a recording handler and a recording
    // one-shot permit verifier, and asserts that both stay at zero calls.
    for marker in [
        "RecordingHandler",
        "RecordingVerifier",
        "ExecutionPermitVerifierPort",
        "handler_calls.load(Ordering::SeqCst),",
        "verifier_calls.load(Ordering::SeqCst),",
        "sealed_broker_for",
        "connector_effect_permit_required",
        "connector_effect_authority_epoch_stale",
        "connector_effect_configuration_epoch_stale",
        "connector_effect_policy_epoch_stale",
        "connector_effect_credential_epoch_stale",
        "connector_effect_data_epoch_stale",
        "connector_effect_permit_expired",
        "connector_effect_permit_reservation_mismatch",
        "connector_binding_revoked",
        "connector_effect_binding_revision_stale",
        "connector_effect_binding_snapshot_stale",
        "connector_effect_scope_digest_mismatch",
        "connector_binding_snapshot_required",
        "connector_configuration_epoch_required",
        "connector_policy_epoch_required",
    ] {
        assert!(
            BROKER_FIXTURES.contains(marker),
            "INT-18 Broker fixture marker missing: {marker}"
        );
    }
    assert!(!BROKER_FIXTURES.contains("#[ignore"));
    assert!(!BROKER_FIXTURES.contains("ignore ="));
}

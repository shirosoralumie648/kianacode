//! INT-18 daemon guard: the local fixture adapter must revalidate the effect fence before any
//! fixture bytes are read, refuse every wire-supplied permit/fence envelope and keep its fixtures
//! wired.  `include_str!` assertions are source-level evidence only; they are not runtime receipts.

const DAEMON_SOURCE: &str = include_str!("../src/connectors.rs");
const DOMAIN_SOURCE: &str = include_str!("../../kiana-domain/src/connector_effect_permit.rs");
const DAEMON_FIXTURES: &str = include_str!("int18_effect_permit.rs");

#[test]
fn daemon_rechecks_effect_permit_before_fixture_adapter_effect() {
    for marker in [
        "connector_effect_permit_required",
        "connector_effect_permit_invalid",
        "ConnectorEffectPermit",
        "ConnectorEffectFence",
        "connector_configuration_epoch",
        "connector_policy_epoch",
        "connector_credential_epoch",
        "validate_for_effect",
        "connector_effect_scope_required",
        "connector_effect_binding_revoked",
        "connector_effect_binding_snapshot_stale",
        "load_fixture",
        "provider_receipt",
    ] {
        assert!(
            DAEMON_SOURCE.contains(marker) || DOMAIN_SOURCE.contains(marker),
            "INT-18 daemon marker missing: {marker}"
        );
    }
    let fence = DAEMON_SOURCE
        .find("validate_for_effect(&reservation, &snapshot, &current, now)")
        .expect("effect fence");
    let fixture = DAEMON_SOURCE
        .rfind("let fixture = load_fixture")
        .expect("fixture adapter");
    assert!(
        fence < fixture,
        "effect fence must precede fixture adapter read"
    );
    assert!(!DAEMON_SOURCE.contains("reqwest::Client"));
    assert!(!DAEMON_SOURCE.contains("ProviderGateway"));
}

#[test]
fn daemon_effect_permit_fixtures_keep_the_wire_envelope_server_owned() {
    for fixture in [
        "wire_supplied_effect_permit_is_refused_before_any_adapter_effect",
        "effect_permit_and_fence_material_is_never_persisted_or_echoed",
    ] {
        assert!(
            DAEMON_FIXTURES.contains(fixture),
            "INT-18 daemon fixture missing: {fixture}"
        );
    }
    for marker in [
        "DaemonHost",
        "connector_arguments_invalid",
        "connector_effect_permit",
        "connector_effect_fence",
        "connector_configuration_epoch",
        "connector_policy_epoch",
        "connector_credential_epoch",
        "kiana.connector-effect-permit.v1",
        "kiana.connector-effect-fence.v1",
        "dispatch_lifecycle",
        "CONNECTOR_STREAM",
    ] {
        assert!(
            DAEMON_FIXTURES.contains(marker),
            "INT-18 daemon fixture marker missing: {marker}"
        );
    }
    assert!(!DAEMON_FIXTURES.contains("#[ignore"));
    assert!(!DAEMON_FIXTURES.contains("ignore ="));
}

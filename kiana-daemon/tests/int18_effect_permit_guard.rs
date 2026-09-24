#[test]
fn daemon_rechecks_effect_permit_before_fixture_adapter_effect() {
    let daemon = include_str!("../src/connectors.rs");
    let domain = include_str!("../../kiana-domain/src/connector_effect_permit.rs");
    for marker in [
        "connector_effect_permit_required",
        "ConnectorEffectPermit",
        "ConnectorEffectFence",
        "connector_configuration_epoch",
        "connector_policy_epoch",
        "connector_credential_epoch",
        "validate_for_effect",
        "connector_effect_binding_revoked",
        "connector_effect_binding_snapshot_stale",
        "load_fixture",
        "provider_receipt",
    ] {
        assert!(
            daemon.contains(marker) || domain.contains(marker),
            "INT-18 daemon marker missing: {marker}"
        );
    }
    let fence = daemon.find("validate_for_effect").expect("effect fence");
    let fixture = daemon
        .rfind("let fixture = load_fixture")
        .expect("fixture adapter");
    assert!(
        fence < fixture,
        "effect fence must precede fixture adapter read"
    );
    assert!(!daemon.contains("reqwest::Client"));
    assert!(!daemon.contains("ProviderGateway"));
}

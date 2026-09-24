#[test]
fn broker_rechecks_effect_permit_before_execution_permit_and_handler() {
    let broker = include_str!("../src/lib.rs");
    let domain = include_str!("../../kiana-domain/src/connector_effect_permit.rs");
    for marker in [
        "connector_effect_permit_required",
        "ConnectorEffectPermit",
        "ConnectorEffectFence",
        "connector_configuration_epoch",
        "connector_policy_epoch",
        "connector_credential_epoch",
        "execution_scope",
        "validate_for_effect",
        "connector_effect_scope_required",
        "connector_effect_permit_invalid",
        "connector_effect_authority_epoch_stale",
        "connector_effect_data_epoch_stale",
        "connector_effect_permit_expired",
        "verify_and_consume",
    ] {
        assert!(
            broker.contains(marker) || domain.contains(marker),
            "INT-18 Broker marker missing: {marker}"
        );
    }
    let fence = broker.find("validate_for_effect").expect("effect fence");
    let verifier = broker.find("verify_and_consume").expect("execution permit");
    assert!(
        fence < verifier,
        "effect fence must precede adapter execution permit"
    );
    assert!(!broker.contains("EventStore::append"));
    assert!(!broker.contains("tokio::spawn(connector"));
}

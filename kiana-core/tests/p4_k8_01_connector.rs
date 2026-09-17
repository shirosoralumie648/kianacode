#[test]
fn connector_cannot_bypass_the_control_plane() {
    let domain = include_str!("../../kiana-domain/src/connectors.rs");
    let actions = include_str!("../../kiana-domain/src/actions.rs");
    let core = include_str!("../src/connectors.rs");
    let commands = include_str!("../src/commands.rs");
    let daemon = include_str!("../../kiana-daemon/src/connectors.rs");
    let effect = include_str!("../../kiana-domain/src/effect_observation.rs");
    let fixture = include_str!("../../kiana-domain/tests/p4_k8_01_connector.rs");

    for marker in [
        "ConnectorDefinition",
        "ConnectorOperation",
        "AccountBinding",
        "ConnectorBindingSnapshot",
        "ProviderReceipt",
        "ConnectorEffect::ReadOnly",
        "ConnectorEffect::Write",
        "connector_invocation_risk",
        "CONNECTOR_MANAGE_OPERATION",
        "CONNECTOR_INVOKE_OPERATION",
        "handle_connector_command",
        "normalize_connector_command",
        "authorize_and_execute",
        "operator_authorized",
        "binding_snapshot",
        "binding_authorized",
        "connector_operator_required",
        "project_untrusted",
        "connector_binding_scope_mismatch",
        "connector_binding_snapshot_changed",
        "connector_final_payload_approval_required",
        "connector_idempotency_payload_mismatch",
        "connector.invoked",
        "connector.reconciled",
        "connector_fixture_hash_mismatch",
        "external_effect_performed",
        "result_unknown",
        "EffectObservation",
        "connector_cannot_bypass_the_control_plane",
    ] {
        assert!(
            domain.contains(marker)
                || actions.contains(marker)
                || core.contains(marker)
                || commands.contains(marker)
                || daemon.contains(marker)
                || effect.contains(marker)
                || fixture.contains(marker),
            "connector marker missing: {marker}"
        );
    }

    assert!(core.contains("self.authorize_and_execute(&context, request).await"));
    assert!(core.contains("arguments.insert(\"binding_snapshot\""));
    assert!(domain.contains("adapter != \"local_fixture\""));
    assert!(daemon.contains("source: \"local_fixture\""));
    assert!(daemon.contains("append_idempotent_expected"));
    assert!(daemon.contains("connector_reconciliation_binding_mismatch"));
    assert!(daemon.contains("connector_rate_limit_exceeded"));
    assert!(daemon.contains("external_effect_performed"));
    assert!(!daemon.contains("reqwest::Client"));
    assert!(!daemon.contains("ProviderGateway"));
    assert!(!core.contains("CapabilityBroker"));
}

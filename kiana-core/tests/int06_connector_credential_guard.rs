#[test]
fn connector_invocation_cannot_override_operator_bound_credential_metadata() {
    let core = include_str!("../src/connectors.rs");
    let broker = include_str!("../../kiana-capability-broker/src/lib.rs");
    let domain = include_str!("../../kiana-domain/src/connector_credentials.rs");
    let daemon = include_str!("../../kiana-daemon/src/connectors.rs");

    assert!(core.contains("binding_snapshot"));
    assert!(
        core.contains("connector_id_required") || core.contains("connector_binding_id_required")
    );
    assert!(core.contains("connector_arguments_invalid"));
    assert!(!core.contains("credential_lease"));
    assert!(core.contains("\"binding_id\" | \"operation\" | \"payload\" | \"idempotency_key\""));
    assert!(core.contains("action == \"bind\""));
    assert!(core.contains("snapshot.validate().map_err(str::to_owned)?"));
    assert!(broker.contains("consume_connector_credential_invocation"));
    assert!(broker.contains("invocation.consume_for("));
    assert!(daemon.contains("ConnectorCredentialInvocation::issue"));
    assert!(daemon.contains("consume_connector_credential_invocation"));
    assert!(daemon.contains("credential_evidence"));
    assert!(daemon.contains("if let Some(secret_ref) = &snapshot.binding.credential_ref"));
    assert!(domain.contains("#[serde(deny_unknown_fields)]"));
    assert!(domain.contains("CONNECTOR_CREDENTIAL_PURPOSE"));
    assert!(domain.contains("effect_target_digest"));
    assert!(domain.contains("lease.consume(now_unix_ms)"));
}

#[test]
fn invocation_rejects_credential_overrides_and_bind_is_operator_validated() {
    let core = include_str!("../src/connectors.rs");
    let broker = include_str!("../../kiana-capability-broker/src/lib.rs");
    let domain = include_str!("../../kiana-domain/src/connector_credentials.rs");
    let daemon = include_str!("../../kiana-daemon/src/connectors.rs");

    let normalizer = core
        .split("async fn normalize_connector_command")
        .nth(1)
        .expect("connector normalizer");
    assert!(normalizer.contains("context.cell_id.is_some()"));
    assert!(normalizer.contains("actor_id"));
    assert!(normalizer.contains("connector_operator_required"));
    assert!(normalizer.contains("context.project_trusted"));
    assert!(normalizer.contains("project_untrusted"));

    let invoke_branch = normalizer
        .split("if intent.name == CONNECTOR_INVOKE_OPERATION {")
        .nth(1)
        .expect("invoke branch")
        .split("let binding_id")
        .next()
        .expect("invoke allowlist");
    assert!(invoke_branch
        .contains("\"binding_id\" | \"operation\" | \"payload\" | \"idempotency_key\""));
    for forbidden in [
        "secret_ref",
        "credential_ref",
        "credential_lease",
        "\"lease\"",
    ] {
        assert!(
            !invoke_branch.contains(forbidden),
            "invoke input must not accept {forbidden}"
        );
    }
    assert!(invoke_branch.contains("connector_arguments_invalid"));

    let manage_branch = normalizer
        .split("} else if intent.name == CONNECTOR_MANAGE_OPERATION {")
        .nth(1)
        .expect("manage branch");
    let bind_validation = manage_branch
        .split("if action == \"bind\" {")
        .nth(1)
        .expect("bind validation branch")
        .split("RiskLevel::ExternalSideEffect")
        .next()
        .expect("bind validation boundary");
    assert!(manage_branch.contains("connector_mutation_fields_required"));
    assert!(bind_validation.contains("arguments.get(\"binding\")"));
    assert!(bind_validation.contains("serde_json::from_value"));
    assert!(bind_validation.contains("snapshot.validate().map_err(str::to_owned)?"));
    assert!(manage_branch.contains("RiskLevel::ExternalSideEffect"));
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

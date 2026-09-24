use kiana_domain::{
    AccountBinding, CapabilityKind, CapabilityRequest, ConnectorBindingSnapshot,
    ConnectorDefinition, ConnectorEffect, ConnectorOperation, PermissionProfile, RequestContext,
    RequestId, RiskLevel,
};
use kiana_policy::{connector_policy_decision, DefaultPolicyEngine, PolicyDecision, PolicyEngine};
use serde_json::json;
use std::collections::{BTreeMap, BTreeSet};

fn context() -> RequestContext {
    let mut context = RequestContext::local("int15-session", "/repo");
    context.project_trusted = true;
    context.permission_profile = PermissionProfile::Balanced;
    context
}

fn binding() -> ConnectorBindingSnapshot {
    ConnectorBindingSnapshot {
        definition: ConnectorDefinition {
            schema: "kiana.connector-definition.v1".to_owned(),
            connector_id: "connector-int15".to_owned(),
            version: "v1".to_owned(),
            provider_id: "provider-int15".to_owned(),
            adapter: "local_fixture".to_owned(),
            operations: BTreeMap::from([
                (
                    "list".to_owned(),
                    ConnectorOperation {
                        effect: ConnectorEffect::ReadOnly,
                        required_scope: "read".to_owned(),
                        data_classes: BTreeSet::new(),
                    },
                ),
                (
                    "send_message".to_owned(),
                    ConnectorOperation {
                        effect: ConnectorEffect::Write,
                        required_scope: "write".to_owned(),
                        data_classes: BTreeSet::new(),
                    },
                ),
                (
                    "delete_account".to_owned(),
                    ConnectorOperation {
                        effect: ConnectorEffect::Write,
                        required_scope: "write".to_owned(),
                        data_classes: BTreeSet::new(),
                    },
                ),
            ]),
            rate_limit_per_minute: 10,
            idempotency_required: true,
            reconciliation_required: true,
            data_processing: "local_only".to_owned(),
        },
        binding: AccountBinding {
            schema: "kiana.account-binding.v1".to_owned(),
            binding_id: "binding-int15".to_owned(),
            connector_id: "connector-int15".to_owned(),
            account_id: "account-int15".to_owned(),
            read_scopes: BTreeSet::from(["read".to_owned(), "write".to_owned()]),
            write_scopes: BTreeSet::from(["write".to_owned()]),
            fixture_path: "fixtures/connector.json".to_owned(),
            fixture_sha256: format!("sha256:{}", "a".repeat(64)),
            credential_ref: None,
        },
        project_root: "/repo".to_owned(),
        revision: 1,
        status: "active".to_owned(),
    }
}

fn request(
    binding: &ConnectorBindingSnapshot,
    operation: &str,
    risk: RiskLevel,
) -> CapabilityRequest {
    let payload = json!({"to":"recipient","body":"final"});
    CapabilityRequest::new(
        RequestId::new(),
        CapabilityKind::Tool,
        "connector.invoke",
        json!({
            "operation": operation,
            "payload": payload,
            "final_payload_digest": kiana_domain::connector_payload_sha256(&json!({"to":"recipient","body":"final"})),
            "binding_id": binding.binding.binding_id,
            "binding_snapshot": binding,
            "binding_authorized": true,
            "operator_authorized": true,
            "project_root": "/repo",
            "actor_id": "local-user",
        }),
    )
    .with_risk(risk)
}

#[test]
fn r0_r1_are_allow_and_r3_is_ask_on_the_existing_policy_engine() {
    let context = context();
    let binding = binding();
    let read = request(&binding, "list", RiskLevel::ReadOnly);
    assert!(matches!(
        connector_policy_decision(&context, &read),
        Some(PolicyDecision::Allow { .. })
    ));
    let write = request(&binding, "send_message", RiskLevel::ExternalSideEffect);
    assert!(matches!(
        connector_policy_decision(&context, &write),
        Some(PolicyDecision::Ask { reason }) if reason == "connector_final_payload_approval_required"
    ));
    assert!(matches!(
        DefaultPolicyEngine.evaluate(&context, &write),
        PolicyDecision::Ask { .. }
    ));
}

#[test]
fn r4_is_default_deny_and_never_becomes_an_approval_gate() {
    let context = context();
    let binding = binding();
    let request = request(&binding, "delete_account", RiskLevel::Critical);
    assert!(matches!(
        connector_policy_decision(&context, &request),
        Some(PolicyDecision::Deny { reason }) if reason == "connector_r4_default_denied"
    ));
    assert!(matches!(
        DefaultPolicyEngine.evaluate(&context, &request),
        PolicyDecision::Deny { reason } if reason == "connector_r4_default_denied"
    ));
}

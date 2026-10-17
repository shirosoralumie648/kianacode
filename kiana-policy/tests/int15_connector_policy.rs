use kiana_domain::{
    connector_payload_sha256, AccountBinding, CapabilityKind, CapabilityRequest,
    ConnectorBindingSnapshot, ConnectorDataGrant, ConnectorDefinition, ConnectorEffect,
    ConnectorOnceApproval, ConnectorOperation, PermissionProfile, PolicyDecision, RequestContext,
    RequestId, RiskLevel,
};
use kiana_policy::{connector_policy_decision, DefaultPolicyEngine, PolicyEngine};
use serde_json::{json, Value};
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
                    "read_document".to_owned(),
                    ConnectorOperation {
                        effect: ConnectorEffect::ReadOnly,
                        required_scope: "read".to_owned(),
                        data_classes: BTreeSet::new(),
                    },
                ),
                (
                    "export_records".to_owned(),
                    ConnectorOperation {
                        effect: ConnectorEffect::ReadOnly,
                        required_scope: "read".to_owned(),
                        data_classes: BTreeSet::from(["internal".to_owned()]),
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

fn payload() -> Value {
    json!({"to":"recipient","body":"final"})
}

fn invoke(
    binding: &ConnectorBindingSnapshot,
    operation: &str,
    risk: RiskLevel,
    payload: Value,
    extra: Value,
) -> CapabilityRequest {
    let mut arguments = json!({
        "operation": operation,
        "payload": payload,
        "final_payload_digest": connector_payload_sha256(&payload),
        "binding_id": binding.binding.binding_id,
        "binding_snapshot": binding,
        "binding_authorized": true,
        "operator_authorized": true,
        "project_root": "/repo",
        "actor_id": "local-user",
    });
    if let (Some(target), Some(source)) = (arguments.as_object_mut(), extra.as_object()) {
        for (key, value) in source {
            target.insert(key.clone(), value.clone());
        }
    }
    CapabilityRequest::new(
        RequestId::new(),
        CapabilityKind::Tool,
        "connector.invoke",
        arguments,
    )
    .with_risk(risk)
}

fn request(
    binding: &ConnectorBindingSnapshot,
    operation: &str,
    risk: RiskLevel,
) -> CapabilityRequest {
    invoke(binding, operation, risk, payload(), json!({}))
}

fn grant(owner: &str, data_classes: &[&str]) -> ConnectorDataGrant {
    ConnectorDataGrant::issue(
        "grant-int15",
        "binding-int15",
        owner,
        "/repo",
        data_classes
            .iter()
            .map(|class| (*class).to_owned())
            .collect(),
        1,
        1,
        1,
        500,
    )
    .unwrap()
}

fn digest(value: &Value) -> String {
    connector_payload_sha256(value)
}

#[test]
fn r0_r1_are_allow_and_r3_is_ask_on_the_existing_policy_engine() {
    let context = context();
    let binding = binding();
    for operation in ["list", "read_document"] {
        let read = request(&binding, operation, RiskLevel::ReadOnly);
        assert!(matches!(
            connector_policy_decision(&context, &read),
            Some(PolicyDecision::Allow { .. })
        ));
        assert!(matches!(
            DefaultPolicyEngine.evaluate(&context, &read),
            PolicyDecision::Allow { .. }
        ));
    }
    let write = request(&binding, "send_message", RiskLevel::ExternalSideEffect);
    assert!(matches!(
        connector_policy_decision(&context, &write),
        Some(PolicyDecision::Ask { reason }) if reason == "connector_final_payload_approval_required"
    ));
    assert!(matches!(
        DefaultPolicyEngine.evaluate(&context, &write),
        PolicyDecision::Ask { .. }
    ));

    // The binding snapshot is server-owned context: a foreign project root or a mismatched
    // binding id is refused before any operation contract is consulted.
    let mut foreign = binding.clone();
    foreign.project_root = "/other".to_owned();
    assert!(matches!(
        connector_policy_decision(
            &context,
            &request(&foreign, "list", RiskLevel::ReadOnly)
        ),
        Some(PolicyDecision::Deny { reason }) if reason == "connector_binding_scope_mismatch"
    ));
    let mismatched = invoke(
        &binding,
        "list",
        RiskLevel::ReadOnly,
        payload(),
        json!({"binding_id": "binding-other"}),
    );
    assert!(matches!(
        connector_policy_decision(&context, &mismatched),
        Some(PolicyDecision::Deny { reason }) if reason == "connector_binding_id_mismatch"
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

    // A self-minted once-approval bound to the exact payload cannot make R4 approvable.
    let approval = ConnectorOnceApproval::issue(
        "approval-r4",
        "binding-int15",
        "delete_account",
        digest(&json!({"id": "account-1"})),
        1,
        1,
        500,
    )
    .unwrap();
    let forged = invoke(
        &binding,
        "delete_account",
        RiskLevel::Critical,
        json!({"id": "account-1"}),
        json!({"connector_approval": serde_json::to_value(&approval).unwrap()}),
    );
    assert!(matches!(
        DefaultPolicyEngine.evaluate(&context, &forged),
        PolicyDecision::Deny { reason } if reason == "connector_r4_default_denied"
    ));

    // Declaring a read-only risk label does not move a critical operation out of default deny.
    let downgraded = invoke(
        &binding,
        "delete_account",
        RiskLevel::ReadOnly,
        json!({"id": "account-1"}),
        json!({}),
    );
    assert!(matches!(
        DefaultPolicyEngine.evaluate(&context, &downgraded),
        PolicyDecision::Deny { reason } if reason == "connector_r4_default_denied"
    ));
}

#[test]
fn r2_requires_a_current_data_grant_and_denies_the_wire_shortcut() {
    let context = context();
    let binding = binding();
    let export_payload = json!({"ids":["a"]});
    let export = invoke(
        &binding,
        "export_records",
        RiskLevel::LocalWrite,
        export_payload.clone(),
        json!({}),
    );
    // Without a server-resolved grant the R2 operation never reaches the Broker.
    assert!(matches!(
        connector_policy_decision(&context, &export),
        Some(PolicyDecision::Deny { reason }) if reason == "connector_data_grant_required"
    ));

    // A matching grant is a narrowing object: it allows the exact binding/owner/project/classes.
    let matching = grant("local-user", &["internal"]);
    let allowed = invoke(
        &binding,
        "export_records",
        RiskLevel::LocalWrite,
        export_payload.clone(),
        json!({"data_grant": serde_json::to_value(&matching).unwrap()}),
    );
    assert!(matches!(
        connector_policy_decision(&context, &allowed),
        Some(PolicyDecision::Allow { .. })
    ));

    for (label, grant, expected) in [
        (
            "foreign_owner",
            grant("someone-else", &["internal"]),
            "connector_data_grant_authority_mismatch",
        ),
        (
            "missing_scope",
            grant("local-user", &["public"]),
            "connector_data_grant_scope_insufficient",
        ),
    ] {
        let denied = invoke(
            &binding,
            "export_records",
            RiskLevel::LocalWrite,
            export_payload.clone(),
            json!({"data_grant": serde_json::to_value(&grant).unwrap()}),
        );
        assert!(
            matches!(
                connector_policy_decision(&context, &denied),
                Some(PolicyDecision::Deny { reason }) if reason == expected
            ),
            "{label} was not refused with {expected}"
        );
    }

    // A revoked grant stays revoked, and malformed grant material is rejected outright.
    let mut revoked = grant("local-user", &["internal"]);
    revoked.revoked = true;
    revoked.grant_digest = revoked.digest();
    let denied = invoke(
        &binding,
        "export_records",
        RiskLevel::LocalWrite,
        export_payload.clone(),
        json!({"data_grant": serde_json::to_value(&revoked).unwrap()}),
    );
    assert!(matches!(
        connector_policy_decision(&context, &denied),
        Some(PolicyDecision::Deny { reason }) if reason == "connector_data_grant_revoked"
    ));
    let malformed = invoke(
        &binding,
        "export_records",
        RiskLevel::LocalWrite,
        export_payload,
        json!({"data_grant": {"schema": "not-a-grant"}}),
    );
    assert!(matches!(
        connector_policy_decision(&context, &malformed),
        Some(PolicyDecision::Deny { reason }) if reason == "connector_data_grant_invalid"
    ));
}

#[test]
fn r3_final_payload_digest_is_derived_and_the_payload_cannot_be_swapped() {
    let context = context();
    let binding = binding();
    let final_payload = payload();
    let final_digest = digest(&final_payload);

    // Missing approval: Ask with the exact final payload digest, never an execution.
    let pending = request(&binding, "send_message", RiskLevel::ExternalSideEffect);
    assert!(matches!(
        connector_policy_decision(&context, &pending),
        Some(PolicyDecision::Ask { reason }) if reason == "connector_final_payload_approval_required"
    ));

    // Exact once-approval is required and is accepted only for this payload/binding/operation.
    let approval = ConnectorOnceApproval::issue(
        "approval-int15",
        "binding-int15",
        "send_message",
        final_digest.clone(),
        1,
        1,
        500,
    )
    .unwrap();
    let approved = invoke(
        &binding,
        "send_message",
        RiskLevel::ExternalSideEffect,
        final_payload.clone(),
        json!({"connector_approval": serde_json::to_value(&approval).unwrap()}),
    );
    assert!(matches!(
        connector_policy_decision(&context, &approved),
        Some(PolicyDecision::Allow { .. })
    ));

    // A caller supplied digest that disagrees with the canonical payload is refused.
    let swapped_digest = invoke(
        &binding,
        "send_message",
        RiskLevel::ExternalSideEffect,
        final_payload.clone(),
        json!({"final_payload_digest": digest(&json!({"to":"attacker"}))}),
    );
    assert!(matches!(
        connector_policy_decision(&context, &swapped_digest),
        Some(PolicyDecision::Deny { reason }) if reason == "connector_final_payload_digest_mismatch"
    ));

    // An approval minted for another payload cannot authorize this one.
    let foreign = ConnectorOnceApproval::issue(
        "approval-foreign",
        "binding-int15",
        "send_message",
        digest(&json!({"to": "attacker"})),
        1,
        1,
        500,
    )
    .unwrap();
    let swapped_payload = invoke(
        &binding,
        "send_message",
        RiskLevel::ExternalSideEffect,
        final_payload.clone(),
        json!({"connector_approval": serde_json::to_value(&foreign).unwrap()}),
    );
    assert!(matches!(
        connector_policy_decision(&context, &swapped_payload),
        Some(PolicyDecision::Deny { reason }) if reason == "connector_once_approval_payload_digest_mismatch"
    ));

    // R3 without a payload can never fall back to a caller supplied digest.
    let no_payload = invoke(
        &binding,
        "send_message",
        RiskLevel::ExternalSideEffect,
        Value::Null,
        json!({}),
    );
    let mut arguments = no_payload.arguments.clone();
    arguments.as_object_mut().unwrap().remove("payload");
    let no_payload = CapabilityRequest::new(
        RequestId::new(),
        CapabilityKind::Tool,
        "connector.invoke",
        arguments,
    )
    .with_risk(RiskLevel::ExternalSideEffect);
    assert!(matches!(
        connector_policy_decision(&context, &no_payload),
        Some(PolicyDecision::Deny { reason }) if reason == "connector_final_payload_required"
    ));
}

#[test]
fn unregistered_and_missing_binding_material_fail_closed() {
    let context = context();
    let binding = binding();
    let unregistered = request(&binding, "wipe_everything", RiskLevel::ReadOnly);
    assert!(matches!(
        connector_policy_decision(&context, &unregistered),
        Some(PolicyDecision::Deny { reason }) if reason == "connector_operation_unregistered"
    ));

    let missing_snapshot = CapabilityRequest::new(
        RequestId::new(),
        CapabilityKind::Tool,
        "connector.invoke",
        json!({"operation": "list", "payload": {}}),
    )
    .with_risk(RiskLevel::ReadOnly);
    assert!(matches!(
        connector_policy_decision(&context, &missing_snapshot),
        Some(PolicyDecision::Deny { reason }) if reason == "connector_binding_snapshot_required"
    ));

    // The connector mapping never applies to a non-connector operation.
    let other = CapabilityRequest::new(
        RequestId::new(),
        CapabilityKind::Tool,
        "workspace.read",
        json!({"path": "README.md"}),
    )
    .with_risk(RiskLevel::ReadOnly);
    assert!(connector_policy_decision(&context, &other).is_none());
}

#[test]
fn policy_engine_and_connector_decision_agree_on_every_operation_class() {
    let context = context();
    let binding = binding();
    let cases = [
        ("list", RiskLevel::ReadOnly),
        ("read_document", RiskLevel::ReadOnly),
        ("export_records", RiskLevel::LocalWrite),
        ("send_message", RiskLevel::ExternalSideEffect),
        ("delete_account", RiskLevel::Critical),
    ];
    for (operation, risk) in cases {
        let request = request(&binding, operation, risk);
        let decision = connector_policy_decision(&context, &request)
            .expect("connector.invoke always has a connector decision");
        assert_eq!(DefaultPolicyEngine.evaluate(&context, &request), decision);
    }
}

use kiana_domain::{
    connector_operation_risk, connector_payload_sha256, evaluate_connector_admission,
    ConnectorAdmission, ConnectorAdmissionInput, ConnectorDataGrant, ConnectorEffect,
    ConnectorOnceApproval, ConnectorOperationRisk, RiskLevel,
};
use serde_json::json;
use std::collections::BTreeSet;

fn classes(values: &[&str]) -> BTreeSet<String> {
    values.iter().map(|value| (*value).to_owned()).collect()
}

fn input<'a>(
    operation: &'a str,
    effect: ConnectorEffect,
    payload: Option<&'a serde_json::Value>,
) -> ConnectorAdmissionInput<'a> {
    let required = Box::leak(Box::new(BTreeSet::new()));
    ConnectorAdmissionInput::new(
        operation,
        effect,
        required,
        match connector_operation_risk(operation, effect) {
            ConnectorOperationRisk::R0ReadOnly | ConnectorOperationRisk::R1ReadOnly => {
                RiskLevel::ReadOnly
            }
            ConnectorOperationRisk::R2DataGrant => RiskLevel::LocalWrite,
            ConnectorOperationRisk::R3OnceApproval => RiskLevel::ExternalSideEffect,
            ConnectorOperationRisk::R4DefaultDeny => RiskLevel::Critical,
        },
        "binding-1",
        "owner-1",
        "/repo",
        payload,
    )
    .with_epochs(4, 7, 9)
    .with_now(100)
}

#[test]
fn r0_and_r1_are_read_only_and_stable() {
    for operation in ["list", "read_document"] {
        let request = input(operation, ConnectorEffect::ReadOnly, None);
        let first = evaluate_connector_admission(&request);
        let second = evaluate_connector_admission(&request);
        assert_eq!(first, second);
        assert!(matches!(
            first,
            ConnectorAdmission::Allowed {
                broker_calls: 1,
                ..
            }
        ));
    }
}

#[test]
fn r2_requires_a_current_data_grant_and_never_widens_scope() {
    let payload = json!({"ids":["a"]});
    let request = input("export_records", ConnectorEffect::ReadOnly, Some(&payload));
    assert!(matches!(
        evaluate_connector_admission(&request),
        ConnectorAdmission::Denied { reason, broker_calls: 0, .. } if reason == "connector_data_grant_required"
    ));
    let grant = ConnectorDataGrant::issue(
        "grant-1",
        "binding-1",
        "owner-1",
        "/repo",
        classes(&["internal"]),
        9,
        4,
        7,
        500,
    )
    .unwrap();
    let required = classes(&["internal"]);
    let request = ConnectorAdmissionInput::new(
        "export_records",
        ConnectorEffect::ReadOnly,
        &required,
        RiskLevel::LocalWrite,
        "binding-1",
        "owner-1",
        "/repo",
        Some(&payload),
    )
    .with_epochs(4, 7, 9)
    .with_now(100)
    .with_data_grant(Some(&grant));
    assert!(matches!(
        evaluate_connector_admission(&request),
        ConnectorAdmission::Allowed {
            broker_calls: 1,
            risk: ConnectorOperationRisk::R2DataGrant,
            ..
        }
    ));
    let mut revoked = grant.clone();
    revoked.revoked = true;
    revoked.grant_digest = revoked.digest();
    assert!(matches!(
        evaluate_connector_admission(&request.with_data_grant(Some(&revoked))),
        ConnectorAdmission::Denied { reason, broker_calls: 0, .. } if reason == "connector_data_grant_revoked"
    ));
}

#[test]
fn r3_is_one_shot_and_binds_the_final_payload_digest() {
    let payload = json!({"to":"recipient","body":"final"});
    let digest = connector_payload_sha256(&payload);
    let request = input("send_message", ConnectorEffect::Write, Some(&payload))
        .with_final_payload_digest(Some(&digest));
    assert!(matches!(
        evaluate_connector_admission(&request),
        ConnectorAdmission::AwaitingApproval { reason, broker_calls: 0, .. } if reason == "connector_final_payload_approval_required"
    ));
    let approval = ConnectorOnceApproval::issue(
        "approval-1",
        "binding-1",
        "send_message",
        digest.clone(),
        4,
        7,
        500,
    )
    .unwrap();
    let approved = request.clone().with_once_approval(Some(&approval));
    assert!(matches!(
        evaluate_connector_admission(&approved),
        ConnectorAdmission::Allowed { broker_calls: 1, approval_id: Some(id), .. } if id == "approval-1"
    ));
    let mut drifted = approved.clone();
    drifted.final_payload_digest = Some(connector_payload_sha256(&json!({"to":"other"})));
    assert!(matches!(
        evaluate_connector_admission(&drifted),
        ConnectorAdmission::Denied { reason, broker_calls: 0, .. } if reason == "connector_final_payload_digest_mismatch"
    ));
    let mut expired = approval.clone();
    expired.expires_at_unix_ms = 100;
    expired.approval_digest = expired.digest();
    let expired_request = request.with_once_approval(Some(&expired));
    assert!(matches!(
        evaluate_connector_admission(&expired_request),
        ConnectorAdmission::Denied { reason, broker_calls: 0, .. } if reason == "connector_once_approval_expired"
    ));
}

#[test]
fn r4_is_default_deny_even_with_critical_risk_or_approval() {
    let request = input("delete_account", ConnectorEffect::Write, None);
    assert!(matches!(
        evaluate_connector_admission(&request),
        ConnectorAdmission::Denied { risk: ConnectorOperationRisk::R4DefaultDeny, reason, broker_calls: 0 } if reason == "connector_r4_default_denied"
    ));
}

#[test]
fn stale_authority_and_policy_epochs_are_zero_broker_calls() {
    let mut request = input("read_document", ConnectorEffect::ReadOnly, None);
    request.binding_authority_epoch = 3;
    request.authority_epoch = 4;
    assert!(matches!(
        evaluate_connector_admission(&request),
        ConnectorAdmission::Denied { reason, broker_calls: 0, .. } if reason == "connector_authority_epoch_mismatch"
    ));
    request.binding_authority_epoch = 4;
    request.binding_policy_epoch = 6;
    assert!(matches!(
        evaluate_connector_admission(&request),
        ConnectorAdmission::Denied { reason, broker_calls: 0, .. } if reason == "connector_policy_epoch_mismatch"
    ));
    request.binding_policy_epoch = 7;
    request.binding_active = false;
    assert!(matches!(
        evaluate_connector_admission(&request),
        ConnectorAdmission::Denied { reason, broker_calls: 0, .. } if reason == "connector_binding_revoked"
    ));
    request.binding_active = true;
    request.binding_expires_at_unix_ms = 100;
    assert!(matches!(
        evaluate_connector_admission(&request),
        ConnectorAdmission::Denied { reason, broker_calls: 0, .. } if reason == "connector_binding_expired"
    ));
}

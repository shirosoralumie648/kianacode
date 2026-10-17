use kiana_domain::{
    connector_operation_risk, connector_payload_sha256, connector_risk_mapping,
    evaluate_connector_admission, ConnectorAdmission, ConnectorAdmissionInput, ConnectorDataGrant,
    ConnectorEffect, ConnectorOnceApproval, ConnectorOperationRisk, ConnectorPolicyRequirement,
    RiskLevel,
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

fn grant(
    data_classes: &[&str],
    data_epoch: u64,
    authority_epoch: u64,
    policy_epoch: u64,
    expires_at_unix_ms: u64,
) -> ConnectorDataGrant {
    ConnectorDataGrant::issue(
        "grant-1",
        "binding-1",
        "owner-1",
        "/repo",
        classes(data_classes),
        data_epoch,
        authority_epoch,
        policy_epoch,
        expires_at_unix_ms,
    )
    .unwrap()
}

fn approval(digest: &str, binding_id: &str, operation: &str) -> ConnectorOnceApproval {
    ConnectorOnceApproval::issue("approval-1", binding_id, operation, digest, 4, 7, 500).unwrap()
}

#[test]
fn r0_and_r1_are_read_only_and_stable() {
    let cases = [
        ("list", ConnectorOperationRisk::R0ReadOnly),
        ("inspect_schema", ConnectorOperationRisk::R0ReadOnly),
        ("read_document", ConnectorOperationRisk::R1ReadOnly),
        ("whoami", ConnectorOperationRisk::R1ReadOnly),
    ];
    for (operation, expected_risk) in cases {
        let mapping =
            connector_risk_mapping(operation, ConnectorEffect::ReadOnly, RiskLevel::ReadOnly);
        assert_eq!(mapping.risk, expected_risk);
        assert_eq!(mapping.required_risk_level, RiskLevel::ReadOnly);
        assert_eq!(mapping.requirement, ConnectorPolicyRequirement::ReadOnly);
        assert_eq!(mapping.broker_calls_on_allow(), 1);
        assert!(expected_risk.is_read_only());
        assert!(!expected_risk.requires_data_grant());
        assert!(!expected_risk.requires_once_approval());
        assert!(!expected_risk.is_default_denied());

        let request = input(operation, ConnectorEffect::ReadOnly, None);
        let first = evaluate_connector_admission(&request);
        let second = evaluate_connector_admission(&request);
        assert_eq!(first, second);
        assert_eq!(first.reason(), None);
        assert!(!first.is_zero_effect());
        assert_eq!(first.broker_calls(), 1);
        assert!(matches!(
            first,
            ConnectorAdmission::Allowed {
                risk,
                broker_calls: 1,
                ..
            } if risk == expected_risk
        ));
    }
}

#[test]
fn r2_requires_a_current_data_grant_and_never_widens_scope() {
    let payload = json!({"ids":["a"]});
    let required = classes(&["internal"]);
    let request = input("export_records", ConnectorEffect::ReadOnly, Some(&payload));
    assert_eq!(request.declared_risk, RiskLevel::LocalWrite);
    assert!(matches!(
        evaluate_connector_admission(&request),
        ConnectorAdmission::Denied { reason, broker_calls: 0, .. } if reason == "connector_data_grant_required"
    ));

    // The valid grant is the only success path, and it binds owner/project/binding/epochs/classes.
    let valid = grant(&["internal"], 9, 4, 7, 500);
    let allowed = ConnectorAdmissionInput::new(
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
    .with_data_grant(Some(&valid));
    assert!(matches!(
        evaluate_connector_admission(&allowed),
        ConnectorAdmission::Allowed {
            broker_calls: 1,
            risk: ConnectorOperationRisk::R2DataGrant,
            ..
        }
    ));

    // Every narrowing dimension fails closed with its own stable reason.
    for (label, mutated, expected) in [
        (
            "revoked",
            {
                let mut value = valid.clone();
                value.revoked = true;
                value.grant_digest = value.digest();
                value
            },
            "connector_data_grant_revoked",
        ),
        (
            "expired",
            {
                let mut value = valid.clone();
                value.expires_at_unix_ms = 100;
                value.grant_digest = value.digest();
                value
            },
            "connector_data_grant_expired",
        ),
        (
            "foreign_owner",
            {
                let mut value = valid.clone();
                value.owner_id = "owner-2".to_owned();
                value.grant_digest = value.digest();
                value
            },
            "connector_data_grant_authority_mismatch",
        ),
        (
            "foreign_binding",
            {
                let mut value = valid.clone();
                value.binding_id = "binding-2".to_owned();
                value.grant_digest = value.digest();
                value
            },
            "connector_data_grant_authority_mismatch",
        ),
        (
            "foreign_project",
            {
                let mut value = valid.clone();
                value.project_root = "/other".to_owned();
                value.grant_digest = value.digest();
                value
            },
            "connector_data_grant_authority_mismatch",
        ),
        (
            "data_epoch",
            grant(&["internal"], 8, 4, 7, 500),
            "connector_data_grant_data_epoch_mismatch",
        ),
        (
            "authority_epoch",
            grant(&["internal"], 9, 3, 7, 500),
            "connector_data_grant_authority_epoch_mismatch",
        ),
        (
            "policy_epoch",
            grant(&["internal"], 9, 4, 6, 500),
            "connector_data_grant_policy_epoch_mismatch",
        ),
        (
            "scope",
            grant(&["public"], 9, 4, 7, 500),
            "connector_data_grant_scope_insufficient",
        ),
    ] {
        assert!(
            matches!(
                evaluate_connector_admission(&allowed.with_data_grant(Some(&mutated))),
                ConnectorAdmission::Denied { reason, broker_calls: 0, .. } if reason == expected
            ),
            "{label} was not refused with {expected}"
        );
    }

    // A tampered digest is refused before the grant narrows anything.
    let mut tampered = valid.clone();
    tampered.grant_digest = format!("sha256:{}", "b".repeat(64));
    assert!(matches!(
        evaluate_connector_admission(&allowed.with_data_grant(Some(&tampered))),
        ConnectorAdmission::Denied { reason, broker_calls: 0, .. } if reason == "connector_data_grant_digest_mismatch"
    ));
}

#[test]
fn r3_is_one_shot_and_binds_the_final_payload_digest() {
    let payload = json!({"to":"recipient","body":"final"});
    let digest = connector_payload_sha256(&payload);
    let request = input("send_message", ConnectorEffect::Write, Some(&payload))
        .with_final_payload_digest(Some(&digest));
    let awaiting = evaluate_connector_admission(&request);
    assert!(awaiting.is_zero_effect());
    assert_eq!(awaiting.broker_calls(), 0);
    assert!(matches!(
        awaiting,
        ConnectorAdmission::AwaitingApproval { risk: ConnectorOperationRisk::R3OnceApproval, reason, broker_calls: 0, .. } if reason == "connector_final_payload_approval_required"
    ));

    let approval = approval(&digest, "binding-1", "send_message");
    let approved = request.with_once_approval(Some(&approval));
    let first = evaluate_connector_admission(&approved);
    let second = evaluate_connector_admission(&approved);
    assert_eq!(first, second);
    assert!(matches!(
        first,
        ConnectorAdmission::Allowed { broker_calls: 1, approval_id: Some(id), .. } if id == "approval-1"
    ));

    // Drifting any binding dimension of the approval turns the decision into a denial.
    let drifted_digest = request
        .clone()
        .with_final_payload_digest(Some(&connector_payload_sha256(&json!({"to":"other"}))));
    assert!(matches!(
        evaluate_connector_admission(&drifted_digest.with_once_approval(Some(&approval))),
        ConnectorAdmission::Denied { reason, broker_calls: 0, .. } if reason == "connector_final_payload_digest_mismatch"
    ));
    for (label, drifted, expected) in [
        (
            "foreign_binding",
            approval(&digest, "binding-2", "send_message"),
            "connector_once_approval_authority_mismatch",
        ),
        (
            "foreign_operation",
            approval(&digest, "binding-1", "send_direct_message"),
            "connector_once_approval_authority_mismatch",
        ),
        (
            "expired",
            {
                let mut value = approval(&digest, "binding-1", "send_message");
                value.expires_at_unix_ms = 100;
                value.approval_digest = value.digest();
                value
            },
            "connector_once_approval_expired",
        ),
        (
            "revoked",
            {
                let mut value = approval(&digest, "binding-1", "send_message");
                value.revoked = true;
                value.approval_digest = value.digest();
                value
            },
            "connector_once_approval_revoked",
        ),
        (
            "authority_epoch",
            {
                let mut value = approval(&digest, "binding-1", "send_message");
                value.authority_epoch = 3;
                value.approval_digest = value.digest();
                value
            },
            "connector_once_approval_authority_epoch_mismatch",
        ),
        (
            "policy_epoch",
            {
                let mut value = approval(&digest, "binding-1", "send_message");
                value.policy_epoch = 6;
                value.approval_digest = value.digest();
                value
            },
            "connector_once_approval_policy_epoch_mismatch",
        ),
        (
            "consumed",
            approval(&digest, "binding-1", "send_message")
                .consume(100)
                .unwrap(),
            "connector_once_approval_consumed",
        ),
    ] {
        assert!(
            matches!(
                evaluate_connector_admission(&request.with_once_approval(Some(&drifted))),
                ConnectorAdmission::Denied { reason, broker_calls: 0, .. } if reason == expected
            ),
            "{label} was not refused with {expected}"
        );
    }

    // R3 without any payload can never fall back to a caller supplied digest.
    let no_payload = input("send_message", ConnectorEffect::Write, None)
        .with_final_payload_digest(Some(&digest));
    assert!(matches!(
        evaluate_connector_admission(&no_payload.with_once_approval(Some(&approval))),
        ConnectorAdmission::Denied { reason, broker_calls: 0, .. } if reason == "connector_final_payload_required"
    ));

    // Consuming the one-shot approval is exactly once and re-signs the digest.
    let consumed = approval(&digest, "binding-1", "send_message")
        .consume(100)
        .unwrap();
    assert!(consumed.consumed);
    assert!(consumed.consume(100).is_err());
    assert!(matches!(
        evaluate_connector_admission(&request.with_once_approval(Some(&consumed))),
        ConnectorAdmission::Denied { reason, broker_calls: 0, .. } if reason == "connector_once_approval_consumed"
    ));
}

#[test]
fn r3_payload_digest_accepts_bare_and_prefixed_sha256_but_rejects_malformed_values() {
    let bare = "a".repeat(64);
    let prefixed = format!("sha256:{bare}");
    for digest in [bare, prefixed] {
        ConnectorOnceApproval::issue("approval", "binding-1", "send_message", digest, 4, 7, 500)
            .expect("payload digest encoding should be accepted");
    }
    assert_eq!(
        ConnectorOnceApproval::issue(
            "approval",
            "binding-1",
            "send_message",
            "not-a-digest",
            4,
            7,
            500,
        )
        .unwrap_err(),
        "connector_once_approval_invalid"
    );
}

#[test]
fn r4_is_default_deny_even_with_critical_risk_or_approval() {
    for operation in [
        "delete_account",
        "payment_refund",
        "release_production",
        "firmware_update",
    ] {
        let request = input(operation, ConnectorEffect::Write, None);
        let denied = evaluate_connector_admission(&request);
        assert!(denied.is_zero_effect());
        assert_eq!(denied.broker_calls(), 0);
        assert!(matches!(
            denied,
            ConnectorAdmission::Denied { risk: ConnectorOperationRisk::R4DefaultDeny, reason, broker_calls: 0 } if reason == "connector_r4_default_denied"
        ));
    }

    // Even a read-only declared risk cannot move R4 out of default deny; the default-deny check
    // runs before any grant or once-approval is consulted.
    let payload = json!({"id":"account-1"});
    let mut downgraded = input("delete_account", ConnectorEffect::Write, Some(&payload));
    downgraded.declared_risk = RiskLevel::ReadOnly;
    assert!(matches!(
        evaluate_connector_admission(&downgraded),
        ConnectorAdmission::Denied { risk: ConnectorOperationRisk::R4DefaultDeny, reason, broker_calls: 0 } if reason == "connector_r4_default_denied"
    ));

    let mapping = connector_risk_mapping(
        "delete_account",
        ConnectorEffect::Write,
        RiskLevel::Critical,
    );
    assert_eq!(mapping.risk, ConnectorOperationRisk::R4DefaultDeny);
    assert_eq!(mapping.required_risk_level, RiskLevel::Critical);
    assert_eq!(mapping.requirement, ConnectorPolicyRequirement::DefaultDeny);
    assert!(mapping.is_default_denied());
    assert!(!mapping.requires_once_approval());
}

#[test]
fn declared_risk_can_only_raise_the_operation_class() {
    // A declared ReadOnly label cannot turn a write or a data export into a read-only decision.
    assert_eq!(
        connector_risk_mapping("send_message", ConnectorEffect::Write, RiskLevel::ReadOnly).risk,
        ConnectorOperationRisk::R3OnceApproval
    );
    let downgraded_write = input(
        "send_message",
        ConnectorEffect::Write,
        Some(&json!({"a":1})),
    );
    let mut request = downgraded_write
        .with_final_payload_digest(Some(&connector_payload_sha256(&json!({"a":1}))));
    request.declared_risk = RiskLevel::ReadOnly;
    assert!(matches!(
        evaluate_connector_admission(&request),
        ConnectorAdmission::Denied { reason, broker_calls: 0, .. } if reason == "connector_final_payload_approval_required"
    ));

    let downgraded_export = connector_risk_mapping(
        "export_records",
        ConnectorEffect::ReadOnly,
        RiskLevel::ReadOnly,
    );
    assert_eq!(downgraded_export.risk, ConnectorOperationRisk::R2DataGrant);
    assert_eq!(downgraded_export.required_risk_level, RiskLevel::LocalWrite);
    assert!(downgraded_export.requires_data_grant());

    // A declared Critical contract is always R4, whatever the operation name suggests.
    let critical_read =
        connector_risk_mapping("list", ConnectorEffect::ReadOnly, RiskLevel::Critical);
    assert_eq!(critical_read.risk, ConnectorOperationRisk::R4DefaultDeny);
    assert!(critical_read.is_default_denied());

    // Stable string identities for the five classes.
    assert_eq!(ConnectorOperationRisk::R0ReadOnly.as_str(), "r0_read_only");
    assert_eq!(ConnectorOperationRisk::R1ReadOnly.as_str(), "r1_read_only");
    assert_eq!(
        ConnectorOperationRisk::R2DataGrant.as_str(),
        "r2_data_grant"
    );
    assert_eq!(
        ConnectorOperationRisk::R3OnceApproval.as_str(),
        "r3_once_approval"
    );
    assert_eq!(
        ConnectorOperationRisk::R4DefaultDeny.as_str(),
        "r4_default_deny"
    );
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

    // Zero-valued identity or clock fields fail closed before any grant or approval is consulted.
    let mut invalid = input("read_document", ConnectorEffect::ReadOnly, None);
    invalid.data_epoch = 0;
    assert!(matches!(
        evaluate_connector_admission(&invalid),
        ConnectorAdmission::Denied { reason, broker_calls: 0, .. } if reason == "connector_admission_context_invalid"
    ));
    invalid.data_epoch = 9;
    invalid.now_unix_ms = 0;
    assert!(matches!(
        evaluate_connector_admission(&invalid),
        ConnectorAdmission::Denied { reason, broker_calls: 0, .. } if reason == "connector_admission_context_invalid"
    ));
}

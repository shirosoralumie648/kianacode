use kiana_domain::{
    ActionRef, MessageId, Notification, NotificationActionAuthorityEffect,
    NotificationActionCommand, NotificationActionDecision, NotificationActionKind,
    NotificationActionOutcome, NotificationActionWaitKey, NotificationChannel,
};
use serde_json::json;
use std::fs;
use std::path::Path;

fn digest(byte: char) -> String {
    format!("sha256:{}", byte.to_string().repeat(64))
}

fn notification(recipient: &str) -> Notification {
    Notification::new(
        MessageId::new(),
        recipient,
        None,
        vec!["project/a".to_owned()],
        NotificationChannel::InApp,
        100,
        10_000,
        3,
        None,
    )
    .unwrap()
}

fn fixture(action: NotificationActionKind) -> (NotificationActionCommand, ActionRef) {
    let notification = notification("actor-10");
    let action_ref = ActionRef::new(
        action.command(),
        3,
        vec![format!("notification:{}", notification.notification_id)],
        100,
        200,
    )
    .unwrap();
    let command = NotificationActionCommand::new(
        action_ref.clone(),
        action,
        notification.notification_id,
        "actor-10",
        3,
        notification.notification_digest,
        7,
        11,
        json!({"reason":"review"}),
        "actor-10",
        "notification-action-10",
    )
    .unwrap();
    (command, action_ref)
}

fn decision(
    command: &NotificationActionCommand,
    outcome: NotificationActionOutcome,
    outcome_code: Option<&str>,
) -> NotificationActionDecision {
    NotificationActionDecision::new(
        command.action_ref.action_ref_id,
        command.notification_id,
        command.action,
        command.command_digest.clone(),
        command.idempotency_key.clone(),
        command.submitted_by.clone(),
        outcome,
        outcome_code.map(str::to_owned),
        150,
        11,
        None,
    )
    .unwrap()
}

fn wait_key(
    command: &NotificationActionCommand,
    wait_key: &str,
    registered_at_unix_ms: u64,
) -> NotificationActionWaitKey {
    NotificationActionWaitKey::new(
        wait_key,
        command.action_ref.action_ref_id,
        command.notification_id,
        3,
        command.expected_target_digest.clone(),
        7,
        11,
        registered_at_unix_ms,
    )
    .unwrap()
}

#[test]
fn fixture_declares_action_command_deny_first_contract() {
    let value: serde_json::Value = serde_json::from_str(
        &fs::read_to_string(
            Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("tests/fixtures/nm10-notification-actions.json"),
        )
        .unwrap(),
    )
    .unwrap();
    assert_eq!(value["schema"], "kiana.notification-action-fixture.v1");
    let denied = value["denied"].as_array().unwrap();
    for marker in [
        "stale",
        "wrong decider",
        "criteria",
        "double consume",
        "read/ACK",
        "unknown action",
        "delivery ACK",
    ] {
        assert!(
            denied
                .iter()
                .any(|item| item.as_str().unwrap().contains(marker)),
            "fixture denied marker missing: {marker}"
        );
    }
}

#[test]
fn action_command_binds_ref_target_epoch_cursor_and_redacted_payload() {
    let (command, _) = fixture(NotificationActionKind::Review);
    command.validate().unwrap();
    assert_eq!(command.action_ref.command, "notification.review");
    assert_eq!(command.expected_source_cursor, 11);
    assert_eq!(command.expected_criteria_digest, None);

    let mut wrong_kind = command.clone();
    wrong_kind.action = NotificationActionKind::Approve;
    assert_eq!(
        wrong_kind.validate().unwrap_err(),
        "notification_action_command_ref_kind_mismatch"
    );

    let mut secret = command.clone();
    secret.payload = json!({"token":"raw-secret"});
    assert_eq!(
        secret.validate().unwrap_err(),
        "notification_action_payload_invalid"
    );

    let mut widened = command.clone();
    widened.action_ref.scope = vec!["project/a".to_owned()];
    assert_eq!(
        widened.validate().unwrap_err(),
        "notification_action_command_target_scope_mismatch"
    );

    let criteria = digest('a');
    let bound = command.with_criteria_digest(criteria.clone()).unwrap();
    assert_eq!(
        bound.expected_criteria_digest.as_deref(),
        Some(criteria.as_str())
    );
    bound.validate().unwrap();

    let (other, _) = fixture(NotificationActionKind::Review);
    assert_eq!(
        other.with_criteria_digest("not-a-digest").unwrap_err(),
        "notification_action_criteria_digest_invalid"
    );
}

#[test]
fn action_command_refuses_forged_decision_fields_and_unknown_actions() {
    for key in [
        "acknowledged",
        "approval",
        "approved",
        "decision",
        "delivered",
        "task_status",
    ] {
        let (mut forged, _) = fixture(NotificationActionKind::Acknowledge);
        forged.payload = json!({ key: true });
        assert_eq!(
            forged.validate().unwrap_err(),
            "notification_action_payload_forged_decision",
            "forged payload key must be refused: {key}"
        );
    }

    for kind in NotificationActionKind::REGISTERED {
        assert_eq!(
            NotificationActionKind::from_command(kind.command()),
            Some(kind)
        );
    }
    for unknown in [
        "notification.bogus",
        "notification.decide",
        "NOTIFICATION.APPROVE",
        "",
    ] {
        assert_eq!(NotificationActionKind::from_command(unknown), None);
    }
    assert!(NotificationActionKind::Acknowledge.is_presentation_only());
    assert!(NotificationActionKind::Snooze.is_presentation_only());
    assert!(!NotificationActionKind::Approve.is_presentation_only());

    // An unregistered action name cannot be wrapped into a structurally valid command either.
    let notification = notification("actor-10");
    let unknown_ref = ActionRef::new(
        "notification.bogus",
        3,
        vec![format!("notification:{}", notification.notification_id)],
        100,
        200,
    )
    .unwrap();
    let error = NotificationActionCommand::new(
        unknown_ref,
        NotificationActionKind::Review,
        notification.notification_id,
        "actor-10",
        3,
        notification.notification_digest,
        7,
        11,
        json!({"reason":"unknown"}),
        "actor-10",
        "notification-action-unknown",
    )
    .unwrap_err();
    assert_eq!(error, "notification_action_command_ref_kind_mismatch");
}

#[test]
fn action_commands_round_trip_and_reject_unknown_fields() {
    let (command, _) = fixture(NotificationActionKind::Reconcile);
    let command = command.with_criteria_digest(digest('b')).unwrap();
    let encoded = serde_json::to_value(&command).unwrap();
    let decoded: NotificationActionCommand = serde_json::from_value(encoded.clone()).unwrap();
    decoded.validate().unwrap();
    assert_eq!(decoded, command);

    let mut forged = encoded.clone();
    forged["approved"] = json!(true);
    assert!(serde_json::from_value::<NotificationActionCommand>(forged).is_err());

    let mut missing_criteria = encoded;
    missing_criteria
        .as_object_mut()
        .unwrap()
        .remove("expected_criteria_digest");
    let decoded: NotificationActionCommand = serde_json::from_value(missing_criteria).unwrap();
    assert_eq!(decoded.expected_criteria_digest, None);
    // The digest still covers the criteria field, so a stripped command cannot validate.
    assert_eq!(
        decoded.validate().unwrap_err(),
        "notification_action_command_digest_mismatch"
    );

    let mut tampered = serde_json::to_value(&command).unwrap();
    tampered["payload"] = json!({"reason":"tampered"});
    let decoded: NotificationActionCommand = serde_json::from_value(tampered).unwrap();
    assert_eq!(
        decoded.validate().unwrap_err(),
        "notification_action_command_digest_mismatch"
    );
}

#[test]
fn presentation_and_governance_actions_never_claim_task_decisions() {
    assert_eq!(
        NotificationActionKind::Acknowledge.authority_effect(),
        NotificationActionAuthorityEffect::Presentation
    );
    assert_eq!(
        NotificationActionKind::Snooze.authority_effect(),
        NotificationActionAuthorityEffect::Presentation
    );
    for kind in [
        NotificationActionKind::Escalate,
        NotificationActionKind::Delegate,
        NotificationActionKind::Withdraw,
    ] {
        assert_eq!(
            kind.authority_effect(),
            NotificationActionAuthorityEffect::Governance
        );
    }
    for kind in [
        NotificationActionKind::Approve,
        NotificationActionKind::Review,
        NotificationActionKind::Reconcile,
    ] {
        assert_eq!(
            kind.authority_effect(),
            NotificationActionAuthorityEffect::Decision
        );
    }

    let (ack, _) = fixture(NotificationActionKind::Acknowledge);
    let decision = decision(&ack, NotificationActionOutcome::Applied, None);
    assert_eq!(
        decision.authority_effect,
        NotificationActionAuthorityEffect::Presentation
    );
    assert!(decision.wait_key.is_none());

    let mut forged = serde_json::to_value(&decision).unwrap();
    forged["authority_effect"] = json!("decision");
    let decoded: NotificationActionDecision = serde_json::from_value(forged).unwrap();
    assert_eq!(
        decoded.validate().unwrap_err(),
        "notification_action_decision_authority_effect_mismatch"
    );

    for key in [
        "approved",
        "delivered",
        "acknowledged",
        "read",
        "task_status",
    ] {
        let mut leaked = serde_json::to_value(&decision).unwrap();
        leaked[key] = json!(true);
        assert!(
            serde_json::from_value::<NotificationActionDecision>(leaked).is_err(),
            "delivery/read field must not decode into a decision: {key}"
        );
    }

    let encoded = serde_json::to_value(&decision).unwrap();
    let keys = encoded.as_object().unwrap().keys().collect::<Vec<_>>();
    for forbidden in [
        "approved",
        "delivered",
        "acknowledged",
        "read",
        "task_status",
    ] {
        assert!(
            !keys.iter().any(|key| key.as_str() == forbidden),
            "decision carries a delivery/read field: {forbidden}"
        );
    }
}

#[test]
fn decision_outcomes_require_their_own_reason_shape() {
    let (command, _) = fixture(NotificationActionKind::Approve);

    let mut applied = decision(&command, NotificationActionOutcome::Applied, None);
    applied.validate().unwrap();
    applied.outcome_code = Some("notification_action_failed".to_owned());
    assert_eq!(
        applied.validate().unwrap_err(),
        "notification_action_decision_applied_with_code"
    );

    let mut failed = decision(
        &command,
        NotificationActionOutcome::Failed,
        Some("notification_action_stale"),
    );
    failed.validate().unwrap();
    failed.outcome_code = None;
    assert_eq!(
        failed.validate().unwrap_err(),
        "notification_action_decision_code_required"
    );

    let unknown = decision(
        &command,
        NotificationActionOutcome::Unknown,
        Some("notification_action_result_unknown"),
    );
    unknown.validate().unwrap();
    assert!(unknown.outcome.is_unknown());
    assert_ne!(unknown.outcome, NotificationActionOutcome::Applied);
    assert_eq!(unknown.outcome.as_str(), "unknown");

    let mut value = serde_json::to_value(&unknown).unwrap();
    value["decision_id"] = json!("00000000-0000-0000-0000-000000000000");
    let decoded: NotificationActionDecision = serde_json::from_value(value).unwrap();
    assert_eq!(
        decoded.validate().unwrap_err(),
        "notification_action_decision_header_invalid"
    );

    let mut value = serde_json::to_value(&unknown).unwrap();
    value["decided_at_unix_ms"] = json!(151);
    let decoded: NotificationActionDecision = serde_json::from_value(value).unwrap();
    assert_eq!(
        decoded.validate().unwrap_err(),
        "notification_action_decision_digest_mismatch"
    );
}

#[test]
fn wait_key_binds_one_exact_action_target() {
    let (command, action_ref) = fixture(NotificationActionKind::Review);
    let key = wait_key(&command, "wait-activation-1", 120);
    key.validate().unwrap();
    assert_eq!(key.action_ref_id, action_ref.action_ref_id);
    assert_eq!(key.target_digest, command.expected_target_digest);

    let encoded = serde_json::to_value(&key).unwrap();
    let decoded: NotificationActionWaitKey = serde_json::from_value(encoded.clone()).unwrap();
    decoded.validate().unwrap();
    assert_eq!(decoded, key);

    let mut leaked = encoded.clone();
    leaked["read"] = json!(true);
    assert!(serde_json::from_value::<NotificationActionWaitKey>(leaked).is_err());

    let mut nil = encoded.clone();
    nil["notification_id"] = json!("00000000-0000-0000-0000-000000000000");
    let decoded: NotificationActionWaitKey = serde_json::from_value(nil).unwrap();
    assert_eq!(
        decoded.validate().unwrap_err(),
        "notification_action_wait_key_header_invalid"
    );

    let mut tampered = encoded;
    tampered["target_revision"] = json!(4);
    let decoded: NotificationActionWaitKey = serde_json::from_value(tampered).unwrap();
    assert_eq!(
        decoded.validate().unwrap_err(),
        "notification_action_wait_key_digest_mismatch"
    );

    assert_eq!(
        NotificationActionWaitKey::new(
            "",
            command.action_ref.action_ref_id,
            command.notification_id,
            3,
            command.expected_target_digest.clone(),
            7,
            11,
            120,
        )
        .unwrap_err(),
        "notification_action_wait_key_invalid"
    );
    assert_eq!(
        NotificationActionWaitKey::new(
            "wait-activation-1",
            command.action_ref.action_ref_id,
            command.notification_id,
            0,
            command.expected_target_digest.clone(),
            7,
            11,
            120,
        )
        .unwrap_err(),
        "notification_action_wait_key_header_invalid"
    );
}

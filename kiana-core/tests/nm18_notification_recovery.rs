use kiana_core::{
    plan_notification_recovery, NotificationRecoveryDisposition, NotificationRecoveryInput,
    NotificationRecoveryState, NOTIFICATION_RECOVERY_SCHEMA,
};
use serde_json::Value;

fn input(state: NotificationRecoveryState) -> NotificationRecoveryInput {
    NotificationRecoveryInput {
        schema: NOTIFICATION_RECOVERY_SCHEMA.to_owned(),
        notification_id: "notification:nm18".to_owned(),
        delivery_key_digest:
            "sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa".to_owned(),
        authority_epoch: 7,
        expected_authority_epoch: 7,
        subscription_active: true,
        state,
        attempt: 1,
        max_pre_send_retries: 3,
        delivery_started: false,
        now_unix_ms: 100,
        expires_at_unix_ms: 1_000,
    }
}

#[test]
fn pending_is_dispatchable_but_known_pre_send_failure_requires_new_admission() {
    let pending = plan_notification_recovery(&input(NotificationRecoveryState::Pending)).unwrap();
    assert_eq!(
        pending.disposition,
        NotificationRecoveryDisposition::DispatchAllowed
    );
    assert!(!pending.new_authority_and_delivery_key_required);

    let retry =
        plan_notification_recovery(&input(NotificationRecoveryState::PreSendFailed)).unwrap();
    assert_eq!(
        retry.disposition,
        NotificationRecoveryDisposition::RetryRequiresReAdmission
    );
    assert_eq!(retry.next_attempt, 2);
    assert!(retry.new_authority_and_delivery_key_required);
    retry.validate().unwrap();
}

#[test]
fn in_flight_submitted_and_unknown_never_auto_retry() {
    for state in [
        NotificationRecoveryState::InFlight,
        NotificationRecoveryState::Submitted,
        NotificationRecoveryState::Unknown,
    ] {
        let plan = plan_notification_recovery(&input(state)).unwrap();
        assert_eq!(
            plan.disposition,
            NotificationRecoveryDisposition::AwaitReconciliation
        );
        assert!(!plan.new_authority_and_delivery_key_required);
    }
}

#[test]
fn cancel_revoke_expiry_and_ack_are_no_send_boundaries() {
    for state in [
        NotificationRecoveryState::CancelRequested,
        NotificationRecoveryState::Revoked,
        NotificationRecoveryState::Expired,
        NotificationRecoveryState::Acknowledged,
    ] {
        let plan = plan_notification_recovery(&input(state)).unwrap();
        assert_eq!(
            plan.disposition,
            NotificationRecoveryDisposition::TerminalNoSend
        );
    }
    let mut cancel_after_send = input(NotificationRecoveryState::CancelRequested);
    cancel_after_send.delivery_started = true;
    assert_eq!(
        plan_notification_recovery(&cancel_after_send)
            .unwrap()
            .disposition,
        NotificationRecoveryDisposition::AwaitReconciliation
    );
}

#[test]
fn authority_epoch_and_subscription_or_expiry_drift_require_reconciliation() {
    let mut epoch = input(NotificationRecoveryState::Pending);
    epoch.expected_authority_epoch = 8;
    assert_eq!(
        plan_notification_recovery(&epoch).unwrap().disposition,
        NotificationRecoveryDisposition::AwaitReconciliation
    );
    let mut revoked = input(NotificationRecoveryState::Pending);
    revoked.subscription_active = false;
    assert_eq!(
        plan_notification_recovery(&revoked).unwrap().disposition,
        NotificationRecoveryDisposition::AwaitReconciliation
    );
    let mut expired = input(NotificationRecoveryState::Pending);
    expired.now_unix_ms = 1_000;
    assert_eq!(
        plan_notification_recovery(&expired).unwrap().disposition,
        NotificationRecoveryDisposition::AwaitReconciliation
    );
}

#[test]
fn bounded_retry_limit_and_fixture_fail_closed() {
    let mut exhausted = input(NotificationRecoveryState::PreSendFailed);
    exhausted.attempt = 4;
    assert_eq!(
        plan_notification_recovery(&exhausted).unwrap().disposition,
        NotificationRecoveryDisposition::AwaitReconciliation
    );
    let fixture: Value =
        serde_json::from_str(include_str!("fixtures/nm18-notification-recovery.json"))
            .expect("valid NM-18 fixture");
    assert_eq!(fixture["schema"], "kiana.notification-recovery.v1");
    for denial in [
        "cancel_requested_is_not_delivered",
        "in_flight_unknown_is_not_resent",
        "unknown_is_not_automatically_retried",
        "retry_requires_new_authority_and_delivery_key",
    ] {
        assert!(fixture["deny_first"]
            .as_array()
            .unwrap()
            .iter()
            .any(|value| value == denial));
    }
}

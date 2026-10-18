//! NM-18 recovery fixtures: cancellation/revocation/expiry/Unknown dispositions, the dead-letter
//! case every uncertainty carries, retry re-admission fences and reconciliation closure. The gate
//! is a planner, so every fixture asserts typed plans/refusals and never assumes a channel, Broker
//! or second execution loop.

use kiana_core::{
    plan_notification_outbox_recovery, plan_notification_recovery, NotificationAttemptObservation,
    NotificationDeadLetterKind, NotificationDeliveryWorker, NotificationReconciliationOutcome,
    NotificationRecoveryDisposition, NotificationRecoveryInput, NotificationRecoveryResolution,
    NotificationRecoveryState, NotificationRetryAdmission, NOTIFICATION_RECOVERY_SCHEMA,
};
use kiana_domain::{
    MessageId, Notification, NotificationChannel, NotificationOutboxClass,
    NotificationOutboxRecord, Subscription, SubscriptionStatus,
};
use serde_json::Value;

const EPOCH: u64 = 7;
const WORKER_ID: &str = "worker-nm18";
const WINDOW_END: u64 = 10_000;

fn key_digest(nibble: char) -> String {
    format!("sha256:{}", nibble.to_string().repeat(64))
}

fn input(state: NotificationRecoveryState) -> NotificationRecoveryInput {
    NotificationRecoveryInput {
        schema: NOTIFICATION_RECOVERY_SCHEMA.to_owned(),
        notification_id: "notification:nm18".to_owned(),
        delivery_key_digest: key_digest('a'),
        authority_epoch: EPOCH,
        expected_authority_epoch: EPOCH,
        subscription_active: true,
        state,
        attempt: 1,
        max_pre_send_retries: 3,
        delivery_started: false,
        now_unix_ms: 100,
        expires_at_unix_ms: 1_000,
    }
}

fn notification(started_at: u64) -> Notification {
    Notification::new(
        MessageId::new(),
        "actor-18",
        None,
        vec!["project/a".to_owned()],
        NotificationChannel::InApp,
        started_at,
        WINDOW_END,
        1,
        None,
    )
    .expect("notification")
}

fn subscription(status: SubscriptionStatus) -> Subscription {
    let mut subscription = Subscription::new(
        "actor-18",
        None,
        vec!["project/a".to_owned()],
        vec![NotificationChannel::InApp],
        1,
        100,
        20_000,
    )
    .expect("subscription");
    subscription.status = status;
    subscription.subscription_digest = subscription.digest();
    subscription.validate().expect("subscription");
    subscription
}

fn record_for(
    subscription: &Subscription,
    notification: Notification,
    authority_epoch: u64,
) -> NotificationOutboxRecord {
    NotificationOutboxRecord::new(
        notification,
        NotificationOutboxClass::Critical,
        subscription.subscription_id,
        authority_epoch,
        100,
    )
    .expect("outbox record")
}

fn worker(id: &str, authority_epoch: u64) -> NotificationDeliveryWorker {
    NotificationDeliveryWorker::new(id, authority_epoch, 5_000).expect("worker")
}

#[test]
fn pending_is_dispatchable_but_known_pre_send_failure_requires_new_admission() {
    let pending = plan_notification_recovery(&input(NotificationRecoveryState::Pending)).unwrap();
    assert_eq!(
        pending.disposition,
        NotificationRecoveryDisposition::DispatchAllowed
    );
    assert!(!pending.new_authority_and_delivery_key_required);
    assert!(pending.dead_letter.is_none());

    let retry =
        plan_notification_recovery(&input(NotificationRecoveryState::PreSendFailed)).unwrap();
    assert_eq!(
        retry.disposition,
        NotificationRecoveryDisposition::RetryRequiresReAdmission
    );
    assert_eq!(retry.next_attempt, 2);
    assert!(retry.new_authority_and_delivery_key_required);
    assert!(retry.dead_letter.is_none());
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
        assert_eq!(
            plan.next_attempt,
            plan.dead_letter.as_ref().unwrap().attempt
        );
        let dead_letter = plan.dead_letter.as_ref().unwrap();
        assert_eq!(
            dead_letter.kind,
            NotificationDeadLetterKind::DeliveryOutcomeUnknown
        );
        assert!(!dead_letter.auto_retry_allowed);
        assert!(dead_letter.requires_reconciliation);
        assert_eq!(dead_letter.reason, "delivery_outcome_unknown");
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
        assert!(plan.dead_letter.is_none());
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
fn cancel_before_send_can_never_be_reconciled_as_delivered() {
    let cancel = plan_notification_recovery(&input(NotificationRecoveryState::CancelRequested))
        .expect("cancel plan");
    assert_eq!(
        cancel.disposition,
        NotificationRecoveryDisposition::TerminalNoSend
    );
    // A refused send has no dead-letter case, so no receipt can rewrite it as delivered.
    assert_eq!(
        cancel
            .reconcile(
                NotificationReconciliationOutcome::Delivered,
                &key_digest('b'),
                200
            )
            .unwrap_err(),
        "notification_reconciliation_no_case"
    );
    let mut cancel_after_send = input(NotificationRecoveryState::CancelRequested);
    cancel_after_send.delivery_started = true;
    let parked = plan_notification_recovery(&cancel_after_send).expect("parked plan");
    assert_eq!(
        parked.dead_letter.as_ref().unwrap().kind,
        NotificationDeadLetterKind::CancelRequestedAfterPossibleSend
    );
    assert_eq!(parked.next_attempt, 1);
}

#[test]
fn authority_epoch_and_subscription_or_expiry_drift_require_reconciliation() {
    let mut epoch = input(NotificationRecoveryState::Pending);
    epoch.expected_authority_epoch = 8;
    let epoch_plan = plan_notification_recovery(&epoch).unwrap();
    assert_eq!(
        epoch_plan.disposition,
        NotificationRecoveryDisposition::AwaitReconciliation
    );
    assert_eq!(
        epoch_plan.dead_letter.as_ref().unwrap().kind,
        NotificationDeadLetterKind::AuthorityEpochChanged
    );
    let mut revoked = input(NotificationRecoveryState::Pending);
    revoked.subscription_active = false;
    let revoked_plan = plan_notification_recovery(&revoked).unwrap();
    assert_eq!(
        revoked_plan.disposition,
        NotificationRecoveryDisposition::AwaitReconciliation
    );
    assert_eq!(
        revoked_plan.dead_letter.as_ref().unwrap().kind,
        NotificationDeadLetterKind::SubscriptionInactive
    );
    let mut expired = input(NotificationRecoveryState::Pending);
    expired.now_unix_ms = 1_000;
    let expired_plan = plan_notification_recovery(&expired).unwrap();
    assert_eq!(
        expired_plan.disposition,
        NotificationRecoveryDisposition::AwaitReconciliation
    );
    assert_eq!(
        expired_plan.dead_letter.as_ref().unwrap().kind,
        NotificationDeadLetterKind::ExpiredBeforeSend
    );
}

#[test]
fn revoke_or_expiry_after_a_possible_send_is_not_reclassified_as_no_send() {
    for (state, kind) in [
        (
            NotificationRecoveryState::Revoked,
            NotificationDeadLetterKind::SubscriptionInactive,
        ),
        (
            NotificationRecoveryState::Expired,
            NotificationDeadLetterKind::ExpiredAfterPossibleSend,
        ),
    ] {
        let mut started = input(state);
        started.delivery_started = true;
        let plan = plan_notification_recovery(&started).unwrap();
        assert_eq!(
            plan.disposition,
            NotificationRecoveryDisposition::AwaitReconciliation
        );
        assert_eq!(plan.dead_letter.as_ref().unwrap().kind, kind);
        assert!(!plan.new_authority_and_delivery_key_required);

        let mut not_started = input(state);
        not_started.delivery_started = false;
        let terminal = plan_notification_recovery(&not_started).unwrap();
        assert_eq!(
            terminal.disposition,
            NotificationRecoveryDisposition::TerminalNoSend
        );
        assert!(terminal.dead_letter.is_none());
    }
}

#[test]
fn retry_admission_requires_a_fresh_delivery_key_and_authority_epoch() {
    let retry =
        plan_notification_recovery(&input(NotificationRecoveryState::PreSendFailed)).unwrap();
    assert_eq!(
        retry.disposition,
        NotificationRecoveryDisposition::RetryRequiresReAdmission
    );

    // The previous key/epoch cannot be recycled into an admission.
    assert_eq!(
        NotificationRetryAdmission::new(
            retry.notification_id.clone(),
            retry.delivery_key_digest.clone(),
            EPOCH,
            retry.delivery_key_digest.clone(),
            EPOCH + 1,
            "operator-nm18",
            300,
        )
        .unwrap_err(),
        "notification_retry_admission_delivery_key_reused"
    );
    assert_eq!(
        NotificationRetryAdmission::new(
            retry.notification_id.clone(),
            retry.delivery_key_digest.clone(),
            EPOCH,
            key_digest('c'),
            EPOCH,
            "operator-nm18",
            300,
        )
        .unwrap_err(),
        "notification_retry_admission_epoch_not_advanced"
    );

    let fresh = NotificationRetryAdmission::new(
        retry.notification_id.clone(),
        retry.delivery_key_digest.clone(),
        EPOCH,
        key_digest('c'),
        EPOCH + 1,
        "operator-nm18",
        300,
    )
    .expect("fresh admission");
    fresh.authorize(&retry).expect("authorize");

    // An admission for another key/epoch or another disposition cannot unlock this plan.
    let stale = NotificationRetryAdmission::new(
        retry.notification_id.clone(),
        key_digest('d'),
        EPOCH,
        key_digest('c'),
        EPOCH + 1,
        "operator-nm18",
        300,
    )
    .expect("stale admission");
    assert_eq!(
        stale.authorize(&retry).unwrap_err(),
        "notification_retry_admission_previous_key_mismatch"
    );
    let found = worker(WORKER_ID, EPOCH);
    let uncertain_subscription = subscription(SubscriptionStatus::Active);
    let uncertain = plan_notification_outbox_recovery(
        &found,
        &uncertain_record(&uncertain_subscription),
        &uncertain_subscription,
        NotificationAttemptObservation::PossiblyStarted,
        false,
        3,
        200,
    )
    .expect("uncertain plan");
    assert_eq!(
        fresh.authorize(&uncertain).unwrap_err(),
        "notification_retry_admission_disposition_invalid"
    );
}

fn uncertain_record(subscription: &Subscription) -> NotificationOutboxRecord {
    let mut record = record_for(subscription, notification(100), EPOCH);
    let lease = record.claim(WORKER_ID, EPOCH, 100, 1_000).expect("claim");
    record.mark_submitted(&lease, 150).expect("submit");
    record
}

#[test]
fn reconciliation_closes_only_with_evidence_and_never_auto_retries() {
    let plan = plan_notification_recovery(&input(NotificationRecoveryState::InFlight)).unwrap();
    assert_eq!(
        plan.reconcile(
            NotificationReconciliationOutcome::Delivered,
            "not-a-digest",
            200
        )
        .unwrap_err(),
        "notification_reconciliation_evidence_invalid"
    );

    let delivered = plan
        .reconcile(
            NotificationReconciliationOutcome::Delivered,
            &key_digest('b'),
            200,
        )
        .expect("delivered resolution");
    assert_eq!(
        delivered.resolution,
        NotificationRecoveryResolution::TerminalAcknowledged
    );
    assert!(!delivered.retry_admission_required);
    assert!(!delivered.auto_retry_allowed);
    delivered.validate().unwrap();

    let not_delivered = plan
        .reconcile(
            NotificationReconciliationOutcome::NotDelivered,
            &key_digest('b'),
            200,
        )
        .expect("not delivered resolution");
    assert_eq!(
        not_delivered.resolution,
        NotificationRecoveryResolution::RetryAdmissionRequired
    );
    assert!(not_delivered.retry_admission_required);
    assert!(!not_delivered.auto_retry_allowed);

    let unknown = plan
        .reconcile(
            NotificationReconciliationOutcome::Unknown,
            &key_digest('b'),
            200,
        )
        .expect("unknown resolution");
    assert_eq!(
        unknown.resolution,
        NotificationRecoveryResolution::RemainsDeadLettered
    );
    assert!(!unknown.retry_admission_required);
    assert!(!unknown.auto_retry_allowed);
}

#[test]
fn outbox_bridge_dead_letters_uncertain_sends_and_refuses_foreign_claims() {
    let active = subscription(SubscriptionStatus::Active);
    let record = uncertain_record(&active);
    let plan = plan_notification_outbox_recovery(
        &worker(WORKER_ID, EPOCH),
        &record,
        &active,
        NotificationAttemptObservation::PossiblyStarted,
        false,
        3,
        200,
    )
    .expect("submitted plan");
    assert_eq!(
        plan.disposition,
        NotificationRecoveryDisposition::AwaitReconciliation
    );
    let dead_letter = plan.dead_letter.as_ref().expect("dead letter");
    assert_eq!(
        dead_letter.kind,
        NotificationDeadLetterKind::DeliveryOutcomeUnknown
    );
    assert_eq!(dead_letter.attempt, 1);
    let again = plan_notification_outbox_recovery(
        &worker(WORKER_ID, EPOCH),
        &record,
        &active,
        NotificationAttemptObservation::PossiblyStarted,
        false,
        3,
        200,
    )
    .expect("submitted plan again");
    assert_eq!(
        dead_letter.case_key(),
        again.dead_letter.unwrap().case_key()
    );

    let mut foreign = record_for(&active, notification(100), EPOCH);
    let _foreign_lease = foreign
        .claim("worker-other", EPOCH, 100, 1_000)
        .expect("foreign claim");
    assert_eq!(
        plan_notification_outbox_recovery(
            &worker(WORKER_ID, EPOCH),
            &foreign,
            &active,
            NotificationAttemptObservation::NeverStarted,
            false,
            3,
            200,
        )
        .unwrap_err(),
        "notification_recovery_foreign_live_claim"
    );

    let mut failed = record_for(&active, notification(100), EPOCH);
    let lease = failed.claim(WORKER_ID, EPOCH, 100, 1_000).expect("claim");
    failed.fail(&lease, 150).expect("known pre-send failure");
    let retry = plan_notification_outbox_recovery(
        &worker(WORKER_ID, EPOCH),
        &failed,
        &active,
        NotificationAttemptObservation::NeverStarted,
        false,
        3,
        200,
    )
    .expect("retry plan");
    assert_eq!(
        retry.disposition,
        NotificationRecoveryDisposition::RetryRequiresReAdmission
    );
    assert_eq!(retry.next_attempt, 2);
    assert!(retry.new_authority_and_delivery_key_required);
    assert!(retry.dead_letter.is_none());

    let uncertain = plan_notification_outbox_recovery(
        &worker(WORKER_ID, EPOCH),
        &failed,
        &active,
        NotificationAttemptObservation::PossiblyStarted,
        false,
        3,
        200,
    )
    .expect("uncertain plan");
    assert_eq!(
        uncertain.disposition,
        NotificationRecoveryDisposition::AwaitReconciliation
    );
    assert_eq!(
        uncertain.dead_letter.as_ref().unwrap().kind,
        NotificationDeadLetterKind::PreSendRetryNotAdmissible
    );
    assert_eq!(uncertain.next_attempt, 1);
}

#[test]
fn outbox_bridge_cancel_is_terminal_before_send_and_revocation_is_never_dispatched() {
    let active = subscription(SubscriptionStatus::Active);
    let mut claimed = record_for(&active, notification(100), EPOCH);
    let _lease = claimed.claim(WORKER_ID, EPOCH, 100, 1_000).expect("claim");
    let cancel = plan_notification_outbox_recovery(
        &worker(WORKER_ID, EPOCH),
        &claimed,
        &active,
        NotificationAttemptObservation::NeverStarted,
        true,
        3,
        200,
    )
    .expect("cancel plan");
    assert_eq!(
        cancel.disposition,
        NotificationRecoveryDisposition::TerminalNoSend
    );
    assert!(cancel.dead_letter.is_none());

    let revoked = subscription(SubscriptionStatus::Revoked);
    let mut record = record_for(&revoked, notification(100), EPOCH);
    let lease = record.claim(WORKER_ID, EPOCH, 100, 1_000).expect("claim");
    let plan = plan_notification_outbox_recovery(
        &worker(WORKER_ID, EPOCH),
        &record,
        &revoked,
        NotificationAttemptObservation::NeverStarted,
        false,
        3,
        200,
    )
    .expect("revoked plan");
    assert_ne!(
        plan.disposition,
        NotificationRecoveryDisposition::DispatchAllowed
    );
    assert_ne!(
        plan.disposition,
        NotificationRecoveryDisposition::RetryRequiresReAdmission
    );
    assert_eq!(
        plan.dead_letter.as_ref().unwrap().kind,
        NotificationDeadLetterKind::SubscriptionInactive
    );
    record.release(&lease, 150).expect("release");
    let released = plan_notification_outbox_recovery(
        &worker(WORKER_ID, EPOCH),
        &record,
        &revoked,
        NotificationAttemptObservation::NeverStarted,
        false,
        3,
        200,
    )
    .expect("revoked plan after release");
    assert_ne!(
        released.disposition,
        NotificationRecoveryDisposition::DispatchAllowed
    );
}

#[test]
fn bounded_retry_limit_and_fixture_fail_closed() {
    let mut exhausted = input(NotificationRecoveryState::PreSendFailed);
    exhausted.attempt = 4;
    let plan = plan_notification_recovery(&exhausted).unwrap();
    assert_eq!(
        plan.disposition,
        NotificationRecoveryDisposition::AwaitReconciliation
    );
    assert_eq!(
        plan.dead_letter.as_ref().unwrap().kind,
        NotificationDeadLetterKind::PreSendRetryNotAdmissible
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
        "cancel_requested_is_never_reconciled_as_delivered",
        "revoked_or_expired_after_send_started_is_not_terminal_no_send",
        "retry_admission_cannot_reuse_the_previous_delivery_key",
        "retry_admission_cannot_keep_the_previous_authority_epoch",
        "reconciliation_requires_an_evidence_digest",
        "reconciliation_unknown_never_auto_retries",
    ] {
        assert!(fixture["deny_first"]
            .as_array()
            .unwrap()
            .iter()
            .any(|value| value == denial));
    }
    for acceptance in [
        "uncertainty_is_dead_lettered_with_a_typed_case",
        "reconciliation_delivered_is_terminal_acknowledged",
        "reconciliation_not_delivered_requires_re_admission",
        "fresh_authority_and_delivery_key_admit_one_retry",
    ] {
        assert!(fixture["accepted"]
            .as_array()
            .unwrap()
            .iter()
            .any(|value| value == acceptance));
    }
    for (kind, requirement) in [
        ("delivery_outcome_unknown", "provider_receipt"),
        ("cancel_requested_after_possible_send", "operator_decision"),
        ("authority_epoch_changed", "operator_decision"),
    ] {
        assert!(fixture["dead_letter_kinds"]
            .as_array()
            .unwrap()
            .iter()
            .any(|value| value == kind));
        assert!(fixture["dead_letter_requirements"]
            .as_array()
            .unwrap()
            .iter()
            .any(|value| value == requirement));
    }
}

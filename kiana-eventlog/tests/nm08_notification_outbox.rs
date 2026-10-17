//! NM-08 durable outbox fixtures: claim-after-crash, lease expiry, revoked subscription, bounded
//! queue pressure, transition rebuild and JSONL restart. Deny-first: every test proves the refusal
//! before it proves the success path.

use kiana_domain::{
    rebuild_notification_outbox_state, ActionRefId, DeliveryAttemptId, DeliveryReceipt,
    DeliveryReceiptStatus, MessageId, Notification, NotificationChannel,
    NotificationOutboxAdmission, NotificationOutboxClass, NotificationOutboxFenceRefusal,
    NotificationOutboxPressure, NotificationOutboxRecord, NotificationOutboxShedReason,
    NotificationOutboxState, NotificationOutboxTerminalReason, NotificationOutboxTransition,
    Subscription, SubscriptionId, SubscriptionStatus,
};
use kiana_eventlog::MemoryNotificationOutboxStore;
use kiana_ports::{NotificationOutboxStore, PortError};
use serde_json::Value;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

const EPOCH: u64 = 7;

fn notification(started_at: u64) -> Notification {
    Notification::new(
        MessageId::new(),
        "actor-08",
        None,
        vec!["project/a".to_owned()],
        NotificationChannel::InApp,
        started_at,
        10_000,
        1,
        None,
    )
    .expect("notification")
}

fn actionable_notification(started_at: u64) -> Notification {
    Notification::new(
        MessageId::new(),
        "actor-08",
        None,
        vec!["project/a".to_owned()],
        NotificationChannel::InApp,
        started_at,
        10_000,
        1,
        Some(ActionRefId::new()),
    )
    .expect("actionable notification")
}

fn record() -> NotificationOutboxRecord {
    NotificationOutboxRecord::new(
        notification(100),
        NotificationOutboxClass::BestEffort,
        SubscriptionId::new(),
        EPOCH,
        100,
    )
    .expect("outbox record")
}

fn critical_record() -> NotificationOutboxRecord {
    NotificationOutboxRecord::new(
        notification(100),
        NotificationOutboxClass::Critical,
        SubscriptionId::new(),
        EPOCH,
        100,
    )
    .expect("critical outbox record")
}

fn actionable_record() -> NotificationOutboxRecord {
    NotificationOutboxRecord::new(
        actionable_notification(100),
        NotificationOutboxClass::Actionable,
        SubscriptionId::new(),
        EPOCH,
        100,
    )
    .expect("actionable outbox record")
}

fn subscription(id: SubscriptionId, status: SubscriptionStatus) -> Subscription {
    let mut subscription = Subscription::new(
        "actor-08",
        None,
        vec!["project/a".to_owned()],
        vec![NotificationChannel::InApp],
        1,
        100,
        20_000,
    )
    .expect("subscription");
    subscription.subscription_id = id;
    subscription.status = status;
    subscription.subscription_digest = subscription.digest();
    subscription.validate().expect("subscription");
    subscription
}

fn record_for(subscription: &Subscription, notification: Notification) -> NotificationOutboxRecord {
    NotificationOutboxRecord::new(
        notification,
        NotificationOutboxClass::BestEffort,
        subscription.subscription_id,
        EPOCH,
        100,
    )
    .expect("outbox record")
}

fn receipt(record: &NotificationOutboxRecord, observed_at: u64) -> DeliveryReceipt {
    DeliveryReceipt::new(
        record.notification.notification_id,
        record.attempt.delivery_attempt_id,
        record.notification.recipient_id.clone(),
        DeliveryReceiptStatus::Acknowledged,
        observed_at,
        None,
    )
    .expect("receipt")
}

fn temp_path(label: &str) -> PathBuf {
    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("clock")
        .as_nanos();
    std::env::temp_dir().join(format!(
        "kiana-nm08-{label}-{}-{stamp}.jsonl",
        std::process::id()
    ))
}

fn cleanup(path: &Path) {
    let _ = fs::remove_file(path);
}

#[test]
fn fixture_declares_durable_outbox_deny_first_contract() {
    let value: Value = serde_json::from_str(
        &fs::read_to_string(
            Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("tests/fixtures/nm08-notification-outbox.json"),
        )
        .expect("fixture"),
    )
    .expect("fixture json");
    assert_eq!(value["schema"], "kiana.notification-outbox-fixture.v1");
    let denied = value["denied"].as_array().expect("denied list");
    for needle in [
        "crash",
        "lease",
        "revoked",
        "queue",
        "duplicate",
        "shutdown",
    ] {
        assert!(
            denied
                .iter()
                .any(|item| item.as_str().is_some_and(|text| text.contains(needle))),
            "deny list is missing {needle}"
        );
    }
}

#[tokio::test]
async fn crash_after_claim_reclaims_a_new_attempt_and_fences_the_old_worker() {
    let store = MemoryNotificationOutboxStore::new();
    let inserted = store.enqueue_notification(record()).await.expect("enqueue");
    let (claimed, lease) = store
        .claim_next_notification("worker-a", EPOCH, 200, 10)
        .await
        .expect("claim")
        .expect("record");
    assert_eq!(claimed.state, NotificationOutboxState::Claimed);
    let crashed_intent = claimed.dispatch_intent(&lease).expect("intent");
    crashed_intent.validate().expect("intent validates");

    // worker-a "crashed" after claiming: the lease expires without a submit.
    let (reclaimed, new_lease) = store
        .claim_next_notification("worker-b", EPOCH, 211, 10)
        .await
        .expect("reclaim")
        .expect("record");
    assert_eq!(reclaimed.state, NotificationOutboxState::Claimed);
    assert_ne!(
        reclaimed.attempt.delivery_attempt_id,
        inserted.attempt.delivery_attempt_id
    );
    assert!(new_lease.lease_token > lease.lease_token);
    assert_ne!(
        crashed_intent.delivery_attempt_id, reclaimed.attempt.delivery_attempt_id,
        "the crashed attempt can never be submitted twice"
    );
    assert_eq!(
        store
            .mark_notification_submitted(&lease, 212)
            .await
            .expect_err("the crashed lease is expired"),
        PortError::Conflict("notification_outbox_lease_expired".to_owned())
    );
    let mut forged = new_lease.clone();
    forged.worker_id = "worker-a".to_owned();
    forged.lease_digest = forged.digest();
    assert_eq!(
        store
            .mark_notification_submitted(&forged, 213)
            .await
            .expect_err("a forged live lease is fenced"),
        PortError::Conflict("notification_outbox_lease_fence_mismatch".to_owned())
    );
    store
        .rebuild_report()
        .await
        .expect("the reclaimed chain rebuilds");
}

#[tokio::test]
async fn expired_claim_before_submit_reclaims_without_duplicate_dispatch() {
    let store = MemoryNotificationOutboxStore::new();
    store.enqueue_notification(record()).await.expect("enqueue");
    let (_, lease) = store
        .claim_next_notification("worker-a", EPOCH, 200, 10)
        .await
        .expect("claim")
        .expect("record");
    // A live lease is not reclaimable, and only the expired one becomes a new attempt.
    assert_eq!(
        store
            .claim_next_notification("worker-b", EPOCH, 205, 10)
            .await
            .expect("claim"),
        None
    );
    let (reclaimed, second) = store
        .claim_next_notification("worker-b", EPOCH, 210, 10)
        .await
        .expect("claim")
        .expect("record");
    assert_ne!(second.lease_token, lease.lease_token);
    assert_eq!(
        store
            .mark_notification_submitted(&lease, 211)
            .await
            .expect_err("the expired lease cannot submit"),
        PortError::Conflict("notification_outbox_lease_expired".to_owned())
    );
    reclaimed
        .dispatch_intent(&second)
        .expect("new intent")
        .validate()
        .expect("intent validates");
}

#[tokio::test]
async fn submitted_lease_expiry_reconciles_unknown_instead_of_retrying() {
    let store = MemoryNotificationOutboxStore::new();
    store.enqueue_notification(record()).await.expect("enqueue");
    let (_, lease) = store
        .claim_next_notification("worker-a", EPOCH, 200, 10)
        .await
        .expect("claim")
        .expect("record");
    store
        .mark_notification_submitted(&lease, 205)
        .await
        .expect("submit");
    assert_eq!(
        store
            .claim_next_notification("worker-b", EPOCH, 212, 10)
            .await
            .expect("claim"),
        None,
        "a submitted record is never re-claimed as pending"
    );
    let expired_lease_error = store
        .reconcile_notification_unknown(&lease, 212)
        .await
        .expect_err("an expired receipt lease is result_unknown, not a silent success");
    assert!(
        expired_lease_error
            .to_string()
            .starts_with("port_failed:result_unknown:"),
        "unexpected reconcile error: {expired_lease_error}"
    );
    let unknown = store
        .reconcile_notification_unknown(&lease, 209)
        .await
        .expect("reconcile within the lease");
    assert_eq!(unknown.state, NotificationOutboxState::Unknown);
    assert_eq!(unknown.terminal_reason, None);
    let report = store.rebuild_report().await.expect("rebuild");
    assert_eq!(report.records.len(), 1);
    assert_eq!(report.records[0].state, NotificationOutboxState::Unknown);
}

#[tokio::test]
async fn revoked_subscription_cannot_claim_dispatch_or_acknowledge() {
    let store = MemoryNotificationOutboxStore::new();
    let active = subscription(SubscriptionId::new(), SubscriptionStatus::Active);
    let record = store
        .enqueue_notification(record_for(&active, notification(100)))
        .await
        .expect("enqueue");
    let (_, lease) = store
        .claim_next_notification("worker-a", EPOCH, 150, 30)
        .await
        .expect("claim")
        .expect("record");
    let revoked = subscription(active.subscription_id, SubscriptionStatus::Revoked);

    assert_eq!(
        store
            .subscription_fence(&record.outbox_id.to_string(), &revoked, 150)
            .await
            .expect("fence"),
        Err(NotificationOutboxFenceRefusal::SubscriptionRevoked)
    );
    assert_eq!(
        record
            .check_subscription_fence(&revoked, 150)
            .expect_err("revoked"),
        "notification_outbox_subscription_revoked"
    );

    // The delivery pipeline abandons with the reason instead of dispatching stale content.
    let abandoned = store
        .abandon_notification(
            &lease,
            NotificationOutboxTerminalReason::SubscriptionRevoked,
            151,
        )
        .await
        .expect("abandon");
    assert_eq!(abandoned.state, NotificationOutboxState::Failed);
    assert_eq!(
        abandoned.terminal_reason,
        Some(NotificationOutboxTerminalReason::SubscriptionRevoked)
    );
    assert_eq!(
        abandoned
            .current_lease()
            .expect_err("no live lease after abandon"),
        "notification_outbox_lease_owner_missing"
    );
    assert_eq!(
        store
            .mark_notification_submitted(&lease, 152)
            .await
            .expect_err("an abandoned claim cannot submit"),
        PortError::Conflict("notification_outbox_lease_fence_mismatch".to_owned())
    );
    assert_eq!(
        store
            .acknowledge_notification(&lease, &receipt(&abandoned, 153), 153)
            .await
            .expect_err("an abandoned claim cannot acknowledge"),
        PortError::Conflict("notification_outbox_lease_fence_mismatch".to_owned())
    );
    assert_eq!(
        store
            .claim_next_notification("worker-b", EPOCH, 154, 10)
            .await
            .expect("claim"),
        None,
        "an abandoned record is never claimed again"
    );
    store.rebuild_report().await.expect("rebuild");

    // Paused/expired subscriptions and a closed window are refused with their own reason; a scope
    // mismatch is a caller error that must leave the stored intent untouched.
    let paused = subscription(active.subscription_id, SubscriptionStatus::Paused);
    let pending = store
        .enqueue_notification(record_for(&active, notification(110)))
        .await
        .expect("enqueue");
    assert_eq!(
        pending
            .check_subscription_fence(&paused, 150)
            .expect_err("paused"),
        "notification_outbox_subscription_paused"
    );
    let expired = subscription(active.subscription_id, SubscriptionStatus::Expired);
    assert_eq!(
        pending
            .check_subscription_fence(&expired, 150)
            .expect_err("expired"),
        "notification_outbox_subscription_expired"
    );
    let active_again = subscription(active.subscription_id, SubscriptionStatus::Active);
    assert_eq!(
        pending
            .check_subscription_fence(&active_again, 20_000)
            .expect_err("subscription window closed"),
        "notification_outbox_subscription_expired"
    );
    let mut wrong_recipient = subscription(active.subscription_id, SubscriptionStatus::Active);
    wrong_recipient.recipient_id = "actor-other".to_owned();
    wrong_recipient.subscription_digest = wrong_recipient.digest();
    let pending_id = pending.outbox_id.to_string();
    assert_eq!(
        store
            .subscription_fence(&pending_id, &wrong_recipient, 150)
            .await
            .expect("fence"),
        Err(NotificationOutboxFenceRefusal::SubscriptionScopeMismatch)
    );
    assert_eq!(
        pending
            .check_subscription_fence(&wrong_recipient, 150)
            .expect_err("scope mismatch"),
        "notification_outbox_subscription_scope_mismatch"
    );
    assert_eq!(
        store.read(&pending_id).await.expect("record"),
        pending,
        "a mismatched subscription never mutates the intent"
    );
}

#[tokio::test]
async fn queue_overflow_sheds_only_best_effort_with_reason_and_keeps_protected_records() {
    let store = MemoryNotificationOutboxStore::bounded(1);
    let first = store
        .admit_notification(record(), 150)
        .await
        .expect("admit");
    assert!(first.admitted);
    assert!(!first.protected);

    let shed = store
        .admit_notification(record(), 150)
        .await
        .expect("admit");
    assert!(!shed.admitted);
    assert_eq!(
        shed.shed_reason,
        Some(NotificationOutboxShedReason::QueueFull)
    );

    let expired = store
        .admit_notification(record(), 10_001)
        .await
        .expect("admit");
    assert_eq!(
        expired.shed_reason,
        Some(NotificationOutboxShedReason::NotificationExpired)
    );

    let protected = store
        .admit_notification(actionable_record(), 150)
        .await
        .expect("admit");
    assert!(protected.admitted);
    assert!(protected.protected);
    assert_eq!(protected.class, NotificationOutboxClass::Actionable);

    let critical = store
        .admit_notification(critical_record(), 150)
        .await
        .expect("admit");
    assert!(critical.admitted);
    assert!(critical.protected);

    let pressure = store.pressure().await;
    assert_eq!(pressure.admitted, 3);
    assert_eq!(pressure.shed, 2);
    assert_eq!(pressure.best_effort_shed, 2);
    assert_eq!(pressure.protected_shed, 0);
    assert_eq!(pressure.shed_by_reason.len(), 2);
    assert_eq!(
        pressure.shed_by_reason[0].reason,
        NotificationOutboxShedReason::QueueFull
    );
    assert_eq!(
        pressure.shed_by_reason[1].reason,
        NotificationOutboxShedReason::NotificationExpired
    );
    pressure.validate().expect("pressure stays explainable");
    assert_eq!(store.records().await.len(), 3);

    // The legacy enqueue entry point enforces the same bound but refuses instead of shedding.
    assert_eq!(
        store
            .enqueue_notification(record())
            .await
            .expect_err("bounded store refuses a best-effort overflow"),
        PortError::Conflict("notification_outbox_capacity_exceeded".to_owned())
    );
    store
        .enqueue_notification(actionable_record())
        .await
        .expect("protected records are never refused");
    assert_eq!(store.max_best_effort().await, Some(1));
}

#[test]
fn understated_class_and_protected_shed_are_refused() {
    let actionable = actionable_notification(100);
    assert_eq!(
        NotificationOutboxRecord::new(
            actionable.clone(),
            NotificationOutboxClass::BestEffort,
            SubscriptionId::new(),
            EPOCH,
            100
        )
        .expect_err("an approval intent cannot be declared best-effort"),
        "notification_outbox_class_understated"
    );
    assert_eq!(
        NotificationOutboxRecord::new(
            actionable.clone(),
            NotificationOutboxClass::Critical,
            SubscriptionId::new(),
            EPOCH,
            100
        )
        .expect_err("critical is still below the actionable floor"),
        "notification_outbox_class_understated"
    );
    assert_eq!(
        NotificationOutboxRecord::new(
            notification(100),
            NotificationOutboxClass::Actionable,
            SubscriptionId::new(),
            EPOCH,
            100
        )
        .expect_err("actionable requires an action ref"),
        "notification_outbox_class_invalid"
    );

    let approved = actionable_record();
    assert_eq!(
        NotificationOutboxAdmission::shed(&approved, NotificationOutboxShedReason::QueueFull)
            .expect_err("protected records cannot be shed"),
        "notification_outbox_protected_shed_refused"
    );
    let critical = critical_record();
    assert_eq!(
        critical.effective_class(),
        NotificationOutboxClass::Critical
    );
    assert!(critical.effective_class().protected());

    let mut forged = NotificationOutboxPressure::new();
    forged.protected_shed = 1;
    assert_eq!(
        forged
            .validate()
            .expect_err("a protected shed is never an acceptable ledger"),
        "notification_outbox_protected_shed_observed"
    );

    assert_eq!(
        record()
            .abandon_unclaimed(NotificationOutboxTerminalReason::DeliveryFailed, 150)
            .expect_err("a record without a delivery attempt cannot claim a delivery failure"),
        "notification_outbox_terminal_reason_invalid"
    );
    let failed = record()
        .abandon_unclaimed(
            NotificationOutboxTerminalReason::NotificationExpired,
            10_001,
        )
        .expect("an expired window is abandoned with a reason");
    assert_eq!(failed.state, NotificationOutboxState::Failed);
    assert_eq!(
        failed.terminal_reason,
        Some(NotificationOutboxTerminalReason::NotificationExpired)
    );
    failed.validate().expect("reason-bound failure validates");
}

#[tokio::test]
async fn lifecycle_is_rebuildable_from_the_transition_chain() {
    let store = MemoryNotificationOutboxStore::new();
    store.enqueue_notification(record()).await.expect("enqueue");
    let (_, lease) = store
        .claim_next_notification("worker-a", EPOCH, 200, 10)
        .await
        .expect("claim")
        .expect("record");
    let submitted = store
        .mark_notification_submitted(&lease, 205)
        .await
        .expect("submit");
    let acknowledged = store
        .acknowledge_notification(&lease, &receipt(&submitted, 206), 206)
        .await
        .expect("ack");
    assert_eq!(acknowledged.state, NotificationOutboxState::Acknowledged);
    assert_eq!(acknowledged.revision, 4);

    let report = store.rebuild_report().await.expect("rebuild");
    assert_eq!(report.records.len(), 1);
    assert_eq!(
        report.records[0].state,
        NotificationOutboxState::Acknowledged
    );
    assert_eq!(report.records[0].revision, 4);
    assert_eq!(report.records[0].transitions, 3);
    assert_eq!(report.records[0].last_claim_token, 1);
    report.validate().expect("rebuild report");

    // The same chain rebuilds through the public domain function.
    let chain = [
        NotificationOutboxTransition::new(
            acknowledged.outbox_id,
            2,
            NotificationOutboxState::Pending,
            NotificationOutboxState::Claimed,
            acknowledged.attempt.delivery_attempt_id,
            1,
            1,
            200,
        )
        .expect("claim transition"),
        NotificationOutboxTransition::new(
            acknowledged.outbox_id,
            3,
            NotificationOutboxState::Claimed,
            NotificationOutboxState::Submitted,
            acknowledged.attempt.delivery_attempt_id,
            1,
            1,
            205,
        )
        .expect("submit transition"),
        NotificationOutboxTransition::new(
            acknowledged.outbox_id,
            4,
            NotificationOutboxState::Submitted,
            NotificationOutboxState::Acknowledged,
            acknowledged.attempt.delivery_attempt_id,
            1,
            1,
            206,
        )
        .expect("ack transition"),
    ];
    let rebuilt = rebuild_notification_outbox_state(
        acknowledged.outbox_id,
        NotificationOutboxState::Pending,
        &chain,
    )
    .expect("chain rebuilds");
    assert_eq!(rebuilt.state, NotificationOutboxState::Acknowledged);
    assert_eq!(rebuilt.revision, acknowledged.revision);
    assert_eq!(rebuilt.transitions, 3);
    assert_eq!(rebuilt.last_claim_token, 1);

    // A chain that does not start at the creation state never rebuilds to the stored row.
    let diverged = rebuild_notification_outbox_state(
        acknowledged.outbox_id,
        NotificationOutboxState::Pending,
        &[NotificationOutboxTransition::new(
            acknowledged.outbox_id,
            3,
            NotificationOutboxState::Submitted,
            NotificationOutboxState::Failed,
            acknowledged.attempt.delivery_attempt_id,
            1,
            1,
            210,
        )
        .expect("transition")],
    )
    .expect_err("chain must start at the creation state");
    assert_eq!(diverged, "notification_outbox_rebuild_diverged");

    // A transition into Claimed must advance the claim token, so a stale claim cannot be re-fenced.
    let stale = NotificationOutboxTransition::new(
        acknowledged.outbox_id,
        2,
        NotificationOutboxState::Pending,
        NotificationOutboxState::Claimed,
        acknowledged.attempt.delivery_attempt_id,
        1,
        0,
        201,
    )
    .expect("transition");
    assert_eq!(
        rebuild_notification_outbox_state(
            acknowledged.outbox_id,
            NotificationOutboxState::Pending,
            &[stale]
        )
        .expect_err("zero claim token"),
        "notification_outbox_rebuild_claim_token_not_advanced"
    );
}

#[tokio::test]
async fn release_and_abandon_transitions_are_fenced_and_reason_bound() {
    let store = MemoryNotificationOutboxStore::new();
    store.enqueue_notification(record()).await.expect("enqueue");
    let (_, lease) = store
        .claim_next_notification("worker-a", EPOCH, 200, 30)
        .await
        .expect("claim")
        .expect("record");
    let released = store
        .release_notification_claim(&lease, 201)
        .await
        .expect("release");
    assert_eq!(released.state, NotificationOutboxState::Pending);
    assert_eq!(released.lease_owner, None);
    assert_eq!(released.terminal_reason, None);
    assert_eq!(
        store
            .mark_notification_submitted(&lease, 202)
            .await
            .expect_err("a released claim cannot submit"),
        PortError::Conflict("notification_outbox_lease_fence_mismatch".to_owned())
    );

    let (_, second) = store
        .claim_next_notification("worker-b", EPOCH, 203, 30)
        .await
        .expect("claim")
        .expect("record");
    assert!(second.lease_token > lease.lease_token);
    assert_eq!(
        store
            .release_notification_claim(&lease, 204)
            .await
            .expect_err("stale release"),
        PortError::Conflict("notification_outbox_lease_fence_mismatch".to_owned())
    );
    let abandoned = store
        .abandon_notification(
            &second,
            NotificationOutboxTerminalReason::SubscriptionRevoked,
            205,
        )
        .await
        .expect("abandon");
    assert_eq!(abandoned.state, NotificationOutboxState::Failed);
    assert_eq!(
        abandoned.terminal_reason,
        Some(NotificationOutboxTerminalReason::SubscriptionRevoked)
    );
    let report = store.rebuild_report().await.expect("rebuild");
    assert_eq!(report.records[0].transitions, 4);
    report.validate().expect("rebuild report");
}

#[tokio::test]
async fn jsonl_journal_replays_across_restart_and_continues_the_fence() {
    let path = temp_path("restart");
    let claimed_id: DeliveryAttemptId;
    let pending_id: DeliveryAttemptId;
    {
        let store = MemoryNotificationOutboxStore::open_jsonl_bounded(&path, Some(4))
            .expect("open journal");
        let first = store.enqueue_notification(record()).await.expect("enqueue");
        let second = store.enqueue_notification(record()).await.expect("enqueue");
        let (_, lease) = store
            .claim_next_notification("worker-a", EPOCH, 200, 10)
            .await
            .expect("claim")
            .expect("record");
        claimed_id = lease.outbox_id;
        pending_id = if first.outbox_id == claimed_id {
            second.outbox_id
        } else {
            first.outbox_id
        };
        assert!(lease.lease_token > 0);
        assert_eq!(store.pressure().await.admitted, 2);
    }

    let reopened =
        MemoryNotificationOutboxStore::open_jsonl_bounded(&path, Some(4)).expect("replay journal");
    let records = reopened.records().await;
    assert_eq!(records.len(), 2);
    let restored = reopened
        .read(&claimed_id.to_string())
        .await
        .expect("restored claim");
    assert_eq!(restored.state, NotificationOutboxState::Claimed);
    assert_eq!(restored.lease_owner.as_deref(), Some("worker-a"));
    assert_eq!(
        reopened.pressure().await.admitted,
        2,
        "pressure counters survive the restart"
    );
    reopened
        .rebuild_report()
        .await
        .expect("restored chain rebuilds");

    // Restart must not steal a live claim; the pending record is claimable, the live one is not.
    let (next, _) = reopened
        .claim_next_notification("worker-b", EPOCH, 205, 10)
        .await
        .expect("claim")
        .expect("record");
    assert_eq!(next.outbox_id, pending_id);
    assert_eq!(
        reopened
            .read(&claimed_id.to_string())
            .await
            .expect("live claim stays with worker-a")
            .lease_owner
            .as_deref(),
        Some("worker-a")
    );

    // The expired claim is reclaimed as a new attempt after the restart.
    let claimed_lease = restored.current_lease().expect("restored lease");
    let (reclaimed, new_lease) = reopened
        .claim_next_notification("worker-b", EPOCH, 211, 10)
        .await
        .expect("reclaim")
        .expect("record");
    assert_eq!(reclaimed.outbox_id, claimed_id);
    assert_ne!(
        reclaimed.attempt.delivery_attempt_id,
        restored.attempt.delivery_attempt_id
    );
    assert!(new_lease.lease_token > claimed_lease.lease_token);
    assert!(reopened
        .mark_notification_submitted(&claimed_lease, 212)
        .await
        .is_err());
    reopened
        .rebuild_report()
        .await
        .expect("post-restart rebuild");
    cleanup(&path);
}

#[tokio::test]
async fn jsonl_journal_refuses_tampered_truncated_and_permissive_files() {
    let path = temp_path("tamper");
    {
        let store = MemoryNotificationOutboxStore::open_jsonl(&path).expect("open journal");
        store.enqueue_notification(record()).await.expect("enqueue");
        let (_, lease) = store
            .claim_next_notification("worker-a", EPOCH, 200, 10)
            .await
            .expect("claim")
            .expect("record");
        store
            .mark_notification_submitted(&lease, 205)
            .await
            .expect("submit");
    }
    let contents = fs::read_to_string(&path).expect("journal");
    assert!(contents.contains("\"record\""));

    let tampered = contents.replacen("\"state\":\"submitted\"", "\"state\":\"pending\"", 1);
    assert_ne!(contents, tampered, "the journal must carry a record state");
    fs::write(&path, &tampered).expect("tamper");
    match MemoryNotificationOutboxStore::open_jsonl(&path) {
        Err(PortError::Failed(reason)) => assert!(
            reason.contains("notification_outbox_journal_corrupt"),
            "unexpected journal refusal: {reason}"
        ),
        other => panic!("tampered journal must be refused, got {other:?}"),
    }

    let record_lines = contents
        .lines()
        .filter(|line| line.contains("\"record\""))
        .collect::<Vec<_>>();
    assert!(record_lines.len() >= 3);
    fs::write(&path, format!("{}\n", record_lines[record_lines.len() - 1])).expect("truncate");
    match MemoryNotificationOutboxStore::open_jsonl(&path) {
        Err(PortError::Failed(reason)) => assert!(
            reason.contains("truncated_or_unbound_record"),
            "unexpected truncation refusal: {reason}"
        ),
        other => panic!("truncated journal must be refused, got {other:?}"),
    }

    let mut reverted = contents.clone();
    reverted.push_str(record_lines[0]);
    reverted.push('\n');
    fs::write(&path, reverted).expect("revert");
    match MemoryNotificationOutboxStore::open_jsonl(&path) {
        Err(PortError::Failed(reason)) => assert!(
            reason.contains("notification_outbox_journal_corrupt"),
            "unexpected reversion refusal: {reason}"
        ),
        other => panic!("reverted journal must be refused, got {other:?}"),
    }

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::write(&path, &contents).expect("restore");
        fs::set_permissions(&path, fs::Permissions::from_mode(0o644)).expect("permissions");
        match MemoryNotificationOutboxStore::open_jsonl(&path) {
            Err(PortError::Failed(reason)) => assert_eq!(
                reason, "notification_outbox_journal_permissions_too_broad",
                "unexpected permission refusal: {reason}"
            ),
            other => panic!("world-readable journal must be refused, got {other:?}"),
        }
    }
    cleanup(&path);
}

#[tokio::test]
async fn durable_store_never_duplicates_a_dispatch_after_repeated_replays() {
    let path = temp_path("replay");
    let outbox_id: DeliveryAttemptId;
    {
        let store = MemoryNotificationOutboxStore::open_jsonl(&path).expect("open journal");
        let inserted = store.enqueue_notification(record()).await.expect("enqueue");
        outbox_id = inserted.outbox_id;
        let (_, lease) = store
            .claim_next_notification("worker-a", EPOCH, 200, 30)
            .await
            .expect("claim")
            .expect("record");
        store
            .mark_notification_submitted(&lease, 205)
            .await
            .expect("submit");
    }
    // Two independent replays must agree byte-for-byte and keep the same fence.
    let first = MemoryNotificationOutboxStore::open_jsonl(&path).expect("replay one");
    let second = MemoryNotificationOutboxStore::open_jsonl(&path).expect("replay two");
    let first_record = first.read(&outbox_id.to_string()).await.expect("record");
    let second_record = second.read(&outbox_id.to_string()).await.expect("record");
    assert_eq!(first_record, second_record);
    assert_eq!(first_record.state, NotificationOutboxState::Submitted);
    assert_eq!(
        first.rebuild_report().await.expect("rebuild").report_digest,
        second
            .rebuild_report()
            .await
            .expect("rebuild")
            .report_digest
    );
    assert_eq!(
        first
            .claim_next_notification("worker-b", EPOCH, 240, 10)
            .await
            .expect("claim"),
        None
    );
    assert_eq!(
        first.pressure().await,
        second.pressure().await,
        "pressure ledger replays identically"
    );
    cleanup(&path);
}

//! NM-08 delivery worker fixtures: subscription fence, restart recovery plan and bounded shutdown
//! drain. The worker is a planner, so every test asserts the typed plan (or the refusal) and never
//! assumes a channel, Broker or second execution loop.

use kiana_core::{
    NotificationDeliveryPlan, NotificationDeliveryWorker, NotificationOutboxDrainAction,
    NotificationOutboxDrainPolicy,
};
use kiana_domain::{
    ActionRefId, DeliveryAttemptId, DeliveryReceipt, DeliveryReceiptStatus, MessageId,
    Notification, NotificationChannel, NotificationOutboxClass, NotificationOutboxLease,
    NotificationOutboxRecord, NotificationOutboxState, NotificationOutboxTerminalReason,
    Subscription, SubscriptionId, SubscriptionStatus,
};
use kiana_eventlog::MemoryNotificationOutboxStore;
use kiana_ports::NotificationOutboxStore;
use std::fs;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

const EPOCH: u64 = 7;
const WORKER_ID: &str = "worker-a";
const WINDOW_END: u64 = 10_000;

fn notification(started_at: u64) -> Notification {
    Notification::new(
        MessageId::new(),
        "actor-08",
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

fn actionable_notification(started_at: u64) -> Notification {
    Notification::new(
        MessageId::new(),
        "actor-08",
        None,
        vec!["project/a".to_owned()],
        NotificationChannel::InApp,
        started_at,
        WINDOW_END,
        1,
        Some(ActionRefId::new()),
    )
    .expect("actionable notification")
}

fn record(
    class: NotificationOutboxClass,
    notification: Notification,
    authority_epoch: u64,
) -> NotificationOutboxRecord {
    NotificationOutboxRecord::new(
        notification,
        class,
        SubscriptionId::new(),
        authority_epoch,
        100,
    )
    .expect("outbox record")
}

fn record_for(
    subscription: &Subscription,
    class: NotificationOutboxClass,
    notification: Notification,
) -> NotificationOutboxRecord {
    NotificationOutboxRecord::new(
        notification,
        class,
        subscription.subscription_id,
        EPOCH,
        100,
    )
    .expect("outbox record")
}

fn subscription(status: SubscriptionStatus, revision: u64) -> Subscription {
    let mut subscription = Subscription::new(
        "actor-08",
        None,
        vec!["project/a".to_owned()],
        vec![NotificationChannel::InApp],
        revision,
        100,
        20_000,
    )
    .expect("subscription");
    subscription.status = status;
    subscription.subscription_digest = subscription.digest();
    subscription.validate().expect("subscription");
    subscription
}

fn with_status(source: &Subscription, status: SubscriptionStatus) -> Subscription {
    let mut subscription = source.clone();
    subscription.status = status;
    subscription.subscription_digest = subscription.digest();
    subscription.validate().expect("subscription");
    subscription
}

fn claimed(
    class: NotificationOutboxClass,
    notification: Notification,
    authority_epoch: u64,
    now_unix_ms: u64,
    lease_ttl_ms: u64,
) -> (NotificationOutboxRecord, NotificationOutboxLease) {
    let mut record = record(class, notification, authority_epoch);
    let lease = record
        .claim(WORKER_ID, authority_epoch, now_unix_ms, lease_ttl_ms)
        .expect("claim");
    (record, lease)
}

fn submitted(
    class: NotificationOutboxClass,
    notification: Notification,
    now_unix_ms: u64,
    lease_ttl_ms: u64,
) -> (NotificationOutboxRecord, NotificationOutboxLease) {
    let (mut record, lease) = claimed(class, notification, EPOCH, 200, lease_ttl_ms);
    record.mark_submitted(&lease, now_unix_ms).expect("submit");
    (record, lease)
}

fn acknowledge(record: &mut NotificationOutboxRecord, lease: &NotificationOutboxLease, now: u64) {
    let receipt = DeliveryReceipt::new(
        record.notification.notification_id,
        record.attempt.delivery_attempt_id,
        record.notification.recipient_id.clone(),
        DeliveryReceiptStatus::Acknowledged,
        now,
        None,
    )
    .expect("receipt");
    record.acknowledge(lease, &receipt, now).expect("ack");
}

fn worker() -> NotificationDeliveryWorker {
    NotificationDeliveryWorker::new(WORKER_ID, EPOCH, 30).expect("worker")
}

fn other_worker() -> NotificationDeliveryWorker {
    NotificationDeliveryWorker::new("worker-b", EPOCH, 30).expect("worker")
}

fn temp_path(label: &str) -> PathBuf {
    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("clock")
        .as_nanos();
    std::env::temp_dir().join(format!(
        "kiana-nm08-worker-{label}-{}-{stamp}.jsonl",
        std::process::id()
    ))
}

#[test]
fn worker_refuses_stale_epoch_missing_lease_and_tampered_digest() {
    let worker = worker();
    let stale = record(
        NotificationOutboxClass::BestEffort,
        notification(100),
        EPOCH + 1,
    );
    assert_eq!(
        worker.plan(&stale, 150).expect("plan"),
        NotificationDeliveryPlan::ReconcileUnknown
    );

    let (mut orphan, _) = claimed(
        NotificationOutboxClass::BestEffort,
        notification(100),
        EPOCH,
        200,
        30,
    );
    orphan.lease_owner = None;
    orphan.lease_token = 0;
    orphan.lease_expires_at_unix_ms = None;
    orphan.record_digest = orphan.digest();
    orphan.validate().expect("orphan claim is self-consistent");
    assert_eq!(
        worker.plan(&orphan, 201).expect_err("no lease to dispatch"),
        "notification_delivery_lease_missing"
    );

    let (claimed_record, _) = claimed(
        NotificationOutboxClass::BestEffort,
        notification(100),
        EPOCH,
        200,
        30,
    );
    let mut tampered = claimed_record.clone();
    tampered.record_digest = format!("sha256:{}", "0".repeat(64));
    assert_eq!(
        worker.plan(&tampered, 201).expect_err("digest first"),
        "notification_outbox_record_digest_mismatch"
    );

    // A claim owned by another live worker is never dispatched from here.
    let (mut foreign, _) = claimed(
        NotificationOutboxClass::BestEffort,
        notification(100),
        EPOCH,
        200,
        30,
    );
    foreign.lease_owner = Some("worker-b".to_owned());
    foreign.record_digest = foreign.digest();
    assert_eq!(
        worker.plan(&foreign, 201).expect("plan"),
        NotificationDeliveryPlan::ForeignClaim
    );
    let owner_lease = foreign.current_lease().expect("owner lease");
    assert_eq!(owner_lease.worker_id, "worker-b");
    assert_eq!(
        other_worker()
            .plan(&foreign, 201)
            .expect("owner may still plan"),
        NotificationDeliveryPlan::Dispatch(foreign.dispatch_intent(&owner_lease).expect("intent"))
    );
}

#[test]
fn worker_abandons_expired_notification_and_never_dispatches_stale_content() {
    let worker = worker();
    let pending = record(
        NotificationOutboxClass::BestEffort,
        notification(100),
        EPOCH,
    );
    assert_eq!(
        worker.plan(&pending, WINDOW_END).expect("plan"),
        NotificationDeliveryPlan::Abandon {
            reason: NotificationOutboxTerminalReason::NotificationExpired
        }
    );

    let (claimed_live, _) = claimed(
        NotificationOutboxClass::BestEffort,
        notification(100),
        EPOCH,
        WINDOW_END,
        30,
    );
    assert_eq!(
        worker.plan(&claimed_live, WINDOW_END + 1).expect("plan"),
        NotificationDeliveryPlan::Abandon {
            reason: NotificationOutboxTerminalReason::NotificationExpired
        },
        "a closed window is abandoned even while the lease is still live"
    );

    let (mut submitted_live, submitted_lease) = claimed(
        NotificationOutboxClass::BestEffort,
        notification(100),
        EPOCH,
        WINDOW_END,
        30,
    );
    submitted_live
        .mark_submitted(&submitted_lease, WINDOW_END)
        .expect("submit inside the live claim");
    assert_eq!(
        worker.plan(&submitted_live, WINDOW_END + 1).expect("plan"),
        NotificationDeliveryPlan::AwaitReceipt,
        "an in-flight send keeps its receipt path"
    );
}

#[test]
fn worker_abandons_revoked_paused_and_expired_subscription_with_reason() {
    let worker = worker();
    let active = subscription(SubscriptionStatus::Active, 1);
    let pending = record_for(
        &active,
        NotificationOutboxClass::BestEffort,
        notification(100),
    );
    assert_eq!(
        worker
            .plan_with_subscription(&pending, &active, 150)
            .expect("plan"),
        NotificationDeliveryPlan::ClaimRequired
    );

    let revoked = with_status(&active, SubscriptionStatus::Revoked);
    assert_eq!(
        worker
            .plan_with_subscription(&pending, &revoked, 150)
            .expect("plan"),
        NotificationDeliveryPlan::Abandon {
            reason: NotificationOutboxTerminalReason::SubscriptionRevoked
        }
    );

    let mut claimed_record = record_for(
        &active,
        NotificationOutboxClass::BestEffort,
        notification(100),
    );
    let lease = claimed_record
        .claim(WORKER_ID, EPOCH, 200, 30)
        .expect("claim");
    assert_eq!(
        worker
            .plan_with_subscription(&claimed_record, &revoked, 201)
            .expect("plan"),
        NotificationDeliveryPlan::Abandon {
            reason: NotificationOutboxTerminalReason::SubscriptionRevoked
        },
        "a live claim of this worker is abandoned before any dispatch"
    );
    let paused = with_status(&active, SubscriptionStatus::Paused);
    assert_eq!(
        worker
            .plan_with_subscription(&pending, &paused, 150)
            .expect("plan"),
        NotificationDeliveryPlan::Abandon {
            reason: NotificationOutboxTerminalReason::SubscriptionPaused
        }
    );
    let expired = with_status(&active, SubscriptionStatus::Expired);
    assert_eq!(
        worker
            .plan_with_subscription(&pending, &expired, 150)
            .expect("plan"),
        NotificationDeliveryPlan::Abandon {
            reason: NotificationOutboxTerminalReason::SubscriptionExpired
        }
    );
    assert_eq!(
        worker
            .plan_with_subscription(&pending, &active, 20_000)
            .expect("plan"),
        NotificationDeliveryPlan::Abandon {
            reason: NotificationOutboxTerminalReason::SubscriptionExpired
        },
        "a subscription past its own window is terminal"
    );

    // A foreign live claim is not abandoned from here: its owner or the reclaiming worker decides.
    let mut foreign_claim = claimed_record.clone();
    foreign_claim.lease_owner = Some("worker-other".to_owned());
    foreign_claim.record_digest = foreign_claim.digest();
    assert_eq!(
        worker
            .plan_with_subscription(&foreign_claim, &revoked, 201)
            .expect("plan"),
        NotificationDeliveryPlan::ForeignClaim
    );

    let (submitted_live, _) = {
        let mut record = claimed_record.clone();
        record.mark_submitted(&lease, 202).expect("submit");
        (record, lease)
    };
    assert_eq!(
        worker
            .plan_with_subscription(&submitted_live, &revoked, 203)
            .expect("plan"),
        NotificationDeliveryPlan::AwaitReceipt
    );
}

#[test]
fn worker_refuses_mismatched_subscription_identity_and_scope() {
    let worker = worker();
    let active = subscription(SubscriptionStatus::Active, 1);
    let pending = record_for(
        &active,
        NotificationOutboxClass::BestEffort,
        notification(100),
    );

    let other_subscription = subscription(SubscriptionStatus::Active, 1);
    assert_eq!(
        worker
            .plan_with_subscription(&pending, &other_subscription, 150)
            .expect_err("foreign subscription id"),
        "notification_outbox_subscription_mismatch"
    );
    let newer_revision = subscription(SubscriptionStatus::Active, 2);
    assert_eq!(
        worker
            .plan_with_subscription(&pending, &newer_revision, 150)
            .expect_err("stale revision"),
        "notification_outbox_subscription_revision_mismatch"
    );
    let mut wrong_recipient = active.clone();
    wrong_recipient.recipient_id = "actor-other".to_owned();
    wrong_recipient.subscription_digest = wrong_recipient.digest();
    assert_eq!(
        worker
            .plan_with_subscription(&pending, &wrong_recipient, 150)
            .expect_err("scope mismatch"),
        "notification_outbox_subscription_scope_mismatch"
    );
    assert_eq!(
        pending.state,
        NotificationOutboxState::Pending,
        "a refused fence never mutates the record"
    );
    assert_eq!(
        worker
            .plan_with_subscription(&pending, &active, 150)
            .expect("plan"),
        NotificationDeliveryPlan::ClaimRequired,
        "the right subscription still plans the happy path"
    );
}

#[test]
fn worker_recovery_plan_restores_each_lifecycle_state_deterministically() {
    let worker = worker();
    let pending = record(
        NotificationOutboxClass::BestEffort,
        notification(100),
        EPOCH,
    );
    let (claimed_live, _) = claimed(
        NotificationOutboxClass::BestEffort,
        notification(100),
        EPOCH,
        200,
        30,
    );
    let (claimed_expired, _) = claimed(
        NotificationOutboxClass::BestEffort,
        notification(100),
        EPOCH,
        200,
        10,
    );
    let (submitted_live, _) = submitted(
        NotificationOutboxClass::BestEffort,
        notification(100),
        201,
        30,
    );
    let (mut acknowledged, ack_lease) = submitted(
        NotificationOutboxClass::BestEffort,
        notification(100),
        201,
        30,
    );
    acknowledge(&mut acknowledged, &ack_lease, 202);
    let (mut failed, fail_lease) = claimed(
        NotificationOutboxClass::BestEffort,
        notification(100),
        EPOCH,
        200,
        30,
    );
    failed.fail(&fail_lease, 203).expect("fail");
    let (mut unknown, unknown_lease) = submitted(
        NotificationOutboxClass::BestEffort,
        notification(100),
        201,
        30,
    );
    unknown
        .reconcile_unknown(&unknown_lease, 204)
        .expect("unknown");

    let records = vec![
        pending.clone(),
        claimed_live.clone(),
        claimed_expired.clone(),
        submitted_live.clone(),
        acknowledged.clone(),
        failed.clone(),
        unknown.clone(),
    ];
    let steps = worker.recovery_plan(&records, 211).expect("recovery plan");
    assert_eq!(steps.len(), records.len());
    let plan_for = |id: DeliveryAttemptId| {
        steps
            .iter()
            .find(|step| step.outbox_id == id)
            .map(|step| step.plan.clone())
            .expect("step")
    };
    assert_eq!(
        plan_for(pending.outbox_id),
        NotificationDeliveryPlan::ClaimRequired
    );
    assert_eq!(
        plan_for(claimed_live.outbox_id),
        NotificationDeliveryPlan::Dispatch(
            claimed_live
                .dispatch_intent(&claimed_live.current_lease().expect("lease"))
                .expect("intent")
        )
    );
    assert_eq!(
        plan_for(claimed_expired.outbox_id),
        NotificationDeliveryPlan::ReclaimExpiredClaim
    );
    assert_eq!(
        plan_for(submitted_live.outbox_id),
        NotificationDeliveryPlan::AwaitReceipt
    );
    assert_eq!(
        plan_for(acknowledged.outbox_id),
        NotificationDeliveryPlan::Terminal
    );
    assert_eq!(
        plan_for(failed.outbox_id),
        NotificationDeliveryPlan::Terminal
    );
    assert_eq!(
        plan_for(unknown.outbox_id),
        NotificationDeliveryPlan::Terminal
    );

    // Deterministic: the same restored set plans identically, and a duplicate is refused.
    assert_eq!(steps, worker.recovery_plan(&records, 211).expect("replan"));
    assert_eq!(
        worker
            .recovery_plan(&[pending.clone(), pending], 211)
            .expect_err("duplicate record"),
        "notification_delivery_recovery_duplicate_record"
    );
}

#[tokio::test]
async fn worker_drain_refuses_a_foreign_claim_and_a_stale_deadline() {
    let worker = worker();
    let store = MemoryNotificationOutboxStore::new();
    store
        .enqueue_notification(record(
            NotificationOutboxClass::BestEffort,
            notification(100),
            EPOCH,
        ))
        .await
        .expect("enqueue");
    store
        .claim_next_notification("worker-other", EPOCH, 200, 60)
        .await
        .expect("claim")
        .expect("record");

    let policy = NotificationOutboxDrainPolicy::new(&worker, 300, 4).expect("policy");
    let records = store.records().await;
    assert_eq!(
        worker
            .drain(&records, &policy, 200)
            .expect_err("another worker owns the claim"),
        "notification_outbox_drain_foreign_claim"
    );
    assert_eq!(
        store
            .read(&records[0].outbox_id.to_string())
            .await
            .expect("record"),
        records[0],
        "a refused drain leaves the claim untouched"
    );

    let stale_deadline = NotificationOutboxDrainPolicy::new(&worker, 199, 4).expect("policy");
    assert_eq!(
        worker
            .drain(&[], &stale_deadline, 200)
            .expect_err("the shutdown window already closed"),
        "notification_outbox_drain_deadline_exceeded"
    );

    let foreign_policy =
        NotificationOutboxDrainPolicy::new(&other_worker(), 300, 4).expect("policy");
    assert_eq!(
        worker
            .drain(&[], &foreign_policy, 200)
            .expect_err("policy belongs to another worker"),
        "notification_outbox_drain_worker_mismatch"
    );

    let stale_epoch = record(
        NotificationOutboxClass::BestEffort,
        notification(100),
        EPOCH + 1,
    );
    assert_eq!(
        worker
            .drain(&[stale_epoch], &policy, 200)
            .expect_err("epoch mismatch"),
        "notification_outbox_drain_epoch_mismatch"
    );
}

#[tokio::test]
async fn worker_drain_is_bounded_and_reports_outstanding_protected() {
    let worker = worker();
    let store = MemoryNotificationOutboxStore::new();

    // Only one record is claimable at a time, so each claim is deterministic.
    let actionable = store
        .enqueue_notification(
            NotificationOutboxRecord::new(
                actionable_notification(100),
                NotificationOutboxClass::Actionable,
                SubscriptionId::new(),
                EPOCH,
                100,
            )
            .expect("actionable record"),
        )
        .await
        .expect("enqueue");
    let (_, actionable_lease) = store
        .claim_next_notification(WORKER_ID, EPOCH, 200, 60)
        .await
        .expect("claim")
        .expect("record");
    let critical = store
        .enqueue_notification(
            NotificationOutboxRecord::new(
                notification(100),
                NotificationOutboxClass::Critical,
                SubscriptionId::new(),
                EPOCH,
                100,
            )
            .expect("critical record"),
        )
        .await
        .expect("enqueue");
    let (_, critical_lease) = store
        .claim_next_notification(WORKER_ID, EPOCH, 200, 60)
        .await
        .expect("claim")
        .expect("record");
    let submitted_record = store
        .enqueue_notification(record(
            NotificationOutboxClass::BestEffort,
            notification(100),
            EPOCH,
        ))
        .await
        .expect("enqueue");
    let (_, submitted_lease) = store
        .claim_next_notification(WORKER_ID, EPOCH, 200, 60)
        .await
        .expect("claim")
        .expect("record");
    store
        .mark_notification_submitted(&submitted_lease, 201)
        .await
        .expect("submit");
    let acknowledged_record = store
        .enqueue_notification(record(
            NotificationOutboxClass::BestEffort,
            notification(100),
            EPOCH,
        ))
        .await
        .expect("enqueue");
    let (_, ack_lease) = store
        .claim_next_notification(WORKER_ID, EPOCH, 200, 60)
        .await
        .expect("claim")
        .expect("record");
    store
        .mark_notification_submitted(&ack_lease, 201)
        .await
        .expect("submit");
    let receipt = DeliveryReceipt::new(
        acknowledged_record.notification.notification_id,
        acknowledged_record.attempt.delivery_attempt_id,
        acknowledged_record.notification.recipient_id.clone(),
        DeliveryReceiptStatus::Acknowledged,
        202,
        None,
    )
    .expect("receipt");
    store
        .acknowledge_notification(&ack_lease, &receipt, 202)
        .await
        .expect("ack");
    let pending = store
        .enqueue_notification(record(
            NotificationOutboxClass::BestEffort,
            notification(100),
            EPOCH,
        ))
        .await
        .expect("enqueue");

    let records = store.records().await;
    assert_eq!(records.len(), 5);

    // A one-release budget can only hand back one of the two live claims; the other protected
    // record stays claimed and is reported as outstanding instead of being rerouted or dropped.
    let strict = NotificationOutboxDrainPolicy::new(&worker, 300, 1).expect("policy");
    let strict_report = worker.drain(&records, &strict, 200).expect("drain plan");
    assert_eq!(strict_report.considered, 5);
    assert_eq!(strict_report.released, 1);
    assert_eq!(strict_report.await_receipt, 1);
    assert_eq!(strict_report.untouched, 2);
    assert_eq!(strict_report.terminal, 1);
    assert_eq!(
        strict_report.protected_outstanding, 2,
        "the released approval and the budget-blocked incident stay visible"
    );
    assert!(strict_report.budget_exhausted);
    strict_report.validate().expect("report validates");
    assert_eq!(
        strict_report
            .dispositions
            .iter()
            .filter(|disposition| disposition.class.protected()
                && disposition.action == NotificationOutboxDrainAction::Untouched)
            .count(),
        1,
        "exactly one protected claim is blocked by the budget"
    );

    // With the budget raised the same records both become releasable, so the first drain was
    // limited by the budget and not by the claim state.
    let full = NotificationOutboxDrainPolicy::new(&worker, 300, 2).expect("policy");
    let report = worker.drain(&records, &full, 200).expect("drain plan");
    assert_eq!(report.released, 2);
    assert!(!report.budget_exhausted);
    report.validate().expect("report validates");
    assert_eq!(report.considered, 5);
    assert_eq!(report.await_receipt, 1);
    assert_eq!(report.untouched, 1);
    assert_eq!(report.terminal, 1);
    assert_eq!(report.protected_outstanding, 2);
    let disposition_for = |id: DeliveryAttemptId| {
        report
            .dispositions
            .iter()
            .find(|disposition| disposition.outbox_id == id)
            .expect("disposition")
    };
    assert_eq!(
        disposition_for(actionable.outbox_id).action,
        NotificationOutboxDrainAction::ReleaseClaim
    );
    assert_eq!(
        disposition_for(critical.outbox_id).action,
        NotificationOutboxDrainAction::ReleaseClaim
    );
    assert_eq!(
        disposition_for(submitted_record.outbox_id).action,
        NotificationOutboxDrainAction::AwaitReceipt
    );
    assert_eq!(
        disposition_for(acknowledged_record.outbox_id).action,
        NotificationOutboxDrainAction::Terminal
    );
    assert_eq!(
        disposition_for(pending.outbox_id).action,
        NotificationOutboxDrainAction::Untouched
    );
    assert!(critical_lease.lease_token > 0);

    // The drain is a plan: nothing changed in the store before the caller executes it.
    assert_eq!(
        store
            .read(&actionable.outbox_id.to_string())
            .await
            .expect("record")
            .state,
        NotificationOutboxState::Claimed
    );

    let released = store
        .release_notification_claim(&actionable_lease, 201)
        .await
        .expect("release");
    assert_eq!(released.state, NotificationOutboxState::Pending);
    assert_eq!(released.terminal_reason, None);
    assert!(store
        .mark_notification_submitted(&actionable_lease, 202)
        .await
        .is_err());
    assert_eq!(
        store
            .read(&critical.outbox_id.to_string())
            .await
            .expect("record")
            .state,
        NotificationOutboxState::Claimed,
        "an untouched protected claim keeps its fence"
    );
    store.rebuild_report().await.expect("rebuild after drain");
}

#[tokio::test]
async fn worker_plans_a_restored_jsonl_outbox_without_duplicate_dispatch() {
    let path = temp_path("restart");
    let (claimed_id, submitted_id) = {
        let store = MemoryNotificationOutboxStore::open_jsonl(&path).expect("open journal");
        let first = store
            .enqueue_notification(record(
                NotificationOutboxClass::BestEffort,
                notification(100),
                EPOCH,
            ))
            .await
            .expect("enqueue");
        let (_, first_lease) = store
            .claim_next_notification(WORKER_ID, EPOCH, 200, 30)
            .await
            .expect("claim")
            .expect("record");
        let second = store
            .enqueue_notification(record(
                NotificationOutboxClass::BestEffort,
                notification(100),
                EPOCH,
            ))
            .await
            .expect("enqueue");
        let (_, second_lease) = store
            .claim_next_notification(WORKER_ID, EPOCH, 200, 30)
            .await
            .expect("claim")
            .expect("record");
        store
            .mark_notification_submitted(&second_lease, 201)
            .await
            .expect("submit");
        assert!(first_lease.lease_token > 0);
        (first.outbox_id, second.outbox_id)
    };

    let reopened = MemoryNotificationOutboxStore::open_jsonl(&path).expect("replay journal");
    let restored = reopened.records().await;
    assert_eq!(restored.len(), 2);
    let worker = worker();
    let steps = worker.recovery_plan(&restored, 211).expect("recovery plan");
    assert_eq!(steps.len(), 2);
    let plan_for = |id: DeliveryAttemptId| {
        steps
            .iter()
            .find(|step| step.outbox_id == id)
            .expect("step")
            .plan
            .clone()
    };
    let restored_claim = reopened
        .read(&claimed_id.to_string())
        .await
        .expect("restored claim");
    match plan_for(claimed_id) {
        NotificationDeliveryPlan::Dispatch(intent) => {
            assert_eq!(
                intent.delivery_attempt_id, restored_claim.attempt.delivery_attempt_id,
                "the restarted worker reuses the restored attempt, it does not mint a duplicate"
            );
            assert_eq!(intent.lease.worker_id, WORKER_ID);
            intent.validate().expect("intent validates");
        }
        other => panic!("a live claim must dispatch its restored intent, got {other:?}"),
    }
    assert_eq!(
        plan_for(submitted_id),
        NotificationDeliveryPlan::AwaitReceipt,
        "a restored in-flight send waits for its receipt"
    );
    assert_eq!(
        reopened
            .claim_next_notification("worker-b", EPOCH, 211, 10)
            .await
            .expect("claim"),
        None,
        "both restored claims keep their fences"
    );
    reopened.rebuild_report().await.expect("rebuild");
    let _ = fs::remove_file(&path);
}

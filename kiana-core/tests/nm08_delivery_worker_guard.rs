//! NM-08 source guard: the outbox stays a fenced storage/planning boundary, and every fixture stays
//! wired into the automatic CI shards plus the manual NM-08 lane.

#[test]
fn notification_delivery_worker_is_a_bounded_plan_not_an_effect_loop() {
    let worker = include_str!("../src/notification_delivery.rs");
    let domain = include_str!("../../kiana-domain/src/notification_outbox.rs");
    let eventlog = include_str!("../../kiana-eventlog/src/notification_outbox.rs");
    for marker in [
        "NotificationDeliveryWorker",
        "NotificationDeliveryPlan",
        "ClaimRequired",
        "Dispatch",
        "AwaitReceipt",
        "ReconcileUnknown",
        "Abandon",
        "ForeignClaim",
        "NotificationDeliveryRecoveryStep",
        "recovery_plan",
        "NotificationOutboxDrainPolicy",
        "NotificationOutboxDrainReport",
        "NotificationOutboxDrainAction",
        "protected_outstanding",
        "NOTIFICATION_DELIVERY_WORKER_SCHEMA",
        "NOTIFICATION_OUTBOX_DRAIN_SCHEMA",
    ] {
        assert!(worker.contains(marker), "worker marker missing: {marker}");
    }
    for marker in [
        "NotificationOutboxRecord",
        "NotificationOutboxLease",
        "NotificationDispatchIntent",
        "NotificationOutboxClass",
        "NotificationOutboxTerminalReason",
        "NotificationOutboxShedReason",
        "NotificationOutboxAdmission",
        "NotificationOutboxPressure",
        "NotificationOutboxTransition",
        "rebuild_notification_outbox_state",
        "NotificationOutboxFenceRefusal",
        "effective_class",
        "subscription_fence",
        "check_subscription_fence",
        "abandon_unclaimed",
        "terminal_reason",
        "lease_token",
        "notification_outbox_lease_fence_mismatch",
        "notification_outbox_receipt_mismatch",
        "notification_outbox_subscription_revoked",
        "notification_outbox_protected_shed_refused",
        "notification_outbox_rebuild_diverged",
    ] {
        assert!(
            domain.contains(marker),
            "outbox contract marker missing: {marker}"
        );
    }
    for marker in [
        "MemoryNotificationOutboxStore",
        "claim_next_notification",
        "mark_notification_submitted",
        "reconcile_notification_unknown",
        "admit_notification",
        "abandon_notification",
        "abandon_unclaimed_notification",
        "release_notification_claim",
        "rebuild_report",
        "open_jsonl_bounded",
        "notification_outbox_rebuild_diverged",
        "notification_outbox_journal_corrupt",
        "notification_outbox_journal_permissions_too_broad",
        "Mutex",
    ] {
        assert!(
            eventlog.contains(marker),
            "outbox adapter marker missing: {marker}"
        );
    }
    for source in [worker, eventlog] {
        for forbidden in [
            "CapabilityBroker",
            "KianaHarness",
            "tokio::spawn",
            "reqwest",
            "std::net",
            "send(",
        ] {
            assert!(
                !source.contains(forbidden),
                "outbox effect boundary widened: {forbidden}"
            );
        }
    }
    for forbidden in ["ControlPlane", "ApprovalJournal", "BrokerPermit"] {
        assert!(
            !worker.contains(forbidden),
            "worker must not take authority: {forbidden}"
        );
    }
}

#[test]
fn notification_outbox_fixtures_stay_wired_to_the_lane_and_shards() {
    let fixtures = include_str!("nm08_notification_delivery.rs");
    for fixture in [
        "worker_refuses_stale_epoch_missing_lease_and_tampered_digest",
        "worker_abandons_expired_notification_and_never_dispatches_stale_content",
        "worker_abandons_revoked_paused_and_expired_subscription_with_reason",
        "worker_refuses_mismatched_subscription_identity_and_scope",
        "worker_recovery_plan_restores_each_lifecycle_state_deterministically",
        "worker_drain_refuses_a_foreign_claim_and_a_stale_deadline",
        "worker_drain_is_bounded_and_reports_outstanding_protected",
        "worker_plans_a_restored_jsonl_outbox_without_duplicate_dispatch",
    ] {
        assert!(fixtures.contains(fixture), "fixture missing: {fixture}");
    }
    assert!(
        !fixtures.contains("#[ignore"),
        "NM-08 fixtures must not be ignored"
    );

    let store_fixtures = include_str!("../../kiana-eventlog/tests/nm08_notification_outbox.rs");
    for fixture in [
        "fixture_declares_durable_outbox_deny_first_contract",
        "crash_after_claim_reclaims_a_new_attempt_and_fences_the_old_worker",
        "expired_claim_before_submit_reclaims_without_duplicate_dispatch",
        "submitted_lease_expiry_reconciles_unknown_instead_of_retrying",
        "revoked_subscription_cannot_claim_dispatch_or_acknowledge",
        "queue_overflow_sheds_only_best_effort_with_reason_and_keeps_protected_records",
        "understated_class_and_protected_shed_are_refused",
        "lifecycle_is_rebuildable_from_the_transition_chain",
        "release_and_abandon_transitions_are_fenced_and_reason_bound",
        "jsonl_journal_replays_across_restart_and_continues_the_fence",
        "jsonl_journal_refuses_tampered_truncated_and_permissive_files",
        "durable_store_never_duplicates_a_dispatch_after_repeated_replays",
    ] {
        assert!(
            store_fixtures.contains(fixture),
            "store fixture missing: {fixture}"
        );
    }
    assert!(
        !store_fixtures.contains("#[ignore"),
        "NM-08 store fixtures must not be ignored"
    );

    let shards = include_str!("../../scripts/ci/test-shards.json");
    for target in [
        "\"nm08_delivery_worker_guard\"",
        "\"nm08_notification_delivery\"",
    ] {
        assert!(shards.contains(target), "shard target missing: {target}");
    }

    let baseline = include_str!("../../docs/roadmap/nm08-notification-outbox-baseline.md");
    for marker in [
        "proof_level=source",
        "Known limits",
        "Completion slice (2026-10-17, CI-only)",
        "notification_outbox_protected_shed_refused",
        "jsonl_journal_replays_across_restart_and_continues_the_fence",
        "worker_drain_is_bounded_and_reports_outstanding_protected",
    ] {
        assert!(
            baseline.contains(marker),
            "baseline marker missing: {marker}"
        );
    }

    let lane = include_str!("../../.github/workflows/nm08-notification-outbox.yml");
    for marker in [
        "workflow_dispatch:",
        "permissions:",
        "contents: read",
        "cargo fetch --locked",
        "cargo fmt --all --check",
        "cargo test -p kiana-eventlog --test nm08_notification_outbox --locked",
        "cargo test -p kiana-core --test nm08_notification_delivery --locked",
        "cargo test -p kiana-core --test nm08_delivery_worker_guard --locked",
    ] {
        assert!(lane.contains(marker), "NM-08 lane marker missing: {marker}");
    }
    for forbidden in [
        "push:",
        "pull_request:",
        "continue-on-error",
        "--ignored",
        "|| true",
    ] {
        assert!(
            !lane.contains(forbidden),
            "NM-08 lane must stay manual and fail-closed: {forbidden}"
        );
    }
}

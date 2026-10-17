//! NM-12 source guard: the notification lifecycle projector stays a rebuildable, append-only
//! projection with no second execution path, and every fixture stays wired into the automatic CI
//! shard plus the manual NM-12 lane.

#[test]
fn notification_projector_is_rebuildable_and_append_only() {
    let source = include_str!("../src/notification_projector.rs");
    let domain = include_str!("../../kiana-domain/src/notification_lifecycle.rs");
    for marker in [
        "NotificationProjector",
        "rebuild_from_committed",
        "rebuild_with_retention",
        "apply_committed",
        "NotificationLifecycleFact",
        "NotificationVisibility",
        "NotificationInboxSnapshot",
        "NotificationReadStateFact",
        "NotificationRetentionDecision",
        "NotificationRetentionAdmission",
        "admit_retention",
        "layer_epochs",
        "source_cursor",
        "data_epoch",
        "TerminalRewrite",
        "FactConflict",
        "ProjectionUnavailable",
        "UnknownNotification",
        "ReadStateConflict",
        "ReadStateRegression",
        "RetentionUnauthorized",
        "RetentionPolicyStale",
        "RetentionLegalHold",
        "RetentionEpochConflict",
        "NOTIFICATION_RETENTION_PAYLOAD_LAYERS",
        "ReceiptAuditMetadata",
        "ReceiptPayloadRef",
        "RevocationLayerObservation",
        "retained_source_event_count",
    ] {
        assert!(
            source.contains(marker) || domain.contains(marker),
            "NM-12 marker missing: {marker}"
        );
    }
    for forbidden in [
        "CapabilityBroker",
        "KianaHarness",
        "ControlPlane",
        "DeliveryWorker",
        "EventStorePort",
        "append_event",
        "tokio",
        "reqwest",
        "std::fs",
        "std::process",
        "NotificationStatus",
        "HumanTaskStatus",
        "ApprovalStatus",
        "task_status",
        "remove_file",
        "remove(",
        "retain(",
        "truncate",
        "purge",
    ] {
        assert!(
            !source.contains(forbidden),
            "NM-12 boundary widened or history mutable: {forbidden}"
        );
    }
}

#[test]
fn notification_projector_fixtures_stay_wired_to_the_lane_shards_and_baseline() {
    let fixtures = include_str!("nm12_notification_projector.rs");
    for fixture in [
        "rebuild_and_lifecycle_replay_are_cursor_and_epoch_bound",
        "supersede_and_epoch_regression_fail_closed",
        "fresh_rebuild_reproduces_the_same_inbox_snapshot_as_incremental_apply",
        "withdraw_expire_and_supersede_only_append_facts_and_never_delete_source_history",
        "retention_admission_refuses_unauthorized_hold_and_stale_policy",
        "retention_refuses_incomplete_payload_layers_and_facts_deletion",
        "retention_admission_appends_one_lifecycle_fact_and_propagates_the_data_epoch",
        "read_state_facts_rebuild_the_same_unread_state_from_the_source_cursor",
        "presentation_mutations_become_replayable_read_state_facts",
        "audit_metadata_and_payload_refs_never_cross_the_boundary",
    ] {
        assert!(fixtures.contains(fixture), "fixture missing: {fixture}");
    }
    for forbidden in ["#[ignore", "--ignored"] {
        assert!(
            !fixtures.contains(forbidden),
            "NM-12 fixtures must not be ignored: {forbidden}"
        );
    }

    let shards = include_str!("../../scripts/ci/test-shards.json");
    for target in [
        "\"nm12_notification_projector\"",
        "\"nm12_notification_projector_guard\"",
        "\"nm12_notification_lifecycle\"",
    ] {
        assert!(shards.contains(target), "shard target missing: {target}");
    }

    let baseline = include_str!("../../docs/roadmap/nm12-notification-lifecycle-baseline.md");
    for marker in [
        "proof_level=source",
        "Known limits",
        "Completion slice (2026-10-17, CI-only)",
        "NotificationInboxSnapshot",
        "NotificationReadStateFact",
        "NotificationRetentionDecision",
        "admit_retention",
        "layer_epochs",
        "RetentionLegalHold",
        "NOTIFICATION_RETENTION_PAYLOAD_LAYERS",
        "fresh_rebuild_reproduces_the_same_inbox_snapshot_as_incremental_apply",
        "withdraw_expire_and_supersede_only_append_facts_and_never_delete_source_history",
        "read_state_facts_rebuild_the_same_unread_state_from_the_source_cursor",
    ] {
        assert!(
            baseline.contains(marker),
            "baseline marker missing: {marker}"
        );
    }

    let lane = include_str!("../../.github/workflows/nm12-notification-lifecycle.yml");
    for marker in [
        "workflow_dispatch:",
        "permissions:",
        "contents: read",
        "cargo fetch --locked",
        "cargo fmt --all --check",
        "cargo test -p kiana-core --test nm12_notification_projector --locked",
        "cargo test -p kiana-core --test nm12_notification_projector_guard --locked",
        "cargo test -p kiana-domain --test nm12_notification_lifecycle --locked",
    ] {
        assert!(lane.contains(marker), "NM-12 lane marker missing: {marker}");
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
            "NM-12 lane must stay manual and fail-closed: {forbidden}"
        );
    }
}

//! NM-06 source guard: the notification materializer stays a display-only projection of
//! committed facts, and every fixture stays wired into the automatic CI shards.

#[test]
fn notification_materializer_is_projection_only_and_server_scoped() {
    let source = include_str!("../src/notification_materializer.rs");
    let domain = include_str!("../../kiana-domain/src/platform.rs");
    for marker in [
        "NotificationMaterializer",
        "apply_committed",
        "validate_notification_runtime_event",
        "NotificationEventSource::EventLog",
        "source_event_id",
        "source_cursor",
        "materialization_digest",
        "human_inbox_items",
        "server_recipient_id",
        "decider_principal_id",
        "class_allows_kind",
        "notification_materializer_event_class_mismatch",
    ] {
        assert!(
            source.contains(marker),
            "materializer marker missing: {marker}"
        );
    }
    for marker in [
        "HumanTaskBridge",
        "impl HumanInboxKind",
        "human_task_target_kind",
        "is_actionable",
        "decider_principal_id",
        "notification_materialization_human_task_required",
        "notification_materialization_human_task_target_kind_mismatch",
        "notification_materialization_human_task_mismatch",
        "notification_materialization_evidence_required",
    ] {
        assert!(
            domain.contains(marker),
            "HumanTask bridge marker missing: {marker}"
        );
    }
    for forbidden in [
        "CapabilityBroker",
        "KianaHarness",
        "DeliveryWorker",
        "NotificationStore",
        "reqwest",
        "tokio::spawn",
        "std::fs",
        "Provider",
    ] {
        assert!(
            !source.contains(forbidden),
            "materializer authority widened: {forbidden}"
        );
    }
    for forbidden in ["HumanTaskStatus", "ApprovalStatus", "task_status"] {
        assert!(
            !domain.contains(forbidden),
            "HumanTask status was copied into the bridge: {forbidden}"
        );
    }
}

#[test]
fn notification_materializer_fixtures_stay_wired_to_the_lane_and_shards() {
    let fixtures = include_str!("nm06_notification_materializer.rs");
    for fixture in [
        "materializer_requires_committed_source_and_complete_materialization",
        "materializer_rejects_cursor_gaps_and_replays_exactly",
        "materializer_filters_recipient_and_wrong_decider_and_orders_items",
        "actionable_kinds_require_the_original_human_task_binding",
        "materializer_never_visible_when_source_evidence_due_or_decider_is_missing",
        "materializer_materializes_every_kind_with_summary_actions_due_and_source_authority",
        "materializer_refuses_a_class_that_cannot_own_the_materialized_kind",
        "bridge_identity_tampering_fails_closed",
        "platform_bridge_projects_server_scoped_items_and_fails_closed",
    ] {
        assert!(fixtures.contains(fixture), "fixture missing: {fixture}");
    }
    assert!(
        !fixtures.contains("#[ignore"),
        "NM-06 fixtures must not be ignored"
    );

    let shards = include_str!("../../scripts/ci/test-shards.json");
    for target in [
        "\"nm06_notification_materializer\"",
        "\"nm06_notification_materializer_guard\"",
    ] {
        assert!(shards.contains(target), "shard target missing: {target}");
    }

    let baseline = include_str!("../../docs/roadmap/nm06-notification-materializer-baseline.md");
    for marker in [
        "proof_level=source",
        "Known limits",
        "materializer_materializes_every_kind_with_summary_actions_due_and_source_authority",
        "materializer_never_visible_when_source_evidence_due_or_decider_is_missing",
        "notification_materialization_human_task_required",
    ] {
        assert!(
            baseline.contains(marker),
            "baseline marker missing: {marker}"
        );
    }

    let lane = include_str!("../../.github/workflows/nm06-notification-materializer.yml");
    for marker in [
        "workflow_dispatch:",
        "permissions:",
        "contents: read",
        "cargo fetch --locked",
        "cargo fmt --all --check",
        "cargo test -p kiana-core --test nm06_notification_materializer --locked",
        "cargo test -p kiana-core --test nm06_notification_materializer_guard --locked",
    ] {
        assert!(lane.contains(marker), "NM-06 lane marker missing: {marker}");
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
            "NM-06 lane must stay manual and fail-closed: {forbidden}"
        );
    }
}

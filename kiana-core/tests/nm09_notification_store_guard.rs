//! NM-09 source guard: the in-process NotificationStore stays a bounded, server-scoped
//! presentation projection, and every fixture stays wired into the automatic CI shard plus the
//! manual NM-09 lane.

#[test]
fn notification_store_is_a_bounded_server_scoped_projection() {
    let source = include_str!("../src/notification_store.rs");
    let materializer = include_str!("../src/notification_materializer.rs");
    for marker in [
        "NotificationStore",
        "NotificationListRequest",
        "NotificationPage",
        "NotificationPageItem",
        "NotificationProjectionMutation",
        "NOTIFICATION_STORE_MAX_PAGE_SIZE",
        "bind_server_recipient",
        "server_recipient",
        "ScopeRequired",
        "ScopeConflict",
        "RecipientMismatch",
        "ProjectionUnavailable",
        "CursorStale",
        "CursorInvalid",
        "ClockRegression",
        "DigestMismatch",
        "page_next_cursor",
        "retained_source_event_count",
        "mark_read",
        "acknowledge",
        "snooze",
        "human_inbox_items",
        "apply_committed",
    ] {
        assert!(
            source.contains(marker) || materializer.contains(marker),
            "NM-09 marker missing: {marker}"
        );
    }
    for forbidden in [
        "CapabilityBroker",
        "KianaHarness",
        "ControlPlane",
        "DeliveryWorker",
        "EventStorePort",
        "EventLog",
        "append_event",
        "commit_transition",
        "tokio",
        "reqwest",
        "std::fs",
        "std::process",
        "NotificationStatus",
        "HumanTaskStatus",
        "ApprovalStatus",
        "task_status",
        "remove(",
        "retain(",
        "truncate",
        "purge",
        "delete",
    ] {
        assert!(
            !source.contains(forbidden),
            "NM-09 authority widened or history mutable: {forbidden}"
        );
    }
}

#[test]
fn notification_store_fixtures_stay_wired_to_the_lane_shards_and_daemon_binding() {
    let fixtures = include_str!("nm09_notification_store.rs");
    for fixture in [
        "unbound_and_foreign_recipient_queries_are_refused",
        "empty_projection_is_not_unavailable_and_page_cap_is_enforced",
        "bounded_pagination_terminates_without_duplicates_or_cross_recipient_leaks",
        "read_and_ack_never_become_an_approval_or_task_status_mutation",
        "projections_only_append_and_mutations_never_delete_source_history",
    ] {
        assert!(fixtures.contains(fixture), "fixture missing: {fixture}");
    }
    for forbidden in ["#[ignore", "--ignored"] {
        assert!(
            !fixtures.contains(forbidden),
            "NM-09 fixtures must not be ignored: {forbidden}"
        );
    }

    let shards = include_str!("../../scripts/ci/test-shards.json");
    for target in [
        "\"nm09_notification_store\"",
        "\"nm09_notification_store_guard\"",
    ] {
        assert!(shards.contains(target), "shard target missing: {target}");
    }

    let baseline = include_str!("../../docs/roadmap/nm09-notification-store-baseline.md");
    for marker in [
        "proof_level=source",
        "Known limits",
        "Completion slice (2026-10-17, CI-only)",
        "bind_server_recipient",
        "ScopeRequired",
        "page_next_cursor",
        "unbound_and_foreign_recipient_queries_are_refused",
        "read_and_ack_never_become_an_approval_or_task_status_mutation",
    ] {
        assert!(
            baseline.contains(marker),
            "baseline marker missing: {marker}"
        );
    }

    let lane = include_str!("../../.github/workflows/nm09-notification-store.yml");
    for marker in [
        "workflow_dispatch:",
        "permissions:",
        "contents: read",
        "cargo fetch --locked",
        "cargo fmt --all --check",
        "cargo test -p kiana-core --test nm09_notification_store --locked",
        "cargo test -p kiana-core --test nm09_notification_store_guard --locked",
    ] {
        assert!(lane.contains(marker), "NM-09 lane marker missing: {marker}");
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
            "NM-09 lane must stay manual and fail-closed: {forbidden}"
        );
    }

    // The composition root derives the store scope from the authenticated principal and binds it
    // before the committed page is applied or answered; the daemon serves no mutation surface.
    let daemon = include_str!("../../kiana-daemon/src/lib.rs");
    let page = daemon
        .split("pub async fn notification_page(")
        .nth(1)
        .and_then(|rest| rest.split("pub async fn ui_snapshot(").next())
        .expect("daemon notification_page must exist for the NM-09 guard");
    let bind = page
        .find("bind_server_recipient")
        .expect("daemon must bind the server recipient");
    let apply = page
        .find("apply_committed")
        .expect("daemon must ingest committed facts");
    let list = page.find(".list(").expect("daemon must build a page");
    assert!(
        bind < apply && apply < list,
        "bind must precede ingest and page"
    );
    assert!(
        !page.contains("mark_read") && !page.contains("acknowledge"),
        "the daemon page path must stay read-only"
    );
}

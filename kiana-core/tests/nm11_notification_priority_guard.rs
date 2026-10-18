//! NM-11 source guard: the urgency/due/read/ACK/snooze/digest projection stays a server-time
//! presentation surface -- never a permission, approval or second execution path -- and every
//! fixture stays wired into the automatic CI shard plus the manual NM-11 lane.

#[test]
fn notification_priority_stays_a_server_time_projection_not_a_permission_path() {
    let priority = include_str!("../src/notification_priority.rs");
    let store = include_str!("../src/notification_store.rs");
    let projector = include_str!("../src/notification_projector.rs");
    for marker in [
        "NotificationUrgency",
        "NotificationDueState",
        "notification_due_state",
        "notification_priority_digest",
        "NOTIFICATION_PRIORITY_FORBIDDEN_DETAIL_HINTS",
        "ClientHint",
        "is_critical",
        "due_at_unix_ms",
        "source_cursor",
        "digest_group",
        "compare_notification_priority",
        "bind_server_clock",
        "server_time_unix_ms",
        "ServerTimeRequired",
        "SnoozeNotHideable",
        "NOTIFICATION_STORE_MAX_SNOOZE_MS",
        "snooze_horizon",
        "snooze_active",
        "due_state",
        "priority_digest",
        "unknown",
        "inbox_summary",
        "digest_folds",
        "read_state_facts",
        "apply_read_state_facts",
        "ReadStateCursorAhead",
        "ReadStateConflict",
        "ReadStateRegression",
        "NotificationReadStateFact",
    ] {
        assert!(
            priority.contains(marker) || store.contains(marker) || projector.contains(marker),
            "NM-11 marker missing: {marker}"
        );
    }
    for source in [priority, store] {
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
            "std::net",
            "NotificationStatus",
            "HumanTaskStatus",
            "ApprovalStatus",
            "task_status",
            "approve(",
            "retry(",
            "remove(",
            "retain(",
            "truncate",
            "purge",
            "delete",
        ] {
            assert!(
                !source.contains(forbidden),
                "NM-11 authority widened or history mutable: {forbidden}"
            );
        }
    }
}

#[test]
fn nm11_fixtures_stay_wired_to_the_lane_shards_and_baseline() {
    let fixtures = include_str!("nm11_notification_priority.rs");
    let store = include_str!("../src/notification_store.rs");
    for fixture in [
        "client_clocks_and_preselected_answers_cannot_reprioritize_or_approve",
        "silence_cannot_hide_critical_or_unknown_items_or_approve_anything",
        "ordering_is_urgency_due_source_sequence_and_never_crosses_the_recipient_scope",
        "read_state_facts_rebuild_the_same_unread_state_and_never_become_authority",
        "summary_and_digest_folds_are_server_time_scoped_to_the_bound_recipient",
        "page_rows_recompute_priority_due_state_and_snooze_activation",
    ] {
        assert!(fixtures.contains(fixture), "fixture missing: {fixture}");
    }
    for forbidden in ["#[ignore", "--ignored"] {
        assert!(
            !fixtures.contains(forbidden),
            "NM-11 fixtures must not be ignored: {forbidden}"
        );
    }
    for code in [
        "notification_priority_client_hint",
        "notification_priority_due_invalid",
        "notification_priority_cursor_invalid",
        "notification_priority_digest_group_invalid",
        "notification_priority_server_time_invalid",
        "notification_store_snooze_not_hideable",
        "notification_store_server_time_required",
        "notification_store_read_state_cursor_ahead",
        "notification_store_read_state_conflict",
        "notification_store_read_state_regression",
        "notification_store_scope_required",
        "notification_store_recipient_mismatch",
        "notification_store_clock_regression",
        "notification_store_projection_unavailable",
    ] {
        let source = include_str!("../src/notification_priority.rs");
        assert!(
            source.contains(code) || store.contains(code),
            "NM-11 refusal code missing from source: {code}"
        );
    }
    for variant in [
        "ClientHint",
        "DueInvalid",
        "CursorInvalid",
        "DigestGroupInvalid",
        "ServerTimeInvalid",
        "SnoozeNotHideable",
        "ServerTimeRequired",
        "ReadStateCursorAhead",
        "ReadStateConflict",
        "ReadStateRegression",
        "ItemNotFound",
        "ProjectionUnavailable",
        "ScopeRequired",
        "RecipientMismatch",
        "ClockRegression",
        "DigestMismatch",
    ] {
        assert!(
            fixtures.contains(variant),
            "fixture does not exercise refusal: {variant}"
        );
    }
    for code in [
        "page_row_binding",
        "page_row_unknown",
        "page_row_server_time",
        "page_next_cursor",
        "snooze_window",
        "snooze_horizon",
        "acknowledgement",
        "due_at_unix_ms",
        "actions",
        "summary_counts",
        "summary_digest_groups",
        "summary_header",
        "digest_fold_members",
        "digest_fold_member_duplicate",
        "digest_fold_member_order",
        "digest_fold_urgency",
        "digest_fold_due",
        "digest_fold_critical_count",
        "digest_fold_unknown_count",
        "digest_fold_unread_count",
        "digest_fold_acknowledged_count",
    ] {
        assert!(store.contains(code), "NM-11 refusal code missing: {code}");
        assert!(fixtures.contains(code), "fixture refusal missing: {code}");
    }

    let shards = include_str!("../../scripts/ci/test-shards.json");
    for target in [
        "\"nm11_notification_priority\"",
        "\"nm11_notification_priority_guard\"",
    ] {
        assert!(shards.contains(target), "shard target missing: {target}");
    }

    let baseline = include_str!("../../docs/roadmap/nm11-notification-priority-baseline.md");
    for marker in [
        "proof_level=source",
        "Known limits",
        "Completion slice (2026-10-17, CI-only)",
        "bind_server_clock",
        "SnoozeNotHideable",
        "notification_priority_digest",
        "read_state_facts",
        "inbox_summary",
        "digest_folds",
        "silence_cannot_hide_critical_or_unknown_items_or_approve_anything",
        "ordering_is_urgency_due_source_sequence_and_never_crosses_the_recipient_scope",
    ] {
        assert!(
            baseline.contains(marker),
            "baseline marker missing: {marker}"
        );
    }

    let lane = include_str!("../../.github/workflows/nm11-notification-priority.yml");
    for marker in [
        "workflow_dispatch:",
        "permissions:",
        "contents: read",
        "cargo fetch --locked",
        "cargo fmt --all --check",
        "cargo test -p kiana-core --test nm11_notification_priority --locked",
        "cargo test -p kiana-core --test nm11_notification_priority_guard --locked",
    ] {
        assert!(lane.contains(marker), "NM-11 lane marker missing: {marker}");
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
            "NM-11 lane must stay manual and fail-closed: {forbidden}"
        );
    }

    // The composition root binds the server clock, not the caller, and the page path stays
    // read-only: the store never serves a mutation through the daemon.
    let daemon = include_str!("../../kiana-daemon/src/lib.rs");
    let page = daemon
        .split("pub async fn notification_page(")
        .nth(1)
        .and_then(|rest| rest.split("pub async fn ui_snapshot(").next())
        .expect("daemon notification_page must exist for the NM-11 guard");
    let scope = page
        .find("bind_server_recipient")
        .expect("daemon must bind the server recipient");
    let clock = page
        .find("bind_server_clock")
        .expect("daemon must bind the server clock");
    let apply = page
        .find("apply_committed")
        .expect("daemon must ingest committed facts");
    let list = page.find(".list(").expect("daemon must build a page");
    assert!(
        scope < clock && clock < apply && apply < list,
        "scope, server clock, ingest and page must stay in that order"
    );
    assert!(
        !page.contains("mark_read") && !page.contains("acknowledge") && !page.contains("snooze"),
        "the daemon page path must stay read-only"
    );
}

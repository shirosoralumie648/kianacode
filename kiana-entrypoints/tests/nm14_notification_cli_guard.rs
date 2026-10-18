//! NM-14 source guard: the CLI/TTY inbox surface stays a display/planning adapter -- one shared
//! parse/plan route behind both `cli.rs` and `workbench_chat.rs`, never a permission or a second
//! execution path -- and every fixture stays wired into the manual NM-14 lane plus the automatic
//! whole-crate shard.

#[test]
fn notification_cli_presenter_has_no_execution_authority() {
    let source = include_str!("../src/notification_cli.rs");
    let workbench = include_str!("../src/workbench_chat.rs");
    let cli = include_str!("../src/cli.rs");
    for marker in [
        "CLI_NOTIFICATION_VIEW_SCHEMA",
        "CLI_RUN_STATUS_VIEW_SCHEMA",
        "present_notification_page",
        "present_run_status",
        "query_original",
        "actions_are_display_only",
        "CLI_NOTIFICATION_MAX_ITEMS",
        "parse_cli_inbox_line",
        "plan_cli_inbox_intent",
        "plan_cli_detach",
        "CliInboxRoute",
        "CliApprovalBinding",
        "from_listing",
        "present_session_badges",
        "render_inbox_lines",
        "render_badge_lines",
        "CLI_INBOX_MAX_SESSIONS",
        "server_time_unix_ms",
    ] {
        assert!(source.contains(marker), "NM-14 marker missing: {marker}");
    }
    for marker in [
        "interpret_inbox_line(name, args)",
        "inbox_routes",
        "plan_inbox",
        "set_inbox_approvals",
        "render_badges_for_view",
        "CliApprovalBinding::from_listing",
        "CliInboxIntent::Detach",
        "view.interpret_line(&command.text)",
    ] {
        assert!(
            workbench.contains(marker),
            "NM-14 workbench wiring marker missing: {marker}"
        );
    }
    assert!(!cli.contains("NotificationDeliveryWorker"));
    for forbidden in [
        "CapabilityBroker",
        "KianaHarness",
        "tokio::spawn",
        "reqwest::Client",
        "std::process::Command",
        "send(",
    ] {
        assert!(
            !source.contains(forbidden),
            "NM-14 presenter authority widened: {forbidden}"
        );
    }
    // The untyped approval fallback is gone: an approval decision must pass the shared planner
    // (server listing -> typed binding -> plan) before anything is sent back to the daemon.
    assert!(
        !workbench.contains("approval_not_available"),
        "NM-14 approval decisions must not bypass the shared planner"
    );
}

#[test]
fn nm14_fixtures_stay_wired_to_the_lane_shards_and_baseline() {
    let fixtures = include_str!("nm14_notification_cli.rs");
    for fixture in [
        "notification_cli_refuses_forged_scope_before_binding_any_action",
        "notification_cli_refuses_client_hints_and_pages_without_server_binding",
        "notification_cli_refuses_targets_and_decisions_outside_the_server_projection",
        "notification_cli_refuses_another_sessions_line_and_never_crosses_routes",
        "notification_cli_read_stays_presentation_and_never_consumes_the_action",
        "notification_cli_disconnect_is_a_detach_and_never_cancels_or_resumes",
        "notification_cli_badges_refuse_client_counters_and_unbound_pages",
        "notification_cli_inbox_approvals_and_badges_share_one_server_bound_route",
        "malformed_and_oversized_pages_fail_closed",
        "presenter_redacts_payload_and_keeps_actions_display_only",
        "run_status_preserves_unknown_and_terminal_retry_semantics",
    ] {
        assert!(
            fixtures.contains(fixture),
            "NM-14 fixture missing: {fixture}"
        );
    }
    for forbidden in ["#[ignore", "--ignored"] {
        assert!(
            !fixtures.contains(forbidden),
            "NM-14 fixtures must not be ignored: {forbidden}"
        );
    }
    let first_refusal = fixtures
        .find("notification_cli_refuses_")
        .expect("NM-14 refusal fixtures must exist");
    let success = fixtures
        .find("notification_cli_inbox_approvals_and_badges_share_one_server_bound_route")
        .expect("NM-14 combined success fixture must exist");
    assert!(
        first_refusal < success,
        "NM-14 refusal fixtures must precede the combined success fixture"
    );

    let source = include_str!("../src/notification_cli.rs");
    for (code, constant) in [
        (
            "cli_notification_client_hint_forbidden",
            "CLI_NOTIFICATION_CLIENT_HINT",
        ),
        (
            "cli_notification_page_digest_invalid",
            "CLI_NOTIFICATION_PAGE_DIGEST_INVALID",
        ),
        ("cli_inbox_line_invalid", "CLI_INBOX_LINE_INVALID"),
        ("cli_inbox_action_unknown", "CLI_INBOX_ACTION_UNKNOWN"),
        ("cli_inbox_scope_forbidden", "CLI_INBOX_SCOPE_FORBIDDEN"),
        ("cli_inbox_target_invalid", "CLI_INBOX_TARGET_INVALID"),
        ("cli_inbox_session_mismatch", "CLI_INBOX_SESSION_MISMATCH"),
        ("cli_inbox_session_duplicate", "CLI_INBOX_SESSION_DUPLICATE"),
        ("cli_inbox_sessions_limit", "CLI_INBOX_SESSIONS_LIMIT"),
        (
            "cli_inbox_page_digest_required",
            "CLI_INBOX_PAGE_DIGEST_REQUIRED",
        ),
        (
            "cli_inbox_server_time_required",
            "CLI_INBOX_SERVER_TIME_REQUIRED",
        ),
        (
            "cli_inbox_action_not_in_projection",
            "CLI_INBOX_ACTION_NOT_IN_PROJECTION",
        ),
        (
            "cli_inbox_decision_not_offered",
            "CLI_INBOX_DECISION_NOT_OFFERED",
        ),
        (
            "cli_inbox_read_is_not_consume",
            "CLI_INBOX_READ_IS_NOT_CONSUME",
        ),
        (
            "cli_inbox_disconnect_is_not_cancel",
            "CLI_INBOX_DISCONNECT_IS_NOT_CANCEL",
        ),
    ] {
        assert!(
            source.contains(code),
            "NM-14 refusal code missing from source: {code}"
        );
        assert!(
            fixtures.contains(constant),
            "NM-14 refusal not exercised by fixtures: {constant}"
        );
    }

    let declared = include_str!("fixtures/nm14-notification-cli.json");
    for marker in [
        "kiana.notification-cli-fixture.v1",
        "one server-bound route",
        "forged scope",
        "dropped connection turned into cancel/resume",
    ] {
        assert!(
            declared.contains(marker),
            "NM-14 declared fixture marker missing: {marker}"
        );
    }

    // Automatic coverage stays the whole-crate kiana-entrypoints shard; NM-14 adds no target
    // entry to the shard ledger.
    let shards = include_str!("../../scripts/ci/test-shards.json");
    assert!(shards.contains(
        "\"kiana-entrypoints\": {\n  \"crate\": \"kiana-entrypoints\",\n  \"targets\": null\n }"
    ));

    let baseline = include_str!("../../docs/roadmap/nm14-notification-cli-baseline.md");
    for marker in [
        "proof_level=source",
        "Known limits",
        "Completion slice (2026-10-17, CI-only)",
        "plan_cli_inbox_intent",
        "CliInboxRoute",
        "present_session_badges",
        "notification_cli_refuses_forged_scope_before_binding_any_action",
        "notification_cli_inbox_approvals_and_badges_share_one_server_bound_route",
    ] {
        assert!(
            baseline.contains(marker),
            "NM-14 baseline marker missing: {marker}"
        );
    }

    let lane = include_str!("../../.github/workflows/nm14-notification-cli.yml");
    for marker in [
        "workflow_dispatch:",
        "permissions:",
        "contents: read",
        "cargo fetch --locked",
        "cargo fmt --all --check",
        "cargo test -p kiana-entrypoints --test nm14_notification_cli --locked",
        "cargo test -p kiana-entrypoints --test nm14_notification_cli_guard --locked",
    ] {
        assert!(lane.contains(marker), "NM-14 lane marker missing: {marker}");
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
            "NM-14 lane must stay manual and fail-closed: {forbidden}"
        );
    }
}

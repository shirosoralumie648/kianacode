//! CAP-21 source guard for bounded stdio MCP protocol and stop lifecycle.

#[path = "support/source_guard.rs"]
mod source_guard;

fn require(source: &str, markers: &[&str], label: &str) {
    for marker in markers {
        let present = match *marker {
            "mcp_result_unknown" => {
                let run = source_guard::item_source(source, "async fn run(");
                let code = source_guard::compact_code(&run);
                code.contains("letneeds_reconciliation=!stop_confirmed||(started&&performed.is_err());")
                    && code.contains("ifcancelled||needs_reconciliation{")
                    && code.contains("CapabilityErrorCode::ResultUnknown")
                    && code.contains("mcp_call_unconfirmed")
                    && code.contains("if!started&&stop_confirmed{AdapterCommitState::NotStarted}else{AdapterCommitState::Unknown}")
                    && code.contains("attach_adapter_result(result,AdapterResultKind::Mcp,commit)")
            }
            "mcp.catalog" => {
                let discovery = source_guard::item_source(source, "async fn discovery(");
                let commit = source_guard::item_source(source, "async fn commit_discovery(");
                let read = source_guard::compact_code(&discovery);
                let write = source_guard::compact_code(&commit);
                read.contains(r#"self.events.read_stream("mcp_discovery",key).await?"#)
                    && read.contains(r#"event.kind=="mcp.discovery_committed""#)
                    && read.contains("mcp_discovery_required:use_operator_mcp.discover")
                    && write.contains("self.events.commit_transition(batch).await?")
                    && write.contains("CommitOutcome::Unknown")
                    && write.contains("self.events.read_command(&command_id).await?.is_none()")
                    && write.contains("result_unknown:mcp_discovery_commit_unconfirmed")
            }
            _ => source.contains(marker),
        };
        assert!(present, "CAP-21 {label} marker missing: {marker}");
    }
}

#[test]
fn mcp_stdio_bounds_frames_requests_notifications_pagination_and_stop() {
    let stdio = include_str!("../../kiana-daemon/src/mcp_stdio.rs");
    let harness =
        &source_guard::production_source(include_str!("../../kiana-daemon/src/harness_mcp.rs"));
    let supervisor = &[
        include_str!("../../kiana-daemon/src/process_supervisor.rs"),
        include_str!("../../kiana-daemon/src/mcp_stdio.rs"),
    ]
    .concat();
    let baseline = include_str!("../../docs/roadmap/capability.md");
    require(
        stdio,
        &[
            "FRAME_BYTES",
            "TOTAL_BYTES",
            "FRAME_COUNT",
            "CALL_TIMEOUT",
            "mcp_request_limit",
            "mcp_response_byte_limit",
            "mcp_response_frame_limit",
            "mcp_response_version_invalid",
            "mcp_response_id_mismatch",
            "mcp_response_envelope_invalid",
            "mcp_notification_budget_exceeded",
            "mcp_cursor_cycle",
            "mcp_tool_catalog_page_limit",
            "mcp_server_message_invalid",
            "Server initiated requests are disabled",
            "cancelled:mcp_request_stopped",
            "ProcessSupervisor::stop",
            "mcp_stop_unconfirmed",
        ],
        "stdio transport",
    );
    require(
        harness,
        &[
            "mcp_result_limit",
            "mcp_content_limit",
            "mcp_tool_schema_unsupported",
            "mcp_output_schema_unsupported",
            "mcp_async_task_unsupported",
            "mcp_result_unknown",
            "mcp.catalog",
        ],
        "MCP adapter",
    );
    require(
        supervisor,
        &[
            "ProcessSupervisor::prepare_command",
            "ProcessSupervisor::stop",
        ],
        "supervisor",
    );
    require(
        baseline,
        &[
            "mcp_stdin_backpressure_is_bounded",
            "mcp_duplicate_or_foreign_response_id_is_rejected",
            "mcp_pagination_cycle_and_notification_flood_fail_closed",
            "mcp_server_requests_cannot_bypass_control_plane",
        ],
        "CAP-21 card",
    );
}

/// The CAP-21 card fixtures are behavioural, so the source guard also proves that they exist,
/// stay deny-first, and are wired to a lane that installs the runtime they depend on. A fixture
/// renamed in only one place, an ignored fixture, or a lane that no longer verifies `bwrap` and
/// `python3` would otherwise let the behaviour rot silently.
#[test]
fn cap21_behavioural_fixtures_are_wired_and_deny_first() {
    let harness = include_str!("../../kiana-daemon/src/harness_mcp.rs");
    let workflow = include_str!("../../.github/workflows/cap21-mcp-bounded-transport.yml");
    let baseline = include_str!("../../docs/roadmap/cap21-mcp-bounded-transport-baseline.md");
    let fixtures = [
        "mcp_stdin_backpressure_is_bounded",
        "mcp_duplicate_or_foreign_response_id_is_rejected",
        "mcp_pagination_cycle_and_notification_flood_fail_closed",
        "mcp_server_requests_cannot_bypass_control_plane",
        "mcp_bounded_notifications_and_fragmented_frames_are_parsed",
        "mcp_paginated_tool_list_is_bounded",
        "mcp_initialize_failure_eof_and_child_crash_all_clean_up",
        "mcp_stderr_diagnostics_are_bounded_evidence",
        "mcp_cancelled_request_never_leaves_the_child_running",
        "mcp_unanswered_call_stays_unknown_and_is_not_replayed",
    ];
    for fixture in fixtures {
        assert!(
            harness.contains(fixture),
            "CAP-21 behavioural fixture missing from kiana-daemon/src/harness_mcp.rs: {fixture}"
        );
        assert!(
            workflow.contains(fixture),
            "CAP-21 behavioural fixture not wired into the focused lane: {fixture}"
        );
        assert!(
            baseline.contains(fixture),
            "CAP-21 behavioural fixture not recorded in the baseline: {fixture}"
        );
    }
    for marker in [
        "const CAP21_MCP_SERVER",
        "cap21_assert_marker_running",
        "cap21_assert_no_survivor",
        "cap21_stop(",
        "mcp_write_timeout",
        "mcp_response_timeout",
        "mcp_response_id_mismatch",
        "mcp_response_envelope_invalid",
        "mcp_response_version_invalid",
        "mcp_notification_budget_exceeded",
        "mcp_notification_invalid",
        "mcp_cursor_cycle",
        "mcp_response_byte_limit",
        "mcp_tool_catalog_page_limit",
        "mcp_tool_catalog_limit",
        "mcp_stream_closed",
        "mcp_catalog_changed",
        "CapabilityErrorCode::ResultUnknown",
        "mcp_call_unconfirmed",
        "reconciliation.json",
    ] {
        assert!(
            harness.contains(marker),
            "CAP-21 fixture coverage marker missing: {marker}"
        );
    }
    assert!(
        !harness.contains("#[ignore]"),
        "CAP-21 fixtures must run, not be ignored"
    );
    for wiring in [
        "bubblewrap",
        "bwrap --version",
        "python3 --version",
        "harness_mcp::tests::",
        "--exact",
        "--test-threads=1",
        "cap21_mcp_bounded_transport_guard",
        "p1_j4_01_mcp",
        "--no-run",
    ] {
        assert!(
            workflow.contains(wiring),
            "CAP-21 focused lane is missing required wiring: {wiring}"
        );
    }
    for bypass in ["--share-net", "privileged: true"] {
        assert!(
            !workflow.contains(bypass),
            "CAP-21 focused lane must not grant sandbox bypass: {bypass}"
        );
    }
    assert!(
        baseline.contains("No local runtime tests"),
        "CAP-21 baseline must state that the authoring session ran no local runtime tests"
    );
}

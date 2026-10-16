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

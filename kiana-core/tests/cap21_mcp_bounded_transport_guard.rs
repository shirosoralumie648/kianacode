//! CAP-21 source guard for bounded stdio MCP protocol and stop lifecycle.

fn require(source: &str, markers: &[&str], label: &str) {
    for marker in markers {
        assert!(
            source.contains(marker),
            "CAP-21 {label} marker missing: {marker}"
        );
    }
}

#[test]
fn mcp_stdio_bounds_frames_requests_notifications_pagination_and_stop() {
    let stdio = include_str!("../../kiana-daemon/src/mcp_stdio.rs");
    let harness = include_str!("../../kiana-daemon/src/harness_mcp.rs");
    let supervisor = include_str!("../../kiana-daemon/src/process_supervisor.rs");
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

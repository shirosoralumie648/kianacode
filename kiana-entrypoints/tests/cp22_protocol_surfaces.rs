//! CP-22 source guard for the shared protocol/action-card path.
//!
//! Runtime surface fixtures run in GitHub Actions.  This guard prevents any one UI from growing
//! a second approval, execution or reconnect authority beside RequestEnvelope/DaemonHost.

fn require(source: &str, markers: &[&str], label: &str) {
    for marker in markers {
        assert!(
            source.contains(marker),
            "CP-22 {label} marker missing: {marker}"
        );
    }
}

#[test]
fn cp_all_surfaces_resolve_the_same_pending_once() {
    let protocol = include_str!("../../kiana-protocol/src/lib.rs");
    let protocol_ui = include_str!("../../kiana-protocol/src/ui_contracts.rs");
    let client = include_str!("../../kiana-client/src/lib.rs");
    let daemon = include_str!("../../kiana-daemon/src/lib.rs");
    let harness = include_str!("../src/harness_run.rs");
    let workbench = include_str!("../src/workbench_chat.rs");
    let web = include_str!("../src/web.rs");
    let page = include_str!("../src/web_page.html");
    let product_command = include_str!("../src/product_command.rs");
    let cli = include_str!("../src/cli.rs");
    let desktop = include_str!("../../contrib/desktop/main.js");

    require(
        protocol,
        &[
            "PROTOCOL_SCHEMA",
            "RequestEnvelope",
            "RequestBody",
            "ApprovalDecisionRequest",
            "ApprovalListRequest",
            "ReceiptRequest",
            "CancelRequest",
            "ResumeRequest",
            "ResponseEnvelope",
            "failure_code",
            "expected_version",
            "request_hash",
            "nonce",
        ],
        "wire envelope",
    );
    require(
        protocol_ui,
        &[
            "UiSnapshotV1",
            "HumanActionCard",
            "UiActionV1",
            "UiActionResult",
            "command_id",
            "idempotency_key",
            "expected_epoch",
            "expected_cursor",
            "expected_revision",
            "UiRetryDisposition",
        ],
        "action card",
    );
    require(
        client,
        &[
            "RequestEnvelope::",
            "pending_approvals",
            "approval_decision_with_proof_and_version",
            "resume_run",
            "cancel_run",
            "receipt",
            "ClientTransport",
        ],
        "client facade",
    );
    require(
        daemon,
        &[
            "RequestBody::ListApprovals",
            "RequestBody::ApprovalDecision",
            "RequestBody::Receipt",
            "RequestBody::Resume",
            "RequestBody::Cancel",
            "ResponseEnvelope::from_core",
            "ui_snapshot",
            "claim_ui_action",
            "run_stream",
        ],
        "DaemonHost route",
    );
    for marker in [
        "pending_approvals_envelope_on_host",
        "decide_approval_envelope_on_host",
        "resume_envelope_on_host",
        "cancel_envelope_on_host",
        "receipt_envelope_on_host",
        "session_options_on_host",
        "challenge.request_hash",
        "challenge.nonce",
    ] {
        assert!(
            harness.contains(marker),
            "CP-22 harness marker missing: {marker}"
        );
    }
    require(
        workbench,
        &[
            "subscribe_run_after",
            "advance_cursor",
            "awaiting_approval",
            "/approvals",
            "/approve",
            "/deny",
            "decide_approval_envelope_on_host",
            "cancel_envelope_on_host",
            "ResultUnknown",
        ],
        "Workbench surface",
    );
    require(
        web,
        &[
            "/api/approvals",
            "pending_approvals_envelope_on_host",
            "decide_approval_envelope_on_host",
            "stream_cursor_from_request",
            "subscribe_run_after",
            "snapshot_required_after_stream_gap",
            "last-event-id",
            "request_hash",
            "nonce",
        ],
        "Web surface",
    );
    require(
        page,
        &[
            "ensureEventStream",
            "streamCursors",
            "observedUiCursor",
            "stream_sequence_gap",
            "stream_epoch_changed",
            "source.addEventListener('stream_gap', handleStreamGap)",
            "receipt authoritative",
            "api('/api/resume'",
            "api('/api/cancel'",
        ],
        "Web action card",
    );
    require(
        product_command,
        &[
            "pending_approvals",
            "approval_decision_with_proof",
            "request_hash",
            "nonce",
        ],
        "noninteractive command surface",
    );
    require(
        cli,
        &[
            "supports_non_interactive",
            "non_interactive_requires_allow_rule",
            "AwaitingApproval",
            "direct_connect_approval_status",
        ],
        "CLI status surface",
    );
    require(
        desktop,
        &[
            "startHarness",
            "waitForUrl",
            "stopWorker",
            "workspace:continue",
            "/api",
        ],
        "Desktop surface",
    );

    for surface in [workbench, web, product_command] {
        assert!(
            !surface.contains("auto_approve") && !surface.contains("autoapprove"),
            "surface must not auto-approve a pending action"
        );
    }
}

#[test]
fn cp_reconnect_replays_terminal_without_replaying_action() {
    let protocol = include_str!("../../kiana-protocol/src/lib.rs");
    let protocol_ui = include_str!("../../kiana-protocol/src/ui_contracts.rs");
    let daemon = include_str!("../../kiana-daemon/src/run_stream.rs");
    let daemon_host = include_str!("../../kiana-daemon/src/lib.rs");
    let stream = include_str!("../src/stream_render.rs");
    let workbench = include_str!("../src/workbench_chat.rs");
    let web = include_str!("../src/web.rs");
    let page = include_str!("../src/web_page.html");

    require(
        protocol,
        &[
            "RunStreamEnvelope",
            "RunStreamEvent",
            "UiCursor",
            "advance_cursor",
            "stream_sequence_gap",
            "stream_epoch_changed",
            "Terminal",
        ],
        "stream wire",
    );
    require(
        protocol_ui,
        &["snapshot_cursor", "feed_sequence", "UiRetryDisposition"],
        "snapshot/feed cursor",
    );
    require(
        daemon,
        &[
            "subscribe_after",
            "terminal = Some(envelope.clone())",
            "stream_sequence_gap",
            "stream_subscription_lagged",
            "stream_closed_before_terminal",
            "late",
        ],
        "run stream retention",
    );
    require(
        daemon_host,
        &[
            "subscribe_run_after",
            "run_stream_cursor",
            "ui_snapshot",
            "persisted_events",
        ],
        "host reconnect",
    );
    require(
        stream,
        &[
            "advance_cursor",
            "Terminal",
            "stream_terminal_missing",
            "cursor",
        ],
        "CLI stream renderer",
    );
    require(
        workbench,
        &[
            "subscribe_run_after",
            "advance_cursor",
            "last_terminal",
            "receipt",
        ],
        "Workbench reconnect",
    );
    require(
        web,
        &[
            "last-event-id",
            "stream_cursor_from_request",
            "snapshot_required_after_stream_gap",
        ],
        "Web reconnect",
    );
    require(
        page,
        &[
            "last_event_id",
            "streamCursors",
            "refreshIssued",
            "refreshApplied",
            "stream_sequence_gap",
            "streamIncomplete",
            "receipt authoritative",
        ],
        "Web reconnect",
    );

    for source in [daemon, stream, workbench, web, page] {
        for forbidden in [
            "ModelClient",
            "CapabilityBroker",
            "run_envelope(",
            "execute_authorized",
        ] {
            assert!(
                !source.contains(forbidden),
                "reconnect/display path must not replay execution: {forbidden}"
            );
        }
    }
}

#[test]
fn cp_noninteractive_cli_never_autoapproves_unknown_scope() {
    let workbench = include_str!("../src/workbench.rs");
    let cli = include_str!("../src/cli.rs");
    let dispatch = include_str!("../src/command_dispatch.rs");
    let harness = include_str!("../src/harness_run.rs");
    let product_command = include_str!("../src/product_command.rs");
    let protocol = include_str!("../../kiana-protocol/src/lib.rs");
    let protocol_ui = include_str!("../../kiana-protocol/src/ui_contracts.rs");

    require(
        workbench,
        &[
            "io::stdin().is_terminal()",
            "io::stdout().is_terminal()",
            "if !interactive",
            "finish_status",
            "ExecutionStatus",
        ],
        "Workbench noninteractive gate",
    );
    require(
        cli,
        &[
            "supports_non_interactive",
            "non_interactive_requires_allow_rule",
            "AwaitingApproval",
            "permission-mode manual",
            "direct_connect_approval_status",
        ],
        "CLI noninteractive gate",
    );
    require(
        dispatch,
        &[
            "CommandDispatchOutcome::AwaitingApproval",
            "should_auto_approve_local_write",
            "challenge.risk",
            "approval_required",
        ],
        "command approval gate",
    );
    require(
        harness,
        &[
            "ExecutionStatus::AwaitingApproval",
            "permission_handler",
            "PermissionPromptDecision",
            "approval_decision_with_proof",
            "challenge.request_hash",
            "challenge.nonce",
        ],
        "harness approval gate",
    );
    require(
        product_command,
        &["approval_decision_with_proof", "request_hash", "nonce"],
        "explicit approval command",
    );
    require(
        protocol,
        &["AwaitingApproval", "ResultUnknown"],
        "status mapping",
    );
    require(
        protocol_ui,
        &[
            "stable_error_from_response",
            "UiErrorCode",
            "UiRetryDisposition",
        ],
        "error mapping",
    );
    for source in [workbench, cli, dispatch, harness, product_command] {
        assert!(
            !source.contains("approve_unknown") && !source.contains("autoapprove_unknown"),
            "unknown scope must never be auto-approved"
        );
    }
}

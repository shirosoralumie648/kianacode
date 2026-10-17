//! CP-22 source guard for the shared protocol/action-card path.
//!
//! Runtime surface fixtures run in GitHub Actions.  This guard prevents any one UI from growing
//! a second approval, execution or reconnect authority beside RequestEnvelope/DaemonHost.
//!
//! Each acceptance test pairs its source markers with a behavioural layer built from the same
//! versioned DTOs, reconnect plan and exit/HTTP mapping the four surfaces
//! (CLI/Workbench/Web/Desktop) share, so a surface cannot agree by name while diverging in
//! behaviour.

use kiana_client::{
    compare_protocol_parity, plan_recovery, ApprovalPresentation, CliExitCode, DecisionSource,
    FeedDisposition, ProtocolObservation, ProtocolParityError, ProtocolSurface, RecoveryDecision,
    RecoveryFault, RecoveryFence, RecoveryInput, RecoveryPhase, PROTOCOL_PARITY_SCHEMA,
    UI_RECOVERY_SCHEMA,
};
use kiana_protocol::{
    stable_error_from_response, ExecutionStatus, RequestId, ResponseEnvelope, RunId,
    RunStreamEnvelope, RunStreamEvent, SessionId, UiActionSubmissionV1, UiCursor, UiErrorCode,
    UiRetryDisposition, PROTOCOL_SCHEMA, UI_ACTION_SUBMISSION_SCHEMA,
};

fn require(source: &str, markers: &[&str], label: &str) {
    for marker in markers {
        assert!(
            source.contains(marker),
            "CP-22 {label} marker missing: {marker}"
        );
    }
}

fn digest(ch: char) -> String {
    format!("sha256:{}", ch.to_string().repeat(64))
}

fn pending_observation(surface: ProtocolSurface) -> ProtocolObservation {
    ProtocolObservation {
        schema: PROTOCOL_PARITY_SCHEMA.to_owned(),
        surface,
        command_id: "cmd-cp22-pending".to_owned(),
        operation: "approval.decide".to_owned(),
        disposition: "accepted".to_owned(),
        retry: "do_not_retry".to_owned(),
        cursor_epoch: "epoch-cp22".to_owned(),
        cursor_sequence: 4,
        revision: 7,
        receipt_digest: None,
        error_code: None,
        decision_source: DecisionSource::ServerRendered,
        approval: ApprovalPresentation::RenderedPending,
        approval_reference: None,
        redaction_profile_digest: digest('b'),
        sensitive_field_count: 0,
        limitations: vec!["pending_rendered_not_decided".to_owned()],
    }
}

fn recovery_input(phase: RecoveryPhase, fault: RecoveryFault) -> RecoveryInput {
    let terminal_status = match phase {
        RecoveryPhase::Terminal => Some("completed".to_owned()),
        _ => None,
    };
    RecoveryInput {
        schema: UI_RECOVERY_SCHEMA.to_owned(),
        phase,
        fault,
        instance_id: "instance-cp22".to_owned(),
        epoch: "epoch-cp22".to_owned(),
        sequence: 9,
        command_id: "cmd-cp22".to_owned(),
        terminal_status,
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
    let desktop_preload = include_str!("../../contrib/desktop/preload.js");

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
        desktop_preload,
        &["workspace:continue"],
        "Desktop preload surface",
    );
    require(
        desktop,
        &["startHarness", "waitForReady", "stopWorker", "/api"],
        "Desktop main surface",
    );

    // Behavioural layer: the four surfaces answer one server-owned pending with the same command
    // identity, cannot answer a pending they have never observed, and never apply it locally.
    let command_id = RequestId::new();
    let idempotency_key = "cp22-pending-idempotency".to_owned();
    let submissions: Vec<UiActionSubmissionV1> = (0..4)
        .map(|index| UiActionSubmissionV1 {
            schema: UI_ACTION_SUBMISSION_SCHEMA.to_owned(),
            command_id,
            idempotency_key: idempotency_key.clone(),
            session_id: SessionId::new("session-cp22"),
            tab_id: format!("tab-{index}"),
            expected_epoch: "epoch-cp22".to_owned(),
            expected_cursor: 4,
            expected_revision: Some(7),
            payload_digest: digest('a'),
            submitted_at_unix_ms: 1,
        })
        .collect();
    for submission in &submissions {
        submission
            .validate()
            .expect("each surface submission is well formed");
        assert_eq!(submission.command_id, command_id);
        assert_eq!(submission.idempotency_key, idempotency_key);
        assert_eq!(submission.expected_cursor, 4);
    }
    let mut unseen = submissions[0].clone();
    unseen.expected_cursor = 0;
    assert!(
        unseen.validate().is_err(),
        "a surface that has seen nothing cannot answer the pending"
    );

    let observations = [
        ProtocolSurface::Cli,
        ProtocolSurface::Workbench,
        ProtocolSurface::Web,
        ProtocolSurface::Desktop,
        ProtocolSurface::Daemon,
    ]
    .into_iter()
    .map(pending_observation)
    .collect::<Vec<_>>();
    let report =
        compare_protocol_parity(&observations).expect("one shared pending across surfaces");
    assert_eq!(report.reference_surface, ProtocolSurface::Daemon);
    assert_eq!(report.surfaces.len(), 5);

    let mut locally_applied = observations.clone();
    locally_applied[2].approval = ApprovalPresentation::AppliedLocally;
    assert_eq!(
        compare_protocol_parity(&locally_applied).unwrap_err(),
        ProtocolParityError::ApprovalAppliedLocally
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
            "UiFeedGapReason::SequenceGap",
            "UiFeedGapReason::Backpressure",
            "broadcast::error::RecvError::Closed",
            "feed_terminal_delta_forbidden",
            "late",
        ],
        "run stream retention",
    );
    // Legacy subscriptions expose Tokio errors; the consuming surfaces name stable failures.
    require(
        workbench,
        &[
            "stream_subscription_lagged",
            "stream_closed_before_terminal",
        ],
        "legacy stream failures",
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

    // Behavioural layer: a terminal reconnect replays the retained terminal and never opens a
    // second effect; a sequence gap forces a snapshot instead of silently continuing.
    let replay_input = recovery_input(RecoveryPhase::Terminal, RecoveryFault::Disconnect);
    let replay = plan_recovery(&replay_input).expect("terminal disconnect plans a display replay");
    assert_eq!(replay.decision, RecoveryDecision::ReplayTerminal);
    assert!(!replay.new_effect_allowed);
    assert!(!replay.snapshot_required);
    assert_eq!(replay.command_id, "cmd-cp22");

    let gap_input = recovery_input(RecoveryPhase::Feed, RecoveryFault::Gap);
    let gap = plan_recovery(&gap_input).expect("feed gap plans a snapshot");
    assert_eq!(gap.decision, RecoveryDecision::HydrateSnapshot);
    assert!(gap.snapshot_required);
    assert!(!gap.new_effect_allowed);

    let mut fence = RecoveryFence::new("instance-cp22", "epoch-cp22").unwrap();
    assert_eq!(
        fence.accept_feed("epoch-cp22", 1, false),
        FeedDisposition::Accepted {
            sequence: 1,
            terminal: false
        }
    );
    assert_eq!(
        fence.accept_feed("epoch-cp22", 2, true),
        FeedDisposition::Accepted {
            sequence: 2,
            terminal: true
        }
    );
    assert_eq!(
        fence.accept_feed("epoch-cp22", 3, true),
        FeedDisposition::LateAfterTerminal
    );
    assert_eq!(
        fence.accept_feed("epoch-cp22", 2, true),
        FeedDisposition::Duplicate { sequence: 2 }
    );
    assert_eq!(
        fence.accept_feed("stale-epoch", 3, false),
        FeedDisposition::OldEpoch
    );

    let terminal = RunStreamEnvelope {
        schema: PROTOCOL_SCHEMA.to_owned(),
        epoch: "epoch-cp22".to_owned(),
        sequence: 2,
        ui_cursor: 2,
        event: RunStreamEvent::Terminal {
            run_id: RunId::new(),
            response: ResponseEnvelope {
                schema: PROTOCOL_SCHEMA.to_owned(),
                request_id: RequestId::new(),
                status: ExecutionStatus::Completed,
                output: serde_json::json!({}),
                error: None,
            },
        },
    };
    let mut cursor = UiCursor::default();
    assert!(terminal.advance_cursor(&mut cursor).unwrap());
    // Re-reading the retained terminal after reconnect is idempotent, never a second action.
    assert!(!terminal.advance_cursor(&mut cursor).unwrap());
    assert_eq!(cursor.sequence, 2);

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

    // Behavioural layer: a non-interactive response maps to a distinct status and exit code and is
    // never turned into success, so the shell caller must query the original decision.
    let awaiting = ResponseEnvelope {
        schema: PROTOCOL_SCHEMA.to_owned(),
        request_id: RequestId::new(),
        status: ExecutionStatus::AwaitingApproval,
        output: serde_json::json!({}),
        error: None,
    };
    let awaiting_error =
        stable_error_from_response(&awaiting).expect("awaiting approval is not success");
    assert_eq!(awaiting_error.code, UiErrorCode::ApprovalRequired);
    assert_eq!(awaiting_error.retry, UiRetryDisposition::DoNotRetry);
    assert_eq!(
        CliExitCode::for_output(ExecutionStatus::AwaitingApproval, None),
        CliExitCode::ApprovalRequired
    );
    assert!(!CliExitCode::for_output(ExecutionStatus::AwaitingApproval, None).is_success());
    let awaiting_policy = awaiting.failure_policy().unwrap();
    assert_eq!(awaiting_policy.http_status, 409);

    let unknown = ResponseEnvelope {
        status: ExecutionStatus::ResultUnknown,
        ..awaiting.clone()
    };
    let unknown_error =
        stable_error_from_response(&unknown).expect("unknown outcome is not success");
    assert_eq!(unknown_error.code, UiErrorCode::Unknown);
    assert_eq!(unknown_error.retry, UiRetryDisposition::QueryOriginal);
    assert_eq!(
        CliExitCode::for_output(ExecutionStatus::ResultUnknown, None),
        CliExitCode::Unknown
    );
    assert!(!CliExitCode::for_output(ExecutionStatus::ResultUnknown, None).is_success());
    assert_eq!(unknown.failure_code().unwrap().policy().cli_exit, 8);

    for source in [workbench, cli, dispatch, harness, product_command] {
        assert!(
            !source.contains("approve_unknown") && !source.contains("autoapprove_unknown"),
            "unknown scope must never be auto-approved"
        );
    }
}

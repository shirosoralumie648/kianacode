use kiana_client::{
    AcpAdapterError, AcpConnectionState, AcpInitializeRequest, AcpPermissionDecision,
    AcpPermissionRequest, AcpProjection, AcpSessionAdapter, AcpUpdateDisposition,
    ACP_INITIALIZE_SCHEMA, ACP_PERMISSION_SCHEMA, ACP_PROTOCOL_V1,
};
use kiana_domain::{ExecutionStatus, RunId, SessionId};
use kiana_protocol::{
    UiCursor, UiFeedCursorV1, UiFeedFrameKind, UiFeedFrameV1, UiSnapshot, PROTOCOL_SCHEMA,
    UI_FEED_FRAME_SCHEMA,
};
use serde_json::json;
use std::fs;
use std::path::Path;

fn digest(seed: &str) -> String {
    let value = if seed == "other" { 'b' } else { 'a' };
    format!("sha256:{}", value.to_string().repeat(64))
}

fn initialize(adapter: &mut AcpSessionAdapter) {
    adapter
        .initialize(AcpInitializeRequest {
            schema: ACP_INITIALIZE_SCHEMA.to_owned(),
            protocol: ACP_PROTOCOL_V1.to_owned(),
            client_name: "fake-ide".to_owned(),
            client_version: "1.0".to_owned(),
            capabilities: vec!["session.resume".to_owned(), "permission.request".to_owned()],
        })
        .unwrap();
}

fn frame(sequence: u64, terminal: bool) -> UiFeedFrameV1 {
    UiFeedFrameV1 {
        schema: UI_FEED_FRAME_SCHEMA.to_owned(),
        kind: if terminal {
            UiFeedFrameKind::Terminal
        } else {
            UiFeedFrameKind::Delta
        },
        cursor: UiFeedCursorV1::new(
            "instance-29",
            "epoch-29",
            sequence,
            UiCursor {
                epoch: "epoch-29".to_owned(),
                sequence: 1,
            },
        )
        .unwrap(),
        event_id: format!("event-{sequence}"),
        replay: false,
        terminal,
        event: Some(json!({"session_id":"session-29","sequence":sequence})),
        gap: None,
    }
}

fn snapshot(sequence: u64, status: ExecutionStatus) -> UiSnapshot {
    UiSnapshot {
        schema: PROTOCOL_SCHEMA.to_owned(),
        cursor: UiCursor {
            epoch: "epoch-29".to_owned(),
            sequence: 99,
        },
        session_id: "session-29".to_owned(),
        run_id: Some(RunId::new()),
        stream_cursor: Some(UiCursor {
            epoch: "epoch-29".to_owned(),
            sequence,
        }),
        status: Some(status),
        pending_actions: vec![],
    }
}

#[test]
fn fixture_declares_ui29_deny_first_contract() {
    let value: serde_json::Value = serde_json::from_str(
        &fs::read_to_string(
            Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/ui29-acp.json"),
        )
        .unwrap(),
    )
    .unwrap();
    assert_eq!(value["schema"], "kiana.acp-fixture.v1");
    assert!(value["denied"]
        .as_array()
        .unwrap()
        .iter()
        .any(|item| item.as_str().unwrap().contains("foreign")));
}

#[test]
fn initialize_session_and_actions_use_shared_ui_contracts() {
    let mut adapter = AcpSessionAdapter::new();
    assert!(matches!(
        adapter.initialize(AcpInitializeRequest {
            schema: ACP_INITIALIZE_SCHEMA.to_owned(),
            protocol: "acp.v0".to_owned(),
            client_name: "fake".to_owned(),
            client_version: "1".to_owned(),
            capabilities: vec![],
        }),
        Err(AcpAdapterError::SchemaUnsupported)
    ));
    initialize(&mut adapter);
    let session = SessionId::new("session-29");
    adapter
        .new_session(session.clone(), digest("owner"), "epoch-29")
        .unwrap();
    assert_eq!(
        adapter.prompt(&session, "too early", "epoch-29", 1, "prompt-early"),
        Err(AcpAdapterError::FeedHandlerRequired)
    );
    adapter.install_feed_handler().unwrap();

    let prompt = adapter
        .prompt(&session, "hello", "epoch-29", 1, "prompt-29")
        .unwrap();
    assert!(matches!(prompt, AcpProjection::Action(_)));
    prompt.validate().unwrap();
    let cancel = adapter
        .cancel(&session, "epoch-29", 1, "cancel-29")
        .unwrap();
    cancel.validate().unwrap();
}

#[test]
fn owner_epoch_permission_and_feed_fences_are_fail_closed() {
    let mut adapter = AcpSessionAdapter::new();
    initialize(&mut adapter);
    let session = SessionId::new("session-29");
    let owner = digest("owner");
    adapter
        .new_session(session.clone(), owner.clone(), "epoch-29")
        .unwrap();
    adapter.install_feed_handler().unwrap();
    assert!(matches!(
        adapter.prompt(&SessionId::new("foreign"), "no", "epoch-29", 1, "bad"),
        Err(AcpAdapterError::SessionMismatch)
    ));
    assert!(matches!(
        adapter.resume_session(&session, &digest("other"), "epoch-29"),
        Err(AcpAdapterError::SessionOwnerMismatch)
    ));

    adapter
        .permission_request(
            AcpPermissionRequest {
                schema: ACP_PERMISSION_SCHEMA.to_owned(),
                permission_id: "permission-29".to_owned(),
                session_id: session.clone(),
                owner_digest: owner.clone(),
                authority_epoch: "epoch-29".to_owned(),
                expires_at_unix_ms: 101,
                summary: "allow a bounded action".to_owned(),
            },
            100,
        )
        .unwrap();
    let decision = adapter
        .resolve_permission(
            "permission-29",
            AcpPermissionDecision::Allow,
            100,
            1,
            "permission-29-decision",
        )
        .unwrap();
    decision.validate().unwrap();
    assert!(matches!(
        adapter.resolve_permission(
            "permission-29",
            AcpPermissionDecision::Deny,
            100,
            1,
            "replay"
        ),
        Err(AcpAdapterError::PermissionUnknown)
    ));

    assert!(matches!(
        adapter.update(frame(1, false)).unwrap(),
        AcpUpdateDisposition::Accepted {
            sequence: 1,
            terminal: false
        }
    ));
    assert!(matches!(
        adapter.update(frame(1, false)).unwrap(),
        AcpUpdateDisposition::Replay { sequence: 1 }
    ));
    assert!(matches!(
        adapter.update(frame(3, false)).unwrap(),
        AcpUpdateDisposition::Gap {
            expected: 2,
            received: 3
        }
    ));
    assert_eq!(
        adapter.update(frame(2, false)),
        Err(AcpAdapterError::ReconcileRequired)
    );
    assert!(
        adapter
            .resume_session(&session, &owner, "epoch-29")
            .unwrap()
            .snapshot_required
    );
    adapter
        .hydrate_snapshot(&session, &owner, &snapshot(1, ExecutionStatus::Running))
        .unwrap();
    assert!(matches!(
        adapter.update(frame(2, false)).unwrap(),
        AcpUpdateDisposition::Accepted {
            sequence: 2,
            terminal: false
        }
    ));
    assert!(matches!(
        adapter.update(frame(3, true)).unwrap(),
        AcpUpdateDisposition::Accepted {
            sequence: 3,
            terminal: true
        }
    ));
    assert_eq!(
        adapter.update(frame(4, true)),
        Err(AcpAdapterError::UpdateAfterTerminal)
    );
}

#[test]
fn disconnect_requires_reinstall_and_resume_requests_hydration() {
    let mut adapter = AcpSessionAdapter::new();
    initialize(&mut adapter);
    let session = SessionId::new("session-29");
    adapter
        .new_session(session.clone(), digest("owner"), "epoch-29")
        .unwrap();
    adapter.install_feed_handler().unwrap();
    adapter.update(frame(1, false)).unwrap();
    adapter.disconnect();
    assert_eq!(
        adapter.resume_session(&session, &digest("owner"), "epoch-29"),
        Err(AcpAdapterError::FeedHandlerRequired)
    );
    adapter.install_feed_handler().unwrap();
    let resumed = adapter
        .resume_session(&session, &digest("owner"), "epoch-29")
        .unwrap();
    assert_eq!(resumed.replay_from_sequence, 1);
    assert!(resumed.snapshot_required);
    assert_eq!(
        adapter.update(frame(2, false)),
        Err(AcpAdapterError::ReconcileRequired)
    );
    adapter
        .hydrate_snapshot(
            &session,
            &digest("owner"),
            &snapshot(1, ExecutionStatus::Running),
        )
        .unwrap();
    assert_eq!(adapter.connection_state(), AcpConnectionState::Attached);
    assert!(matches!(
        adapter.update(frame(2, false)).unwrap(),
        AcpUpdateDisposition::Accepted { sequence: 2, .. }
    ));
}

#[test]
fn resume_prepares_hydration_without_releasing_disconnect_or_gap_fences() {
    for disconnected in [true, false] {
        let mut adapter = AcpSessionAdapter::new();
        initialize(&mut adapter);
        let session = SessionId::new("session-29");
        adapter
            .new_session(session.clone(), digest("owner"), "epoch-29")
            .unwrap();
        adapter.install_feed_handler().unwrap();
        adapter.update(frame(1, false)).unwrap();
        if disconnected {
            adapter.disconnect();
        } else {
            assert!(matches!(
                adapter.update(frame(3, false)).unwrap(),
                AcpUpdateDisposition::Gap { .. }
            ));
        }
        adapter.install_feed_handler().unwrap();
        assert_eq!(
            adapter.resume_session(&session, &digest("other"), "epoch-29"),
            Err(AcpAdapterError::SessionOwnerMismatch)
        );
        assert_eq!(
            adapter.resume_session(&SessionId::new("foreign"), &digest("owner"), "epoch-29"),
            Err(AcpAdapterError::SessionMismatch)
        );
        assert_eq!(
            adapter.resume_session(&session, &digest("owner"), "old-epoch"),
            Err(AcpAdapterError::EpochMismatch)
        );
        let plan = adapter
            .resume_session(&session, &digest("owner"), "epoch-29")
            .unwrap();
        assert!(plan.snapshot_required);
        assert_eq!(plan.replay_from_sequence, 1);
        assert_eq!(adapter.connection_state(), AcpConnectionState::Degraded);
        assert_eq!(
            adapter.update(frame(2, false)),
            Err(AcpAdapterError::ReconcileRequired)
        );
        assert_eq!(adapter.last_sequence(), 1);
    }
}

#[test]
fn snapshot_hydration_requires_handler_plan_and_original_owner() {
    let mut adapter = AcpSessionAdapter::new();
    initialize(&mut adapter);
    let session = SessionId::new("session-29");
    let owner = digest("owner");
    adapter
        .new_session(session.clone(), owner.clone(), "epoch-29")
        .unwrap();
    adapter.install_feed_handler().unwrap();
    adapter.update(frame(1, false)).unwrap();
    adapter.disconnect();
    let known = snapshot(1, ExecutionStatus::Running);
    assert_eq!(
        adapter.hydrate_snapshot(&session, &owner, &known),
        Err(AcpAdapterError::FeedHandlerRequired)
    );
    adapter.install_feed_handler().unwrap();
    assert_eq!(
        adapter.hydrate_snapshot(&session, &owner, &known),
        Err(AcpAdapterError::ReconcileRequired)
    );
    adapter
        .resume_session(&session, &owner, "epoch-29")
        .unwrap();
    assert_eq!(
        adapter.hydrate_snapshot(&session, &digest("other"), &known),
        Err(AcpAdapterError::SessionOwnerMismatch)
    );
    assert_eq!(
        adapter.hydrate_snapshot(&SessionId::new("foreign"), &owner, &known),
        Err(AcpAdapterError::SessionMismatch)
    );
    let mut foreign = known.clone();
    foreign.session_id = "foreign".to_owned();
    assert_eq!(
        adapter.hydrate_snapshot(&session, &owner, &foreign),
        Err(AcpAdapterError::SessionMismatch)
    );
    assert_eq!(adapter.connection_state(), AcpConnectionState::Degraded);
    assert_eq!(adapter.last_sequence(), 1);
    let reference = adapter.hydrate_snapshot(&session, &owner, &known).unwrap();
    assert_eq!(reference.session_id, session);
    assert_eq!(reference.owner_digest, owner);
    assert_eq!(reference.feed_sequence, 1);
    assert_eq!(adapter.connection_state(), AcpConnectionState::Attached);
}

#[test]
fn malformed_stale_and_unknown_snapshots_do_not_release_the_recovery_fence() {
    let mut adapter = AcpSessionAdapter::new();
    initialize(&mut adapter);
    let session = SessionId::new("session-29");
    let owner = digest("owner");
    adapter
        .new_session(session.clone(), owner.clone(), "epoch-29")
        .unwrap();
    adapter.install_feed_handler().unwrap();
    adapter.update(frame(1, false)).unwrap();
    adapter.update(frame(2, false)).unwrap();
    assert!(matches!(
        adapter.update(frame(4, false)).unwrap(),
        AcpUpdateDisposition::Gap { .. }
    ));
    assert_eq!(
        adapter.new_session(SessionId::new("replacement"), owner.clone(), "epoch-29"),
        Err(AcpAdapterError::ReconcileRequired)
    );
    assert!(matches!(
        adapter.initialize(AcpInitializeRequest {
            schema: ACP_INITIALIZE_SCHEMA.to_owned(),
            protocol: ACP_PROTOCOL_V1.to_owned(),
            client_name: "fake-ide".to_owned(),
            client_version: "1.0".to_owned(),
            capabilities: vec![],
        }),
        Err(AcpAdapterError::ReconcileRequired)
    ));
    adapter
        .resume_session(&session, &owner, "epoch-29")
        .unwrap();

    for (mut candidate, expected) in [
        (
            snapshot(1, ExecutionStatus::Running),
            AcpAdapterError::InvalidInput("snapshot_stream_cursor"),
        ),
        (
            snapshot(0, ExecutionStatus::Running),
            AcpAdapterError::ProjectionInvalid("ui_cursor_invalid".to_owned()),
        ),
        (
            snapshot(u64::MAX, ExecutionStatus::Running),
            AcpAdapterError::InvalidInput("snapshot_stream_cursor"),
        ),
        (
            snapshot(2, ExecutionStatus::ResultUnknown),
            AcpAdapterError::ReconcileRequired,
        ),
    ] {
        assert_eq!(
            adapter.hydrate_snapshot(&session, &owner, &candidate),
            Err(expected)
        );
        candidate.stream_cursor.as_mut().unwrap().epoch = "old-epoch".to_owned();
        if candidate.stream_cursor.as_ref().unwrap().sequence > 0 {
            assert_eq!(
                adapter.hydrate_snapshot(&session, &owner, &candidate),
                Err(AcpAdapterError::EpochMismatch)
            );
        }
        assert_eq!(adapter.connection_state(), AcpConnectionState::Degraded);
        assert_eq!(adapter.last_sequence(), 2);
    }

    for missing in ["stream", "run", "status", "schema", "cursor", "pending"] {
        let mut candidate = snapshot(2, ExecutionStatus::Running);
        match missing {
            "stream" => candidate.stream_cursor = None,
            "run" => candidate.run_id = None,
            "status" => candidate.status = None,
            "schema" => candidate.schema = "future.schema".to_owned(),
            "cursor" => candidate.cursor.epoch = "bad\nepoch".to_owned(),
            "pending" => candidate.pending_actions = vec![json!({}); 129],
            _ => unreachable!(),
        }
        assert!(
            adapter
                .hydrate_snapshot(&session, &owner, &candidate)
                .is_err(),
            "invalid {missing}"
        );
        assert_eq!(adapter.connection_state(), AcpConnectionState::Degraded);
        assert_eq!(adapter.last_sequence(), 2);
    }
    assert_eq!(
        adapter.update(frame(3, false)),
        Err(AcpAdapterError::ReconcileRequired)
    );
    adapter
        .hydrate_snapshot(&session, &owner, &snapshot(3, ExecutionStatus::Running))
        .unwrap();
    // The per-run display tail is 3; the hub-wide display cursor remains 99.
    assert_eq!(adapter.last_sequence(), 3);
    assert!(matches!(
        adapter.update(frame(4, false)).unwrap(),
        AcpUpdateDisposition::Accepted { sequence: 4, .. }
    ));
}

#[test]
fn terminal_snapshot_keeps_late_updates_and_terminal_regressions_denied() {
    for status in [
        ExecutionStatus::Completed,
        ExecutionStatus::Failed,
        ExecutionStatus::Cancelled,
        ExecutionStatus::Denied,
        ExecutionStatus::Blocked,
    ] {
        let mut adapter = AcpSessionAdapter::new();
        initialize(&mut adapter);
        let session = SessionId::new("session-29");
        let owner = digest("owner");
        adapter
            .new_session(session.clone(), owner.clone(), "epoch-29")
            .unwrap();
        adapter.install_feed_handler().unwrap();
        adapter.update(frame(1, false)).unwrap();
        adapter.disconnect();
        adapter.install_feed_handler().unwrap();
        adapter
            .resume_session(&session, &owner, "epoch-29")
            .unwrap();
        adapter
            .hydrate_snapshot(&session, &owner, &snapshot(2, status))
            .unwrap();
        assert_eq!(
            adapter.update(frame(3, false)),
            Err(AcpAdapterError::UpdateAfterTerminal)
        );
        adapter.disconnect();
        adapter.install_feed_handler().unwrap();
        adapter
            .resume_session(&session, &owner, "epoch-29")
            .unwrap();
        assert_eq!(
            adapter.hydrate_snapshot(&session, &owner, &snapshot(3, ExecutionStatus::Running)),
            Err(AcpAdapterError::InvalidInput(
                "snapshot_terminal_regression"
            ))
        );
        assert_eq!(adapter.connection_state(), AcpConnectionState::Degraded);
        assert_eq!(adapter.last_sequence(), 2);
    }
}

#[test]
fn a_complete_idle_snapshot_restores_only_an_unstarted_session() {
    let mut adapter = AcpSessionAdapter::new();
    initialize(&mut adapter);
    let session = SessionId::new("session-29");
    let owner = digest("owner");
    adapter
        .new_session(session.clone(), owner.clone(), "epoch-29")
        .unwrap();
    adapter.install_feed_handler().unwrap();
    adapter.disconnect();
    adapter.install_feed_handler().unwrap();
    adapter
        .resume_session(&session, &owner, "epoch-29")
        .unwrap();
    let idle = UiSnapshot {
        schema: PROTOCOL_SCHEMA.to_owned(),
        cursor: UiCursor {
            epoch: "epoch-29".to_owned(),
            sequence: 0,
        },
        session_id: session.to_string(),
        run_id: None,
        stream_cursor: None,
        status: None,
        pending_actions: vec![],
    };
    for invalid in ["run", "status", "stream", "pending", "epoch"] {
        let mut candidate = idle.clone();
        match invalid {
            "run" => candidate.run_id = Some(RunId::new()),
            "status" => candidate.status = Some(ExecutionStatus::Running),
            "stream" => {
                candidate.stream_cursor = Some(UiCursor {
                    epoch: "epoch-29".to_owned(),
                    sequence: 0,
                })
            }
            "pending" => candidate
                .pending_actions
                .push(json!({"approval_id":"unexpected"})),
            "epoch" => candidate.cursor.epoch = "old-epoch".to_owned(),
            _ => unreachable!(),
        }
        assert!(
            adapter
                .hydrate_snapshot(&session, &owner, &candidate)
                .is_err(),
            "{invalid}"
        );
        assert_eq!(adapter.connection_state(), AcpConnectionState::Degraded);
        assert_eq!(adapter.last_sequence(), 0);
    }
    adapter.hydrate_snapshot(&session, &owner, &idle).unwrap();
    assert_eq!(adapter.connection_state(), AcpConnectionState::Attached);
    assert_eq!(adapter.last_sequence(), 0);
    adapter.update(frame(1, false)).unwrap();
    adapter.disconnect();
    adapter.install_feed_handler().unwrap();
    adapter
        .resume_session(&session, &owner, "epoch-29")
        .unwrap();
    assert!(adapter.hydrate_snapshot(&session, &owner, &idle).is_err());
    assert_eq!(adapter.connection_state(), AcpConnectionState::Degraded);
    assert_eq!(adapter.last_sequence(), 1);
}

#[test]
fn a_current_epoch_zero_tail_is_valid_only_before_the_first_display_update() {
    let mut adapter = AcpSessionAdapter::new();
    initialize(&mut adapter);
    let session = SessionId::new("session-29");
    let owner = digest("owner");
    adapter
        .new_session(session.clone(), owner.clone(), "epoch-29")
        .unwrap();
    adapter.install_feed_handler().unwrap();
    adapter.disconnect();
    adapter.install_feed_handler().unwrap();
    adapter
        .resume_session(&session, &owner, "epoch-29")
        .unwrap();
    let mut known = snapshot(0, ExecutionStatus::Running);
    known.cursor = UiCursor {
        epoch: "epoch-29".to_owned(),
        sequence: 0,
    };
    let mut unknown = known.clone();
    unknown.status = Some(ExecutionStatus::ResultUnknown);
    assert_eq!(
        adapter.hydrate_snapshot(&session, &owner, &unknown),
        Err(AcpAdapterError::ReconcileRequired)
    );
    assert_eq!(adapter.connection_state(), AcpConnectionState::Degraded);
    adapter.hydrate_snapshot(&session, &owner, &known).unwrap();
    assert_eq!(adapter.last_sequence(), 0);
    adapter.update(frame(1, false)).unwrap();
}

#[test]
fn hydration_binds_both_display_epochs_and_pending_approval_facts() {
    let mut adapter = AcpSessionAdapter::new();
    initialize(&mut adapter);
    let session = SessionId::new("session-29");
    let owner = digest("owner");
    adapter
        .new_session(session.clone(), owner.clone(), "epoch-29")
        .unwrap();
    adapter.install_feed_handler().unwrap();
    adapter.update(frame(1, false)).unwrap();
    adapter.disconnect();
    adapter.install_feed_handler().unwrap();
    adapter
        .resume_session(&session, &owner, "epoch-29")
        .unwrap();
    let mut known = snapshot(1, ExecutionStatus::AwaitingApproval);
    known.pending_actions.push(json!({
        "approval_id": kiana_domain::ApprovalId::new(),
        "run_id": known.run_id,
        "session_id": session.as_str(),
        "capability_request_id": kiana_domain::RequestId::new(),
        "request_hash": digest("owner"),
        "actor_id": "local-user",
        "expires_at_unix_ms": 101,
        "attempt": 1,
        "effect_started": false,
        "effect_known": true,
        "zero_effect": true,
        "stop_state": "not_requested",
        "fenced": false,
        "action_digest": digest("owner"),
    }));
    kiana_domain::validate_event_payload("approval.requested", &known.pending_actions[0]).unwrap();
    for invalid in [
        "display_epoch",
        "stream_epoch",
        "both_epochs",
        "approval_object",
        "approval_id",
        "approval_run",
        "approval_session",
        "approval_extra",
    ] {
        let mut candidate = known.clone();
        match invalid {
            "display_epoch" => candidate.cursor.epoch = "old-epoch".to_owned(),
            "stream_epoch" => {
                candidate.stream_cursor.as_mut().unwrap().epoch = "old-epoch".to_owned()
            }
            "both_epochs" => {
                candidate.cursor.epoch = "old-epoch".to_owned();
                candidate.stream_cursor.as_mut().unwrap().epoch = "old-epoch".to_owned();
            }
            "approval_object" => candidate.pending_actions[0] = json!("not-an-approval"),
            "approval_id" => candidate.pending_actions[0]["approval_id"] = json!("not-a-uuid"),
            "approval_run" => candidate.pending_actions[0]["run_id"] = json!(RunId::new()),
            "approval_session" => {
                candidate.pending_actions[0]["session_id"] = json!("foreign-session")
            }
            "approval_extra" => candidate.pending_actions[0]["direct_execute"] = json!(true),
            _ => unreachable!(),
        }
        assert!(
            adapter
                .hydrate_snapshot(&session, &owner, &candidate)
                .is_err(),
            "{invalid}"
        );
        assert_eq!(adapter.connection_state(), AcpConnectionState::Degraded);
        assert_eq!(adapter.last_sequence(), 1);
    }
    adapter.hydrate_snapshot(&session, &owner, &known).unwrap();
    assert_eq!(adapter.connection_state(), AcpConnectionState::Attached);
    assert_eq!(adapter.last_sequence(), 1);
}

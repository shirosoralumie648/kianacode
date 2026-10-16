use kiana_client::{
    AcpInitializeRequest, AcpPermissionDecision, AcpPermissionRequest, AcpProjection,
    AcpUpdateDisposition, LiveAcpError, LiveAcpOptIn, LiveAcpSession, LiveAcpState,
    LiveAcpTransport, ACP_INITIALIZE_SCHEMA, ACP_PERMISSION_SCHEMA, ACP_PROTOCOL_V1,
    LIVE_ACP_OPT_IN_SCHEMA,
};
use kiana_domain::{ExecutionStatus, RunId, SessionId};
use kiana_protocol::{
    UiFeedFrameKind, UiFeedFrameV1, UiHostCapability, UiHostCapabilityDisposition, UiSnapshot,
    UiSurface, PROTOCOL_SCHEMA, UI_FEED_FRAME_SCHEMA, UI_HOST_CAPABILITY_SCHEMA,
};
use serde_json::json;
use std::fs;
use std::path::Path;

fn digest(seed: char) -> String {
    format!("sha256:{}", seed.to_string().repeat(64))
}

fn capability(direct_effect: bool) -> UiHostCapability {
    UiHostCapability {
        schema: UI_HOST_CAPABILITY_SCHEMA.to_owned(),
        capability_id: "editor.selection".to_owned(),
        actions: vec!["selection.read".to_owned()],
        scope_digest: digest('a'),
        disposition: UiHostCapabilityDisposition::Advertised,
        direct_effect,
        delegated_to_kiana: true,
        reason: None,
    }
}

fn opt_in() -> LiveAcpOptIn {
    LiveAcpOptIn {
        schema: LIVE_ACP_OPT_IN_SCHEMA.to_owned(),
        opt_in: true,
        protocol: ACP_PROTOCOL_V1.to_owned(),
        surface: UiSurface::Ide,
        host_id: "editor-host".to_owned(),
        host_version: "1.2.3".to_owned(),
        environment_digest: digest('b'),
        workspace_digest: digest('c'),
        operator_approval_ref: "approval:ui39".to_owned(),
        transport: LiveAcpTransport::LocalStdio,
        redacted_payloads: true,
        host_capabilities: vec![capability(false)],
    }
}

fn frame(sequence: u64, terminal: bool) -> UiFeedFrameV1 {
    UiFeedFrameV1 {
        schema: UI_FEED_FRAME_SCHEMA.to_owned(),
        kind: if terminal {
            UiFeedFrameKind::Terminal
        } else {
            UiFeedFrameKind::Delta
        },
        cursor: kiana_protocol::UiFeedCursorV1::new(
            "instance-39",
            "epoch-39",
            sequence,
            kiana_protocol::UiCursor {
                epoch: "epoch-39".to_owned(),
                sequence: 1,
            },
        )
        .unwrap(),
        event_id: format!("event-{sequence}"),
        replay: false,
        terminal,
        event: Some(json!({"session_id":"session-39","sequence":sequence})),
        gap: None,
    }
}

fn snapshot(sequence: u64, status: ExecutionStatus) -> UiSnapshot {
    UiSnapshot {
        schema: PROTOCOL_SCHEMA.to_owned(),
        cursor: kiana_protocol::UiCursor {
            epoch: "epoch-39".to_owned(),
            sequence: 99,
        },
        session_id: "session-39".to_owned(),
        run_id: Some(RunId::new()),
        stream_cursor: Some(kiana_protocol::UiCursor {
            epoch: "epoch-39".to_owned(),
            sequence,
        }),
        status: Some(status),
        pending_actions: vec![],
    }
}

fn initialize(session: &mut LiveAcpSession) {
    session
        .initialize(AcpInitializeRequest {
            schema: ACP_INITIALIZE_SCHEMA.to_owned(),
            protocol: ACP_PROTOCOL_V1.to_owned(),
            client_name: "editor-host".to_owned(),
            client_version: "1.2.3".to_owned(),
            capabilities: vec!["session.resume".to_owned(), "permission.request".to_owned()],
        })
        .unwrap();
}

fn attach(session: &mut LiveAcpSession) -> SessionId {
    initialize(session);
    let id = SessionId::new("session-39");
    session
        .new_session(id.clone(), digest('d'), "epoch-39")
        .unwrap();
    session.install_feed_handler().unwrap();
    id
}

#[test]
fn fixture_declares_explicit_live_opt_in_contract() {
    let value: serde_json::Value = serde_json::from_str(
        &fs::read_to_string(
            Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/ui39-live-acp-opt-in.json"),
        )
        .unwrap(),
    )
    .unwrap();
    assert_eq!(value["schema"], "kiana.live-acp-opt-in-fixture.v1");
    assert!(value["denied"]
        .as_array()
        .unwrap()
        .iter()
        .any(|item| item.as_str().unwrap().contains("remote")));
}

#[test]
fn opt_in_is_explicit_local_redacted_and_delegated() {
    let mut missing = opt_in();
    missing.opt_in = false;
    assert!(matches!(
        LiveAcpSession::start(missing),
        Err(LiveAcpError::InvalidOptIn("explicit_opt_in"))
    ));

    let mut unredacted = opt_in();
    unredacted.redacted_payloads = false;
    assert!(matches!(
        LiveAcpSession::start(unredacted),
        Err(LiveAcpError::InvalidOptIn("redacted_payloads"))
    ));

    let mut direct = opt_in();
    direct.host_capabilities[0].direct_effect = true;
    assert!(matches!(
        LiveAcpSession::start(direct),
        Err(LiveAcpError::InvalidOptIn("host_capability"))
    ));
}

#[test]
fn opt_in_session_routes_all_events_through_shared_adapter() {
    let mut session = LiveAcpSession::start(opt_in()).unwrap();
    let id = attach(&mut session);
    assert!(matches!(session.state(), LiveAcpState::Attached));

    assert!(matches!(
        session
            .prompt(&id, "show selection", "epoch-39", 1, "prompt-39")
            .unwrap(),
        AcpProjection::Action(_)
    ));
    session
        .permission_request(
            AcpPermissionRequest {
                schema: ACP_PERMISSION_SCHEMA.to_owned(),
                permission_id: "permission-39".to_owned(),
                session_id: id.clone(),
                owner_digest: digest('d'),
                authority_epoch: "epoch-39".to_owned(),
                expires_at_unix_ms: 101,
                summary: "allow a bounded action".to_owned(),
            },
            100,
        )
        .unwrap();
    assert!(matches!(
        session
            .resolve_permission(
                "permission-39",
                AcpPermissionDecision::Allow,
                100,
                1,
                "permission-39-decision",
            )
            .unwrap(),
        AcpProjection::Action(_)
    ));
    assert!(matches!(
        session.update(frame(1, false)).unwrap(),
        AcpUpdateDisposition::Accepted { sequence: 1, .. }
    ));
    assert!(matches!(
        session.cancel(&id, "epoch-39", 1, "cancel-39").unwrap(),
        AcpProjection::Action(_)
    ));
    session.record_receipt(digest('e')).unwrap();

    let evidence = session.evidence().unwrap();
    evidence.validate().unwrap();
    assert_eq!(evidence.status, kiana_protocol::UiLiveHostStatus::OptedIn);
    assert!(evidence.handshake_verified);
    assert!(evidence.session_verified);
    assert!(evidence.prompt_routed);
    assert!(evidence.update_observed);
    assert!(evidence.permission_routed);
    assert!(evidence.cancel_fence_verified);
    assert_eq!(evidence.receipt_digest, Some(digest('e')));
    assert!(!evidence.host_capabilities[0].direct_effect);
}

#[test]
fn disconnect_and_feed_gap_remain_unknown_until_reconnect() {
    let mut session = LiveAcpSession::start(opt_in()).unwrap();
    let id = attach(&mut session);
    session.update(frame(1, false)).unwrap();
    session.disconnect();
    assert_eq!(session.state(), LiveAcpState::Unknown);
    assert_eq!(
        session.evidence().unwrap().status,
        kiana_protocol::UiLiveHostStatus::Unknown
    );

    session.install_feed_handler().unwrap();
    let resumed = session
        .resume_session(&id, &digest('d'), "epoch-39")
        .unwrap();
    assert!(resumed.snapshot_required);
    assert_eq!(session.state(), LiveAcpState::Unknown);
    assert!(!session.evidence().unwrap().reconnect_verified);
    assert!(matches!(
        session.update(frame(2, false)),
        Err(LiveAcpError::Adapter(
            kiana_client::AcpAdapterError::ReconcileRequired
        ))
    ));
    session
        .hydrate_snapshot(&id, &digest('d'), &snapshot(1, ExecutionStatus::Running))
        .unwrap();
    assert_eq!(session.state(), LiveAcpState::Attached);
    assert!(session.evidence().unwrap().reconnect_verified);
}

#[test]
fn feed_gap_cannot_be_cleared_by_handler_reinstallation() {
    let mut session = LiveAcpSession::start(opt_in()).unwrap();
    let id = attach(&mut session);
    session.update(frame(1, false)).unwrap();
    assert!(matches!(
        session.update(frame(3, false)),
        Ok(AcpUpdateDisposition::Gap {
            expected: 2,
            received: 3
        })
    ));
    assert_eq!(session.state(), LiveAcpState::Unknown);

    session.install_feed_handler().unwrap();
    assert_eq!(session.state(), LiveAcpState::Unknown);
    assert!(matches!(
        session.update(frame(2, false)),
        Err(LiveAcpError::Adapter(
            kiana_client::AcpAdapterError::ReconcileRequired
        ))
    ));

    session
        .resume_session(&id, &digest('d'), "epoch-39")
        .unwrap();
    assert_eq!(session.state(), LiveAcpState::Unknown);
    session
        .hydrate_snapshot(&id, &digest('d'), &snapshot(1, ExecutionStatus::Running))
        .unwrap();
    assert_eq!(session.state(), LiveAcpState::Attached);
    assert!(matches!(
        session.update(frame(2, false)).unwrap(),
        AcpUpdateDisposition::Accepted { sequence: 2, .. }
    ));
}

#[test]
fn source_adapter_has_no_external_process_or_network_authority() {
    let source = include_str!("../src/live_acp.rs");
    for forbidden in [
        "std::process::Command",
        "TcpStream",
        "reqwest",
        "CapabilityBroker",
    ] {
        assert!(
            !source.contains(forbidden),
            "forbidden source marker: {forbidden}"
        );
    }
}

#[test]
fn live_resume_plan_keeps_unknown_until_server_snapshot_is_hydrated() {
    let mut session = LiveAcpSession::start(opt_in()).unwrap();
    let id = attach(&mut session);
    session.update(frame(1, false)).unwrap();
    session.disconnect();
    session.install_feed_handler().unwrap();
    let plan = session
        .resume_session(&id, &digest('d'), "epoch-39")
        .unwrap();
    assert!(plan.snapshot_required);
    assert_eq!(session.state(), LiveAcpState::Unknown);
    assert!(!session.evidence().unwrap().reconnect_verified);
    assert!(matches!(
        session.update(frame(2, false)),
        Err(LiveAcpError::Adapter(
            kiana_client::AcpAdapterError::ReconcileRequired
        ))
    ));
}

#[test]
fn failed_live_hydration_preserves_unknown_and_never_promotes_external_proof() {
    let mut session = LiveAcpSession::start(opt_in()).unwrap();
    let id = attach(&mut session);
    session.update(frame(1, false)).unwrap();
    session.disconnect();
    session.install_feed_handler().unwrap();
    session
        .resume_session(&id, &digest('d'), "epoch-39")
        .unwrap();
    for status in [ExecutionStatus::Running, ExecutionStatus::ResultUnknown] {
        let candidate = snapshot(1, status);
        if status == ExecutionStatus::Running {
            assert_eq!(
                session.hydrate_snapshot(&id, &digest('e'), &candidate),
                Err(LiveAcpError::Adapter(
                    kiana_client::AcpAdapterError::SessionOwnerMismatch
                ))
            );
        } else {
            assert_eq!(
                session.hydrate_snapshot(&id, &digest('d'), &candidate),
                Err(LiveAcpError::Adapter(
                    kiana_client::AcpAdapterError::ReconcileRequired
                ))
            );
        }
        assert_eq!(session.state(), LiveAcpState::Unknown);
        let evidence = session.evidence().unwrap();
        assert_eq!(evidence.status, kiana_protocol::UiLiveHostStatus::Unknown);
        assert!(!evidence.reconnect_verified);
        assert!(!evidence.host_capabilities[0].direct_effect);
    }
    assert!(matches!(
        session.new_session(SessionId::new("replacement"), digest('d'), "epoch-39"),
        Err(LiveAcpError::Adapter(
            kiana_client::AcpAdapterError::ReconcileRequired
        ))
    ));
    assert!(matches!(
        session.initialize(AcpInitializeRequest {
            schema: ACP_INITIALIZE_SCHEMA.to_owned(),
            protocol: ACP_PROTOCOL_V1.to_owned(),
            client_name: "editor-host".to_owned(),
            client_version: "1.2.3".to_owned(),
            capabilities: vec![],
        }),
        Err(LiveAcpError::Adapter(
            kiana_client::AcpAdapterError::ReconcileRequired
        ))
    ));
    assert_eq!(session.state(), LiveAcpState::Unknown);
    session
        .hydrate_snapshot(&id, &digest('d'), &snapshot(2, ExecutionStatus::Running))
        .unwrap();
    assert_eq!(session.state(), LiveAcpState::Attached);
    let evidence = session.evidence().unwrap();
    assert_eq!(evidence.status, kiana_protocol::UiLiveHostStatus::OptedIn);
    assert!(evidence.reconnect_verified);
    assert!(!evidence.host_capabilities[0].direct_effect);
    assert!(matches!(
        session.update(frame(3, false)).unwrap(),
        AcpUpdateDisposition::Accepted { sequence: 3, .. }
    ));
    assert!(matches!(
        session.update(frame(5, false)).unwrap(),
        AcpUpdateDisposition::Gap { .. }
    ));
    assert_eq!(session.state(), LiveAcpState::Unknown);
    assert!(!session.evidence().unwrap().reconnect_verified);
}

#[test]
fn idle_reconnect_restores_the_display_without_claiming_live_verification() {
    let mut session = LiveAcpSession::start(opt_in()).unwrap();
    let id = attach(&mut session);
    session.disconnect();
    session.install_feed_handler().unwrap();
    session
        .resume_session(&id, &digest('d'), "epoch-39")
        .unwrap();
    let idle = UiSnapshot {
        schema: PROTOCOL_SCHEMA.to_owned(),
        cursor: kiana_protocol::UiCursor {
            epoch: "epoch-39".to_owned(),
            sequence: 0,
        },
        session_id: id.to_string(),
        run_id: None,
        stream_cursor: None,
        status: None,
        pending_actions: vec![],
    };
    session.hydrate_snapshot(&id, &digest('d'), &idle).unwrap();
    assert_eq!(session.state(), LiveAcpState::Attached);
    let evidence = session.evidence().unwrap();
    assert!(evidence.reconnect_verified);
    assert_eq!(evidence.status, kiana_protocol::UiLiveHostStatus::OptedIn);
    assert!(!evidence.update_observed);
    assert!(!evidence.prompt_routed);
    assert!(!evidence.host_capabilities[0].direct_effect);
}

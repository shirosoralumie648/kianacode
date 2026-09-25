use kiana_client::{
    AcpAdapterError, AcpInitializeRequest, AcpPermissionDecision, AcpPermissionRequest,
    AcpProjection, AcpSessionAdapter, AcpUpdateDisposition, ACP_INITIALIZE_SCHEMA,
    ACP_PERMISSION_SCHEMA, ACP_PROTOCOL_V1,
};
use kiana_domain::SessionId;
use kiana_protocol::{
    UiCursor, UiFeedCursorV1, UiFeedFrameKind, UiFeedFrameV1, UI_FEED_FRAME_SCHEMA,
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
                owner_digest: owner,
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
}

use async_trait::async_trait;
use kiana_client::{
    ActionClient, ActionRequest, ClientError, ClientRequestOptions, ClientTransport, FeedClient,
    HistoryRequest, QueryClient, SnapshotRequest,
};
use kiana_domain::json_digest;
use kiana_protocol::{
    ExecutionStatus, RequestBody, RequestEnvelope, RequestId, ResponseEnvelope, UiActionV1,
    UiCursor, UiFeedCursorV1, UiFeedFrameKind, UiFeedFrameV1, UiHandshakeRequest, UiSurface,
    PROTOCOL_SCHEMA, UI_ACTION_RESULT_SCHEMA, UI_FEED_FRAME_SCHEMA, UI_HANDSHAKE_REQUEST_SCHEMA,
    UI_HANDSHAKE_RESPONSE_SCHEMA,
};
use serde_json::{json, Value};
use std::sync::{Arc, Mutex};

#[derive(Clone, Default)]
struct MockTransport {
    requests: Arc<Mutex<Vec<RequestEnvelope>>>,
    unknown_action: bool,
}

#[async_trait]
impl ClientTransport for MockTransport {
    async fn send(&self, request: RequestEnvelope) -> Result<ResponseEnvelope, ClientError> {
        let request_id = request.metadata.request_id;
        let operation = match &request.body {
            RequestBody::Command(command) => command.name.clone(),
            _ => "other".to_owned(),
        };
        self.requests.lock().unwrap().push(request);
        let output = match operation.as_str() {
            "ui.initialize" => json!({
                "schema": UI_HANDSHAKE_RESPONSE_SCHEMA,
                "server_version": "fixture",
                "instance_id": "instance-1",
                "authority_epoch": 1,
                "surface": "cli",
                "capabilities": [],
                "limitations": []
            }),
            "ui.action.submit" if self.unknown_action => json!({
                "schema": UI_ACTION_RESULT_SCHEMA,
                "command_id": request_id,
                "disposition": "unknown",
                "resulting_cursor": null,
                "resulting_revision": null,
                "receipt": null,
                "error": {
                    "schema": "kiana.ui-error.v1",
                    "code": "unknown",
                    "message": "transport_lost",
                    "retry": "query_original"
                },
                "retry": "query_original"
            }),
            "ui.action.submit" | "ui.action.status" => json!({
                "schema": UI_ACTION_RESULT_SCHEMA,
                "command_id": request_id,
                "disposition": if operation == "ui.action.submit" { "accepted" } else { "unknown" },
                "resulting_cursor": null,
                "resulting_revision": null,
                "receipt": null,
                "error": if operation == "ui.action.submit" { Value::Null } else { json!({
                    "schema": "kiana.ui-error.v1",
                    "code": "unknown",
                    "message": "query_original",
                    "retry": "query_original"
                }) },
                "retry": if operation == "ui.action.submit" { "do_not_retry" } else { "query_original" }
            }),
            "ui.feed.subscribe" => Value::Null,
            _ => Value::Null,
        };
        Ok(ResponseEnvelope {
            schema: PROTOCOL_SCHEMA.to_owned(),
            request_id,
            status: if operation == "ui.feed.subscribe" {
                ExecutionStatus::Accepted
            } else if self.unknown_action && operation == "ui.action.submit" {
                ExecutionStatus::ResultUnknown
            } else {
                ExecutionStatus::Completed
            },
            output,
            error: None,
        })
    }
}

fn metadata() -> kiana_protocol::RequestMetadata {
    kiana_protocol::RequestMetadata::local("session-1", "/workspace")
}

fn handshake() -> UiHandshakeRequest {
    UiHandshakeRequest {
        schema: UI_HANDSHAKE_REQUEST_SCHEMA.to_owned(),
        client_version: "fixture".to_owned(),
        surface: UiSurface::Cli,
        requested_capabilities: Vec::new(),
        known_instance_id: None,
        known_authority_epoch: None,
    }
}

fn action(command_id: RequestId) -> UiActionV1 {
    let payload = json!({"decision": "approve"});
    UiActionV1 {
        schema: kiana_protocol::UI_ACTION_SCHEMA.to_owned(),
        command_id,
        idempotency_key: "fixture-action-1".to_owned(),
        target_id: "approval-1".to_owned(),
        expected_epoch: "epoch-1".to_owned(),
        expected_cursor: 1,
        expected_revision: None,
        payload_digest: json_digest(&payload),
        payload,
        submitted_by: "fixture-user".to_owned(),
        deadline_unix_ms: Some(u64::MAX),
    }
}

#[tokio::test]
async fn typed_query_fails_closed_before_initialize_and_deadline() {
    let query = QueryClient::new(MockTransport::default());
    let error = query
        .snapshot(
            metadata(),
            SnapshotRequest::default(),
            ClientRequestOptions::default(),
        )
        .await
        .unwrap_err();
    assert_eq!(error, ClientError::NotInitialized);

    let error = query
        .snapshot(
            metadata(),
            SnapshotRequest::default(),
            ClientRequestOptions::default().with_deadline_unix_ms(1),
        )
        .await
        .unwrap_err();
    assert_eq!(error, ClientError::DeadlineExceeded);
}

#[tokio::test]
async fn accepted_action_is_not_promoted_to_applied_and_wrong_retry_is_denied() {
    let transport = MockTransport::default();
    let action_client = ActionClient::new(transport);
    action_client
        .initialize(metadata(), handshake())
        .await
        .unwrap();
    let id = RequestId::new();
    let first = action_client
        .submit(
            metadata(),
            ActionRequest::new(action(id)).unwrap(),
            ClientRequestOptions::default(),
        )
        .await
        .unwrap();
    assert_eq!(
        first.disposition,
        kiana_protocol::UiActionDisposition::Accepted
    );

    let wrong = action_client
        .submit(
            metadata(),
            ActionRequest::new(action(RequestId::new())).unwrap(),
            ClientRequestOptions::default(),
        )
        .await
        .unwrap_err();
    assert_eq!(wrong, ClientError::CommandRetryForbidden);
}

#[tokio::test]
async fn unknown_action_is_queryable_by_original_idempotency_key() {
    let action_client = ActionClient::new(MockTransport {
        unknown_action: true,
        ..MockTransport::default()
    });
    action_client
        .initialize(metadata(), handshake())
        .await
        .unwrap();
    let id = RequestId::new();
    let result = action_client
        .submit(
            metadata(),
            ActionRequest::new(action(id)).unwrap(),
            ClientRequestOptions::default(),
        )
        .await
        .unwrap();
    assert_eq!(
        result.disposition,
        kiana_protocol::UiActionDisposition::Unknown
    );
    assert_eq!(
        result.retry,
        kiana_protocol::UiRetryDisposition::QueryOriginal
    );
    let reconciled = action_client
        .query_original(
            metadata(),
            id,
            "fixture-action-1",
            ClientRequestOptions::default(),
        )
        .await
        .unwrap();
    assert_eq!(
        reconciled.disposition,
        kiana_protocol::UiActionDisposition::Unknown
    );
}

#[tokio::test]
async fn feed_listener_is_installed_before_subscribe_and_removed_on_drop() {
    let feed = FeedClient::new(MockTransport::default());
    let clients = feed.clone();
    clients.initialize(metadata(), handshake()).await.unwrap();
    let first = clients
        .subscribe(
            metadata(),
            kiana_protocol::RunId::new(),
            None,
            "listener-1",
            |_| {},
            ClientRequestOptions::default(),
        )
        .await
        .unwrap();
    let duplicate = clients
        .subscribe(
            metadata(),
            kiana_protocol::RunId::new(),
            None,
            "listener-1",
            |_| {},
            ClientRequestOptions::default(),
        )
        .await;
    assert!(matches!(duplicate, Err(ClientError::DuplicateListener(_))));

    let token = first.token().clone();
    first.cancel();
    let frame = UiFeedFrameV1 {
        schema: UI_FEED_FRAME_SCHEMA.to_owned(),
        kind: UiFeedFrameKind::Heartbeat,
        cursor: UiFeedCursorV1::new(
            "instance-1",
            "epoch-1",
            1,
            UiCursor {
                epoch: "epoch-1".to_owned(),
                sequence: 1,
            },
        )
        .unwrap(),
        event_id: "heartbeat-1".to_owned(),
        replay: false,
        terminal: false,
        event: None,
        gap: None,
    };
    assert!(matches!(
        clients.dispatch(&token, frame),
        Err(ClientError::ListenerInactive)
    ));
}

#[allow(dead_code)]
fn _history_request_fixture() -> HistoryRequest {
    HistoryRequest {
        session_id: "session-1".to_owned(),
        after: None,
        limit: 16,
    }
}

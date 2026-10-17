//! UI-07 typed client query/feed/action/artifact lifecycle fixtures.
//!
//! Every fixture is deny-first: the local fences (handshake, workspace, schema, request id,
//! deadline, cancellation, listener generation, command identity) are asserted before the
//! matching success path.  The transport below is a scripted mock: it records the envelopes the
//! client actually sent and the cancel-hook calls, and it can hold a response until the client
//! deadline has certainly elapsed without sleeping inside the fixture.

use async_trait::async_trait;
use kiana_client::{
    ActionClient, ActionRequest, ArtifactPageRequest, CancellationToken, ClientError,
    ClientRequestOptions, ClientTransport, CommandStatusRequest, FeedClient, HistoryRequest,
    KianaClient, QueryClient, SnapshotRequest, TypedClients, UiCommandStatusV1,
    UI_ACTION_CANCEL_OPERATION, UI_ACTION_CONTINUE_OPERATION, UI_ACTION_STATUS_OPERATION,
    UI_ACTION_SUBMIT_OPERATION, UI_ARTIFACT_PAGE_OPERATION, UI_ARTIFACT_PAGE_SCHEMA,
    UI_COMMAND_STATUS_SCHEMA, UI_FEED_RESUME_OPERATION, UI_FEED_SUBSCRIBE_OPERATION,
    UI_HISTORY_SCHEMA, UI_INITIALIZE_OPERATION, UI_QUERY_COMMAND_STATUS_OPERATION,
    UI_QUERY_HISTORY_OPERATION, UI_QUERY_SNAPSHOT_OPERATION,
};
use kiana_domain::json_digest;
use kiana_protocol::{
    ArtifactId, AuthenticatedPrincipalRef, ExecutionStatus, ProjectId, RequestBody,
    RequestEnvelope, RequestId, RequestMetadata, ResponseEnvelope, RunId, UiActionDisposition,
    UiActionResult, UiActionV1, UiCursor, UiCursorV1, UiError, UiErrorCode, UiFeedCursorV1,
    UiFeedFrameKind, UiFeedFrameV1, UiFeedGapReason, UiFeedGapV1, UiHandshakeRequest,
    UiHandshakeResponse, UiRetryDisposition, UiSnapshotV1, UiSurface, PROTOCOL_SCHEMA,
    UI_ACTION_RESULT_SCHEMA, UI_ACTION_SCHEMA, UI_FEED_FRAME_SCHEMA, UI_FEED_GAP_SCHEMA,
    UI_HANDSHAKE_REQUEST_SCHEMA, UI_HANDSHAKE_RESPONSE_SCHEMA, UI_SNAPSHOT_SCHEMA,
};
use serde::Deserialize;
use serde_json::{json, Value};
use std::path::Path;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};

// ---------------------------------------------------------------------------------------------
// Scripted transport
// ---------------------------------------------------------------------------------------------

type Handler = dyn Fn(&RequestEnvelope) -> Result<ResponseEnvelope, ClientError> + Send + Sync;
type Route = (&'static str, Box<Handler>);

#[derive(Clone, Debug, PartialEq)]
struct RecordedRequest {
    operation: String,
    request_id: RequestId,
    arguments: Value,
}

/// A transport that answers the operations a fixture routed, records every envelope it received
/// and records the best-effort cancel hook.  Nothing here owns a daemon, a broker or a command
/// executor: it returns protocol envelopes, never effects.
#[derive(Clone)]
struct MockTransport {
    routes: Arc<Vec<Route>>,
    requests: Arc<Mutex<Vec<RecordedRequest>>>,
    cancellations: Arc<Mutex<Vec<RequestId>>>,
}

impl MockTransport {
    fn new(routes: Vec<Route>) -> Self {
        Self {
            routes: Arc::new(routes),
            requests: Arc::new(Mutex::new(Vec::new())),
            cancellations: Arc::new(Mutex::new(Vec::new())),
        }
    }

    fn requests(&self) -> Vec<RecordedRequest> {
        self.requests.lock().expect("requests").clone()
    }

    fn operations(&self) -> Vec<String> {
        self.requests()
            .into_iter()
            .map(|recorded| recorded.operation)
            .collect()
    }

    fn cancellations(&self) -> Vec<RequestId> {
        self.cancellations.lock().expect("cancellations").clone()
    }
}

#[async_trait]
impl ClientTransport for MockTransport {
    async fn send(&self, request: RequestEnvelope) -> Result<ResponseEnvelope, ClientError> {
        let operation = operation_of(&request);
        let arguments = match &request.body {
            RequestBody::Command(command) => command.arguments.clone(),
            _ => Value::Null,
        };
        self.requests
            .lock()
            .expect("requests")
            .push(RecordedRequest {
                operation: operation.clone(),
                request_id: request.metadata.request_id,
                arguments,
            });
        for (name, handler) in self.routes.iter() {
            if *name == operation.as_str() {
                return handler(&request);
            }
        }
        Ok(completed(&request, Value::Null))
    }

    async fn cancel(&self, request_id: RequestId) -> Result<(), ClientError> {
        self.cancellations
            .lock()
            .expect("cancellations")
            .push(request_id);
        Ok(())
    }
}

fn route<F>(name: &'static str, handler: F) -> Route
where
    F: Fn(&RequestEnvelope) -> Result<ResponseEnvelope, ClientError> + Send + Sync + 'static,
{
    (name, Box::new(handler))
}

fn operation_of(request: &RequestEnvelope) -> String {
    match &request.body {
        RequestBody::Command(command) => command.name.clone(),
        _ => "other".to_owned(),
    }
}

// ---------------------------------------------------------------------------------------------
// Protocol envelopes
// ---------------------------------------------------------------------------------------------

fn completed(request: &RequestEnvelope, output: Value) -> ResponseEnvelope {
    with_status(request, ExecutionStatus::Completed, output, None)
}

fn accepted(request: &RequestEnvelope, output: Value) -> ResponseEnvelope {
    with_status(request, ExecutionStatus::Accepted, output, None)
}

fn with_status(
    request: &RequestEnvelope,
    status: ExecutionStatus,
    output: Value,
    error: Option<String>,
) -> ResponseEnvelope {
    ResponseEnvelope {
        schema: PROTOCOL_SCHEMA.to_owned(),
        request_id: request.metadata.request_id,
        status,
        output,
        error,
    }
}

fn handshake_route() -> Route {
    route(UI_INITIALIZE_OPERATION, |request| {
        Ok(completed(request, handshake_value()))
    })
}

fn handshake_response() -> UiHandshakeResponse {
    UiHandshakeResponse {
        schema: UI_HANDSHAKE_RESPONSE_SCHEMA.to_owned(),
        server_version: "fixture".to_owned(),
        instance_id: "instance-1".to_owned(),
        authority_epoch: 1,
        surface: UiSurface::Cli,
        capabilities: Vec::new(),
        limitations: Vec::new(),
    }
}

fn handshake_value() -> Value {
    serde_json::to_value(handshake_response()).expect("handshake")
}

fn handshake_request() -> UiHandshakeRequest {
    UiHandshakeRequest {
        schema: UI_HANDSHAKE_REQUEST_SCHEMA.to_owned(),
        client_version: "fixture".to_owned(),
        surface: UiSurface::Cli,
        requested_capabilities: Vec::new(),
        known_instance_id: None,
        known_authority_epoch: None,
    }
}

fn action_result_struct(
    request: &RequestEnvelope,
    disposition: UiActionDisposition,
) -> UiActionResult {
    let (error, retry) = match disposition {
        UiActionDisposition::Unknown => (
            Some(UiError::new(
                UiErrorCode::Unknown,
                "query the original command",
                UiRetryDisposition::QueryOriginal,
            )),
            UiRetryDisposition::QueryOriginal,
        ),
        UiActionDisposition::Rejected => (
            Some(UiError::new(
                UiErrorCode::PermissionDenied,
                "permission denied",
                UiRetryDisposition::DoNotRetry,
            )),
            UiRetryDisposition::DoNotRetry,
        ),
        UiActionDisposition::Accepted | UiActionDisposition::Applied => {
            (None, UiRetryDisposition::DoNotRetry)
        }
    };
    UiActionResult {
        schema: UI_ACTION_RESULT_SCHEMA.to_owned(),
        command_id: request.metadata.request_id,
        disposition,
        resulting_cursor: None,
        resulting_revision: None,
        receipt: None,
        error,
        retry,
    }
}

fn action_result(request: &RequestEnvelope, disposition: UiActionDisposition) -> Value {
    serde_json::to_value(action_result_struct(request, disposition)).expect("action result")
}

fn snapshot_value() -> Value {
    let snapshot = UiSnapshotV1 {
        schema: UI_SNAPSHOT_SCHEMA.to_owned(),
        instance_id: "instance-1".to_owned(),
        authority_epoch: 1,
        snapshot_cursor: UiCursorV1 {
            epoch: "epoch-1".to_owned(),
            sequence: 1,
        },
        generated_at_unix_ms: 1,
        principal: AuthenticatedPrincipalRef::local(),
        workspace_id: ProjectId::new(),
        capabilities: Vec::new(),
        sessions: Vec::new(),
        active_session_id: None,
        runs: Vec::new(),
        pending_actions: Vec::new(),
        artifacts: Vec::new(),
        notices: Vec::new(),
        next_page: None,
        limitations: Vec::new(),
    };
    serde_json::to_value(snapshot).expect("snapshot")
}

fn history_value(instance_id: &str, authority_epoch: &str, frame: UiFeedFrameV1) -> Value {
    json!({
        "schema": UI_HISTORY_SCHEMA,
        "instance_id": instance_id,
        "snapshot_cursor": {"epoch": authority_epoch, "sequence": 1},
        "frames": [frame],
        "next_page": null,
        "limitations": [],
    })
}

fn command_status_value(request: &RequestEnvelope) -> Value {
    let status = UiCommandStatusV1 {
        schema: UI_COMMAND_STATUS_SCHEMA.to_owned(),
        command_id: request.metadata.request_id,
        status: ExecutionStatus::Completed,
        action: Some(action_result_struct(request, UiActionDisposition::Accepted)),
        error: None,
        retry: UiRetryDisposition::DoNotRetry,
    };
    serde_json::to_value(status).expect("command status")
}

fn artifact_page_value(digest: &str) -> Value {
    json!({
        "schema": UI_ARTIFACT_PAGE_SCHEMA,
        "artifact_id": ArtifactId::new(),
        "digest": digest,
        "mime": "text/plain",
        "offset": 0,
        "content": {"text": "diff"},
        "next_page": null,
        "limitations": [],
    })
}

fn feed_cursor(feed_sequence: u64, ui_sequence: u64) -> UiFeedCursorV1 {
    UiFeedCursorV1::new(
        "instance-1",
        "epoch-1",
        feed_sequence,
        UiCursor {
            epoch: if ui_sequence == 0 {
                String::new()
            } else {
                "epoch-1".to_owned()
            },
            sequence: ui_sequence,
        },
    )
    .expect("feed cursor")
}

fn frame(
    kind: UiFeedFrameKind,
    feed_sequence: u64,
    ui_sequence: u64,
    gap: Option<UiFeedGapV1>,
) -> UiFeedFrameV1 {
    UiFeedFrameV1 {
        schema: UI_FEED_FRAME_SCHEMA.to_owned(),
        terminal: matches!(kind, UiFeedFrameKind::Terminal),
        cursor: feed_cursor(feed_sequence, ui_sequence),
        event_id: format!("{kind:?}-{feed_sequence}"),
        kind,
        replay: false,
        event: None,
        gap,
    }
}

fn boundary_frame() -> UiFeedFrameV1 {
    frame(UiFeedFrameKind::SnapshotBoundary, 0, 0, None)
}

fn heartbeat_frame(feed_sequence: u64) -> UiFeedFrameV1 {
    frame(UiFeedFrameKind::Heartbeat, feed_sequence, 1, None)
}

fn delta_frame(feed_sequence: u64) -> UiFeedFrameV1 {
    let mut frame = frame(UiFeedFrameKind::Delta, feed_sequence, 1, None);
    frame.event = Some(json!({"kind": "run.delta", "text": "hello"}));
    frame
}

fn gap_frame(feed_sequence: u64, reason: UiFeedGapReason) -> UiFeedFrameV1 {
    let cursor = feed_cursor(feed_sequence, 1);
    let gap = UiFeedGapV1 {
        schema: UI_FEED_GAP_SCHEMA.to_owned(),
        reason,
        from: None,
        to: cursor.clone(),
        snapshot_required: true,
    };
    frame(UiFeedFrameKind::Gap, feed_sequence, 1, Some(gap))
}

fn metadata() -> RequestMetadata {
    RequestMetadata::local("session-1", "/workspace")
}

fn foreign_metadata() -> RequestMetadata {
    RequestMetadata::local("session-1", "/other-workspace")
}

fn action(command_id: RequestId, idempotency_key: &str) -> UiActionV1 {
    let payload = json!({"decision": "approve"});
    UiActionV1 {
        schema: UI_ACTION_SCHEMA.to_owned(),
        command_id,
        idempotency_key: idempotency_key.to_owned(),
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

fn now_unix_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis().min(u64::MAX as u128) as u64)
        .unwrap_or(0)
}

/// Hold the response until the client deadline has elapsed, without sleeping.
fn wait_until(deadline_unix_ms: u64) {
    while now_unix_ms() < deadline_unix_ms {
        std::hint::spin_loop();
    }
}

// ---------------------------------------------------------------------------------------------
// Deny-first fixtures
// ---------------------------------------------------------------------------------------------

#[tokio::test]
async fn typed_query_fails_closed_before_initialize_and_deadline() {
    let transport = MockTransport::new(Vec::new());
    let query = QueryClient::new(transport.clone());
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

    assert!(
        transport.requests().is_empty(),
        "a denied query must never reach the transport"
    );
}

#[tokio::test]
async fn accepted_action_is_not_promoted_to_applied_and_wrong_retry_is_denied() {
    let transport = MockTransport::new(vec![
        handshake_route(),
        route(UI_ACTION_SUBMIT_OPERATION, |request| {
            Ok(completed(
                request,
                action_result(request, UiActionDisposition::Accepted),
            ))
        }),
    ]);
    let action_client = ActionClient::new(transport.clone());
    action_client
        .initialize(metadata(), handshake_request())
        .await
        .unwrap();

    let id = RequestId::new();
    let first = action_client
        .submit(
            metadata(),
            ActionRequest::new(action(id, "fixture-action-1")).unwrap(),
            ClientRequestOptions::default(),
        )
        .await
        .unwrap();
    assert_eq!(first.disposition, UiActionDisposition::Accepted);
    assert_ne!(first.disposition, UiActionDisposition::Applied);
    assert!(first.receipt.is_none() && first.resulting_cursor.is_none());

    let wrong = action_client
        .submit(
            metadata(),
            ActionRequest::new(action(RequestId::new(), "fixture-action-1")).unwrap(),
            ClientRequestOptions::default(),
        )
        .await
        .unwrap_err();
    assert_eq!(wrong, ClientError::CommandRetryForbidden);
    assert_eq!(
        transport.operations(),
        vec![
            UI_INITIALIZE_OPERATION.to_owned(),
            UI_ACTION_SUBMIT_OPERATION.to_owned()
        ],
        "the denied retry must not reach the transport"
    );
}

#[tokio::test]
async fn unknown_action_is_queryable_by_original_idempotency_key() {
    let transport = MockTransport::new(vec![
        handshake_route(),
        route(UI_ACTION_SUBMIT_OPERATION, |request| {
            Ok(with_status(
                request,
                ExecutionStatus::ResultUnknown,
                action_result(request, UiActionDisposition::Unknown),
                Some("transport_lost".to_owned()),
            ))
        }),
        route(UI_ACTION_STATUS_OPERATION, |request| {
            Ok(completed(
                request,
                action_result(request, UiActionDisposition::Unknown),
            ))
        }),
    ]);
    let action_client = ActionClient::new(transport.clone());
    action_client
        .initialize(metadata(), handshake_request())
        .await
        .unwrap();

    let id = RequestId::new();
    let result = action_client
        .submit(
            metadata(),
            ActionRequest::new(action(id, "fixture-action-1")).unwrap(),
            ClientRequestOptions::default(),
        )
        .await
        .unwrap();
    assert_eq!(result.disposition, UiActionDisposition::Unknown);
    assert_ne!(result.disposition, UiActionDisposition::Rejected);
    assert_eq!(result.retry, UiRetryDisposition::QueryOriginal);

    let reconciled = action_client
        .query_original(
            metadata(),
            id,
            "fixture-action-1",
            ClientRequestOptions::default(),
        )
        .await
        .unwrap();
    assert_eq!(reconciled.disposition, UiActionDisposition::Unknown);

    let error = action_client
        .query_original(metadata(), id, "", ClientRequestOptions::default())
        .await
        .unwrap_err();
    assert_eq!(
        error,
        ClientError::Protocol("ui_action_idempotency_key_invalid".to_owned())
    );
    assert_eq!(
        transport.operations(),
        vec![
            UI_INITIALIZE_OPERATION.to_owned(),
            UI_ACTION_SUBMIT_OPERATION.to_owned(),
            UI_ACTION_STATUS_OPERATION.to_owned()
        ]
    );
}

#[test]
fn history_rejects_foreign_instance_or_authority_epoch_frames() {
    let frame = UiFeedFrameV1 {
        schema: UI_FEED_FRAME_SCHEMA.to_owned(),
        kind: UiFeedFrameKind::Heartbeat,
        cursor: UiFeedCursorV1::new(
            "foreign-instance",
            "epoch-2",
            1,
            UiCursor {
                epoch: "epoch-2".to_owned(),
                sequence: 1,
            },
        )
        .expect("frame cursor"),
        event_id: "heartbeat-foreign".to_owned(),
        replay: false,
        terminal: false,
        event: None,
        gap: None,
    };
    let history = kiana_client::UiHistoryV1 {
        schema: UI_HISTORY_SCHEMA.to_owned(),
        instance_id: "instance-1".to_owned(),
        snapshot_cursor: UiCursorV1 {
            epoch: "epoch-1".to_owned(),
            sequence: 1,
        },
        frames: vec![frame],
        next_page: None,
        limitations: Vec::new(),
    };
    assert_eq!(
        history.validate().unwrap_err(),
        "ui_history_instance_mismatch"
    );

    let mut epoch_history = history;
    epoch_history.frames[0].cursor = UiFeedCursorV1::new(
        "instance-1",
        "epoch-2",
        1,
        UiCursor {
            epoch: "epoch-2".to_owned(),
            sequence: 1,
        },
    )
    .expect("epoch cursor");
    assert_eq!(
        epoch_history.validate().unwrap_err(),
        "ui_history_epoch_mismatch"
    );
}

#[tokio::test]
async fn feed_listener_is_installed_before_subscribe_and_removed_on_drop() {
    let transport = MockTransport::new(vec![
        handshake_route(),
        route(UI_FEED_SUBSCRIBE_OPERATION, |request| {
            Ok(accepted(request, Value::Null))
        }),
    ]);
    let feed = FeedClient::new(transport.clone());
    feed.initialize(metadata(), handshake_request())
        .await
        .unwrap();

    let deliveries = Arc::new(AtomicUsize::new(0));
    let first = feed
        .subscribe(
            metadata(),
            RunId::new(),
            None,
            "listener-1",
            {
                let deliveries = Arc::clone(&deliveries);
                move |_| {
                    deliveries.fetch_add(1, Ordering::SeqCst);
                }
            },
            ClientRequestOptions::default(),
        )
        .await
        .unwrap();
    let duplicate = feed
        .subscribe(
            metadata(),
            RunId::new(),
            None,
            "listener-1",
            |_| {},
            ClientRequestOptions::default(),
        )
        .await;
    assert!(matches!(duplicate, Err(ClientError::DuplicateListener(_))));
    assert_eq!(
        transport.operations(),
        vec![
            UI_INITIALIZE_OPERATION.to_owned(),
            UI_FEED_SUBSCRIBE_OPERATION.to_owned()
        ],
        "a duplicate listener is refused locally"
    );
    feed.dispatch(first.token(), heartbeat_frame(1))
        .expect("an active generation receives its frame");
    assert_eq!(deliveries.load(Ordering::SeqCst), 1);

    let token = first.token().clone();
    first.cancel();
    assert_eq!(
        feed.dispatch(&token, heartbeat_frame(2)).unwrap_err(),
        ClientError::ListenerInactive
    );
    assert_eq!(deliveries.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn every_typed_facade_requires_the_same_handshake_and_workspace() {
    let transport = MockTransport::new(vec![handshake_route()]);
    let clients: TypedClients<MockTransport> = KianaClient::new(transport.clone()).typed_clients();

    assert_eq!(
        clients
            .query
            .snapshot(
                metadata(),
                SnapshotRequest::default(),
                ClientRequestOptions::default()
            )
            .await
            .unwrap_err(),
        ClientError::NotInitialized
    );
    assert_eq!(
        clients
            .query
            .history(
                metadata(),
                HistoryRequest {
                    session_id: "session-1".to_owned(),
                    after: None,
                    limit: 16,
                },
                ClientRequestOptions::default(),
            )
            .await
            .unwrap_err(),
        ClientError::NotInitialized
    );
    assert_eq!(
        clients
            .query
            .command_status(
                metadata(),
                CommandStatusRequest {
                    command_id: RequestId::new(),
                    idempotency_key: None,
                },
                ClientRequestOptions::default(),
            )
            .await
            .unwrap_err(),
        ClientError::NotInitialized
    );
    assert_eq!(
        clients
            .artifact
            .page(
                metadata(),
                ArtifactPageRequest {
                    artifact_id: ArtifactId::new(),
                    page_size: 8,
                    after: None,
                },
                ClientRequestOptions::default(),
            )
            .await
            .unwrap_err(),
        ClientError::NotInitialized
    );
    assert_eq!(
        clients
            .action
            .submit(
                metadata(),
                ActionRequest::new(action(RequestId::new(), "fixture-action-1")).unwrap(),
                ClientRequestOptions::default(),
            )
            .await
            .unwrap_err(),
        ClientError::NotInitialized
    );
    let feed_error = clients
        .feed
        .subscribe(
            metadata(),
            RunId::new(),
            None,
            "listener-1",
            |_| {},
            ClientRequestOptions::default(),
        )
        .await
        .err()
        .expect("subscribe must fail before the handshake");
    assert_eq!(feed_error, ClientError::NotInitialized);
    assert!(
        transport.requests().is_empty(),
        "no facade dispatches before the negotiated session exists"
    );

    clients
        .initialize(metadata(), handshake_request())
        .await
        .unwrap();
    assert_eq!(
        clients
            .query
            .snapshot(
                foreign_metadata(),
                SnapshotRequest::default(),
                ClientRequestOptions::default()
            )
            .await
            .unwrap_err(),
        ClientError::WorkspaceMismatch
    );
    assert_eq!(
        clients
            .artifact
            .page(
                foreign_metadata(),
                ArtifactPageRequest {
                    artifact_id: ArtifactId::new(),
                    page_size: 8,
                    after: None,
                },
                ClientRequestOptions::default(),
            )
            .await
            .unwrap_err(),
        ClientError::WorkspaceMismatch
    );
    assert_eq!(
        clients
            .action
            .submit(
                foreign_metadata(),
                ActionRequest::new(action(RequestId::new(), "fixture-action-2")).unwrap(),
                ClientRequestOptions::default(),
            )
            .await
            .unwrap_err(),
        ClientError::WorkspaceMismatch
    );
    let feed_error = clients
        .feed
        .subscribe(
            foreign_metadata(),
            RunId::new(),
            None,
            "listener-2",
            |_| {},
            ClientRequestOptions::default(),
        )
        .await
        .err()
        .expect("a foreign workspace cannot subscribe");
    assert_eq!(feed_error, ClientError::WorkspaceMismatch);
    assert_eq!(
        transport.operations(),
        vec![UI_INITIALIZE_OPERATION.to_owned()]
    );
}

#[tokio::test]
async fn unknown_schemas_are_rejected_before_any_typed_projection() {
    let mismatched = MockTransport::new(vec![route(UI_INITIALIZE_OPERATION, |request| {
        let mut value = handshake_value();
        value["schema"] = json!("kiana.ui-handshake-response.v2");
        Ok(completed(request, value))
    })]);
    let query = QueryClient::new(mismatched.clone());
    assert_eq!(
        query
            .initialize(metadata(), handshake_request())
            .await
            .unwrap_err(),
        ClientError::UnknownSchema("kiana.ui-handshake-response.v2".to_owned())
    );
    assert_eq!(
        query
            .snapshot(
                metadata(),
                SnapshotRequest::default(),
                ClientRequestOptions::default()
            )
            .await
            .unwrap_err(),
        ClientError::NotInitialized,
        "a rejected handshake must not install a session"
    );

    let transport = MockTransport::new(vec![
        handshake_route(),
        route(UI_FEED_SUBSCRIBE_OPERATION, |request| {
            Ok(accepted(request, Value::Null))
        }),
        route(UI_ACTION_SUBMIT_OPERATION, |request| {
            let mut value = action_result(request, UiActionDisposition::Accepted);
            value["schema"] = json!("kiana.ui-action-result.v2");
            Ok(completed(request, value))
        }),
    ]);
    let clients = KianaClient::new(transport.clone()).typed_clients();
    clients
        .initialize(metadata(), handshake_request())
        .await
        .unwrap();
    assert_eq!(
        clients
            .action
            .submit(
                metadata(),
                ActionRequest::new(action(RequestId::new(), "fixture-action-3")).unwrap(),
                ClientRequestOptions::default(),
            )
            .await
            .unwrap_err(),
        ClientError::UnknownSchema("kiana.ui-action-result.v2".to_owned())
    );

    let subscription = clients
        .feed
        .subscribe(
            metadata(),
            RunId::new(),
            None,
            "listener-1",
            |_| {},
            ClientRequestOptions::default(),
        )
        .await
        .unwrap();
    let mut foreign_schema = heartbeat_frame(1);
    foreign_schema.schema = "kiana.ui-feed-frame.v2".to_owned();
    assert_eq!(
        clients
            .feed
            .dispatch(subscription.token(), foreign_schema)
            .unwrap_err(),
        ClientError::UnknownSchema("kiana.ui-feed-frame.v2".to_owned())
    );
    subscription.cancel();
}

#[tokio::test]
async fn cancelled_or_expired_requests_never_reach_the_transport() {
    let transport = MockTransport::new(vec![handshake_route()]);
    let clients = KianaClient::new(transport.clone()).typed_clients();
    clients
        .initialize(metadata(), handshake_request())
        .await
        .unwrap();

    let cancelled = CancellationToken::new();
    cancelled.cancel();
    assert_eq!(
        clients
            .query
            .snapshot(
                metadata(),
                SnapshotRequest::default(),
                ClientRequestOptions::default().with_cancellation(cancelled),
            )
            .await
            .unwrap_err(),
        ClientError::Cancelled
    );
    assert_eq!(
        clients
            .query
            .snapshot(
                metadata(),
                SnapshotRequest::default(),
                ClientRequestOptions::default().with_deadline_unix_ms(1),
            )
            .await
            .unwrap_err(),
        ClientError::DeadlineExceeded
    );
    assert_eq!(
        clients
            .query
            .snapshot(
                metadata(),
                SnapshotRequest {
                    session_id: None,
                    page_size: 257,
                    after: None,
                },
                ClientRequestOptions::default(),
            )
            .await
            .unwrap_err(),
        ClientError::Protocol("ui_snapshot_page_size_invalid".to_owned())
    );
    assert_eq!(
        clients
            .query
            .history(
                metadata(),
                HistoryRequest {
                    session_id: String::new(),
                    after: None,
                    limit: 16,
                },
                ClientRequestOptions::default(),
            )
            .await
            .unwrap_err(),
        ClientError::Protocol("ui_history_session_invalid".to_owned())
    );
    assert_eq!(
        clients
            .artifact
            .page(
                metadata(),
                ArtifactPageRequest {
                    artifact_id: ArtifactId::new(),
                    page_size: 0,
                    after: None,
                },
                ClientRequestOptions::default(),
            )
            .await
            .unwrap_err(),
        ClientError::Protocol("ui_artifact_page_size_invalid".to_owned())
    );
    assert_eq!(
        transport.operations(),
        vec![UI_INITIALIZE_OPERATION.to_owned()],
        "cancelled, expired and out-of-range requests stay local"
    );
}

#[tokio::test]
async fn a_released_listener_generation_never_receives_a_late_frame() {
    let transport = MockTransport::new(vec![
        handshake_route(),
        route(UI_FEED_SUBSCRIBE_OPERATION, |request| {
            Ok(accepted(request, Value::Null))
        }),
        route(UI_FEED_RESUME_OPERATION, |request| {
            Ok(accepted(request, Value::Null))
        }),
    ]);
    let feed = FeedClient::new(transport.clone());
    feed.initialize(metadata(), handshake_request())
        .await
        .unwrap();

    let deliveries = Arc::new(AtomicUsize::new(0));
    let options = ClientRequestOptions::default();
    let controller_token = options.cancellation.clone();
    let first = feed
        .subscribe(
            metadata(),
            RunId::new(),
            None,
            "listener-1",
            {
                let deliveries = Arc::clone(&deliveries);
                move |_| {
                    deliveries.fetch_add(1, Ordering::SeqCst);
                }
            },
            options,
        )
        .await
        .unwrap();
    feed.dispatch(first.token(), heartbeat_frame(1))
        .expect("an active generation receives its frame");
    assert_eq!(deliveries.load(Ordering::SeqCst), 1);

    // Cancelling the shared controller token retires the generation before the handle is dropped.
    controller_token.cancel();
    assert_eq!(
        feed.dispatch(first.token(), heartbeat_frame(2))
            .unwrap_err(),
        ClientError::ListenerInactive
    );
    assert_eq!(
        feed.resume(
            metadata(),
            RunId::new(),
            first.token(),
            None,
            ClientRequestOptions::default(),
        )
        .await
        .unwrap_err(),
        ClientError::ListenerInactive
    );
    assert_eq!(deliveries.load(Ordering::SeqCst), 1);

    // The retired token stays inactive after release, and a reconnect gets a fresh generation.
    let retired = first.token().clone();
    first.cancel();
    assert_eq!(
        feed.dispatch(&retired, heartbeat_frame(3)).unwrap_err(),
        ClientError::ListenerInactive
    );
    let second = feed
        .subscribe(
            metadata(),
            RunId::new(),
            None,
            "listener-1",
            |_| {},
            ClientRequestOptions::default(),
        )
        .await
        .unwrap();
    assert!(second.token().generation > retired.generation);
    feed.dispatch(second.token(), heartbeat_frame(4))
        .expect("the new generation receives frames");
    assert_eq!(
        feed.dispatch(&retired, heartbeat_frame(5)).unwrap_err(),
        ClientError::ListenerInactive
    );
    assert_eq!(deliveries.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn late_and_mismatched_responses_are_fenced_and_the_cancel_hook_is_used() {
    let transport = MockTransport::new(vec![
        handshake_route(),
        route(UI_ACTION_SUBMIT_OPERATION, |request| {
            if let Some(deadline) = request.metadata.deadline_unix_ms {
                wait_until(deadline);
            }
            Ok(completed(
                request,
                action_result(request, UiActionDisposition::Accepted),
            ))
        }),
        route(UI_QUERY_SNAPSHOT_OPERATION, |request| {
            let mut response = completed(request, snapshot_value());
            response.request_id = RequestId::new();
            Ok(response)
        }),
    ]);
    let clients = KianaClient::new(transport.clone()).typed_clients();
    clients
        .initialize(metadata(), handshake_request())
        .await
        .unwrap();

    let options = ClientRequestOptions::default().with_deadline_unix_ms(now_unix_ms() + 5);
    let request_id = options.request_id;
    assert_eq!(
        clients
            .action
            .submit(
                metadata(),
                ActionRequest::new(action(RequestId::new(), "fixture-action-4")).unwrap(),
                options,
            )
            .await
            .unwrap_err(),
        ClientError::LateResponse(request_id)
    );
    assert_eq!(
        transport.cancellations(),
        vec![request_id],
        "a late read must be fenced through the transport cancel hook"
    );

    assert_eq!(
        clients
            .query
            .snapshot(
                metadata(),
                SnapshotRequest::default(),
                ClientRequestOptions::default()
            )
            .await
            .unwrap_err(),
        ClientError::Protocol("client_request_id_mismatch".to_owned())
    );
}

#[tokio::test]
async fn feed_reconnect_resumes_from_the_server_cursor_and_surfaces_gaps() {
    let transport = MockTransport::new(vec![
        handshake_route(),
        route(UI_FEED_SUBSCRIBE_OPERATION, |request| {
            Ok(accepted(request, Value::Null))
        }),
        route(UI_FEED_RESUME_OPERATION, |request| {
            Ok(accepted(request, Value::Null))
        }),
    ]);
    let feed = FeedClient::new(transport.clone());
    feed.initialize(metadata(), handshake_request())
        .await
        .unwrap();

    let seen = Arc::new(Mutex::new(Vec::new()));
    let run_id = RunId::new();
    let subscription = feed
        .subscribe(
            metadata(),
            run_id,
            None,
            "listener-1",
            {
                let seen = Arc::clone(&seen);
                move |frame: UiFeedFrameV1| seen.lock().expect("seen").push(frame.kind)
            },
            ClientRequestOptions::default(),
        )
        .await
        .unwrap();
    feed.dispatch(subscription.token(), boundary_frame())
        .expect("boundary");
    feed.dispatch(subscription.token(), delta_frame(1))
        .expect("delta");
    feed.dispatch(
        subscription.token(),
        gap_frame(2, UiFeedGapReason::OldEpoch),
    )
    .expect("a gap frame is delivered so the presenter can hydrate");
    assert_eq!(
        *seen.lock().expect("seen"),
        vec![
            UiFeedFrameKind::SnapshotBoundary,
            UiFeedFrameKind::Delta,
            UiFeedFrameKind::Gap,
        ]
    );

    let cursor = feed_cursor(2, 1);
    feed.resume(
        metadata(),
        run_id,
        subscription.token(),
        Some(cursor.clone()),
        ClientRequestOptions::default(),
    )
    .await
    .unwrap();
    let resumes: Vec<RecordedRequest> = transport
        .requests()
        .into_iter()
        .filter(|recorded| recorded.operation == UI_FEED_RESUME_OPERATION)
        .collect();
    assert_eq!(resumes.len(), 1);
    assert_eq!(
        resumes[0].arguments["after"],
        serde_json::to_value(&cursor).expect("cursor"),
        "resume carries the server cursor instead of a locally re-derived position"
    );
    assert_eq!(
        resumes[0].arguments["after"]["feed_sequence"],
        json!(2),
        "the resume position comes from the server frame, not from the local frame count"
    );

    subscription.cancel();
    assert_eq!(
        feed.resume(
            metadata(),
            run_id,
            subscription.token(),
            Some(cursor),
            ClientRequestOptions::default(),
        )
        .await
        .unwrap_err(),
        ClientError::ListenerInactive
    );
}

#[tokio::test]
async fn a_transport_failure_or_unknown_status_is_never_reported_as_success() {
    let transport = MockTransport::new(vec![
        handshake_route(),
        route(UI_QUERY_SNAPSHOT_OPERATION, |_| {
            Err(ClientError::Transport("offline".to_owned()))
        }),
        route(UI_QUERY_HISTORY_OPERATION, |request| {
            Ok(accepted(request, Value::Null))
        }),
        route(UI_ARTIFACT_PAGE_OPERATION, |request| {
            Ok(with_status(
                request,
                ExecutionStatus::Failed,
                Value::Null,
                Some("artifact_unavailable".to_owned()),
            ))
        }),
    ]);
    let clients = KianaClient::new(transport.clone()).typed_clients();
    clients
        .initialize(metadata(), handshake_request())
        .await
        .unwrap();

    assert_eq!(
        clients
            .query
            .snapshot(
                metadata(),
                SnapshotRequest::default(),
                ClientRequestOptions::default()
            )
            .await
            .unwrap_err(),
        ClientError::Transport("offline".to_owned())
    );
    let accepted_read = clients
        .query
        .history(
            metadata(),
            HistoryRequest {
                session_id: "session-1".to_owned(),
                after: None,
                limit: 16,
            },
            ClientRequestOptions::default(),
        )
        .await
        .unwrap_err();
    assert!(
        matches!(accepted_read, ClientError::Protocol(_)),
        "an Accepted envelope is not a read result: {accepted_read:?}"
    );
    let failed_page = clients
        .artifact
        .page(
            metadata(),
            ArtifactPageRequest {
                artifact_id: ArtifactId::new(),
                page_size: 8,
                after: None,
            },
            ClientRequestOptions::default(),
        )
        .await
        .unwrap_err();
    assert!(
        matches!(failed_page, ClientError::Protocol(_)),
        "a Failed envelope is not a page: {failed_page:?}"
    );
}

#[tokio::test]
async fn command_status_history_and_artifact_pages_share_the_same_fences() {
    let history_calls = Arc::new(AtomicUsize::new(0));
    let artifact_calls = Arc::new(AtomicUsize::new(0));
    let transport = MockTransport::new(vec![
        handshake_route(),
        route(UI_QUERY_COMMAND_STATUS_OPERATION, |request| {
            Ok(completed(request, command_status_value(request)))
        }),
        route(UI_QUERY_HISTORY_OPERATION, {
            let history_calls = Arc::clone(&history_calls);
            move |request| match history_calls.fetch_add(1, Ordering::SeqCst) {
                0 => Ok(completed(
                    request,
                    history_value("foreign-instance", "epoch-1", heartbeat_frame(1)),
                )),
                1 => {
                    let mut value = history_value("instance-1", "epoch-1", heartbeat_frame(1));
                    value["schema"] = json!("kiana.ui-history.v2");
                    Ok(completed(request, value))
                }
                _ => Ok(completed(
                    request,
                    history_value("instance-1", "epoch-1", heartbeat_frame(1)),
                )),
            }
        }),
        route(UI_ARTIFACT_PAGE_OPERATION, {
            let artifact_calls = Arc::clone(&artifact_calls);
            move |request| match artifact_calls.fetch_add(1, Ordering::SeqCst) {
                0 => Ok(completed(
                    request,
                    artifact_page_value("sha256:not-a-digest"),
                )),
                _ => Ok(completed(
                    request,
                    artifact_page_value(&format!("sha256:{}", "b".repeat(64))),
                )),
            }
        }),
    ]);
    let clients = KianaClient::new(transport.clone()).typed_clients();
    clients
        .initialize(metadata(), handshake_request())
        .await
        .unwrap();

    let status = clients
        .query
        .command_status(
            metadata(),
            CommandStatusRequest {
                command_id: RequestId::new(),
                idempotency_key: Some("fixture-action-1".to_owned()),
            },
            ClientRequestOptions::default(),
        )
        .await
        .unwrap();
    assert_eq!(status.schema, UI_COMMAND_STATUS_SCHEMA);
    assert_eq!(
        status.action.as_ref().map(|action| action.disposition),
        Some(UiActionDisposition::Accepted)
    );
    assert_eq!(status.retry, UiRetryDisposition::DoNotRetry);

    let history_request = HistoryRequest {
        session_id: "session-1".to_owned(),
        after: None,
        limit: 16,
    };
    assert_eq!(
        clients
            .query
            .history(
                metadata(),
                history_request.clone(),
                ClientRequestOptions::default()
            )
            .await
            .unwrap_err(),
        ClientError::Protocol("ui_history_instance_mismatch".to_owned())
    );
    assert_eq!(
        clients
            .query
            .history(
                metadata(),
                history_request.clone(),
                ClientRequestOptions::default()
            )
            .await
            .unwrap_err(),
        ClientError::UnknownSchema("kiana.ui-history.v2".to_owned())
    );
    let history = clients
        .query
        .history(metadata(), history_request, ClientRequestOptions::default())
        .await
        .unwrap();
    assert_eq!(history.schema, UI_HISTORY_SCHEMA);
    assert_eq!(history.instance_id, "instance-1");

    assert_eq!(
        clients
            .artifact
            .page(
                metadata(),
                ArtifactPageRequest {
                    artifact_id: ArtifactId::new(),
                    page_size: 8,
                    after: None,
                },
                ClientRequestOptions::default(),
            )
            .await
            .unwrap_err(),
        ClientError::Protocol("ui_artifact_page_invalid".to_owned())
    );
    let page = clients
        .artifact
        .page(
            metadata(),
            ArtifactPageRequest {
                artifact_id: ArtifactId::new(),
                page_size: 8,
                after: None,
            },
            ClientRequestOptions::default(),
        )
        .await
        .unwrap();
    assert_eq!(page.schema, UI_ARTIFACT_PAGE_SCHEMA);
    assert_eq!(
        transport.operations(),
        vec![
            UI_INITIALIZE_OPERATION.to_owned(),
            UI_QUERY_COMMAND_STATUS_OPERATION.to_owned(),
            UI_QUERY_HISTORY_OPERATION.to_owned(),
            UI_QUERY_HISTORY_OPERATION.to_owned(),
            UI_QUERY_HISTORY_OPERATION.to_owned(),
            UI_ARTIFACT_PAGE_OPERATION.to_owned(),
            UI_ARTIFACT_PAGE_OPERATION.to_owned(),
        ]
    );
}

#[tokio::test]
async fn cancel_and_continue_use_the_typed_action_operations() {
    let transport = MockTransport::new(vec![
        handshake_route(),
        route(UI_ACTION_CANCEL_OPERATION, |request| {
            Ok(completed(
                request,
                action_result(request, UiActionDisposition::Accepted),
            ))
        }),
        route(UI_ACTION_CONTINUE_OPERATION, |request| {
            Ok(completed(
                request,
                action_result(request, UiActionDisposition::Accepted),
            ))
        }),
    ]);
    let action_client = ActionClient::new(transport.clone());
    action_client
        .initialize(metadata(), handshake_request())
        .await
        .unwrap();

    let id = RequestId::new();
    let cancelled = action_client
        .cancel(
            metadata(),
            ActionRequest::new(action(id, "fixture-action-1")).unwrap(),
            ClientRequestOptions::default(),
        )
        .await
        .unwrap();
    assert_eq!(cancelled.disposition, UiActionDisposition::Accepted);
    let continued = action_client
        .continue_action(
            metadata(),
            ActionRequest::new(action(id, "fixture-action-1")).unwrap(),
            ClientRequestOptions::default(),
        )
        .await
        .unwrap();
    assert_eq!(continued.disposition, UiActionDisposition::Accepted);
    assert_eq!(
        action_client
            .continue_action(
                metadata(),
                ActionRequest::new(action(RequestId::new(), "fixture-action-1")).unwrap(),
                ClientRequestOptions::default(),
            )
            .await
            .unwrap_err(),
        ClientError::CommandRetryForbidden
    );
    assert_eq!(
        transport.operations(),
        vec![
            UI_INITIALIZE_OPERATION.to_owned(),
            UI_ACTION_CANCEL_OPERATION.to_owned(),
            UI_ACTION_CONTINUE_OPERATION.to_owned()
        ]
    );

    let cancel_request = transport
        .requests()
        .into_iter()
        .find(|recorded| recorded.operation == UI_ACTION_CANCEL_OPERATION)
        .expect("cancel request");
    assert_eq!(cancel_request.arguments["command_id"], json!(id));
    assert_eq!(
        cancel_request.arguments["idempotency_key"],
        json!("fixture-action-1")
    );
    assert!(
        cancel_request.arguments.get("payload").is_none(),
        "cancel carries command identity only, never a re-sent payload"
    );
}

#[tokio::test]
async fn typed_clients_share_one_negotiated_session_across_facades() {
    let transport = MockTransport::new(vec![
        handshake_route(),
        route(UI_QUERY_SNAPSHOT_OPERATION, |request| {
            Ok(completed(request, snapshot_value()))
        }),
        route(UI_ARTIFACT_PAGE_OPERATION, |request| {
            Ok(completed(
                request,
                artifact_page_value(&format!("sha256:{}", "c".repeat(64))),
            ))
        }),
    ]);
    let clients = KianaClient::new(transport.clone()).typed_clients();
    let handshake = clients
        .initialize(metadata(), handshake_request())
        .await
        .unwrap();
    assert_eq!(handshake.instance_id, "instance-1");

    let snapshot = clients
        .query
        .snapshot(
            metadata(),
            SnapshotRequest::default(),
            ClientRequestOptions::default(),
        )
        .await
        .unwrap();
    assert_eq!(snapshot.schema, UI_SNAPSHOT_SCHEMA);
    clients
        .artifact
        .page(
            metadata(),
            ArtifactPageRequest {
                artifact_id: ArtifactId::new(),
                page_size: 8,
                after: None,
            },
            ClientRequestOptions::default(),
        )
        .await
        .unwrap();
    assert_eq!(
        transport.operations(),
        vec![
            UI_INITIALIZE_OPERATION.to_owned(),
            UI_QUERY_SNAPSHOT_OPERATION.to_owned(),
            UI_ARTIFACT_PAGE_OPERATION.to_owned()
        ],
        "one handshake covers every facade"
    );
}

#[tokio::test]
async fn a_denied_handshake_never_installs_a_session() {
    let transport = MockTransport::new(vec![route(UI_INITIALIZE_OPERATION, |request| {
        Ok(with_status(
            request,
            ExecutionStatus::Denied,
            Value::Null,
            Some("untrusted_workspace".to_owned()),
        ))
    })]);
    let query = QueryClient::new(transport.clone());
    let denied = query
        .initialize(metadata(), handshake_request())
        .await
        .unwrap_err();
    assert!(
        matches!(denied, ClientError::Protocol(_)),
        "a denied handshake is not a session: {denied:?}"
    );
    assert_eq!(
        query
            .snapshot(
                metadata(),
                SnapshotRequest::default(),
                ClientRequestOptions::default()
            )
            .await
            .unwrap_err(),
        ClientError::NotInitialized
    );
    assert_eq!(
        transport.operations(),
        vec![UI_INITIALIZE_OPERATION.to_owned()]
    );
}

#[test]
fn ui07_deny_matrix_fixture_stays_wired_to_the_typed_client_source() {
    #[derive(Debug, Deserialize)]
    #[serde(deny_unknown_fields)]
    struct Budgets {
        max_page_size: usize,
        default_page_size: usize,
        default_history_limit: usize,
        max_history_limit: usize,
        max_limitations: usize,
    }

    #[derive(Debug, Deserialize)]
    #[serde(deny_unknown_fields)]
    struct DenyMatrix {
        schema: String,
        operations: Vec<String>,
        facades: Vec<String>,
        lifecycle: Vec<String>,
        budgets: Budgets,
        fixtures: Vec<String>,
        required: Vec<String>,
        denied: Vec<String>,
        limitations: Vec<String>,
    }

    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/ui07-typed-clients.json");
    let raw = std::fs::read_to_string(&path).expect("ui07 fixture file");
    let plan: DenyMatrix = serde_json::from_str(&raw).expect("ui07 fixture schema");
    assert_eq!(plan.schema, "kiana.ui07-typed-clients-fixture.v1");
    assert_eq!(
        plan.operations,
        vec![
            UI_INITIALIZE_OPERATION.to_owned(),
            UI_QUERY_SNAPSHOT_OPERATION.to_owned(),
            UI_QUERY_HISTORY_OPERATION.to_owned(),
            UI_QUERY_COMMAND_STATUS_OPERATION.to_owned(),
            UI_FEED_SUBSCRIBE_OPERATION.to_owned(),
            UI_FEED_RESUME_OPERATION.to_owned(),
            UI_ARTIFACT_PAGE_OPERATION.to_owned(),
            UI_ACTION_SUBMIT_OPERATION.to_owned(),
            UI_ACTION_CANCEL_OPERATION.to_owned(),
            UI_ACTION_CONTINUE_OPERATION.to_owned(),
            UI_ACTION_STATUS_OPERATION.to_owned(),
        ]
    );
    assert_eq!(
        plan.facades,
        vec![
            "QueryClient",
            "FeedClient",
            "ActionClient",
            "ArtifactClient"
        ]
    );
    assert_eq!(plan.budgets.max_page_size, 256);
    assert_eq!(plan.budgets.default_page_size, 64);
    assert_eq!(plan.budgets.default_history_limit, 128);
    assert_eq!(plan.budgets.max_history_limit, 256);
    assert_eq!(plan.budgets.max_limitations, 32);
    assert!(plan.lifecycle.contains(&"uninitialized".to_owned()));
    assert!(plan.lifecycle.contains(&"late".to_owned()));
    assert!(!plan.required.is_empty());
    assert!(!plan.denied.is_empty());
    assert!(!plan.limitations.is_empty());

    let source = include_str!("ui07_typed_clients.rs");
    for fixture in &plan.fixtures {
        assert!(
            source.contains(&format!("fn {fixture}(")),
            "fixture {fixture} is not defined in ui07_typed_clients.rs"
        );
    }
    for marker in plan.required.iter().chain(plan.denied.iter()) {
        assert!(!marker.trim().is_empty(), "marker must not be blank");
    }
}

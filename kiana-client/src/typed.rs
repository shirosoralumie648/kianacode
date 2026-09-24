//! Typed UI client facades.
//!
//! The four facades in this module are deliberately thin protocol adapters.  They keep the
//! negotiated session, request fences and listener registry in one shared state object, then
//! delegate every request to [`ClientTransport`].  They do not own a daemon, a broker or a model
//! loop; an accepted response remains an accepted response until a typed receipt says otherwise.

use super::{ClientError, ClientSessionState, ClientTransport};
use kiana_protocol::{
    stable_error_from_response, ArtifactId, ExecutionStatus, RequestEnvelope, RequestId,
    RequestMetadata, ResponseEnvelope, UiActionResult, UiActionV1, UiCursorV1, UiError,
    UiFeedCursorV1, UiFeedFrameV1, UiHandshakeRequest, UiHandshakeResponse, UiRetryDisposition,
    UiSnapshotV1, PROTOCOL_SCHEMA, UI_ACTION_RESULT_SCHEMA, UI_FEED_FRAME_SCHEMA,
    UI_HANDSHAKE_RESPONSE_SCHEMA, UI_SNAPSHOT_SCHEMA,
};
use serde::{de::DeserializeOwned, Deserialize, Serialize};
use serde_json::{json, Value};
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc, Mutex,
};
use std::time::{SystemTime, UNIX_EPOCH};

pub const UI_INITIALIZE_OPERATION: &str = "ui.initialize";
pub const UI_QUERY_SNAPSHOT_OPERATION: &str = "ui.query.snapshot";
pub const UI_QUERY_HISTORY_OPERATION: &str = "ui.query.history";
pub const UI_QUERY_COMMAND_STATUS_OPERATION: &str = "ui.query.command_status";
pub const UI_FEED_SUBSCRIBE_OPERATION: &str = "ui.feed.subscribe";
pub const UI_FEED_RESUME_OPERATION: &str = "ui.feed.resume";
pub const UI_ARTIFACT_PAGE_OPERATION: &str = "ui.artifact.page";
pub const UI_ACTION_SUBMIT_OPERATION: &str = "ui.action.submit";
pub const UI_ACTION_CANCEL_OPERATION: &str = "ui.action.cancel";
pub const UI_ACTION_CONTINUE_OPERATION: &str = "ui.action.continue";
pub const UI_ACTION_STATUS_OPERATION: &str = "ui.action.status";

pub const UI_HISTORY_SCHEMA: &str = "kiana.ui-history.v1";
pub const UI_COMMAND_STATUS_SCHEMA: &str = "kiana.ui-command-status.v1";
pub const UI_ARTIFACT_PAGE_SCHEMA: &str = "kiana.ui-artifact-page.v1";

/// A cancellation fence shared by all typed requests derived from one controller.
#[derive(Clone, Debug, Default)]
pub struct CancellationToken(Arc<AtomicBool>);

impl CancellationToken {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn cancel(&self) {
        self.0.store(true, Ordering::Release);
    }

    pub fn is_cancelled(&self) -> bool {
        self.0.load(Ordering::Acquire)
    }
}

/// Request identity and lifecycle fences applied consistently by every typed facade.
#[derive(Clone, Debug)]
pub struct ClientRequestOptions {
    pub request_id: RequestId,
    pub deadline_unix_ms: Option<u64>,
    pub cancellation: CancellationToken,
}

impl Default for ClientRequestOptions {
    fn default() -> Self {
        Self {
            request_id: RequestId::new(),
            deadline_unix_ms: None,
            cancellation: CancellationToken::new(),
        }
    }
}

impl ClientRequestOptions {
    pub fn with_deadline_unix_ms(mut self, deadline_unix_ms: u64) -> Self {
        self.deadline_unix_ms = Some(deadline_unix_ms);
        self
    }

    pub fn with_request_id(mut self, request_id: RequestId) -> Self {
        self.request_id = request_id;
        self
    }

    pub fn with_cancellation(mut self, cancellation: CancellationToken) -> Self {
        self.cancellation = cancellation;
        self
    }
}

/// Negotiated identity used to fence all subsequent typed requests.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ClientSession {
    pub workspace: String,
    pub session_id: kiana_protocol::SessionId,
    pub instance_id: String,
    pub authority_epoch: u64,
}

/// A listener callback is kept behind an `Arc` so dispatch can release the registry lock before
/// invoking user code.  A callback never receives a request envelope or a capability handle.
pub(crate) struct ListenerRecord {
    pub(crate) generation: u64,
    pub(crate) callback: Arc<dyn Fn(UiFeedFrameV1) + Send + Sync>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SnapshotRequest {
    #[serde(default)]
    pub session_id: Option<String>,
    #[serde(default = "default_page_size")]
    pub page_size: usize,
    #[serde(default)]
    pub after: Option<String>,
}

fn default_page_size() -> usize {
    64
}

impl Default for SnapshotRequest {
    fn default() -> Self {
        Self {
            session_id: None,
            page_size: default_page_size(),
            after: None,
        }
    }
}

impl SnapshotRequest {
    fn validate(&self) -> Result<(), ClientError> {
        if self.page_size == 0 || self.page_size > 256 {
            return Err(ClientError::Protocol(
                "ui_snapshot_page_size_invalid".to_owned(),
            ));
        }
        if self
            .session_id
            .as_deref()
            .is_some_and(|value| value.trim().is_empty() || value.len() > 256)
        {
            return Err(ClientError::Protocol(
                "ui_snapshot_session_invalid".to_owned(),
            ));
        }
        if self.after.as_deref().is_some_and(str::is_empty) {
            return Err(ClientError::Protocol(
                "ui_snapshot_after_invalid".to_owned(),
            ));
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HistoryRequest {
    pub session_id: String,
    #[serde(default)]
    pub after: Option<String>,
    #[serde(default = "default_history_limit")]
    pub limit: usize,
}

fn default_history_limit() -> usize {
    128
}

impl HistoryRequest {
    fn validate(&self) -> Result<(), ClientError> {
        if self.session_id.trim().is_empty() || self.session_id.len() > 256 {
            return Err(ClientError::Protocol(
                "ui_history_session_invalid".to_owned(),
            ));
        }
        if self.limit == 0 || self.limit > 256 {
            return Err(ClientError::Protocol("ui_history_limit_invalid".to_owned()));
        }
        if self.after.as_deref().is_some_and(str::is_empty) {
            return Err(ClientError::Protocol("ui_history_after_invalid".to_owned()));
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CommandStatusRequest {
    pub command_id: RequestId,
    #[serde(default)]
    pub idempotency_key: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct UiHistoryV1 {
    pub schema: String,
    pub instance_id: String,
    pub snapshot_cursor: UiCursorV1,
    pub frames: Vec<UiFeedFrameV1>,
    #[serde(default)]
    pub next_page: Option<String>,
    #[serde(default)]
    pub limitations: Vec<String>,
}

impl UiHistoryV1 {
    pub fn validate(&self) -> Result<(), String> {
        if self.schema != UI_HISTORY_SCHEMA
            || self.instance_id.trim().is_empty()
            || self.instance_id.len() > 256
            || self.frames.len() > 256
            || self.limitations.len() > 32
        {
            return Err("ui_history_header_invalid".to_owned());
        }
        self.snapshot_cursor.validate()?;
        for frame in &self.frames {
            frame.validate()?;
        }
        if self
            .limitations
            .iter()
            .any(|value| value.trim().is_empty() || value.len() > 512)
        {
            return Err("ui_history_limitation_invalid".to_owned());
        }
        Ok(())
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct UiCommandStatusV1 {
    pub schema: String,
    pub command_id: RequestId,
    pub status: ExecutionStatus,
    #[serde(default)]
    pub action: Option<UiActionResult>,
    #[serde(default)]
    pub error: Option<UiError>,
    pub retry: UiRetryDisposition,
}

impl UiCommandStatusV1 {
    pub fn validate(&self) -> Result<(), String> {
        if self.schema != UI_COMMAND_STATUS_SCHEMA || self.command_id.as_uuid().is_nil() {
            return Err("ui_command_status_header_invalid".to_owned());
        }
        if let Some(action) = &self.action {
            action.validate()?;
        }
        if let Some(error) = &self.error {
            error.validate()?;
        }
        Ok(())
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct UiArtifactPageV1 {
    pub schema: String,
    pub artifact_id: ArtifactId,
    pub digest: String,
    pub mime: String,
    pub offset: u64,
    pub content: Value,
    #[serde(default)]
    pub next_page: Option<String>,
    #[serde(default)]
    pub limitations: Vec<String>,
}

impl UiArtifactPageV1 {
    pub fn validate(&self) -> Result<(), String> {
        let valid_digest = self
            .digest
            .strip_prefix("sha256:")
            .is_some_and(|hex| hex.len() == 64 && hex.bytes().all(|byte| byte.is_ascii_hexdigit()));
        if self.schema != UI_ARTIFACT_PAGE_SCHEMA
            || !valid_digest
            || self.mime.trim().is_empty()
            || self.mime.len() > 128
            || self.limitations.len() > 32
            || self
                .limitations
                .iter()
                .any(|value| value.trim().is_empty() || value.len() > 512)
        {
            return Err("ui_artifact_page_invalid".to_owned());
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct FeedListenerToken {
    pub listener_id: String,
    pub generation: u64,
}

/// RAII listener handle. Dropping or cancelling it prevents all future callback delivery for the
/// old generation, including responses that were already in flight when the handle was released.
pub struct FeedSubscription<T> {
    client: FeedClient<T>,
    token: FeedListenerToken,
    cancelled: CancellationToken,
}

impl<T> FeedSubscription<T> {
    pub fn token(&self) -> &FeedListenerToken {
        &self.token
    }

    pub fn cancel(&self) {
        self.cancelled.cancel();
        self.client.unregister(&self.token);
    }
}

impl<T> Drop for FeedSubscription<T> {
    fn drop(&mut self) {
        self.cancelled.cancel();
        self.client.unregister(&self.token);
    }
}

#[derive(Clone)]
pub struct QueryClient<T> {
    pub(crate) transport: Arc<T>,
    pub(crate) state: Arc<Mutex<ClientSessionState>>,
}

#[derive(Clone)]
pub struct FeedClient<T> {
    pub(crate) transport: Arc<T>,
    pub(crate) state: Arc<Mutex<ClientSessionState>>,
}

#[derive(Clone)]
pub struct ActionClient<T> {
    pub(crate) transport: Arc<T>,
    pub(crate) state: Arc<Mutex<ClientSessionState>>,
}

#[derive(Clone)]
pub struct ArtifactClient<T> {
    pub(crate) transport: Arc<T>,
    pub(crate) state: Arc<Mutex<ClientSessionState>>,
}

pub struct TypedClients<T> {
    pub query: QueryClient<T>,
    pub feed: FeedClient<T>,
    pub action: ActionClient<T>,
    pub artifact: ArtifactClient<T>,
}

impl<T> TypedClients<T>
where
    T: ClientTransport,
{
    pub(crate) fn from_shared(transport: Arc<T>, state: Arc<Mutex<ClientSessionState>>) -> Self {
        Self {
            query: QueryClient {
                transport: Arc::clone(&transport),
                state: Arc::clone(&state),
            },
            feed: FeedClient {
                transport: Arc::clone(&transport),
                state: Arc::clone(&state),
            },
            action: ActionClient {
                transport: Arc::clone(&transport),
                state: Arc::clone(&state),
            },
            artifact: ArtifactClient { transport, state },
        }
    }

    pub async fn initialize(
        &self,
        metadata: RequestMetadata,
        handshake: UiHandshakeRequest,
    ) -> Result<UiHandshakeResponse, ClientError> {
        initialize_shared(
            &self.query.transport,
            &self.query.state,
            metadata,
            handshake,
        )
        .await
    }
}

impl<T> QueryClient<T>
where
    T: ClientTransport,
{
    pub fn new(transport: T) -> Self {
        super::KianaClient::new(transport).query_client()
    }

    pub(crate) fn from_shared(transport: Arc<T>, state: Arc<Mutex<ClientSessionState>>) -> Self {
        Self { transport, state }
    }

    pub async fn initialize(
        &self,
        metadata: RequestMetadata,
        handshake: UiHandshakeRequest,
    ) -> Result<UiHandshakeResponse, ClientError> {
        initialize_shared(&self.transport, &self.state, metadata, handshake).await
    }

    pub async fn snapshot(
        &self,
        metadata: RequestMetadata,
        request: SnapshotRequest,
        options: ClientRequestOptions,
    ) -> Result<UiSnapshotV1, ClientError> {
        request.validate()?;
        let response = dispatch(
            &self.transport,
            &self.state,
            metadata,
            UI_QUERY_SNAPSHOT_OPERATION,
            serde_json::to_value(request)
                .map_err(|error| ClientError::Protocol(error.to_string()))?,
            &options,
            true,
        )
        .await?;
        let response = decode_success(response, UI_QUERY_SNAPSHOT_OPERATION)?;
        let output = response.output;
        let value = output
            .get("snapshot")
            .cloned()
            .unwrap_or_else(|| output.clone());
        let snapshot: UiSnapshotV1 = serde_json::from_value(value)
            .map_err(|error| ClientError::Protocol(format!("ui_snapshot_response:{error}")))?;
        if snapshot.schema != UI_SNAPSHOT_SCHEMA {
            return Err(ClientError::UnknownSchema(snapshot.schema));
        }
        snapshot.validate().map_err(ClientError::Protocol)?;
        Ok(snapshot)
    }

    pub async fn history(
        &self,
        metadata: RequestMetadata,
        request: HistoryRequest,
        options: ClientRequestOptions,
    ) -> Result<UiHistoryV1, ClientError> {
        request.validate()?;
        let response = dispatch(
            &self.transport,
            &self.state,
            metadata,
            UI_QUERY_HISTORY_OPERATION,
            serde_json::to_value(request)
                .map_err(|error| ClientError::Protocol(error.to_string()))?,
            &options,
            true,
        )
        .await?;
        let response = decode_success(response, UI_QUERY_HISTORY_OPERATION)?;
        decode_typed(
            response.output,
            "history",
            UI_HISTORY_SCHEMA,
            |history: UiHistoryV1| {
                history.validate().map_err(ClientError::Protocol)?;
                Ok(history)
            },
        )
    }

    pub async fn command_status(
        &self,
        metadata: RequestMetadata,
        request: CommandStatusRequest,
        options: ClientRequestOptions,
    ) -> Result<UiCommandStatusV1, ClientError> {
        let response = dispatch(
            &self.transport,
            &self.state,
            metadata,
            UI_QUERY_COMMAND_STATUS_OPERATION,
            serde_json::to_value(request)
                .map_err(|error| ClientError::Protocol(error.to_string()))?,
            &options,
            true,
        )
        .await?;
        let response = decode_success(response, UI_QUERY_COMMAND_STATUS_OPERATION)?;
        decode_typed(
            response.output,
            "status",
            UI_COMMAND_STATUS_SCHEMA,
            |status: UiCommandStatusV1| {
                status.validate().map_err(ClientError::Protocol)?;
                Ok(status)
            },
        )
    }
}

impl<T> FeedClient<T>
where
    T: ClientTransport,
{
    pub fn new(transport: T) -> Self {
        super::KianaClient::new(transport).feed_client()
    }

    pub(crate) fn from_shared(transport: Arc<T>, state: Arc<Mutex<ClientSessionState>>) -> Self {
        Self { transport, state }
    }

    pub async fn initialize(
        &self,
        metadata: RequestMetadata,
        handshake: UiHandshakeRequest,
    ) -> Result<UiHandshakeResponse, ClientError> {
        initialize_shared(&self.transport, &self.state, metadata, handshake).await
    }

    /// Install the listener before sending the resume request.  Any failed request removes the
    /// token again, so an old controller cannot receive a late frame after reconnect.
    pub async fn subscribe<F>(
        &self,
        metadata: RequestMetadata,
        run_id: kiana_protocol::RunId,
        after: Option<UiFeedCursorV1>,
        listener_id: impl Into<String> + Send,
        listener: F,
        options: ClientRequestOptions,
    ) -> Result<FeedSubscription<T>, ClientError>
    where
        F: Fn(UiFeedFrameV1) + Send + Sync + 'static,
    {
        let listener_id = listener_id.into();
        if listener_id.trim().is_empty() || listener_id.len() > 256 {
            return Err(ClientError::Protocol("ui_listener_id_invalid".to_owned()));
        }
        let token = self.register(listener_id.clone(), listener)?;
        let arguments = json!({"run_id": run_id, "after": after});
        let response = dispatch(
            &self.transport,
            &self.state,
            metadata,
            UI_FEED_SUBSCRIBE_OPERATION,
            arguments,
            &options,
            true,
        )
        .await;
        let response = match response.and_then(|response| accept_feed_response(response)) {
            Ok(response) => response,
            Err(error) => {
                self.unregister(&token);
                return Err(error);
            }
        };
        let _ = response;
        Ok(FeedSubscription {
            client: FeedClient {
                transport: Arc::clone(&self.transport),
                state: Arc::clone(&self.state),
            },
            token,
            cancelled: options.cancellation,
        })
    }

    pub async fn resume(
        &self,
        metadata: RequestMetadata,
        run_id: kiana_protocol::RunId,
        token: &FeedListenerToken,
        after: Option<UiFeedCursorV1>,
        options: ClientRequestOptions,
    ) -> Result<(), ClientError> {
        self.assert_active(token)?;
        let response = dispatch(
            &self.transport,
            &self.state,
            metadata,
            UI_FEED_RESUME_OPERATION,
            json!({"run_id": run_id, "after": after, "listener_id": token.listener_id, "generation": token.generation}),
            &options,
            true,
        )
        .await?;
        accept_feed_response(response).map(|_| ())
    }

    pub fn dispatch(
        &self,
        token: &FeedListenerToken,
        frame: UiFeedFrameV1,
    ) -> Result<(), ClientError> {
        if frame.schema != UI_FEED_FRAME_SCHEMA {
            return Err(ClientError::UnknownSchema(frame.schema));
        }
        frame.validate().map_err(ClientError::Protocol)?;
        let callback = {
            let state = self
                .state
                .lock()
                .map_err(|_| ClientError::Protocol("client_state_poisoned".to_owned()))?;
            let Some(record) = state.listeners.get(&token.listener_id) else {
                return Err(ClientError::ListenerInactive);
            };
            if record.generation != token.generation {
                return Err(ClientError::ListenerInactive);
            }
            Arc::clone(&record.callback)
        };
        callback(frame);
        Ok(())
    }

    fn register<F>(
        &self,
        listener_id: String,
        listener: F,
    ) -> Result<FeedListenerToken, ClientError>
    where
        F: Fn(UiFeedFrameV1) + Send + Sync + 'static,
    {
        let mut state = self
            .state
            .lock()
            .map_err(|_| ClientError::Protocol("client_state_poisoned".to_owned()))?;
        if state.listeners.contains_key(&listener_id) {
            return Err(ClientError::DuplicateListener(listener_id));
        }
        state.next_listener_generation = state.next_listener_generation.saturating_add(1);
        let generation = state.next_listener_generation;
        state.listeners.insert(
            listener_id.clone(),
            ListenerRecord {
                generation,
                callback: Arc::new(listener),
            },
        );
        Ok(FeedListenerToken {
            listener_id,
            generation,
        })
    }

    fn unregister(&self, token: &FeedListenerToken) {
        if let Ok(mut state) = self.state.lock() {
            let remove = state
                .listeners
                .get(&token.listener_id)
                .is_some_and(|record| record.generation == token.generation);
            if remove {
                state.listeners.remove(&token.listener_id);
            }
        }
    }

    fn assert_active(&self, token: &FeedListenerToken) -> Result<(), ClientError> {
        let state = self
            .state
            .lock()
            .map_err(|_| ClientError::Protocol("client_state_poisoned".to_owned()))?;
        state
            .listeners
            .get(&token.listener_id)
            .filter(|record| record.generation == token.generation)
            .map(|_| ())
            .ok_or(ClientError::ListenerInactive)
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ActionRequest {
    pub action: UiActionV1,
}

impl ActionRequest {
    pub fn new(action: UiActionV1) -> Result<Self, ClientError> {
        action.validate().map_err(ClientError::Protocol)?;
        Ok(Self { action })
    }
}

impl From<UiActionV1> for ActionRequest {
    fn from(action: UiActionV1) -> Self {
        Self { action }
    }
}

impl<T> ActionClient<T>
where
    T: ClientTransport,
{
    pub fn new(transport: T) -> Self {
        super::KianaClient::new(transport).action_client()
    }

    pub(crate) fn from_shared(transport: Arc<T>, state: Arc<Mutex<ClientSessionState>>) -> Self {
        Self { transport, state }
    }

    pub async fn initialize(
        &self,
        metadata: RequestMetadata,
        handshake: UiHandshakeRequest,
    ) -> Result<UiHandshakeResponse, ClientError> {
        initialize_shared(&self.transport, &self.state, metadata, handshake).await
    }

    pub async fn submit<R: Into<ActionRequest> + Send>(
        &self,
        metadata: RequestMetadata,
        request: R,
        options: ClientRequestOptions,
    ) -> Result<UiActionResult, ClientError> {
        let request = request.into();
        request.action.validate().map_err(ClientError::Protocol)?;
        self.assert_command_identity(&request.action)?;
        let response = dispatch(
            &self.transport,
            &self.state,
            metadata,
            UI_ACTION_SUBMIT_OPERATION,
            serde_json::to_value(&request.action)
                .map_err(|error| ClientError::Protocol(error.to_string()))?,
            &options,
            true,
        )
        .await?;
        self.decode_action(response)
    }

    pub async fn cancel<R: Into<ActionRequest> + Send>(
        &self,
        metadata: RequestMetadata,
        request: R,
        options: ClientRequestOptions,
    ) -> Result<UiActionResult, ClientError> {
        let request = request.into();
        request.action.validate().map_err(ClientError::Protocol)?;
        self.assert_command_identity(&request.action)?;
        let response = dispatch(
            &self.transport,
            &self.state,
            metadata,
            UI_ACTION_CANCEL_OPERATION,
            json!({"command_id": request.action.command_id, "idempotency_key": request.action.idempotency_key}),
            &options,
            true,
        )
        .await?;
        self.decode_action(response)
    }

    pub async fn continue_action<R: Into<ActionRequest> + Send>(
        &self,
        metadata: RequestMetadata,
        request: R,
        options: ClientRequestOptions,
    ) -> Result<UiActionResult, ClientError> {
        let request = request.into();
        request.action.validate().map_err(ClientError::Protocol)?;
        self.assert_command_identity(&request.action)?;
        let response = dispatch(
            &self.transport,
            &self.state,
            metadata,
            UI_ACTION_CONTINUE_OPERATION,
            serde_json::to_value(&request.action)
                .map_err(|error| ClientError::Protocol(error.to_string()))?,
            &options,
            true,
        )
        .await?;
        self.decode_action(response)
    }

    pub async fn continue_<R: Into<ActionRequest> + Send>(
        &self,
        metadata: RequestMetadata,
        request: R,
        options: ClientRequestOptions,
    ) -> Result<UiActionResult, ClientError> {
        self.continue_action(metadata, request, options).await
    }

    pub async fn r#continue<R: Into<ActionRequest> + Send>(
        &self,
        metadata: RequestMetadata,
        request: R,
        options: ClientRequestOptions,
    ) -> Result<UiActionResult, ClientError> {
        self.continue_action(metadata, request, options).await
    }

    /// Reconcile an Unknown command by its original idempotency key.  This is the only supported
    /// path after a late/unknown submit; a new command ID is never synthesized as a retry.
    pub async fn query_original(
        &self,
        metadata: RequestMetadata,
        command_id: RequestId,
        idempotency_key: impl Into<String> + Send,
        options: ClientRequestOptions,
    ) -> Result<UiActionResult, ClientError> {
        let idempotency_key = idempotency_key.into();
        if idempotency_key.trim().is_empty() || idempotency_key.len() > 256 {
            return Err(ClientError::Protocol(
                "ui_action_idempotency_key_invalid".to_owned(),
            ));
        }
        let response = dispatch(
            &self.transport,
            &self.state,
            metadata,
            UI_ACTION_STATUS_OPERATION,
            json!({"command_id": command_id, "idempotency_key": idempotency_key}),
            &options,
            true,
        )
        .await?;
        self.decode_action(response)
    }

    fn assert_command_identity(&self, action: &UiActionV1) -> Result<(), ClientError> {
        let mut state = self
            .state
            .lock()
            .map_err(|_| ClientError::Protocol("client_state_poisoned".to_owned()))?;
        if let Some(original) = state.action_commands.get(&action.idempotency_key) {
            if original != &action.command_id {
                return Err(ClientError::CommandRetryForbidden);
            }
        } else {
            state
                .action_commands
                .insert(action.idempotency_key.clone(), action.command_id);
        }
        Ok(())
    }

    fn decode_action(&self, response: ResponseEnvelope) -> Result<UiActionResult, ClientError> {
        let status = response.status;
        let unknown_message = (status == ExecutionStatus::ResultUnknown)
            .then(|| stable_error_from_response(&response).map(|error| error.message));
        let output = response.output;
        let result = output
            .get("result")
            .cloned()
            .unwrap_or_else(|| output.clone());
        let result: UiActionResult = match serde_json::from_value(result) {
            Ok(result) => result,
            Err(error) if status == ExecutionStatus::ResultUnknown => {
                return Err(ClientError::Protocol(
                    unknown_message
                        .flatten()
                        .unwrap_or_else(|| format!("result_unknown:{error}")),
                ));
            }
            Err(error) => {
                return Err(ClientError::Protocol(format!(
                    "ui_action_result_response:{error}"
                )))
            }
        };
        if result.schema != UI_ACTION_RESULT_SCHEMA {
            return Err(ClientError::UnknownSchema(result.schema));
        }
        result.validate().map_err(ClientError::Protocol)?;
        // Accepted is intentionally returned as Accepted.  It is never promoted to Applied
        // merely because the transport returned a successful envelope.
        Ok(result)
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ArtifactPageRequest {
    pub artifact_id: ArtifactId,
    #[serde(default = "default_page_size")]
    pub page_size: usize,
    #[serde(default)]
    pub after: Option<String>,
}

impl ArtifactPageRequest {
    fn validate(&self) -> Result<(), ClientError> {
        if self.page_size == 0 || self.page_size > 256 {
            return Err(ClientError::Protocol(
                "ui_artifact_page_size_invalid".to_owned(),
            ));
        }
        if self.after.as_deref().is_some_and(str::is_empty) {
            return Err(ClientError::Protocol(
                "ui_artifact_after_invalid".to_owned(),
            ));
        }
        Ok(())
    }
}

impl<T> ArtifactClient<T>
where
    T: ClientTransport,
{
    pub fn new(transport: T) -> Self {
        super::KianaClient::new(transport).artifact_client()
    }

    pub(crate) fn from_shared(transport: Arc<T>, state: Arc<Mutex<ClientSessionState>>) -> Self {
        Self { transport, state }
    }

    pub async fn initialize(
        &self,
        metadata: RequestMetadata,
        handshake: UiHandshakeRequest,
    ) -> Result<UiHandshakeResponse, ClientError> {
        initialize_shared(&self.transport, &self.state, metadata, handshake).await
    }

    pub async fn page(
        &self,
        metadata: RequestMetadata,
        request: ArtifactPageRequest,
        options: ClientRequestOptions,
    ) -> Result<UiArtifactPageV1, ClientError> {
        request.validate()?;
        let response = dispatch(
            &self.transport,
            &self.state,
            metadata,
            UI_ARTIFACT_PAGE_OPERATION,
            serde_json::to_value(request)
                .map_err(|error| ClientError::Protocol(error.to_string()))?,
            &options,
            true,
        )
        .await?;
        let response = decode_success(response, UI_ARTIFACT_PAGE_OPERATION)?;
        decode_typed(
            response.output,
            "page",
            UI_ARTIFACT_PAGE_SCHEMA,
            |page: UiArtifactPageV1| {
                page.validate().map_err(ClientError::Protocol)?;
                Ok(page)
            },
        )
    }
}

async fn initialize_shared<T>(
    transport: &Arc<T>,
    state: &Arc<Mutex<ClientSessionState>>,
    metadata: RequestMetadata,
    handshake: UiHandshakeRequest,
) -> Result<UiHandshakeResponse, ClientError>
where
    T: ClientTransport,
{
    handshake.validate().map_err(ClientError::Protocol)?;
    let project_root = metadata.project_root.clone();
    let session_id = metadata.session_id.clone();
    let options = ClientRequestOptions::default().with_request_id(metadata.request_id);
    let response = dispatch(
        transport,
        state,
        metadata,
        UI_INITIALIZE_OPERATION,
        serde_json::to_value(handshake)
            .map_err(|error| ClientError::Protocol(format!("ui_handshake_encode:{error}")))?,
        &options,
        false,
    )
    .await?;
    let response = decode_success(response, UI_INITIALIZE_OPERATION)?;
    let handshake: UiHandshakeResponse = serde_json::from_value(response.output)
        .map_err(|error| ClientError::Protocol(format!("ui_handshake_response:{error}")))?;
    if handshake.schema != UI_HANDSHAKE_RESPONSE_SCHEMA {
        return Err(ClientError::UnknownSchema(handshake.schema));
    }
    handshake.validate().map_err(ClientError::Protocol)?;
    let mut state = state
        .lock()
        .map_err(|_| ClientError::Protocol("client_state_poisoned".to_owned()))?;
    state.session = Some(ClientSession {
        workspace: project_root,
        session_id,
        instance_id: handshake.instance_id.clone(),
        authority_epoch: handshake.authority_epoch,
    });
    Ok(handshake)
}

async fn dispatch<T>(
    transport: &Arc<T>,
    state: &Arc<Mutex<ClientSessionState>>,
    mut metadata: RequestMetadata,
    operation: &str,
    mut arguments: Value,
    options: &ClientRequestOptions,
    require_initialized: bool,
) -> Result<ResponseEnvelope, ClientError>
where
    T: ClientTransport,
{
    if options.cancellation.is_cancelled() {
        return Err(ClientError::Cancelled);
    }
    if options
        .deadline_unix_ms
        .is_some_and(|deadline| deadline <= now_unix_ms())
    {
        return Err(ClientError::DeadlineExceeded);
    }
    let session = {
        let state = state
            .lock()
            .map_err(|_| ClientError::Protocol("client_state_poisoned".to_owned()))?;
        if require_initialized && state.session.is_none() {
            return Err(ClientError::NotInitialized);
        }
        if state
            .session
            .as_ref()
            .is_some_and(|session| session.workspace != metadata.project_root)
        {
            return Err(ClientError::WorkspaceMismatch);
        }
        state.session.clone()
    };
    metadata.request_id = options.request_id;
    metadata.deadline_unix_ms = options.deadline_unix_ms;
    if let Some(deadline) = options.deadline_unix_ms {
        if let Some(object) = arguments.as_object_mut() {
            object.insert("deadline_unix_ms".to_owned(), json!(deadline));
        }
    }
    if let Some(session) = session {
        if metadata.session_id != session.session_id {
            return Err(ClientError::WorkspaceMismatch);
        }
    }
    let response = transport
        .send(RequestEnvelope::command(metadata, operation, arguments))
        .await?;
    if response.schema != PROTOCOL_SCHEMA {
        return Err(ClientError::UnknownSchema(response.schema));
    }
    if response.request_id != options.request_id {
        return Err(ClientError::Protocol(
            "client_request_id_mismatch".to_owned(),
        ));
    }
    if options.cancellation.is_cancelled() {
        let _ = transport.cancel(options.request_id).await;
        return Err(ClientError::LateResponse(options.request_id));
    }
    if options
        .deadline_unix_ms
        .is_some_and(|deadline| deadline <= now_unix_ms())
    {
        let _ = transport.cancel(options.request_id).await;
        return Err(ClientError::LateResponse(options.request_id));
    }
    Ok(response)
}

fn decode_success(
    response: ResponseEnvelope,
    operation: &str,
) -> Result<ResponseEnvelope, ClientError> {
    if response.status != ExecutionStatus::Completed {
        return Err(ClientError::Protocol(
            stable_error_from_response(&response)
                .map(|error| error.message)
                .unwrap_or_else(|| format!("{operation}_rejected")),
        ));
    }
    if response.error.is_some() {
        return Err(ClientError::Protocol(
            response
                .error
                .unwrap_or_else(|| format!("{operation}_failed")),
        ));
    }
    Ok(response)
}

fn accept_feed_response(response: ResponseEnvelope) -> Result<ResponseEnvelope, ClientError> {
    if matches!(
        response.status,
        ExecutionStatus::Accepted
            | ExecutionStatus::Queued
            | ExecutionStatus::Running
            | ExecutionStatus::Completed
    ) && response.error.is_none()
    {
        Ok(response)
    } else {
        Err(ClientError::Protocol(
            stable_error_from_response(&response)
                .map(|error| error.message)
                .unwrap_or_else(|| "ui_feed_subscribe_rejected".to_owned()),
        ))
    }
}

fn decode_typed<T, F>(
    output: Value,
    key: &str,
    expected_schema: &str,
    validate: F,
) -> Result<T, ClientError>
where
    T: DeserializeOwned,
    F: FnOnce(T) -> Result<T, ClientError>,
{
    let value = output.get(key).cloned().unwrap_or_else(|| output.clone());
    if let Some(schema) = value.get("schema").and_then(Value::as_str) {
        if schema != expected_schema {
            return Err(ClientError::UnknownSchema(schema.to_owned()));
        }
    }
    let typed: T = serde_json::from_value(value)
        .map_err(|error| ClientError::Protocol(format!("typed_response:{error}")))?;
    validate(typed)
}

fn now_unix_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis().min(u64::MAX as u128) as u64)
        .unwrap_or(0)
}

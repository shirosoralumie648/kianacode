use crate::sessions_websocket::{
    SessionsWebSocket, SessionsWebSocketCallbacks, WebSocketError, DEFAULT_API_BASE_URL,
};
use crate::types::*;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;
use thiserror::Error;
use tokio::sync::Mutex;
use uuid::Uuid;

const CCR_BYOC_BETA: &str = "ccr-byoc-2025-07-29";
const SEND_EVENT_TIMEOUT: Duration = Duration::from_secs(30);
const SESSION_API_TIMEOUT: Duration = Duration::from_secs(15);

pub struct RemoteSessionConfig {
    pub session_id: String,
    pub get_access_token: Arc<dyn Fn() -> String + Send + Sync>,
    pub org_uuid: String,
    pub has_initial_prompt: bool,
    pub viewer_only: bool,
    pub api_base_url: String,
}

#[async_trait::async_trait]
pub trait RemoteSessionCallbacks: Send + Sync {
    async fn on_message(&self, message: SDKMessage);
    async fn on_permission_request(&self, request: SDKControlPermissionRequest, request_id: String);
    async fn on_permission_cancelled(&self, _request_id: String, _tool_use_id: Option<String>) {}
    async fn refresh_after_unauthorized(&self, _stale_access_token: String) -> bool {
        false
    }
    async fn on_connected(&self) {}
    async fn on_disconnected(&self) {}
    async fn on_reconnecting(&self) {}
    async fn on_error(&self, _error: WebSocketError) {}
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(untagged)]
pub enum RemoteMessageContent {
    Text(String),
    Blocks(Vec<Value>),
}

impl From<String> for RemoteMessageContent {
    fn from(value: String) -> Self {
        Self::Text(value)
    }
}

impl From<&str> for RemoteMessageContent {
    fn from(value: &str) -> Self {
        Self::Text(value.to_string())
    }
}

#[derive(Debug, Clone, Default)]
pub struct SendRemoteMessageOptions {
    pub uuid: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ListSessionsResponse {
    pub data: Vec<SessionResource>,
    #[serde(default)]
    pub has_more: bool,
    #[serde(default)]
    pub first_id: Option<String>,
    #[serde(default)]
    pub last_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct SessionResource {
    #[serde(default, rename = "type")]
    pub resource_type: Option<String>,
    pub id: String,
    #[serde(default)]
    pub title: Option<String>,
    #[serde(default)]
    pub session_status: String,
    #[serde(default)]
    pub environment_id: Option<String>,
    #[serde(default)]
    pub created_at: Option<String>,
    #[serde(default)]
    pub updated_at: Option<String>,
    #[serde(default)]
    pub session_context: SessionContext,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct SessionContext {
    #[serde(default)]
    pub sources: Vec<SessionContextSource>,
    #[serde(default)]
    pub cwd: Option<String>,
    #[serde(default)]
    pub outcomes: Option<Vec<Value>>,
    #[serde(default)]
    pub custom_system_prompt: Option<String>,
    #[serde(default)]
    pub append_system_prompt: Option<String>,
    #[serde(default)]
    pub model: Option<String>,
    #[serde(default)]
    pub seed_bundle_file_id: Option<String>,
    #[serde(default)]
    pub github_pr: Option<Value>,
    #[serde(default)]
    pub reuse_outcome_branches: Option<bool>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "type")]
pub enum SessionContextSource {
    #[serde(rename = "git_repository")]
    GitRepository {
        url: String,
        #[serde(default)]
        revision: Option<String>,
        #[serde(default)]
        allow_unrestricted_git_push: Option<bool>,
    },
    #[serde(rename = "knowledge_base")]
    KnowledgeBase { knowledge_base_id: String },
    #[serde(other)]
    Unknown,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct CodeSession {
    pub id: String,
    pub title: String,
    pub description: String,
    pub status: String,
    pub repo: Option<CodeSessionRepo>,
    pub turns: Vec<String>,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct CodeSessionRepo {
    pub name: String,
    pub owner: CodeSessionRepoOwner,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub default_branch: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct CodeSessionRepoOwner {
    pub login: String,
}

#[derive(Debug, Clone, Default)]
pub struct CreateRemoteSessionOptions {
    pub title: Option<String>,
    pub environment_id: String,
    pub initial_message: Option<RemoteMessageContent>,
    pub permission_mode: Option<String>,
    pub permission_request_id: Option<String>,
    pub event_uuid: Option<String>,
    pub session_context: SessionContext,
    pub source: Option<String>,
}

#[derive(Debug, Error)]
pub enum RemoteSessionApiError {
    #[error("{0}")]
    Message(String),
    #[error("remote session request failed: {0}")]
    Request(#[from] reqwest::Error),
}

struct WSCallbackAdapter {
    manager: Arc<Mutex<RemoteSessionManagerInner>>,
    callbacks: Arc<dyn RemoteSessionCallbacks>,
}

#[async_trait::async_trait]
impl SessionsWebSocketCallbacks for WSCallbackAdapter {
    async fn on_message(&self, message: SessionMessage) {
        let mut manager = self.manager.lock().await;
        manager.handle_message(message, &self.callbacks).await;
    }

    async fn on_close(&self) {
        self.callbacks.on_disconnected().await;
    }

    async fn refresh_after_unauthorized(&self, stale_access_token: String) -> bool {
        self.callbacks
            .refresh_after_unauthorized(stale_access_token)
            .await
    }

    async fn on_error(&self, error: WebSocketError) {
        self.callbacks.on_error(error).await;
    }

    async fn on_connected(&self) {
        self.callbacks.on_connected().await;
    }

    async fn on_reconnecting(&self) {
        self.callbacks.on_reconnecting().await;
    }
}

struct RemoteSessionManagerInner {
    pending_permission_requests: HashMap<String, SDKControlPermissionRequest>,
}

impl RemoteSessionManagerInner {
    fn new() -> Self {
        Self {
            pending_permission_requests: HashMap::new(),
        }
    }

    async fn handle_message(
        &mut self,
        message: SessionMessage,
        callbacks: &Arc<dyn RemoteSessionCallbacks>,
    ) {
        match message {
            SessionMessage::Control(ControlMessage::Request(req)) => {
                self.handle_control_request(req, callbacks).await;
            }
            SessionMessage::Control(ControlMessage::CancelRequest(cancel)) => {
                let pending = self.pending_permission_requests.remove(&cancel.request_id);
                callbacks
                    .on_permission_cancelled(cancel.request_id, pending.map(|p| p.tool_use_id))
                    .await;
            }
            SessionMessage::Control(ControlMessage::Response(_)) => {}
            SessionMessage::SDK(sdk_msg) => {
                callbacks.on_message(sdk_msg).await;
            }
        }
    }

    async fn handle_control_request(
        &mut self,
        request: SDKControlRequest,
        callbacks: &Arc<dyn RemoteSessionCallbacks>,
    ) {
        match request.request {
            SDKControlRequestInner::CanUseTool(perm_req) => {
                self.pending_permission_requests
                    .insert(request.request_id.clone(), perm_req.clone());
                callbacks
                    .on_permission_request(perm_req, request.request_id)
                    .await;
            }
            SDKControlRequestInner::Initialize
            | SDKControlRequestInner::SetModel { .. }
            | SDKControlRequestInner::SetMaxThinkingTokens { .. }
            | SDKControlRequestInner::SetPermissionMode { .. } => {}
            SDKControlRequestInner::Interrupt => {}
            SDKControlRequestInner::Unknown { subtype, .. } => {
                callbacks
                    .on_error(WebSocketError::Parse(format!(
                        "Unsupported control request subtype: {subtype}"
                    )))
                    .await;
            }
        }
    }
}

pub struct RemoteSessionManager {
    config: RemoteSessionConfig,
    websocket: Arc<Mutex<Option<Arc<SessionsWebSocket>>>>,
    inner: Arc<Mutex<RemoteSessionManagerInner>>,
    callbacks: Arc<dyn RemoteSessionCallbacks>,
}

impl RemoteSessionManager {
    pub fn new(config: RemoteSessionConfig, callbacks: Arc<dyn RemoteSessionCallbacks>) -> Self {
        Self {
            config,
            websocket: Arc::new(Mutex::new(None)),
            inner: Arc::new(Mutex::new(RemoteSessionManagerInner::new())),
            callbacks,
        }
    }

    pub async fn connect(&self) -> Result<(), WebSocketError> {
        let ws_callbacks = Arc::new(WSCallbackAdapter {
            manager: self.inner.clone(),
            callbacks: self.callbacks.clone(),
        });

        let ws = Arc::new(SessionsWebSocket::with_api_base_url(
            self.config.session_id.clone(),
            self.config.org_uuid.clone(),
            self.config.get_access_token.clone(),
            ws_callbacks,
            self.config.api_base_url.clone(),
        ));

        ws.connect().await?;
        *self.websocket.lock().await = Some(ws);
        Ok(())
    }

    pub async fn respond_to_permission_request(
        &self,
        request_id: String,
        result: RemotePermissionResponse,
    ) -> Result<(), WebSocketError> {
        let mut inner = self.inner.lock().await;
        let pending_request = inner.pending_permission_requests.remove(&request_id);

        if pending_request.is_none() {
            return Ok(());
        }

        let response = SDKControlResponse::permission_success(
            request_id,
            match result {
                RemotePermissionResponse::Allow { updated_input } => {
                    PermissionResponse::Allow { updated_input }
                }
                RemotePermissionResponse::Deny { message } => PermissionResponse::Deny { message },
            },
        );

        let ws_guard = self.websocket.lock().await;
        if let Some(ws) = ws_guard.as_ref() {
            ws.send_control_response(response).await?;
        }

        Ok(())
    }

    pub async fn send_message(
        &self,
        content: impl Into<RemoteMessageContent>,
        options: SendRemoteMessageOptions,
    ) -> bool {
        send_event_to_remote_session(
            &self.config.api_base_url,
            &self.config.session_id,
            &self.config.org_uuid,
            &(self.config.get_access_token)(),
            content.into(),
            options,
        )
        .await
    }

    pub async fn cancel_permission_request(
        &self,
        request_id: String,
    ) -> Result<(), WebSocketError> {
        let mut inner = self.inner.lock().await;
        let pending_request = inner.pending_permission_requests.remove(&request_id);

        if pending_request.is_none() {
            return Ok(());
        }
        drop(inner);

        let ws_guard = self.websocket.lock().await;
        if let Some(ws) = ws_guard.as_ref() {
            ws.send_control_cancel_request(request_id).await?;
        }

        Ok(())
    }

    pub async fn is_connected(&self) -> bool {
        let ws_guard = self.websocket.lock().await;
        if let Some(ws) = ws_guard.as_ref() {
            ws.is_connected().await
        } else {
            false
        }
    }

    pub async fn cancel_session(&self) -> Result<(), WebSocketError> {
        let ws_guard = self.websocket.lock().await;
        if let Some(ws) = ws_guard.as_ref() {
            ws.send_control_request(SDKControlRequestInner::Interrupt)
                .await?;
        }
        Ok(())
    }

    pub fn get_session_id(&self) -> &str {
        &self.config.session_id
    }

    pub async fn disconnect(&self) {
        let mut ws_guard = self.websocket.lock().await;
        if let Some(ws) = ws_guard.as_ref() {
            ws.close().await;
        }
        *ws_guard = None;
        self.inner.lock().await.pending_permission_requests.clear();
    }

    pub async fn reconnect(&self) {
        let ws_guard = self.websocket.lock().await;
        if let Some(ws) = ws_guard.as_ref() {
            ws.reconnect().await;
        }
    }
}

pub fn create_remote_session_config(
    session_id: String,
    get_access_token: Arc<dyn Fn() -> String + Send + Sync>,
    org_uuid: String,
    has_initial_prompt: bool,
    viewer_only: bool,
) -> RemoteSessionConfig {
    create_remote_session_config_with_api_base_url(
        session_id,
        get_access_token,
        org_uuid,
        has_initial_prompt,
        viewer_only,
        DEFAULT_API_BASE_URL.to_string(),
    )
}

pub fn create_remote_session_config_with_api_base_url(
    session_id: String,
    get_access_token: Arc<dyn Fn() -> String + Send + Sync>,
    org_uuid: String,
    has_initial_prompt: bool,
    viewer_only: bool,
    api_base_url: String,
) -> RemoteSessionConfig {
    RemoteSessionConfig {
        session_id,
        get_access_token,
        org_uuid,
        has_initial_prompt,
        viewer_only,
        api_base_url,
    }
}

pub async fn fetch_code_sessions_from_sessions_api(
    api_base_url: &str,
    org_uuid: &str,
    access_token: &str,
) -> Result<Vec<CodeSession>, RemoteSessionApiError> {
    let response = fetch_sessions(api_base_url, org_uuid, access_token).await?;
    Ok(response
        .data
        .iter()
        .map(code_session_from_session_resource)
        .collect())
}

pub async fn fetch_sessions(
    api_base_url: &str,
    org_uuid: &str,
    access_token: &str,
) -> Result<ListSessionsResponse, RemoteSessionApiError> {
    let base_url = normalized_api_base_url(api_base_url)?;
    let url = format!("{base_url}/v1/sessions");
    let response = add_session_api_headers(reqwest::Client::new().get(url), access_token, org_uuid)
        .timeout(SESSION_API_TIMEOUT)
        .send()
        .await?;

    if response.status() != reqwest::StatusCode::OK {
        return Err(RemoteSessionApiError::Message(format!(
            "Failed to fetch code sessions: {}",
            response.status()
        )));
    }

    Ok(response.json::<ListSessionsResponse>().await?)
}

pub async fn create_remote_session(
    api_base_url: &str,
    org_uuid: &str,
    access_token: &str,
    options: CreateRemoteSessionOptions,
) -> Result<SessionResource, RemoteSessionApiError> {
    let base_url = normalized_api_base_url(api_base_url)?;
    if options.environment_id.trim().is_empty() {
        return Err(RemoteSessionApiError::Message(
            "create_remote_session requires an environment ID".to_string(),
        ));
    }

    let url = format!("{base_url}/v1/sessions");
    let body = create_remote_session_body(options);
    let response =
        add_session_api_headers(reqwest::Client::new().post(url), access_token, org_uuid)
            .timeout(SESSION_API_TIMEOUT)
            .json(&body)
            .send()
            .await?;

    if response.status() != reqwest::StatusCode::OK
        && response.status() != reqwest::StatusCode::CREATED
    {
        return Err(session_create_error(response).await);
    }

    Ok(response.json::<SessionResource>().await?)
}

pub async fn fetch_session(
    api_base_url: &str,
    session_id: &str,
    org_uuid: &str,
    access_token: &str,
) -> Result<SessionResource, RemoteSessionApiError> {
    let base_url = normalized_api_base_url(api_base_url)?;
    if session_id.trim().is_empty() {
        return Err(RemoteSessionApiError::Message(
            "fetch_session requires a session ID".to_string(),
        ));
    }
    let url = format!("{base_url}/v1/sessions/{session_id}");
    let response = add_session_api_headers(reqwest::Client::new().get(url), access_token, org_uuid)
        .timeout(SESSION_API_TIMEOUT)
        .send()
        .await?;

    if response.status() != reqwest::StatusCode::OK {
        return Err(session_fetch_error(response, session_id).await);
    }

    Ok(response.json::<SessionResource>().await?)
}

pub async fn archive_remote_session(
    api_base_url: &str,
    session_id: &str,
    org_uuid: &str,
    access_token: &str,
) -> bool {
    let Ok(base_url) = normalized_api_base_url(api_base_url) else {
        return false;
    };
    if session_id.trim().is_empty() {
        return false;
    }
    let url = format!("{base_url}/v1/sessions/{session_id}/archive");
    let response =
        add_session_api_headers(reqwest::Client::new().post(url), access_token, org_uuid)
            .timeout(SESSION_API_TIMEOUT)
            .json(&json!({}))
            .send()
            .await;

    match response {
        Ok(response) => {
            response.status() == reqwest::StatusCode::OK
                || response.status() == reqwest::StatusCode::CREATED
                || response.status() == reqwest::StatusCode::CONFLICT
        }
        Err(_) => false,
    }
}

pub async fn update_session_title(
    api_base_url: &str,
    session_id: &str,
    org_uuid: &str,
    access_token: &str,
    title: &str,
) -> bool {
    let Ok(base_url) = normalized_api_base_url(api_base_url) else {
        return false;
    };
    if session_id.trim().is_empty() {
        return false;
    }
    let url = format!("{base_url}/v1/sessions/{session_id}");
    let response =
        add_session_api_headers(reqwest::Client::new().patch(url), access_token, org_uuid)
            .timeout(SESSION_API_TIMEOUT)
            .json(&json!({ "title": title }))
            .send()
            .await;

    match response {
        Ok(response) => response.status() == reqwest::StatusCode::OK,
        Err(_) => false,
    }
}

pub fn get_branch_from_session(session: &SessionResource) -> Option<String> {
    session
        .session_context
        .outcomes
        .as_ref()?
        .iter()
        .find(|outcome| outcome.get("type").and_then(Value::as_str) == Some("git_repository"))
        .and_then(|outcome| outcome.get("git_info"))
        .and_then(|git_info| git_info.get("branches"))
        .and_then(Value::as_array)
        .and_then(|branches| branches.first())
        .and_then(Value::as_str)
        .map(str::to_string)
}

fn create_remote_session_body(options: CreateRemoteSessionOptions) -> Value {
    let mut events = Vec::new();

    if let Some(mode) = options
        .permission_mode
        .as_deref()
        .map(str::trim)
        .filter(|mode| !mode.is_empty())
    {
        let request_id = options
            .permission_request_id
            .filter(|value| !value.trim().is_empty())
            .unwrap_or_else(|| format!("set-mode-{}", Uuid::new_v4()));
        events.push(wrap_create_session_event(json!({
            "type": "control_request",
            "request_id": request_id,
            "request": {
                "subtype": "set_permission_mode",
                "mode": mode,
                "ultraplan": false,
            },
        })));
    }

    if let Some(content) = options.initial_message {
        let event_uuid = options
            .event_uuid
            .filter(|value| !value.trim().is_empty())
            .unwrap_or_else(|| Uuid::new_v4().to_string());
        events.push(wrap_create_session_event(json!({
            "uuid": event_uuid,
            "session_id": "",
            "type": "user",
            "parent_tool_use_id": null,
            "message": {
                "role": "user",
                "content": content,
            },
        })));
    }

    let mut body = json!({
        "events": events,
        "session_context": options.session_context,
        "environment_id": options.environment_id,
    });

    if let Some(title) = options.title.filter(|title| !title.trim().is_empty()) {
        body["title"] = Value::String(title);
    }
    if let Some(source) = options.source.filter(|source| !source.trim().is_empty()) {
        body["source"] = Value::String(source);
    }

    body
}

fn wrap_create_session_event(data: Value) -> Value {
    json!({
        "type": "event",
        "data": data,
    })
}

fn code_session_from_session_resource(session: &SessionResource) -> CodeSession {
    let repo = session.session_context.sources.iter().find_map(|source| {
        let SessionContextSource::GitRepository { url, revision, .. } = source else {
            return None;
        };
        let (owner, name) = parse_github_repository(url)?;
        Some(CodeSessionRepo {
            name,
            owner: CodeSessionRepoOwner { login: owner },
            default_branch: revision.clone(),
        })
    });

    CodeSession {
        id: session.id.clone(),
        title: session
            .title
            .clone()
            .filter(|title| !title.trim().is_empty())
            .unwrap_or_else(|| "Untitled".to_string()),
        description: String::new(),
        status: session.session_status.clone(),
        repo,
        turns: Vec::new(),
        created_at: session.created_at.clone().unwrap_or_default(),
        updated_at: session.updated_at.clone().unwrap_or_default(),
    }
}

fn parse_github_repository(url: &str) -> Option<(String, String)> {
    let path = if let Some(path) = url.strip_prefix("git@github.com:") {
        path
    } else {
        let marker = "github.com/";
        let index = url.find(marker)?;
        &url[index + marker.len()..]
    };
    let path = path
        .trim()
        .trim_matches('/')
        .trim_end_matches(".git")
        .trim_matches('/');
    let mut parts = path.split('/');
    let owner = parts.next()?.trim();
    let name = parts.next()?.trim();
    if owner.is_empty() || name.is_empty() {
        None
    } else {
        Some((owner.to_string(), name.to_string()))
    }
}

fn normalized_api_base_url(api_base_url: &str) -> Result<String, RemoteSessionApiError> {
    let base_url = api_base_url.trim().trim_end_matches('/').to_string();
    if base_url.is_empty() {
        Err(RemoteSessionApiError::Message(
            "remote session API base URL is empty".to_string(),
        ))
    } else {
        Ok(base_url)
    }
}

fn add_session_api_headers(
    builder: reqwest::RequestBuilder,
    access_token: &str,
    org_uuid: &str,
) -> reqwest::RequestBuilder {
    builder
        .bearer_auth(access_token)
        .header("content-type", "application/json")
        .header("anthropic-version", "2023-06-01")
        .header("anthropic-beta", CCR_BYOC_BETA)
        .header("x-organization-uuid", org_uuid)
}

async fn session_fetch_error(
    response: reqwest::Response,
    session_id: &str,
) -> RemoteSessionApiError {
    let status = response.status();
    if status == reqwest::StatusCode::NOT_FOUND {
        return RemoteSessionApiError::Message(format!("Session not found: {session_id}"));
    }
    if status == reqwest::StatusCode::UNAUTHORIZED {
        return RemoteSessionApiError::Message(
            "Session expired. Please run /login to sign in again.".to_string(),
        );
    }

    let api_message = response
        .json::<Value>()
        .await
        .ok()
        .and_then(|value| {
            value
                .get("error")
                .and_then(|error| error.get("message"))
                .and_then(Value::as_str)
                .map(str::to_string)
        })
        .filter(|message| !message.trim().is_empty());
    RemoteSessionApiError::Message(
        api_message.unwrap_or_else(|| format!("Failed to fetch session: {status}")),
    )
}

async fn session_create_error(response: reqwest::Response) -> RemoteSessionApiError {
    let status = response.status();
    let api_message = response
        .json::<Value>()
        .await
        .ok()
        .and_then(|value| {
            value
                .get("error")
                .and_then(|error| error.get("message"))
                .and_then(Value::as_str)
                .map(str::to_string)
        })
        .filter(|message| !message.trim().is_empty());
    RemoteSessionApiError::Message(
        api_message.unwrap_or_else(|| format!("Failed to create remote session: {status}")),
    )
}

pub async fn send_event_to_remote_session(
    api_base_url: &str,
    session_id: &str,
    org_uuid: &str,
    access_token: &str,
    content: RemoteMessageContent,
    options: SendRemoteMessageOptions,
) -> bool {
    let Ok(base_url) = normalized_api_base_url(api_base_url) else {
        return false;
    };
    if session_id.trim().is_empty() {
        return false;
    }
    let url = format!("{base_url}/v1/sessions/{session_id}/events");
    let event_uuid = options.uuid.unwrap_or_else(|| Uuid::new_v4().to_string());
    let body = json!({
        "events": [{
            "uuid": event_uuid,
            "session_id": session_id,
            "type": "user",
            "parent_tool_use_id": null,
            "message": {
                "role": "user",
                "content": content,
            },
        }]
    });

    let response =
        add_session_api_headers(reqwest::Client::new().post(url), access_token, org_uuid)
            .timeout(SEND_EVENT_TIMEOUT)
            .json(&body)
            .send()
            .await;

    match response {
        Ok(response) => {
            response.status() == reqwest::StatusCode::OK
                || response.status() == reqwest::StatusCode::CREATED
        }
        Err(_) => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex as StdMutex;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::{TcpListener, TcpStream};

    #[derive(Debug, Clone)]
    struct RecordedRequest {
        method: String,
        path: String,
        authorization: Option<String>,
        anthropic_version: Option<String>,
        anthropic_beta: Option<String>,
        organization_uuid: Option<String>,
        content_type: Option<String>,
        body: String,
    }

    #[tokio::test]
    async fn send_message_posts_reference_event_shape_and_headers() {
        let (base_url, request) = spawn_mock_event_server(201).await;
        let manager = RemoteSessionManager::new(
            create_remote_session_config_with_api_base_url(
                "session-1".to_string(),
                Arc::new(|| "access-token".to_string()),
                "org-1".to_string(),
                false,
                false,
                base_url,
            ),
            Arc::new(NoopCallbacks),
        );

        let sent = manager
            .send_message(
                "hello remote",
                SendRemoteMessageOptions {
                    uuid: Some("event-1".to_string()),
                },
            )
            .await;

        assert!(sent);
        let request = request.lock().unwrap().clone().expect("request captured");
        assert_eq!(request.method, "POST");
        assert_eq!(request.path, "/v1/sessions/session-1/events");
        assert_eq!(
            request.authorization.as_deref(),
            Some("Bearer access-token")
        );
        assert_eq!(request.anthropic_version.as_deref(), Some("2023-06-01"));
        assert_eq!(request.anthropic_beta.as_deref(), Some(CCR_BYOC_BETA));
        assert_eq!(request.organization_uuid.as_deref(), Some("org-1"));
        assert!(request
            .content_type
            .as_deref()
            .unwrap_or_default()
            .starts_with("application/json"));
        let body: Value = serde_json::from_str(&request.body).unwrap();
        let event = &body["events"][0];
        assert_eq!(event["uuid"], "event-1");
        assert_eq!(event["session_id"], "session-1");
        assert_eq!(event["type"], "user");
        assert_eq!(event["parent_tool_use_id"], Value::Null);
        assert_eq!(event["message"]["role"], "user");
        assert_eq!(event["message"]["content"], "hello remote");
    }

    #[tokio::test]
    async fn send_event_to_remote_session_returns_false_for_non_success_status() {
        let (base_url, _request) = spawn_mock_event_server(409).await;

        let sent = send_event_to_remote_session(
            &base_url,
            "session-1",
            "org-1",
            "access-token",
            RemoteMessageContent::Blocks(vec![json!({
                "type": "text",
                "text": "hello"
            })]),
            SendRemoteMessageOptions::default(),
        )
        .await;

        assert!(!sent);
    }

    #[tokio::test]
    async fn fetch_code_sessions_maps_reference_list_response_and_headers() {
        let body = json!({
            "data": [{
                "type": "session",
                "id": "session-1",
                "title": "Fix issue",
                "session_status": "running",
                "environment_id": "env-1",
                "created_at": "2026-06-15T00:00:00Z",
                "updated_at": "2026-06-15T00:01:00Z",
                "session_context": {
                    "sources": [{
                        "type": "git_repository",
                        "url": "https://github.com/acme/widgets.git",
                        "revision": "main"
                    }],
                    "cwd": "/repo",
                    "outcomes": null,
                    "custom_system_prompt": null,
                    "append_system_prompt": null,
                    "model": null
                }
            }],
            "has_more": false,
            "first_id": "session-1",
            "last_id": "session-1"
        })
        .to_string();
        let (base_url, request) = spawn_mock_session_api_server(200, body).await;

        let sessions = fetch_code_sessions_from_sessions_api(&base_url, "org-1", "access-token")
            .await
            .unwrap();

        assert_eq!(sessions.len(), 1);
        assert_eq!(sessions[0].id, "session-1");
        assert_eq!(sessions[0].title, "Fix issue");
        assert_eq!(sessions[0].status, "running");
        let repo = sessions[0].repo.as_ref().unwrap();
        assert_eq!(repo.owner.login, "acme");
        assert_eq!(repo.name, "widgets");
        assert_eq!(repo.default_branch.as_deref(), Some("main"));

        let request = request.lock().unwrap().clone().expect("request captured");
        assert_eq!(request.method, "GET");
        assert_eq!(request.path, "/v1/sessions");
        assert_eq!(
            request.authorization.as_deref(),
            Some("Bearer access-token")
        );
        assert_eq!(request.anthropic_version.as_deref(), Some("2023-06-01"));
        assert_eq!(request.anthropic_beta.as_deref(), Some(CCR_BYOC_BETA));
        assert_eq!(request.organization_uuid.as_deref(), Some("org-1"));
    }

    #[tokio::test]
    async fn create_remote_session_posts_reference_shape_and_headers() {
        let (base_url, request) =
            spawn_mock_session_api_server(201, json!({ "id": "session-1" }).to_string()).await;

        let session = create_remote_session(
            &base_url,
            "org-1",
            "access-token",
            CreateRemoteSessionOptions {
                title: Some("Remote task".to_string()),
                environment_id: "env-1".to_string(),
                initial_message: Some(RemoteMessageContent::Text("hello remote".to_string())),
                permission_mode: Some("acceptEdits".to_string()),
                permission_request_id: Some("set-mode-1".to_string()),
                event_uuid: Some("event-1".to_string()),
                session_context: SessionContext {
                    sources: vec![SessionContextSource::GitRepository {
                        url: "https://github.com/acme/widgets.git".to_string(),
                        revision: Some("main".to_string()),
                        allow_unrestricted_git_push: None,
                    }],
                    outcomes: Some(vec![json!({
                        "type": "git_repository",
                        "git_info": {
                            "type": "github",
                            "repo": "acme/widgets",
                            "branches": ["claude/task"]
                        }
                    })]),
                    model: Some("claude-sonnet-4-20250514".to_string()),
                    ..Default::default()
                },
                source: Some("remote-control".to_string()),
            },
        )
        .await
        .unwrap();

        assert_eq!(session.id, "session-1");

        let request = request.lock().unwrap().clone().expect("request captured");
        assert_eq!(request.method, "POST");
        assert_eq!(request.path, "/v1/sessions");
        assert_eq!(
            request.authorization.as_deref(),
            Some("Bearer access-token")
        );
        assert_eq!(request.anthropic_version.as_deref(), Some("2023-06-01"));
        assert_eq!(request.anthropic_beta.as_deref(), Some(CCR_BYOC_BETA));
        assert_eq!(request.organization_uuid.as_deref(), Some("org-1"));
        assert!(request
            .content_type
            .as_deref()
            .unwrap_or_default()
            .starts_with("application/json"));

        let body: Value = serde_json::from_str(&request.body).unwrap();
        assert_eq!(body["title"], "Remote task");
        assert_eq!(body["environment_id"], "env-1");
        assert_eq!(body["source"], "remote-control");
        assert_eq!(
            body["session_context"]["sources"][0]["url"],
            "https://github.com/acme/widgets.git"
        );
        assert_eq!(body["session_context"]["sources"][0]["revision"], "main");
        assert_eq!(
            body["session_context"]["outcomes"][0]["git_info"]["branches"][0],
            "claude/task"
        );
        assert_eq!(body["session_context"]["model"], "claude-sonnet-4-20250514");
        assert_eq!(body["events"][0]["type"], "event");
        assert_eq!(body["events"][0]["data"]["type"], "control_request");
        assert_eq!(body["events"][0]["data"]["request_id"], "set-mode-1");
        assert_eq!(
            body["events"][0]["data"]["request"]["subtype"],
            "set_permission_mode"
        );
        assert_eq!(body["events"][0]["data"]["request"]["mode"], "acceptEdits");
        assert_eq!(body["events"][0]["data"]["request"]["ultraplan"], false);
        assert_eq!(body["events"][1]["type"], "event");
        assert_eq!(body["events"][1]["data"]["uuid"], "event-1");
        assert_eq!(body["events"][1]["data"]["session_id"], "");
        assert_eq!(body["events"][1]["data"]["type"], "user");
        assert_eq!(body["events"][1]["data"]["parent_tool_use_id"], Value::Null);
        assert_eq!(body["events"][1]["data"]["message"]["role"], "user");
        assert_eq!(
            body["events"][1]["data"]["message"]["content"],
            "hello remote"
        );
    }

    #[tokio::test]
    async fn fetch_session_returns_resource_and_branch_from_outcomes() {
        let body = json!({
            "type": "session",
            "id": "session-1",
            "title": null,
            "session_status": "idle",
            "environment_id": "env-1",
            "created_at": "2026-06-15T00:00:00Z",
            "updated_at": "2026-06-15T00:01:00Z",
            "session_context": {
                "sources": [{
                    "type": "git_repository",
                    "url": "git@github.com:acme/widgets.git"
                }],
                "cwd": "/repo",
                "outcomes": [{
                    "type": "git_repository",
                    "git_info": {
                        "type": "github",
                        "repo": "acme/widgets",
                        "branches": ["feature/remote-session"]
                    }
                }],
                "custom_system_prompt": null,
                "append_system_prompt": null,
                "model": null
            }
        })
        .to_string();
        let (base_url, request) = spawn_mock_session_api_server(200, body).await;

        let session = fetch_session(&base_url, "session-1", "org-1", "access-token")
            .await
            .unwrap();

        assert_eq!(session.id, "session-1");
        assert_eq!(session.title, None);
        assert_eq!(session.session_status, "idle");
        assert_eq!(
            get_branch_from_session(&session).as_deref(),
            Some("feature/remote-session")
        );

        let request = request.lock().unwrap().clone().expect("request captured");
        assert_eq!(request.method, "GET");
        assert_eq!(request.path, "/v1/sessions/session-1");
    }

    #[tokio::test]
    async fn fetch_session_reports_reference_not_found_error() {
        let (base_url, _request) = spawn_mock_session_api_server(404, "{}".to_string()).await;

        let error = fetch_session(&base_url, "missing-session", "org-1", "access-token")
            .await
            .unwrap_err()
            .to_string();

        assert_eq!(error, "Session not found: missing-session");
    }

    #[tokio::test]
    async fn archive_remote_session_posts_reference_endpoint_and_accepts_conflict() {
        let (base_url, request) = spawn_mock_session_api_server(409, "{}".to_string()).await;

        let archived =
            archive_remote_session(&base_url, "session-1", "org-1", "access-token").await;

        assert!(archived);
        let request = request.lock().unwrap().clone().expect("request captured");
        assert_eq!(request.method, "POST");
        assert_eq!(request.path, "/v1/sessions/session-1/archive");
        assert_eq!(
            request.authorization.as_deref(),
            Some("Bearer access-token")
        );
        assert_eq!(request.anthropic_version.as_deref(), Some("2023-06-01"));
        assert_eq!(request.anthropic_beta.as_deref(), Some(CCR_BYOC_BETA));
        assert_eq!(request.organization_uuid.as_deref(), Some("org-1"));
        let body: Value = serde_json::from_str(&request.body).unwrap();
        assert_eq!(body, json!({}));
    }

    #[tokio::test]
    async fn update_session_title_patches_reference_shape_and_headers() {
        let (base_url, request) = spawn_mock_session_api_server(200, "{}".to_string()).await;

        let updated =
            update_session_title(&base_url, "session-1", "org-1", "access-token", "New title")
                .await;

        assert!(updated);
        let request = request.lock().unwrap().clone().expect("request captured");
        assert_eq!(request.method, "PATCH");
        assert_eq!(request.path, "/v1/sessions/session-1");
        assert_eq!(
            request.authorization.as_deref(),
            Some("Bearer access-token")
        );
        assert_eq!(request.anthropic_version.as_deref(), Some("2023-06-01"));
        assert_eq!(request.anthropic_beta.as_deref(), Some(CCR_BYOC_BETA));
        assert_eq!(request.organization_uuid.as_deref(), Some("org-1"));
        let body: Value = serde_json::from_str(&request.body).unwrap();
        assert_eq!(body["title"], "New title");
    }

    struct NoopCallbacks;

    #[async_trait::async_trait]
    impl RemoteSessionCallbacks for NoopCallbacks {
        async fn on_message(&self, _message: SDKMessage) {}

        async fn on_permission_request(
            &self,
            _request: SDKControlPermissionRequest,
            _request_id: String,
        ) {
        }
    }

    async fn spawn_mock_event_server(
        status: u16,
    ) -> (String, Arc<StdMutex<Option<RecordedRequest>>>) {
        spawn_mock_session_api_server(status, "{}".to_string()).await
    }

    async fn spawn_mock_session_api_server(
        status: u16,
        response_body: String,
    ) -> (String, Arc<StdMutex<Option<RecordedRequest>>>) {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let request = Arc::new(StdMutex::new(None));
        let shared_request = request.clone();

        tokio::spawn(async move {
            let Ok((mut stream, _)) = listener.accept().await else {
                return;
            };
            if let Ok(recorded) = read_http_request(&mut stream).await {
                *shared_request.lock().unwrap() = Some(recorded);
            }
            let _ = write_http_response(&mut stream, status, &response_body).await;
        });

        (format!("http://{}", address), request)
    }

    async fn read_http_request(stream: &mut TcpStream) -> std::io::Result<RecordedRequest> {
        let mut buffer = Vec::new();
        let mut chunk = [0_u8; 1024];
        let header_end = loop {
            let read = stream.read(&mut chunk).await?;
            if read == 0 {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::UnexpectedEof,
                    "connection closed before headers",
                ));
            }
            buffer.extend_from_slice(&chunk[..read]);
            if let Some(index) = find_header_end(&buffer) {
                break index;
            }
        };

        let header_text = String::from_utf8_lossy(&buffer[..header_end]);
        let mut lines = header_text.lines();
        let request_line = lines.next().unwrap_or_default();
        let mut request_parts = request_line.split_whitespace();
        let method = request_parts.next().unwrap_or_default().to_string();
        let path = request_parts.next().unwrap_or_default().to_string();
        let mut authorization = None;
        let mut anthropic_version = None;
        let mut anthropic_beta = None;
        let mut organization_uuid = None;
        let mut content_type = None;
        let mut content_length = 0_usize;

        for line in lines {
            let Some((name, value)) = line.split_once(':') else {
                continue;
            };
            let value = value.trim().to_string();
            if name.eq_ignore_ascii_case("authorization") {
                authorization = Some(value);
            } else if name.eq_ignore_ascii_case("anthropic-version") {
                anthropic_version = Some(value);
            } else if name.eq_ignore_ascii_case("anthropic-beta") {
                anthropic_beta = Some(value);
            } else if name.eq_ignore_ascii_case("x-organization-uuid") {
                organization_uuid = Some(value);
            } else if name.eq_ignore_ascii_case("content-type") {
                content_type = Some(value);
            } else if name.eq_ignore_ascii_case("content-length") {
                content_length = value.parse().unwrap_or_default();
            }
        }

        let body_start = header_end + 4;
        while buffer.len() < body_start + content_length {
            let read = stream.read(&mut chunk).await?;
            if read == 0 {
                break;
            }
            buffer.extend_from_slice(&chunk[..read]);
        }
        let body = String::from_utf8_lossy(
            &buffer[body_start..buffer.len().min(body_start + content_length)],
        )
        .to_string();

        Ok(RecordedRequest {
            method,
            path,
            authorization,
            anthropic_version,
            anthropic_beta,
            organization_uuid,
            content_type,
            body,
        })
    }

    fn find_header_end(buffer: &[u8]) -> Option<usize> {
        buffer.windows(4).position(|window| window == b"\r\n\r\n")
    }

    async fn write_http_response(
        stream: &mut TcpStream,
        status: u16,
        body: &str,
    ) -> std::io::Result<()> {
        let status_text = match status {
            200 => "OK",
            201 => "Created",
            401 => "Unauthorized",
            404 => "Not Found",
            409 => "Conflict",
            500 => "Internal Server Error",
            _ => "Status",
        };
        stream
            .write_all(
                format!(
                    "HTTP/1.1 {} {}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                    status,
                    status_text,
                    body.len(),
                    body
                )
                .as_bytes(),
            )
            .await?;
        stream.flush().await
    }
}

use crate::types::*;
use futures_util::stream::{SplitSink, SplitStream};
use futures_util::{SinkExt, StreamExt};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use thiserror::Error;
use tokio::net::TcpStream;
use tokio::sync::Mutex;
use tokio::time::{sleep, Duration};
use tokio_tungstenite::{
    connect_async,
    tungstenite::{
        client::IntoClientRequest, http::header::AUTHORIZATION, http::HeaderValue, Message,
    },
    MaybeTlsStream, WebSocketStream,
};

const PING_INTERVAL_MS: u64 = 30000;
const RECONNECT_DELAY_MS: u64 = 2000;
const MAX_RECONNECT_ATTEMPTS: u32 = 5;
const MAX_SESSION_NOT_FOUND_RETRIES: u32 = 3;
const SESSION_NOT_FOUND_CLOSE_CODE: u16 = 4001;
const UNAUTHORIZED_CLOSE_CODE: u16 = 4003;
pub const DEFAULT_API_BASE_URL: &str = "https://api.anthropic.com";

#[derive(Debug, Error)]
pub enum WebSocketError {
    #[error("Connection error: {0}")]
    Connection(String),
    #[error("Not connected")]
    NotConnected,
    #[error("Parse error: {0}")]
    Parse(String),
}

#[derive(Debug, Clone, Copy, PartialEq)]
enum WebSocketState {
    Connecting,
    Connected,
    Closed,
}

type SessionWsStream = WebSocketStream<MaybeTlsStream<TcpStream>>;
type SessionWsWriter = SplitSink<SessionWsStream, Message>;
type SessionWsReader = SplitStream<SessionWsStream>;

#[derive(Clone)]
struct SessionsWebSocketRuntime {
    session_id: String,
    org_uuid: String,
    get_access_token: Arc<dyn Fn() -> String + Send + Sync>,
    callbacks: Arc<dyn SessionsWebSocketCallbacks>,
    api_base_url: String,
    writer: Arc<Mutex<Option<SessionWsWriter>>>,
    state: Arc<Mutex<WebSocketState>>,
    reconnect_attempts: Arc<Mutex<u32>>,
    session_not_found_retries: Arc<Mutex<u32>>,
    last_access_token: Arc<Mutex<Option<String>>>,
    reconnect_delay: Duration,
    reconnect_generation: Arc<AtomicU64>,
}

#[async_trait::async_trait]
pub trait SessionsWebSocketCallbacks: Send + Sync {
    async fn on_message(&self, message: SessionMessage);
    async fn on_control_request(&self, request: SDKControlRequest) -> Option<SDKControlResponse> {
        default_control_request_response(&request)
    }
    async fn refresh_after_unauthorized(&self, _stale_access_token: String) -> bool {
        false
    }
    async fn on_close(&self) {}
    async fn on_error(&self, _error: WebSocketError) {}
    async fn on_connected(&self) {}
    async fn on_reconnecting(&self) {}
}

pub struct SessionsWebSocket {
    session_id: String,
    org_uuid: String,
    get_access_token: Arc<dyn Fn() -> String + Send + Sync>,
    callbacks: Arc<dyn SessionsWebSocketCallbacks>,
    api_base_url: String,
    writer: Arc<Mutex<Option<SessionWsWriter>>>,
    state: Arc<Mutex<WebSocketState>>,
    reconnect_attempts: Arc<Mutex<u32>>,
    session_not_found_retries: Arc<Mutex<u32>>,
    last_access_token: Arc<Mutex<Option<String>>>,
    reconnect_delay: Duration,
    reconnect_generation: Arc<AtomicU64>,
}

impl SessionsWebSocket {
    pub fn new(
        session_id: String,
        org_uuid: String,
        get_access_token: Arc<dyn Fn() -> String + Send + Sync>,
        callbacks: Arc<dyn SessionsWebSocketCallbacks>,
    ) -> Self {
        Self::with_api_base_url(
            session_id,
            org_uuid,
            get_access_token,
            callbacks,
            DEFAULT_API_BASE_URL.to_string(),
        )
    }

    pub fn with_api_base_url(
        session_id: String,
        org_uuid: String,
        get_access_token: Arc<dyn Fn() -> String + Send + Sync>,
        callbacks: Arc<dyn SessionsWebSocketCallbacks>,
        api_base_url: String,
    ) -> Self {
        Self::with_api_base_url_and_reconnect_delay(
            session_id,
            org_uuid,
            get_access_token,
            callbacks,
            api_base_url,
            Duration::from_millis(RECONNECT_DELAY_MS),
        )
    }

    pub fn with_api_base_url_and_reconnect_delay(
        session_id: String,
        org_uuid: String,
        get_access_token: Arc<dyn Fn() -> String + Send + Sync>,
        callbacks: Arc<dyn SessionsWebSocketCallbacks>,
        api_base_url: String,
        reconnect_delay: Duration,
    ) -> Self {
        Self {
            session_id,
            org_uuid,
            get_access_token,
            callbacks,
            api_base_url,
            writer: Arc::new(Mutex::new(None)),
            state: Arc::new(Mutex::new(WebSocketState::Closed)),
            reconnect_attempts: Arc::new(Mutex::new(0)),
            session_not_found_retries: Arc::new(Mutex::new(0)),
            last_access_token: Arc::new(Mutex::new(None)),
            reconnect_delay,
            reconnect_generation: Arc::new(AtomicU64::new(0)),
        }
    }

    pub async fn connect(&self) -> Result<(), WebSocketError> {
        connect_runtime(self.runtime()).await
    }

    fn runtime(&self) -> SessionsWebSocketRuntime {
        SessionsWebSocketRuntime {
            session_id: self.session_id.clone(),
            org_uuid: self.org_uuid.clone(),
            get_access_token: self.get_access_token.clone(),
            callbacks: self.callbacks.clone(),
            api_base_url: self.api_base_url.clone(),
            writer: self.writer.clone(),
            state: self.state.clone(),
            reconnect_attempts: self.reconnect_attempts.clone(),
            session_not_found_retries: self.session_not_found_retries.clone(),
            last_access_token: self.last_access_token.clone(),
            reconnect_delay: self.reconnect_delay,
            reconnect_generation: self.reconnect_generation.clone(),
        }
    }

    pub async fn send_control_response(
        &self,
        response: SDKControlResponse,
    ) -> Result<(), WebSocketError> {
        let state = self.state.lock().await;

        if *state != WebSocketState::Connected {
            return Err(WebSocketError::NotConnected);
        }
        drop(state);

        let mut writer_guard = self.writer.lock().await;
        if let Some(writer) = writer_guard.as_mut() {
            let msg = serde_json::to_string(&ControlMessage::Response(response))
                .map_err(|e| WebSocketError::Parse(e.to_string()))?;
            writer
                .send(Message::Text(msg.into()))
                .await
                .map_err(|e| WebSocketError::Connection(e.to_string()))?;
            Ok(())
        } else {
            Err(WebSocketError::NotConnected)
        }
    }

    pub async fn send_control_request(
        &self,
        request: SDKControlRequestInner,
    ) -> Result<(), WebSocketError> {
        let control_request = SDKControlRequest {
            request_id: uuid::Uuid::new_v4().to_string(),
            request,
        };

        let state = self.state.lock().await;

        if *state != WebSocketState::Connected {
            return Err(WebSocketError::NotConnected);
        }
        drop(state);

        let mut writer_guard = self.writer.lock().await;
        if let Some(writer) = writer_guard.as_mut() {
            let msg = serde_json::to_string(&ControlMessage::Request(control_request))
                .map_err(|e| WebSocketError::Parse(e.to_string()))?;
            writer
                .send(Message::Text(msg.into()))
                .await
                .map_err(|e| WebSocketError::Connection(e.to_string()))?;
            Ok(())
        } else {
            Err(WebSocketError::NotConnected)
        }
    }

    pub async fn send_control_cancel_request(
        &self,
        request_id: String,
    ) -> Result<(), WebSocketError> {
        let cancel_request = SDKControlCancelRequest { request_id };

        let state = self.state.lock().await;

        if *state != WebSocketState::Connected {
            return Err(WebSocketError::NotConnected);
        }
        drop(state);

        let mut writer_guard = self.writer.lock().await;
        if let Some(writer) = writer_guard.as_mut() {
            let msg = serde_json::to_string(&ControlMessage::CancelRequest(cancel_request))
                .map_err(|e| WebSocketError::Parse(e.to_string()))?;
            writer
                .send(Message::Text(msg.into()))
                .await
                .map_err(|e| WebSocketError::Connection(e.to_string()))?;
            Ok(())
        } else {
            Err(WebSocketError::NotConnected)
        }
    }

    pub async fn is_connected(&self) -> bool {
        *self.state.lock().await == WebSocketState::Connected
    }

    pub async fn close(&self) {
        self.reconnect_generation.fetch_add(1, Ordering::SeqCst);
        *self.state.lock().await = WebSocketState::Closed;
        let mut writer_guard = self.writer.lock().await;
        if let Some(writer) = writer_guard.as_mut() {
            let _ = writer.close().await;
        }
        *writer_guard = None;
    }

    pub async fn reconnect(&self) {
        self.close().await;
        *self.reconnect_attempts.lock().await = 0;
        *self.session_not_found_retries.lock().await = 0;
        let generation = self.reconnect_generation.load(Ordering::SeqCst);
        sleep(Duration::from_millis(500)).await;
        if self.reconnect_generation.load(Ordering::SeqCst) == generation {
            let _ = self.connect().await;
        }
    }
}

async fn connect_runtime(runtime: SessionsWebSocketRuntime) -> Result<(), WebSocketError> {
    let mut state = runtime.state.lock().await;
    if *state == WebSocketState::Connecting {
        return Ok(());
    }
    *state = WebSocketState::Connecting;
    drop(state);

    let result = connect_runtime_inner(runtime.clone()).await;
    if result.is_err() {
        *runtime.state.lock().await = WebSocketState::Closed;
        *runtime.writer.lock().await = None;
    }
    result
}

async fn connect_runtime_inner(runtime: SessionsWebSocketRuntime) -> Result<(), WebSocketError> {
    let access_token = (runtime.get_access_token)();
    *runtime.last_access_token.lock().await = Some(access_token.clone());
    let url = sessions_websocket_url(
        &runtime.api_base_url,
        &runtime.session_id,
        &runtime.org_uuid,
    )?;

    let mut request = url
        .into_client_request()
        .map_err(|e| WebSocketError::Connection(e.to_string()))?;
    let auth_value = HeaderValue::from_str(&format!("Bearer {}", access_token))
        .map_err(|e| WebSocketError::Connection(e.to_string()))?;
    request.headers_mut().insert(AUTHORIZATION, auth_value);
    request
        .headers_mut()
        .insert("anthropic-version", HeaderValue::from_static("2023-06-01"));

    let (ws_stream, _) = connect_async(request)
        .await
        .map_err(|e| WebSocketError::Connection(e.to_string()))?;
    let (writer, reader) = ws_stream.split();

    *runtime.writer.lock().await = Some(writer);
    *runtime.state.lock().await = WebSocketState::Connected;
    *runtime.reconnect_attempts.lock().await = 0;
    *runtime.session_not_found_retries.lock().await = 0;

    runtime.callbacks.on_connected().await;
    start_message_loop(runtime.clone(), reader);
    start_ping_loop(runtime.writer.clone(), runtime.state.clone());

    Ok(())
}

fn start_message_loop(runtime: SessionsWebSocketRuntime, mut reader: SessionWsReader) {
    tokio::spawn(async move {
        loop {
            match reader.next().await {
                Some(Ok(Message::Text(text))) => {
                    match serde_json::from_str::<SessionMessage>(&text) {
                        Ok(message) => {
                            if let SessionMessage::Control(ControlMessage::Request(request)) =
                                &message
                            {
                                match runtime.callbacks.on_control_request(request.clone()).await {
                                    Some(response) => {
                                        if let Err(error) = send_control_response_with_writer(
                                            &runtime.writer,
                                            response,
                                        )
                                        .await
                                        {
                                            runtime.callbacks.on_error(error).await;
                                        }
                                    }
                                    None => runtime.callbacks.on_message(message).await,
                                }
                            } else {
                                runtime.callbacks.on_message(message).await;
                            }
                        }
                        Err(error) => {
                            runtime
                                .callbacks
                                .on_error(WebSocketError::Parse(error.to_string()))
                                .await;
                        }
                    }
                }
                Some(Ok(Message::Close(close))) => {
                    let close_code = close.as_ref().map(|frame| u16::from(frame.code));
                    handle_socket_closed(runtime.clone(), close_code, None).await;
                    break;
                }
                Some(Err(error)) => {
                    handle_socket_closed(
                        runtime.clone(),
                        None,
                        Some(WebSocketError::Connection(error.to_string())),
                    )
                    .await;
                    break;
                }
                Some(Ok(_)) => {}
                None => {
                    handle_socket_closed(runtime.clone(), None, None).await;
                    break;
                }
            }
        }
    });
}

fn start_ping_loop(writer: Arc<Mutex<Option<SessionWsWriter>>>, state: Arc<Mutex<WebSocketState>>) {
    tokio::spawn(async move {
        loop {
            sleep(Duration::from_millis(PING_INTERVAL_MS)).await;
            let state_guard = state.lock().await;
            if *state_guard != WebSocketState::Connected {
                break;
            }
            drop(state_guard);

            let mut writer_guard = writer.lock().await;
            if let Some(writer) = writer_guard.as_mut() {
                let _ = writer.send(Message::Ping(Vec::new().into())).await;
            }
        }
    });
}

async fn mark_closed(state: &Arc<Mutex<WebSocketState>>) -> Option<WebSocketState> {
    let mut guard = state.lock().await;
    let previous = *guard;
    if previous == WebSocketState::Closed {
        return None;
    }
    *guard = WebSocketState::Closed;
    Some(previous)
}

async fn handle_socket_closed(
    runtime: SessionsWebSocketRuntime,
    close_code: Option<u16>,
    error: Option<WebSocketError>,
) {
    let Some(previous_state) = mark_closed(&runtime.state).await else {
        return;
    };
    *runtime.writer.lock().await = None;

    if let Some(error) = error {
        runtime.callbacks.on_error(error).await;
    }

    if close_code == Some(UNAUTHORIZED_CLOSE_CODE) {
        if refresh_unauthorized_token(&runtime).await {
            let delay = runtime.reconnect_delay;
            schedule_reconnect(runtime, delay).await;
        } else {
            runtime.callbacks.on_close().await;
        }
        return;
    }

    if close_code == Some(SESSION_NOT_FOUND_CLOSE_CODE) {
        let mut retries = runtime.session_not_found_retries.lock().await;
        *retries += 1;
        if *retries > MAX_SESSION_NOT_FOUND_RETRIES {
            drop(retries);
            runtime.callbacks.on_close().await;
            return;
        }
        let delay = runtime.reconnect_delay * *retries;
        drop(retries);
        spawn_reconnect(runtime, delay);
        return;
    }

    if previous_state == WebSocketState::Connected {
        if schedule_reconnect(runtime.clone(), runtime.reconnect_delay).await {
            return;
        }
    }

    runtime.callbacks.on_close().await;
}

async fn refresh_unauthorized_token(runtime: &SessionsWebSocketRuntime) -> bool {
    let stale_access_token = runtime
        .last_access_token
        .lock()
        .await
        .clone()
        .unwrap_or_else(|| (runtime.get_access_token)());
    if !runtime
        .callbacks
        .refresh_after_unauthorized(stale_access_token.clone())
        .await
    {
        return false;
    }
    let refreshed_access_token = (runtime.get_access_token)();
    if refreshed_access_token.is_empty() || refreshed_access_token == stale_access_token {
        return false;
    }
    *runtime.last_access_token.lock().await = Some(refreshed_access_token);
    true
}

async fn schedule_reconnect(runtime: SessionsWebSocketRuntime, delay: Duration) -> bool {
    let mut attempts = runtime.reconnect_attempts.lock().await;
    if *attempts >= MAX_RECONNECT_ATTEMPTS {
        return false;
    }
    *attempts += 1;
    drop(attempts);
    spawn_reconnect(runtime, delay);
    true
}

fn spawn_reconnect(runtime: SessionsWebSocketRuntime, delay: Duration) {
    let generation = runtime.reconnect_generation.load(Ordering::SeqCst);
    tokio::spawn(async move {
        runtime.callbacks.on_reconnecting().await;
        sleep(delay).await;
        if runtime.reconnect_generation.load(Ordering::SeqCst) != generation {
            return;
        }
        if let Err(error) = connect_runtime(runtime.clone()).await {
            runtime.callbacks.on_error(error).await;
            handle_reconnect_connect_failure(runtime).await;
        }
    });
}

async fn handle_reconnect_connect_failure(runtime: SessionsWebSocketRuntime) {
    let mut attempts = runtime.reconnect_attempts.lock().await;
    if *attempts < MAX_RECONNECT_ATTEMPTS {
        *attempts += 1;
        let delay = runtime.reconnect_delay;
        drop(attempts);
        spawn_reconnect(runtime, delay);
    } else {
        drop(attempts);
        runtime.callbacks.on_close().await;
    }
}

fn default_control_request_response(request: &SDKControlRequest) -> Option<SDKControlResponse> {
    match &request.request {
        SDKControlRequestInner::CanUseTool(_) => None,
        SDKControlRequestInner::Initialize => Some(SDKControlResponse::success(
            request.request_id.clone(),
            Some(serde_json::json!({
                "commands": [],
                "output_style": "normal",
                "available_output_styles": ["normal"],
                "models": [],
                "account": {},
                "pid": std::process::id(),
            })),
        )),
        SDKControlRequestInner::Interrupt => Some(SDKControlResponse::success(
            request.request_id.clone(),
            None,
        )),
        SDKControlRequestInner::SetModel { .. }
        | SDKControlRequestInner::SetMaxThinkingTokens { .. }
        | SDKControlRequestInner::SetPermissionMode { .. } => Some(SDKControlResponse::error(
            request.request_id.clone(),
            format!(
                "Remote session listener does not handle control_request subtype: {}",
                control_request_subtype(&request.request)
            ),
        )),
        SDKControlRequestInner::Unknown { subtype, .. } => Some(SDKControlResponse::error(
            request.request_id.clone(),
            format!("Unsupported control request subtype: {subtype}"),
        )),
    }
}

fn control_request_subtype(request: &SDKControlRequestInner) -> &'static str {
    match request {
        SDKControlRequestInner::Initialize => "initialize",
        SDKControlRequestInner::CanUseTool(_) => "can_use_tool",
        SDKControlRequestInner::Interrupt => "interrupt",
        SDKControlRequestInner::SetModel { .. } => "set_model",
        SDKControlRequestInner::SetMaxThinkingTokens { .. } => "set_max_thinking_tokens",
        SDKControlRequestInner::SetPermissionMode { .. } => "set_permission_mode",
        SDKControlRequestInner::Unknown { .. } => "unknown",
    }
}

async fn send_control_response_with_writer(
    writer: &Arc<Mutex<Option<SessionWsWriter>>>,
    response: SDKControlResponse,
) -> Result<(), WebSocketError> {
    let mut writer_guard = writer.lock().await;
    let Some(writer) = writer_guard.as_mut() else {
        return Err(WebSocketError::NotConnected);
    };
    let msg = serde_json::to_string(&ControlMessage::Response(response))
        .map_err(|e| WebSocketError::Parse(e.to_string()))?;
    writer
        .send(Message::Text(msg.into()))
        .await
        .map_err(|e| WebSocketError::Connection(e.to_string()))
}

pub fn sessions_websocket_url(
    api_base_url: &str,
    session_id: &str,
    org_uuid: &str,
) -> Result<String, WebSocketError> {
    let base = api_base_url.trim().trim_end_matches('/');
    if base.is_empty() {
        return Err(WebSocketError::Connection(
            "remote API base URL cannot be empty".to_string(),
        ));
    }
    let websocket_base = if let Some(rest) = base.strip_prefix("https://") {
        format!("wss://{rest}")
    } else if let Some(rest) = base.strip_prefix("http://") {
        format!("ws://{rest}")
    } else if base.starts_with("wss://") || base.starts_with("ws://") {
        base.to_string()
    } else {
        return Err(WebSocketError::Connection(format!(
            "remote API base URL must start with http://, https://, ws://, or wss://: {api_base_url}"
        )));
    };

    Ok(format!(
        "{websocket_base}/v1/sessions/ws/{session_id}/subscribe?organization_uuid={org_uuid}"
    ))
}

#[cfg(test)]
mod tests {
    use super::{
        sessions_websocket_url, SessionsWebSocket, SessionsWebSocketCallbacks, WebSocketError,
        DEFAULT_API_BASE_URL,
    };
    use crate::types::{
        ControlMessage, PermissionResponse, SDKControlRequestInner, SDKControlResponse, SDKMessage,
        SessionMessage,
    };
    use futures_util::{SinkExt, StreamExt};
    use std::sync::{Arc, Mutex as StdMutex};
    use tokio::net::TcpListener;
    use tokio::sync::mpsc;
    use tokio::time::{timeout, Duration};
    use tokio_tungstenite::{
        accept_async, accept_hdr_async,
        tungstenite::{
            handshake::server::{Request, Response},
            protocol::{frame::coding::CloseCode, CloseFrame},
            Message,
        },
    };

    struct TestCallbacks {
        messages: mpsc::UnboundedSender<SessionMessage>,
        events: Option<mpsc::UnboundedSender<&'static str>>,
    }

    #[async_trait::async_trait]
    impl SessionsWebSocketCallbacks for TestCallbacks {
        async fn on_message(&self, message: SessionMessage) {
            let _ = self.messages.send(message);
        }

        async fn on_connected(&self) {
            if let Some(events) = &self.events {
                let _ = events.send("connected");
            }
        }

        async fn on_reconnecting(&self) {
            if let Some(events) = &self.events {
                let _ = events.send("reconnecting");
            }
        }

        async fn on_close(&self) {
            if let Some(events) = &self.events {
                let _ = events.send("closed");
            }
        }

        async fn on_error(&self, _error: WebSocketError) {
            if let Some(events) = &self.events {
                let _ = events.send("error");
            }
        }
    }

    #[test]
    fn websocket_url_uses_default_anthropic_endpoint() {
        assert_eq!(
            sessions_websocket_url(DEFAULT_API_BASE_URL, "session-1", "org-1").unwrap(),
            "wss://api.anthropic.com/v1/sessions/ws/session-1/subscribe?organization_uuid=org-1"
        );
    }

    #[test]
    fn websocket_url_accepts_custom_http_and_ws_bases() {
        assert_eq!(
            sessions_websocket_url("http://localhost:8080", "session-1", "org-1").unwrap(),
            "ws://localhost:8080/v1/sessions/ws/session-1/subscribe?organization_uuid=org-1"
        );
        assert_eq!(
            sessions_websocket_url("ws://localhost:9000/proxy/", "session-1", "org-1").unwrap(),
            "ws://localhost:9000/proxy/v1/sessions/ws/session-1/subscribe?organization_uuid=org-1"
        );
    }

    #[test]
    fn websocket_url_rejects_empty_or_unsupported_bases() {
        let empty = sessions_websocket_url("", "session-1", "org-1")
            .unwrap_err()
            .to_string();
        assert!(empty.contains("cannot be empty"));
        let invalid = sessions_websocket_url("ftp://localhost", "session-1", "org-1")
            .unwrap_err()
            .to_string();
        assert!(invalid.contains("must start with"));
    }

    #[tokio::test]
    async fn websocket_can_send_while_read_loop_is_waiting() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let (server_tx, mut server_rx) = mpsc::unbounded_channel();
        tokio::spawn(async move {
            let (stream, _) = listener.accept().await.unwrap();
            let mut ws = accept_async(stream).await.unwrap();
            ws.send(Message::Text(
                serde_json::to_string(&SessionMessage::SDK(SDKMessage::AuthStatus))
                    .unwrap()
                    .into(),
            ))
            .await
            .unwrap();
            for _ in 0..2 {
                if let Some(Ok(Message::Text(text))) = ws.next().await {
                    let _ = server_tx.send(text);
                }
            }
        });

        let (message_tx, mut message_rx) = mpsc::unbounded_channel();
        let ws = SessionsWebSocket::with_api_base_url(
            "session-1".to_string(),
            "org-1".to_string(),
            Arc::new(|| "token".to_string()),
            Arc::new(TestCallbacks {
                messages: message_tx,
                events: None,
            }),
            format!("http://{addr}"),
        );

        ws.connect().await.unwrap();
        timeout(Duration::from_secs(1), message_rx.recv())
            .await
            .unwrap()
            .unwrap();

        timeout(
            Duration::from_secs(1),
            ws.send_control_response(SDKControlResponse::permission_success(
                "request-1",
                PermissionResponse::Deny {
                    message: "no".to_string(),
                },
            )),
        )
        .await
        .unwrap()
        .unwrap();

        timeout(
            Duration::from_secs(1),
            ws.send_control_cancel_request("request-2".to_string()),
        )
        .await
        .unwrap()
        .unwrap();

        let sent = timeout(Duration::from_secs(1), server_rx.recv())
            .await
            .unwrap()
            .unwrap();
        assert!(sent.contains("request-1"));
        assert!(sent.contains("control_response"));
        let sent = timeout(Duration::from_secs(1), server_rx.recv())
            .await
            .unwrap()
            .unwrap();
        assert!(sent.contains("request-2"));
        assert!(sent.contains("control_cancel_request"));
        ws.close().await;
    }

    #[tokio::test]
    async fn websocket_replies_success_for_initialize_control_request() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let (server_tx, mut server_rx) = mpsc::unbounded_channel();
        tokio::spawn(async move {
            let (stream, _) = listener.accept().await.unwrap();
            let mut ws = accept_async(stream).await.unwrap();
            ws.send(Message::Text(
                serde_json::json!({
                    "type": "control_request",
                    "request_id": "req-init",
                    "request": {
                        "subtype": "initialize"
                    }
                })
                .to_string()
                .into(),
            ))
            .await
            .unwrap();
            if let Some(Ok(Message::Text(text))) = ws.next().await {
                let _ = server_tx.send(text);
            }
        });

        let (message_tx, _message_rx) = mpsc::unbounded_channel();
        let ws = SessionsWebSocket::with_api_base_url(
            "session-1".to_string(),
            "org-1".to_string(),
            Arc::new(|| "token".to_string()),
            Arc::new(TestCallbacks {
                messages: message_tx,
                events: None,
            }),
            format!("http://{addr}"),
        );

        ws.connect().await.unwrap();
        let sent = timeout(Duration::from_secs(1), server_rx.recv())
            .await
            .unwrap()
            .unwrap();
        let response: serde_json::Value = serde_json::from_str(&sent).unwrap();
        assert_eq!(response["type"], "control_response");
        assert_eq!(response["response"]["subtype"], "success");
        assert_eq!(response["response"]["request_id"], "req-init");
        assert_eq!(
            response["response"]["response"]["commands"],
            serde_json::json!([])
        );
        assert_eq!(response["response"]["response"]["output_style"], "normal");
        assert!(response["response"]["response"]["pid"].is_number());
        ws.close().await;
    }

    #[tokio::test]
    async fn websocket_replies_error_for_known_unsupported_control_request() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let (server_tx, mut server_rx) = mpsc::unbounded_channel();
        tokio::spawn(async move {
            let (stream, _) = listener.accept().await.unwrap();
            let mut ws = accept_async(stream).await.unwrap();
            ws.send(Message::Text(
                serde_json::json!({
                    "type": "control_request",
                    "request_id": "req-model",
                    "request": {
                        "subtype": "set_model",
                        "model": "claude-sonnet-4-5"
                    }
                })
                .to_string()
                .into(),
            ))
            .await
            .unwrap();
            if let Some(Ok(Message::Text(text))) = ws.next().await {
                let _ = server_tx.send(text);
            }
        });

        let (message_tx, _message_rx) = mpsc::unbounded_channel();
        let ws = SessionsWebSocket::with_api_base_url(
            "session-1".to_string(),
            "org-1".to_string(),
            Arc::new(|| "token".to_string()),
            Arc::new(TestCallbacks {
                messages: message_tx,
                events: None,
            }),
            format!("http://{addr}"),
        );

        ws.connect().await.unwrap();
        let sent = timeout(Duration::from_secs(1), server_rx.recv())
            .await
            .unwrap()
            .unwrap();
        let response: serde_json::Value = serde_json::from_str(&sent).unwrap();
        assert_eq!(response["type"], "control_response");
        assert_eq!(response["response"]["subtype"], "error");
        assert_eq!(response["response"]["request_id"], "req-model");
        assert!(response["response"]["error"].as_str().unwrap().contains(
            "Remote session listener does not handle control_request subtype: set_model"
        ));
        ws.close().await;
    }

    #[tokio::test]
    async fn websocket_replies_error_for_unknown_control_request() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let (server_tx, mut server_rx) = mpsc::unbounded_channel();
        tokio::spawn(async move {
            let (stream, _) = listener.accept().await.unwrap();
            let mut ws = accept_async(stream).await.unwrap();
            ws.send(Message::Text(
                serde_json::json!({
                    "type": "control_request",
                    "request_id": "req-unknown",
                    "request": {
                        "subtype": "launch_missiles"
                    }
                })
                .to_string()
                .into(),
            ))
            .await
            .unwrap();
            if let Some(Ok(Message::Text(text))) = ws.next().await {
                let _ = server_tx.send(text);
            }
        });

        let (message_tx, _message_rx) = mpsc::unbounded_channel();
        let ws = SessionsWebSocket::with_api_base_url(
            "session-1".to_string(),
            "org-1".to_string(),
            Arc::new(|| "token".to_string()),
            Arc::new(TestCallbacks {
                messages: message_tx,
                events: None,
            }),
            format!("http://{addr}"),
        );

        ws.connect().await.unwrap();
        let sent = timeout(Duration::from_secs(1), server_rx.recv())
            .await
            .unwrap()
            .unwrap();
        let response: serde_json::Value = serde_json::from_str(&sent).unwrap();
        assert_eq!(response["type"], "control_response");
        assert_eq!(response["response"]["subtype"], "error");
        assert_eq!(response["response"]["request_id"], "req-unknown");
        assert!(response["response"]["error"]
            .as_str()
            .unwrap()
            .contains("Unsupported control request subtype: launch_missiles"));
        ws.close().await;
    }

    #[tokio::test]
    async fn websocket_forwards_can_use_tool_control_request_to_callbacks() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move {
            let (stream, _) = listener.accept().await.unwrap();
            let mut ws = accept_async(stream).await.unwrap();
            ws.send(Message::Text(
                serde_json::json!({
                    "type": "control_request",
                    "request_id": "req-permission",
                    "request": {
                        "subtype": "can_use_tool",
                        "tool_name": "Bash",
                        "tool_use_id": "tool-1",
                        "input": {"command": "pwd"}
                    }
                })
                .to_string()
                .into(),
            ))
            .await
            .unwrap();
            let _ = ws.next().await;
        });

        let (message_tx, mut message_rx) = mpsc::unbounded_channel();
        let ws = SessionsWebSocket::with_api_base_url(
            "session-1".to_string(),
            "org-1".to_string(),
            Arc::new(|| "token".to_string()),
            Arc::new(TestCallbacks {
                messages: message_tx,
                events: None,
            }),
            format!("http://{addr}"),
        );

        ws.connect().await.unwrap();
        let message = timeout(Duration::from_secs(1), message_rx.recv())
            .await
            .unwrap()
            .unwrap();
        match message {
            SessionMessage::Control(ControlMessage::Request(request)) => {
                assert_eq!(request.request_id, "req-permission");
                match request.request {
                    SDKControlRequestInner::CanUseTool(permission) => {
                        assert_eq!(permission.tool_name, "Bash");
                        assert_eq!(permission.tool_use_id, "tool-1");
                        assert_eq!(permission.input["command"], "pwd");
                    }
                    other => panic!("expected can_use_tool, got {:?}", other),
                }
            }
            other => panic!("expected control request, got {:?}", other),
        }
        ws.close().await;
    }

    #[tokio::test]
    async fn websocket_auto_reconnects_after_transient_close() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move {
            let (stream, _) = listener.accept().await.unwrap();
            let mut ws = accept_async(stream).await.unwrap();
            ws.close(None).await.unwrap();

            let (stream, _) = listener.accept().await.unwrap();
            let mut ws = accept_async(stream).await.unwrap();
            ws.send(Message::Text(
                serde_json::to_string(&SessionMessage::SDK(SDKMessage::AuthStatus))
                    .unwrap()
                    .into(),
            ))
            .await
            .unwrap();
            let _ = ws.next().await;
        });

        let (message_tx, mut message_rx) = mpsc::unbounded_channel();
        let (event_tx, mut event_rx) = mpsc::unbounded_channel();
        let ws = SessionsWebSocket::with_api_base_url_and_reconnect_delay(
            "session-1".to_string(),
            "org-1".to_string(),
            Arc::new(|| "token".to_string()),
            Arc::new(TestCallbacks {
                messages: message_tx,
                events: Some(event_tx),
            }),
            format!("http://{addr}"),
            Duration::from_millis(10),
        );

        ws.connect().await.unwrap();
        let mut saw_reconnecting = false;
        for _ in 0..4 {
            let event = timeout(Duration::from_secs(1), event_rx.recv())
                .await
                .unwrap()
                .unwrap();
            if event == "reconnecting" {
                saw_reconnecting = true;
                break;
            }
        }
        assert!(
            saw_reconnecting,
            "transient close did not emit reconnecting"
        );

        let message = timeout(Duration::from_secs(1), message_rx.recv())
            .await
            .unwrap()
            .unwrap();
        assert!(matches!(
            message,
            SessionMessage::SDK(SDKMessage::AuthStatus)
        ));
        ws.close().await;
    }

    struct RefreshCallbacks {
        messages: mpsc::UnboundedSender<SessionMessage>,
        events: mpsc::UnboundedSender<&'static str>,
        token: Arc<StdMutex<String>>,
        fresh_token: String,
    }

    #[async_trait::async_trait]
    impl SessionsWebSocketCallbacks for RefreshCallbacks {
        async fn on_message(&self, message: SessionMessage) {
            let _ = self.messages.send(message);
        }

        async fn refresh_after_unauthorized(&self, stale_access_token: String) -> bool {
            if stale_access_token != "stale-token" {
                return false;
            }
            *self.token.lock().unwrap() = self.fresh_token.clone();
            let _ = self.events.send("refreshed");
            true
        }

        async fn on_connected(&self) {
            let _ = self.events.send("connected");
        }

        async fn on_reconnecting(&self) {
            let _ = self.events.send("reconnecting");
        }

        async fn on_close(&self) {
            let _ = self.events.send("closed");
        }

        async fn on_error(&self, _error: WebSocketError) {
            let _ = self.events.send("error");
        }
    }

    #[tokio::test]
    async fn websocket_refreshes_token_and_reconnects_after_unauthorized_close() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let (auth_tx, mut auth_rx) = mpsc::unbounded_channel();
        tokio::spawn(async move {
            let (stream, _) = listener.accept().await.unwrap();
            let first_auth_tx = auth_tx.clone();
            let mut ws = accept_hdr_async(stream, move |request: &Request, response: Response| {
                let auth = request
                    .headers()
                    .get("authorization")
                    .and_then(|value| value.to_str().ok())
                    .unwrap_or_default()
                    .to_string();
                let _ = first_auth_tx.send(auth);
                Ok(response)
            })
            .await
            .unwrap();
            ws.send(Message::Close(Some(CloseFrame {
                code: CloseCode::Library(4003),
                reason: "unauthorized".into(),
            })))
            .await
            .unwrap();
            let _ = ws.next().await;

            let (stream, _) = listener.accept().await.unwrap();
            let second_auth_tx = auth_tx.clone();
            let mut ws = accept_hdr_async(stream, move |request: &Request, response: Response| {
                let auth = request
                    .headers()
                    .get("authorization")
                    .and_then(|value| value.to_str().ok())
                    .unwrap_or_default()
                    .to_string();
                let _ = second_auth_tx.send(auth);
                Ok(response)
            })
            .await
            .unwrap();
            ws.send(Message::Text(
                serde_json::to_string(&SessionMessage::SDK(SDKMessage::AuthStatus))
                    .unwrap()
                    .into(),
            ))
            .await
            .unwrap();
            let _ = ws.next().await;
        });

        let token = Arc::new(StdMutex::new("stale-token".to_string()));
        let token_provider = {
            let token = token.clone();
            Arc::new(move || token.lock().unwrap().clone())
        };
        let (message_tx, mut message_rx) = mpsc::unbounded_channel();
        let (event_tx, mut event_rx) = mpsc::unbounded_channel();
        let ws = SessionsWebSocket::with_api_base_url_and_reconnect_delay(
            "session-1".to_string(),
            "org-1".to_string(),
            token_provider,
            Arc::new(RefreshCallbacks {
                messages: message_tx,
                events: event_tx,
                token,
                fresh_token: "fresh-token".to_string(),
            }),
            format!("http://{addr}"),
            Duration::from_millis(10),
        );

        ws.connect().await.unwrap();
        assert_eq!(
            timeout(Duration::from_secs(1), auth_rx.recv())
                .await
                .unwrap()
                .unwrap(),
            "Bearer stale-token"
        );

        let mut saw_refresh = false;
        let mut saw_reconnecting = false;
        for _ in 0..4 {
            let event = timeout(Duration::from_secs(1), event_rx.recv())
                .await
                .unwrap()
                .unwrap();
            saw_refresh |= event == "refreshed";
            saw_reconnecting |= event == "reconnecting";
            if saw_refresh && saw_reconnecting {
                break;
            }
        }
        assert!(saw_refresh, "4003 close did not trigger token refresh");
        assert!(saw_reconnecting, "4003 refresh did not schedule reconnect");
        assert_eq!(
            timeout(Duration::from_secs(1), auth_rx.recv())
                .await
                .unwrap()
                .unwrap(),
            "Bearer fresh-token"
        );

        let message = timeout(Duration::from_secs(1), message_rx.recv())
            .await
            .unwrap()
            .unwrap();
        assert!(matches!(
            message,
            SessionMessage::SDK(SDKMessage::AuthStatus)
        ));
        ws.close().await;
    }

    #[tokio::test]
    async fn websocket_permanent_close_does_not_reconnect() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move {
            let (stream, _) = listener.accept().await.unwrap();
            let mut ws = accept_async(stream).await.unwrap();
            ws.send(Message::Close(Some(CloseFrame {
                code: CloseCode::Library(4003),
                reason: "unauthorized".into(),
            })))
            .await
            .unwrap();
            let _ = ws.next().await;
        });

        let (message_tx, _message_rx) = mpsc::unbounded_channel();
        let (event_tx, mut event_rx) = mpsc::unbounded_channel();
        let ws = SessionsWebSocket::with_api_base_url_and_reconnect_delay(
            "session-1".to_string(),
            "org-1".to_string(),
            Arc::new(|| "token".to_string()),
            Arc::new(TestCallbacks {
                messages: message_tx,
                events: Some(event_tx),
            }),
            format!("http://{addr}"),
            Duration::from_millis(10),
        );

        ws.connect().await.unwrap();
        let mut saw_closed = false;
        for _ in 0..4 {
            let event = timeout(Duration::from_secs(1), event_rx.recv())
                .await
                .unwrap()
                .unwrap();
            assert_ne!(event, "reconnecting", "4003 close should be permanent");
            if event == "closed" {
                saw_closed = true;
                break;
            }
        }
        assert!(saw_closed, "permanent close did not emit closed");
        assert!(
            timeout(Duration::from_millis(50), event_rx.recv())
                .await
                .is_err(),
            "permanent close emitted extra reconnect events"
        );
        ws.close().await;
    }
}

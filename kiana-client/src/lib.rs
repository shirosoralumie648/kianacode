//! 不感知传输实现的 Kiana 协议客户端。
//!
//! `KianaClient` 只负责把调用者提供的元数据、命令和会话内容封装成 versioned wire
//! envelope，再交给 [`ClientTransport`]。它不持有 ControlPlane、不执行工具，也不在本地
//! 重复授权；因此 transport 返回的响应仍必须按协议状态和 receipt 解释。

use async_trait::async_trait;
use kiana_protocol::{
    ApprovalDecision, ApprovalId, AuditExportRequest, AuditQueryRequest, EntryPointKind,
    ExtensionVisibilitySnapshot, ParityRequest, RequestEnvelope, RequestMetadata, ResponseEnvelope,
    RunId, TurnId, UiHandshakeRequest, UiHandshakeResponse, UiHealth, WorkPacket,
};
use serde_json::Value;
use std::sync::{Arc, Mutex};

mod typed;

pub use typed::{
    ActionClient, ActionRequest, ArtifactClient, ArtifactPageRequest, CancellationToken,
    ClientRequestOptions, ClientSession, CommandStatusRequest, FeedClient, FeedListenerToken,
    FeedSubscription, HistoryRequest, QueryClient, SnapshotRequest, TypedClients, UiArtifactPageV1,
    UiCommandStatusV1, UiHistoryV1,
};

#[async_trait]
/// 协议请求的异步传输端口。
///
/// 实现负责把完整 [`RequestEnvelope`] 送到受控服务并返回 [`ResponseEnvelope`]。端口只
/// 表示传输错误，不把策略拒绝压扁成网络错误；调用方应继续检查响应中的业务状态。
pub trait ClientTransport: Send + Sync {
    /// 发送一个已经构造完成的协议请求。
    async fn send(&self, request: RequestEnvelope) -> Result<ResponseEnvelope, ClientError>;

    /// Best-effort cancellation hook for transports that can fence an in-flight request.
    ///
    /// The default is intentionally a no-op: a transport that cannot cancel an already sent
    /// request still has to return its late response, which the typed client will discard after
    /// checking its request token.  This hook never grants or performs a capability effect.
    async fn cancel(&self, _request_id: kiana_protocol::RequestId) -> Result<(), ClientError> {
        Ok(())
    }
}

#[derive(Default)]
pub(crate) struct ClientSessionState {
    pub(crate) session: Option<ClientSession>,
    pub(crate) listeners: std::collections::BTreeMap<String, typed::ListenerRecord>,
    pub(crate) next_listener_generation: u64,
    pub(crate) action_commands: std::collections::BTreeMap<String, kiana_protocol::RequestId>,
}

/// 面向协议调用者的轻量客户端 facade。
///
/// 泛型 transport 被按值持有，避免客户端偷偷创建全局连接或可变单例。所有方法都只构造
/// 请求并委托给 transport，权限、审批、执行与事实写入仍在服务端完成。
pub struct KianaClient<T> {
    transport: Arc<T>,
    pub(crate) state: Arc<Mutex<ClientSessionState>>,
}

impl<T> KianaClient<T>
where
    T: ClientTransport,
{
    /// Negotiate the versioned UI surface without performing a command or capability action.
    pub async fn initialize(
        &self,
        metadata: RequestMetadata,
        handshake: UiHandshakeRequest,
    ) -> Result<UiHandshakeResponse, ClientError> {
        handshake.validate().map_err(ClientError::Protocol)?;
        let project_root = metadata.project_root.clone();
        let session_id = metadata.session_id.clone();
        let response = self
            .transport
            .send(RequestEnvelope::command(
                metadata,
                "ui.initialize",
                serde_json::to_value(handshake).map_err(|error| {
                    ClientError::Protocol(format!("ui_handshake_encode:{error}"))
                })?,
            ))
            .await?;
        if response.status != kiana_protocol::ExecutionStatus::Completed {
            return Err(ClientError::Protocol(
                kiana_protocol::stable_error_from_response(&response)
                    .map(|error| error.message)
                    .unwrap_or_else(|| "ui_handshake_rejected".to_owned()),
            ));
        }
        let handshake: UiHandshakeResponse = serde_json::from_value(response.output)
            .map_err(|error| ClientError::Protocol(format!("ui_handshake_response:{error}")))?;
        handshake.validate().map_err(ClientError::Protocol)?;
        if let Ok(mut state) = self.state.lock() {
            state.session = Some(ClientSession {
                workspace: project_root,
                session_id,
                instance_id: handshake.instance_id.clone(),
                authority_epoch: handshake.authority_epoch,
            });
        }
        Ok(handshake)
    }

    /// Read the bounded health projection; raw paths, tokens and internal errors never cross this
    /// typed client boundary.
    pub async fn health(&self, metadata: RequestMetadata) -> Result<UiHealth, ClientError> {
        let response = self
            .transport
            .send(RequestEnvelope::command(metadata, "ui.health", Value::Null))
            .await?;
        if response.status != kiana_protocol::ExecutionStatus::Completed {
            return Err(ClientError::Protocol(
                kiana_protocol::stable_error_from_response(&response)
                    .map(|error| error.message)
                    .unwrap_or_else(|| "ui_health_unavailable".to_owned()),
            ));
        }
        let health: UiHealth = serde_json::from_value(response.output)
            .map_err(|error| ClientError::Protocol(format!("ui_health_response:{error}")))?;
        health.validate().map_err(ClientError::Protocol)?;
        Ok(health)
    }

    /// 使用给定传输端口创建客户端。
    pub fn new(transport: T) -> Self {
        Self {
            transport: Arc::new(transport),
            state: Arc::new(Mutex::new(ClientSessionState::default())),
        }
    }

    /// Return typed, state-sharing facades for the query/feed/action/artifact surfaces.
    pub fn typed_clients(&self) -> TypedClients<T> {
        TypedClients::from_shared(Arc::clone(&self.transport), Arc::clone(&self.state))
    }

    pub fn query_client(&self) -> QueryClient<T> {
        self.typed_clients().query
    }

    pub fn feed_client(&self) -> FeedClient<T> {
        self.typed_clients().feed
    }

    pub fn action_client(&self) -> ActionClient<T> {
        self.typed_clients().action
    }

    pub fn artifact_client(&self) -> ArtifactClient<T> {
        self.typed_clients().artifact
    }

    pub async fn resume_run(
        &self,
        metadata: RequestMetadata,
        run_id: Option<RunId>,
    ) -> Result<ResponseEnvelope, ClientError> {
        self.transport
            .send(RequestEnvelope::resume_run(metadata, run_id))
            .await
    }

    pub async fn pending_approvals(
        &self,
        metadata: RequestMetadata,
        run_id: Option<RunId>,
    ) -> Result<ResponseEnvelope, ClientError> {
        self.transport
            .send(RequestEnvelope::pending_approvals(metadata, run_id))
            .await
    }

    /// 发送通用命令请求；命令名称和参数不会在客户端本地解释。
    pub async fn command(
        &self,
        metadata: RequestMetadata,
        name: impl Into<String> + Send,
        arguments: Value,
    ) -> Result<ResponseEnvelope, ClientError> {
        self.transport
            .send(RequestEnvelope::command(metadata, name, arguments))
            .await
    }

    /// Read the authenticated CompanyOS governance-chain projection.
    pub async fn company_governance(
        &self,
        metadata: RequestMetadata,
        project_id: impl Into<String> + Send,
    ) -> Result<ResponseEnvelope, ClientError> {
        self.transport
            .send(RequestEnvelope::company_governance(metadata, project_id))
            .await
    }

    /// 提交不带证明材料的审批决定。
    ///
    /// 旧调用者可以使用该方法，但服务端可能要求 request hash/nonce；方法返回响应不代表
    /// 审批必然生效。
    pub async fn approval_decision(
        &self,
        metadata: RequestMetadata,
        approval_id: ApprovalId,
        decision: ApprovalDecision,
    ) -> Result<ResponseEnvelope, ClientError> {
        self.transport
            .send(RequestEnvelope::approval_decision(
                metadata,
                approval_id,
                decision,
            ))
            .await
    }

    /// 提交带 request hash 和 nonce 的审批决定。
    ///
    /// 证明字段由服务端验证其与待审批请求、时效和一次性状态的匹配；客户端只负责传输。
    pub async fn approval_decision_with_proof(
        &self,
        metadata: RequestMetadata,
        approval_id: ApprovalId,
        decision: ApprovalDecision,
        request_hash: Option<String>,
        nonce: Option<String>,
    ) -> Result<ResponseEnvelope, ClientError> {
        self.approval_decision_with_proof_and_version(
            metadata,
            approval_id,
            decision,
            request_hash,
            nonce,
            None,
        )
        .await
    }

    /// Submit a proof-bound approval decision with an optional expected journal version.  The
    /// server treats the version as an optimistic-concurrency check and replays durable
    /// decisions by command identity.
    pub async fn approval_decision_with_proof_and_version(
        &self,
        metadata: RequestMetadata,
        approval_id: ApprovalId,
        decision: ApprovalDecision,
        request_hash: Option<String>,
        nonce: Option<String>,
        expected_version: Option<u64>,
    ) -> Result<ResponseEnvelope, ClientError> {
        self.transport
            .send(RequestEnvelope::approval_decision_with_proof_and_version(
                metadata,
                approval_id,
                decision,
                request_hash,
                nonce,
                expected_version,
            ))
            .await
    }

    /// 开始一个没有显式历史消息的 harness run。
    pub async fn run(
        &self,
        metadata: RequestMetadata,
        prompt: impl Into<String> + Send,
        sandbox: Option<String>,
    ) -> Result<ResponseEnvelope, ClientError> {
        self.transport
            .send(RequestEnvelope::run(metadata, prompt, sandbox))
            .await
    }

    /// 开始一个附带结构化历史消息的 harness run。
    ///
    /// 历史仅作为模型上下文输入，不是新的事实源；服务端仍以 session、策略和 EventLog
    /// 状态为准。
    pub async fn run_with_history(
        &self,
        metadata: RequestMetadata,
        prompt: impl Into<String> + Send,
        history: Vec<kiana_protocol::ConversationMessage>,
        sandbox: Option<String>,
    ) -> Result<ResponseEnvelope, ClientError> {
        self.transport
            .send(RequestEnvelope::run_with_history(
                metadata, prompt, history, sandbox,
            ))
            .await
    }

    /// 继续已有 run；`run_id = None` 时由服务端按 session 解析当前运行。
    pub async fn continue_run(
        &self,
        metadata: RequestMetadata,
        prompt: impl Into<String> + Send,
        sandbox: Option<String>,
        run_id: Option<RunId>,
    ) -> Result<ResponseEnvelope, ClientError> {
        self.transport
            .send(RequestEnvelope::continue_run(
                metadata, prompt, sandbox, run_id,
            ))
            .await
    }

    /// Queue a turn-bound steering message; the server rejects a stale expected turn.
    pub async fn steer_run(
        &self,
        metadata: RequestMetadata,
        run_id: RunId,
        expected_turn_id: TurnId,
        text: impl Into<String> + Send,
    ) -> Result<ResponseEnvelope, ClientError> {
        self.transport
            .send(RequestEnvelope::steer_run(
                metadata,
                run_id,
                expected_turn_id,
                text,
            ))
            .await
    }

    /// Queue source-labelled input for a future step/turn without waking an idle run.
    pub async fn inject_run(
        &self,
        metadata: RequestMetadata,
        run_id: RunId,
        target: impl Into<String> + Send,
        source: impl Into<String> + Send,
        text: impl Into<String> + Send,
        target_turn_id: Option<TurnId>,
    ) -> Result<ResponseEnvelope, ClientError> {
        self.transport
            .send(RequestEnvelope::inject_run(
                metadata,
                run_id,
                target,
                source,
                text,
                target_turn_id,
            ))
            .await
    }

    /// 创建一个显式的新 Turn；终态旧 Run 不会被复活，服务端会记录 predecessor 关联。
    pub async fn new_turn(
        &self,
        metadata: RequestMetadata,
        prompt: impl Into<String> + Send,
        sandbox: Option<String>,
        previous_run_id: Option<RunId>,
    ) -> Result<ResponseEnvelope, ClientError> {
        self.transport
            .send(RequestEnvelope::new_turn(
                metadata,
                prompt,
                sandbox,
                previous_run_id,
            ))
            .await
    }

    /// 请求取消已有 run。
    ///
    /// 返回成功只表示取消请求被协议层接受，目标 worker 是否已经停止要以后续状态或
    /// receipt 为准。
    pub async fn cancel_run(
        &self,
        metadata: RequestMetadata,
        run_id: Option<RunId>,
        reason: impl Into<String> + Send,
    ) -> Result<ResponseEnvelope, ClientError> {
        self.transport
            .send(RequestEnvelope::cancel_run(metadata, run_id, reason))
            .await
    }

    /// 请求读取指定 run 的回执投影。
    pub async fn receipt(
        &self,
        metadata: RequestMetadata,
        run_id: Option<RunId>,
    ) -> Result<ResponseEnvelope, ClientError> {
        self.transport
            .send(RequestEnvelope::receipt(metadata, run_id))
            .await
    }

    /// Query the server-authenticated audit projection; owner/scope are never client-selected.
    pub async fn audit_query(
        &self,
        metadata: RequestMetadata,
        query: AuditQueryRequest,
    ) -> Result<ResponseEnvelope, ClientError> {
        self.transport
            .send(RequestEnvelope::audit_query(metadata, query))
            .await
    }

    /// Materialize a server-authorized, redacted audit export.
    pub async fn audit_export(
        &self,
        metadata: RequestMetadata,
        export: AuditExportRequest,
    ) -> Result<ResponseEnvelope, ClientError> {
        self.transport
            .send(RequestEnvelope::audit_export(metadata, export))
            .await
    }

    /// Read the server-owned parity projection shared by CLI/Web/Workbench/Desktop.
    pub async fn parity(
        &self,
        metadata: RequestMetadata,
        entrypoint: EntryPointKind,
        run_id: Option<RunId>,
    ) -> Result<ResponseEnvelope, ClientError> {
        self.transport
            .send(RequestEnvelope::parity(
                metadata,
                ParityRequest { entrypoint, run_id },
            ))
            .await
    }

    /// Read the server-owned extension visibility projection.  `list`, `search` and `inspect`
    /// are metadata-only actions; the returned snapshot is never treated as a capability grant.
    pub async fn extension_visibility(
        &self,
        metadata: RequestMetadata,
        action: impl Into<String> + Send,
        query: Option<String>,
        extension_id: Option<String>,
        max_results: Option<usize>,
    ) -> Result<ExtensionVisibilitySnapshot, ClientError> {
        let mut arguments = serde_json::Map::new();
        arguments.insert("action".to_owned(), Value::String(action.into()));
        if let Some(query) = query {
            arguments.insert("query".to_owned(), Value::String(query));
        }
        if let Some(extension_id) = extension_id {
            arguments.insert("extension_id".to_owned(), Value::String(extension_id));
        }
        if let Some(max_results) = max_results {
            arguments.insert("max_results".to_owned(), serde_json::json!(max_results));
        }
        let response = self
            .command(
                metadata,
                kiana_protocol::EXTENSION_MANAGE_OPERATION,
                Value::Object(arguments),
            )
            .await?;
        if response.status != kiana_protocol::ExecutionStatus::Completed {
            return Err(ClientError::Protocol(
                kiana_protocol::stable_error_from_response(&response)
                    .map(|error| error.message)
                    .unwrap_or_else(|| "extension_visibility_rejected".to_owned()),
            ));
        }
        let snapshot: ExtensionVisibilitySnapshot = serde_json::from_value(
            response
                .output
                .get("snapshot")
                .cloned()
                .unwrap_or(Value::Null),
        )
        .map_err(|error| ClientError::Protocol(format!("extension_visibility_response:{error}")))?;
        snapshot
            .validate()
            .map_err(ClientError::Protocol)
            .map(|()| snapshot)
    }

    /// 提交 WorkPacket 的 spawn 请求；不会在客户端本地派生 cell。
    pub async fn spawn(
        &self,
        metadata: RequestMetadata,
        packet: WorkPacket,
        sandbox: Option<String>,
    ) -> Result<ResponseEnvelope, ClientError> {
        self.transport
            .send(RequestEnvelope::spawn(metadata, packet, sandbox))
            .await
    }

    /// 请求召开受控 symposium；参与者、轮数和审批由服务端验证。
    pub async fn convene(
        &self,
        metadata: RequestMetadata,
        goal: impl Into<String> + Send,
        anti_meeting: bool,
        max_rounds: u32,
        sandbox: Option<String>,
    ) -> Result<ResponseEnvelope, ClientError> {
        self.transport
            .send(RequestEnvelope::symposium(
                metadata,
                goal,
                anti_meeting,
                max_rounds,
                sandbox,
            ))
            .await
    }

    /// 请求对作者 run 生成 reviewer 视角的审查结果。
    pub async fn review(
        &self,
        metadata: RequestMetadata,
        author_session_id: impl Into<String> + Send,
        author_run_id: Option<RunId>,
    ) -> Result<ResponseEnvelope, ClientError> {
        self.transport
            .send(RequestEnvelope::review(
                metadata,
                author_session_id,
                author_run_id,
            ))
            .await
    }

    /// 请求关闭作者 run 并生成 closing 结果。
    pub async fn close(
        &self,
        metadata: RequestMetadata,
        author_session_id: impl Into<String> + Send,
        author_run_id: Option<RunId>,
    ) -> Result<ResponseEnvelope, ClientError> {
        self.transport
            .send(RequestEnvelope::close(
                metadata,
                author_session_id,
                author_run_id,
            ))
            .await
    }
}

#[derive(Clone, Debug, Eq, PartialEq, thiserror::Error)]
/// 客户端在协议响应之外遇到的传输错误。
pub enum ClientError {
    /// 传输层失败；不表示远端命令被执行或没有执行。
    #[error("client_transport_failed:{0}")]
    Transport(String),
    /// Remote response or typed handshake validation failed; no local retry is implied.
    #[error("client_protocol_failed:{0}")]
    Protocol(String),
    /// The UI facade has not completed the versioned handshake yet.
    #[error("client_not_initialized")]
    NotInitialized,
    /// The request targets a workspace different from the negotiated session.
    #[error("client_workspace_mismatch")]
    WorkspaceMismatch,
    /// A response or request carried a schema that this client cannot understand.
    #[error("client_unknown_schema:{0}")]
    UnknownSchema(String),
    /// The request was cancelled before dispatch.
    #[error("client_cancelled")]
    Cancelled,
    /// The request deadline had elapsed before dispatch or before its response was observed.
    #[error("client_deadline_exceeded")]
    DeadlineExceeded,
    /// A response arrived after its cancellation/deadline fence and cannot be delivered.
    #[error("client_late_response:{0}")]
    LateResponse(kiana_protocol::RequestId),
    /// A listener token has already been registered for the same controller.
    #[error("client_duplicate_listener:{0}")]
    DuplicateListener(String),
    /// A listener token was released or belongs to a previous controller generation.
    #[error("client_listener_inactive")]
    ListenerInactive,
    /// A mutation cannot be retried without reconciling its original command first.
    #[error("client_command_retry_forbidden")]
    CommandRetryForbidden,
}

#[cfg(test)]
mod tests {
    use super::*;
    use kiana_protocol::{ExecutionStatus, PROTOCOL_SCHEMA};

    struct EchoTransport;

    #[async_trait]
    impl ClientTransport for EchoTransport {
        async fn send(&self, request: RequestEnvelope) -> Result<ResponseEnvelope, ClientError> {
            Ok(ResponseEnvelope {
                schema: PROTOCOL_SCHEMA.to_owned(),
                request_id: request.metadata.request_id,
                status: ExecutionStatus::Completed,
                output: Value::Null,
                error: None,
            })
        }
    }

    #[tokio::test]
    async fn client_constructs_protocol_request_without_core_access() {
        let client = KianaClient::new(EchoTransport);
        let metadata = RequestMetadata::local("session-1", "/repo");
        let request_id = metadata.request_id;
        let response = client
            .command(metadata, "system.architecture", Value::Null)
            .await
            .unwrap();
        assert_eq!(response.request_id, request_id);
        assert_eq!(response.status, ExecutionStatus::Completed);
    }

    #[tokio::test]
    async fn client_constructs_run_request_without_core_access() {
        let client = KianaClient::new(EchoTransport);
        let metadata = RequestMetadata::local("session-1", "/repo");
        let request_id = metadata.request_id;
        let response = client
            .run(metadata, "map the architecture", None)
            .await
            .unwrap();
        assert_eq!(response.request_id, request_id);
        assert_eq!(response.status, ExecutionStatus::Completed);
    }

    #[tokio::test]
    async fn client_constructs_symposium_request_without_core_access() {
        let client = KianaClient::new(EchoTransport);
        let metadata = RequestMetadata::local("chair-1", "/repo");
        let request_id = metadata.request_id;
        let response = client
            .convene(
                metadata,
                "create GOLDEN_PATH.txt containing hello",
                true,
                4,
                Some("workspace-write".to_owned()),
            )
            .await
            .unwrap();
        assert_eq!(response.request_id, request_id);
        assert_eq!(response.status, ExecutionStatus::Completed);
    }

    #[tokio::test]
    async fn client_constructs_review_request_without_core_access() {
        let client = KianaClient::new(EchoTransport);
        let metadata = RequestMetadata::local("reviewer-1", "/repo");
        let request_id = metadata.request_id;
        let response = client
            .review(metadata, "builder-session", None)
            .await
            .unwrap();
        assert_eq!(response.request_id, request_id);
        assert_eq!(response.status, ExecutionStatus::Completed);
    }
}

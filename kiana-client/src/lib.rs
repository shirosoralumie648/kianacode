//! 不感知传输实现的 Kiana 协议客户端。
//!
//! `KianaClient` 只负责把调用者提供的元数据、命令和会话内容封装成 versioned wire
//! envelope，再交给 [`ClientTransport`]。它不持有 ControlPlane、不执行工具，也不在本地
//! 重复授权；因此 transport 返回的响应仍必须按协议状态和 receipt 解释。

use async_trait::async_trait;
use kiana_protocol::{
    ApprovalDecision, ApprovalId, RequestEnvelope, RequestMetadata, ResponseEnvelope, RunId,
    WorkPacket,
};
use serde_json::Value;

#[async_trait]
/// 协议请求的异步传输端口。
///
/// 实现负责把完整 [`RequestEnvelope`] 送到受控服务并返回 [`ResponseEnvelope`]。端口只
/// 表示传输错误，不把策略拒绝压扁成网络错误；调用方应继续检查响应中的业务状态。
pub trait ClientTransport: Send + Sync {
    /// 发送一个已经构造完成的协议请求。
    async fn send(&self, request: RequestEnvelope) -> Result<ResponseEnvelope, ClientError>;
}

/// 面向协议调用者的轻量客户端 facade。
///
/// 泛型 transport 被按值持有，避免客户端偷偷创建全局连接或可变单例。所有方法都只构造
/// 请求并委托给 transport，权限、审批、执行与事实写入仍在服务端完成。
pub struct KianaClient<T> {
    transport: T,
}

impl<T> KianaClient<T>
where
    T: ClientTransport,
{
    /// 使用给定传输端口创建客户端。
    pub fn new(transport: T) -> Self {
        Self { transport }
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
        self.transport
            .send(RequestEnvelope::approval_decision_with_proof(
                metadata,
                approval_id,
                decision,
                request_hash,
                nonce,
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

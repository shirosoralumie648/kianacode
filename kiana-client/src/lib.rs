//! Transport-neutral client for Kiana protocol requests.

use async_trait::async_trait;
use kiana_protocol::{
    ApprovalDecision, ApprovalId, RequestEnvelope, RequestMetadata, ResponseEnvelope, RunId,
    WorkPacket,
};
use serde_json::Value;

#[async_trait]
pub trait ClientTransport: Send + Sync {
    async fn send(&self, request: RequestEnvelope) -> Result<ResponseEnvelope, ClientError>;
}

pub struct KianaClient<T> {
    transport: T,
}

impl<T> KianaClient<T>
where
    T: ClientTransport,
{
    pub fn new(transport: T) -> Self {
        Self { transport }
    }

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

    pub async fn receipt(
        &self,
        metadata: RequestMetadata,
        run_id: Option<RunId>,
    ) -> Result<ResponseEnvelope, ClientError> {
        self.transport
            .send(RequestEnvelope::receipt(metadata, run_id))
            .await
    }

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
}

#[derive(Clone, Debug, Eq, PartialEq, thiserror::Error)]
pub enum ClientError {
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

//! Transport-neutral client for Kiana protocol requests.

use async_trait::async_trait;
use kiana_protocol::{
    ApprovalDecision, ApprovalId, RequestEnvelope, RequestMetadata, ResponseEnvelope, RunId,
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
}

//! Stable ports implemented by Kiana daemon adapters.

use async_trait::async_trait;
use kiana_domain::{
    ApprovalChallenge, ApprovalId, AuthorizedCapabilityRequest, CapabilityRequest,
    CapabilityResult, PendingApproval, RequestContext, RequestId, RuntimeEvent,
};
use kiana_runner_protocol::{RunnerCommand, RunnerEvent};

#[async_trait]
pub trait EventStorePort: Send + Sync {
    async fn append(&self, event: RuntimeEvent) -> Result<(), PortError>;

    async fn read_request(&self, request_id: &RequestId) -> Result<Vec<RuntimeEvent>, PortError>;

    async fn read_all(&self) -> Result<Vec<RuntimeEvent>, PortError> {
        Err(PortError::Failed(
            "event_store_read_all_unsupported".to_owned(),
        ))
    }
}

#[async_trait]
pub trait CapabilityBrokerPort: Send + Sync {
    async fn execute(
        &self,
        request: AuthorizedCapabilityRequest,
    ) -> Result<CapabilityResult, PortError>;
}

#[async_trait]
pub trait ApprovalStorePort: Send + Sync {
    async fn stage(
        &self,
        context: &RequestContext,
        request: CapabilityRequest,
        reason: &str,
    ) -> Result<ApprovalChallenge, PortError>;

    async fn activate(&self, approval_id: ApprovalId) -> Result<(), PortError>;

    async fn consume(
        &self,
        context: &RequestContext,
        approval_id: ApprovalId,
    ) -> Result<PendingApproval, PortError>;
}

#[async_trait]
pub trait RunnerPort: Send + Sync {
    async fn send(&self, command: RunnerCommand) -> Result<Vec<RunnerEvent>, PortError>;
}

#[derive(Clone, Debug, Eq, PartialEq, thiserror::Error)]
pub enum PortError {
    #[error("port_unavailable:{0}")]
    Unavailable(String),
    #[error("port_conflict:{0}")]
    Conflict(String),
    #[error("port_failed:{0}")]
    Failed(String),
}

//! Stable ports implemented by Kiana daemon adapters.

use async_trait::async_trait;
use kiana_domain::{AuthorizedCapabilityRequest, CapabilityResult, RequestId, RuntimeEvent};
use kiana_runner_protocol::{RunnerCommand, RunnerEvent};

#[async_trait]
pub trait EventStorePort: Send + Sync {
    async fn append(&self, event: RuntimeEvent) -> Result<(), PortError>;

    async fn read_request(&self, request_id: &RequestId) -> Result<Vec<RuntimeEvent>, PortError>;
}

#[async_trait]
pub trait CapabilityBrokerPort: Send + Sync {
    async fn execute(
        &self,
        request: AuthorizedCapabilityRequest,
    ) -> Result<CapabilityResult, PortError>;
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

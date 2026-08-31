//! Stable ports implemented by Kiana daemon adapters.

use async_trait::async_trait;
use kiana_domain::{
    AgentTemplate, ApprovalChallenge, ApprovalId, AuthorizedCapabilityRequest, BudgetLease,
    BudgetLeaseId, CapabilityGrant, CapabilityGrantId, CapabilityRequest, CapabilityResult, CellId,
    CellLifecycle, CellSpec, PendingApproval, RequestContext, RequestId, RetirementRecord, RunId,
    RuntimeEvent, SpawnPlan, SpawnPlanId, SupervisionLease, WorkFingerprint,
};
use kiana_runner_protocol::{RunnerCommand, RunnerEvent};

#[derive(Clone, Debug, PartialEq)]
pub struct EventAppendResult {
    pub event: RuntimeEvent,
    pub replayed: bool,
}

#[derive(Clone, Debug, PartialEq)]
pub struct SpawnReservationRequest {
    pub plan: SpawnPlan,
    pub cell: CellSpec,
    pub template: AgentTemplate,
    pub budget: BudgetLease,
    pub grant: CapabilityGrant,
    pub supervision: SupervisionLease,
    pub fingerprint: WorkFingerprint,
    pub owned_paths: Vec<String>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct SpawnReservation {
    pub plan: SpawnPlan,
    pub cell: CellSpec,
    pub template: AgentTemplate,
    pub budget: BudgetLease,
    pub grant: CapabilityGrant,
    pub supervision: SupervisionLease,
    pub fingerprint: WorkFingerprint,
    pub owned_paths: Vec<String>,
    pub replayed: bool,
}

#[derive(Clone, Debug, PartialEq)]
pub struct CapabilityLease {
    pub request_id: RequestId,
    pub cell_id: CellId,
    pub capability_grant_id: CapabilityGrantId,
    pub budget_lease_id: BudgetLeaseId,
    pub effect_count: u32,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CapabilityOutcome {
    Succeeded,
    Failed,
    Unknown,
}

#[async_trait]
pub trait CellRegistryPort: Send + Sync {
    async fn resolve_template(
        &self,
        role_id: &str,
        version: &str,
    ) -> Result<AgentTemplate, PortError>;

    async fn reserve_spawn(
        &self,
        request: SpawnReservationRequest,
    ) -> Result<SpawnReservation, PortError>;

    async fn transition_cell(
        &self,
        cell_id: CellId,
        expected: CellLifecycle,
        next: CellLifecycle,
    ) -> Result<CellSpec, PortError>;

    async fn begin_capability(
        &self,
        _cell_id: CellId,
        _capability_grant_id: CapabilityGrantId,
        _budget_lease_id: BudgetLeaseId,
        _request: &CapabilityRequest,
    ) -> Result<CapabilityLease, PortError> {
        Err(PortError::Failed(
            "cell_registry_capability_fence_unsupported".to_owned(),
        ))
    }

    async fn finish_capability(
        &self,
        _lease: CapabilityLease,
        _outcome: CapabilityOutcome,
    ) -> Result<(), PortError> {
        Err(PortError::Failed(
            "cell_registry_capability_accounting_unsupported".to_owned(),
        ))
    }

    async fn commit_spawn(&self, plan_id: SpawnPlanId) -> Result<SpawnReservation, PortError> {
        let _ = plan_id;
        Err(PortError::Failed(
            "cell_registry_commit_unsupported".to_owned(),
        ))
    }

    async fn abort_spawn(&self, plan_id: SpawnPlanId, reason: &str) -> Result<(), PortError>;

    async fn retire_cell(
        &self,
        cell_id: CellId,
        reason: &str,
    ) -> Result<RetirementRecord, PortError>;

    async fn cell_for_run(&self, run_id: RunId) -> Result<Option<CellId>, PortError>;

    async fn reservation_for_cell(
        &self,
        cell_id: CellId,
    ) -> Result<Option<SpawnReservation>, PortError> {
        let _ = cell_id;
        Err(PortError::Failed(
            "cell_registry_lookup_unsupported".to_owned(),
        ))
    }
}

#[async_trait]
pub trait EventStorePort: Send + Sync {
    async fn append(&self, event: RuntimeEvent) -> Result<(), PortError>;

    async fn append_expected(
        &self,
        event: RuntimeEvent,
        expected_version: Option<u64>,
    ) -> Result<(), PortError> {
        if expected_version.is_some() {
            return Err(PortError::Failed(
                "event_store_expected_version_unsupported".to_owned(),
            ));
        }
        self.append(event).await
    }

    async fn append_idempotent(
        &self,
        _event: RuntimeEvent,
    ) -> Result<EventAppendResult, PortError> {
        Err(PortError::Failed(
            "event_store_idempotency_unsupported".to_owned(),
        ))
    }

    async fn append_idempotent_expected(
        &self,
        event: RuntimeEvent,
        expected_version: Option<u64>,
    ) -> Result<EventAppendResult, PortError> {
        if expected_version.is_none() {
            return self.append_idempotent(event).await;
        }
        self.append_expected(event.clone(), expected_version)
            .await?;
        Ok(EventAppendResult {
            event,
            replayed: false,
        })
    }

    async fn read_request(&self, request_id: &RequestId) -> Result<Vec<RuntimeEvent>, PortError>;

    async fn read_all(&self) -> Result<Vec<RuntimeEvent>, PortError> {
        Err(PortError::Failed(
            "event_store_read_all_unsupported".to_owned(),
        ))
    }

    async fn read_stream(
        &self,
        aggregate_type: &str,
        aggregate_id: &str,
    ) -> Result<Vec<RuntimeEvent>, PortError> {
        Ok(self
            .read_all()
            .await?
            .into_iter()
            .filter(|event| {
                event.aggregate_type.as_deref() == Some(aggregate_type)
                    && event.aggregate_id.as_deref() == Some(aggregate_id)
            })
            .collect())
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

    async fn consume_with_proof(
        &self,
        context: &RequestContext,
        approval_id: ApprovalId,
        _request_hash: Option<&str>,
        _nonce: Option<&str>,
    ) -> Result<PendingApproval, PortError> {
        self.consume(context, approval_id).await
    }

    async fn invalidate(
        &self,
        _context: &RequestContext,
        _approval_id: ApprovalId,
        _reason: &str,
    ) -> Result<(), PortError> {
        Err(PortError::Failed(
            "approval_invalidation_unsupported".to_owned(),
        ))
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum PreToolHookDecision {
    Allow,
    Block(String),
    Ask { reason: String },
}

#[async_trait]
pub trait PreToolHookPort: Send + Sync {
    async fn decide(
        &self,
        context: &RequestContext,
        request: &CapabilityRequest,
    ) -> Result<PreToolHookDecision, PortError>;
}

#[derive(Debug, Default)]
pub struct AllowAllPreToolHooks;

#[async_trait]
impl PreToolHookPort for AllowAllPreToolHooks {
    async fn decide(
        &self,
        _context: &RequestContext,
        _request: &CapabilityRequest,
    ) -> Result<PreToolHookDecision, PortError> {
        Ok(PreToolHookDecision::Allow)
    }
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

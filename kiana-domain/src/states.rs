use crate::{EventId, RequestId};
use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ApprovalState {
    Staged,
    Active,
    Approved,
    Denied,
    Expired,
    Cancelled,
    Consumed,
}

impl ApprovalState {
    pub fn can_transition_to(self, next: Self) -> bool {
        matches!(
            (self, next),
            (Self::Staged, Self::Active)
                | (Self::Active, Self::Approved)
                | (Self::Active, Self::Denied)
                | (Self::Active, Self::Expired)
                | (Self::Active, Self::Cancelled)
                | (Self::Approved, Self::Consumed)
        )
    }

    pub fn transition(self, next: Self) -> Result<Self, DomainError> {
        if self.can_transition_to(next) {
            Ok(next)
        } else {
            Err(DomainError::InvalidStateTransition {
                aggregate: "approval",
                from: self.as_str(),
                to: next.as_str(),
            })
        }
    }

    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Staged => "staged",
            Self::Active => "active",
            Self::Approved => "approved",
            Self::Denied => "denied",
            Self::Expired => "expired",
            Self::Cancelled => "cancelled",
            Self::Consumed => "consumed",
        }
    }

    pub const fn is_terminal(self) -> bool {
        matches!(
            self,
            Self::Denied | Self::Expired | Self::Cancelled | Self::Consumed
        )
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CapabilityExecutionState {
    Requested,
    PolicyChecked,
    AwaitingApproval,
    Authorized,
    Denied,
    Dispatching,
    Executing,
    Succeeded,
    Failed,
    Cancelled,
    Unknown,
}

impl CapabilityExecutionState {
    pub fn can_transition_to(self, next: Self) -> bool {
        matches!(
            (self, next),
            (Self::Requested, Self::PolicyChecked)
                | (Self::PolicyChecked, Self::AwaitingApproval)
                | (Self::PolicyChecked, Self::Authorized)
                | (Self::PolicyChecked, Self::Denied)
                | (Self::Authorized, Self::Dispatching)
                | (Self::Dispatching, Self::Executing)
                | (Self::Executing, Self::Succeeded)
                | (Self::Executing, Self::Failed)
                | (Self::Executing, Self::Cancelled)
                | (Self::Executing, Self::Unknown)
                | (Self::AwaitingApproval, Self::Authorized)
                | (Self::AwaitingApproval, Self::Denied)
                | (Self::AwaitingApproval, Self::Cancelled)
                | (Self::AwaitingApproval, Self::Unknown)
        )
    }

    pub fn transition(self, next: Self) -> Result<Self, DomainError> {
        if self.can_transition_to(next) {
            Ok(next)
        } else {
            Err(DomainError::InvalidStateTransition {
                aggregate: "capability_execution",
                from: self.as_str(),
                to: next.as_str(),
            })
        }
    }

    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Requested => "requested",
            Self::PolicyChecked => "policy_checked",
            Self::AwaitingApproval => "awaiting_approval",
            Self::Authorized => "authorized",
            Self::Denied => "denied",
            Self::Dispatching => "dispatching",
            Self::Executing => "executing",
            Self::Succeeded => "succeeded",
            Self::Failed => "failed",
            Self::Cancelled => "cancelled",
            Self::Unknown => "unknown",
        }
    }

    pub const fn is_terminal(self) -> bool {
        matches!(
            self,
            Self::Denied | Self::Succeeded | Self::Failed | Self::Cancelled | Self::Unknown
        )
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WorkPacketStatus {
    Draft,
    Approved,
    Assigned,
    Running,
    Blocked,
    AwaitingApproval,
    Succeeded,
    Reviewed,
    Closed,
    Failed,
    Cancelled,
}

impl WorkPacketStatus {
    pub fn can_transition_to(self, next: Self) -> bool {
        matches!(
            (self, next),
            (Self::Draft, Self::Approved)
                | (Self::Approved, Self::Assigned)
                | (Self::Assigned, Self::Running)
                | (Self::Running, Self::Blocked)
                | (Self::Running, Self::AwaitingApproval)
                | (Self::Running, Self::Succeeded)
                | (Self::Running, Self::Failed)
                | (Self::Running, Self::Cancelled)
                | (Self::Blocked, Self::Running)
                | (Self::AwaitingApproval, Self::Running)
                | (Self::Succeeded, Self::Reviewed)
                | (Self::Reviewed, Self::Closed)
        )
    }

    pub fn transition(self, next: Self) -> Result<Self, DomainError> {
        if self.can_transition_to(next) {
            Ok(next)
        } else {
            Err(DomainError::InvalidStateTransition {
                aggregate: "work_packet",
                from: self.as_str(),
                to: next.as_str(),
            })
        }
    }

    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Draft => "draft",
            Self::Approved => "approved",
            Self::Assigned => "assigned",
            Self::Running => "running",
            Self::Blocked => "blocked",
            Self::AwaitingApproval => "awaiting_approval",
            Self::Succeeded => "succeeded",
            Self::Reviewed => "reviewed",
            Self::Closed => "closed",
            Self::Failed => "failed",
            Self::Cancelled => "cancelled",
        }
    }

    pub const fn is_terminal(self) -> bool {
        matches!(self, Self::Closed | Self::Failed | Self::Cancelled)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CellLifecycle {
    Proposed,
    Validated,
    Spawning,
    Ready,
    Running,
    WaitingInput,
    Blocked,
    Checkpointing,
    ReadyToMerge,
    Merging,
    Succeeded,
    Retiring,
    Retired,
    CancelRequested,
    Cancelled,
    Stalled,
    Retrying,
    Failed,
    Quarantined,
}

impl CellLifecycle {
    pub fn can_transition_to(self, next: Self) -> bool {
        matches!(
            (self, next),
            (Self::Proposed, Self::Validated)
                | (Self::Validated, Self::Spawning)
                | (Self::Spawning, Self::Ready)
                | (Self::Ready, Self::Running)
                | (Self::Running, Self::WaitingInput)
                | (Self::Running, Self::Blocked)
                | (Self::Running, Self::Checkpointing)
                | (Self::Running, Self::ReadyToMerge)
                | (Self::Running, Self::CancelRequested)
                | (Self::Running, Self::Stalled)
                | (Self::Running, Self::Failed)
                | (Self::Running, Self::Quarantined)
                | (Self::WaitingInput, Self::Running)
                | (Self::Blocked, Self::Running)
                | (Self::Checkpointing, Self::Running)
                | (Self::Checkpointing, Self::ReadyToMerge)
                | (Self::ReadyToMerge, Self::Merging)
                | (Self::ReadyToMerge, Self::Retiring)
                | (Self::Merging, Self::Succeeded)
                | (Self::Succeeded, Self::Retiring)
                | (Self::Retiring, Self::Retired)
                | (Self::Stalled, Self::Retrying)
                | (Self::Retrying, Self::Running)
                | (Self::Retrying, Self::Failed)
                | (Self::Failed, Self::Quarantined)
                | (Self::CancelRequested, Self::Cancelled)
                | (Self::Cancelled, Self::Retiring)
                | (Self::Quarantined, Self::Retiring)
        )
    }

    pub fn transition(self, next: Self) -> Result<Self, DomainError> {
        if self.can_transition_to(next) {
            Ok(next)
        } else {
            Err(DomainError::InvalidStateTransition {
                aggregate: "cell",
                from: self.as_str(),
                to: next.as_str(),
            })
        }
    }

    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Proposed => "proposed",
            Self::Validated => "validated",
            Self::Spawning => "spawning",
            Self::Ready => "ready",
            Self::Running => "running",
            Self::WaitingInput => "waiting_input",
            Self::Blocked => "blocked",
            Self::Checkpointing => "checkpointing",
            Self::ReadyToMerge => "ready_to_merge",
            Self::Merging => "merging",
            Self::Succeeded => "succeeded",
            Self::Retiring => "retiring",
            Self::Retired => "retired",
            Self::CancelRequested => "cancel_requested",
            Self::Cancelled => "cancelled",
            Self::Stalled => "stalled",
            Self::Retrying => "retrying",
            Self::Failed => "failed",
            Self::Quarantined => "quarantined",
        }
    }

    pub const fn is_terminal(self) -> bool {
        matches!(self, Self::Retired | Self::Cancelled | Self::Quarantined)
    }
}

impl Default for ApprovalState {
    fn default() -> Self {
        Self::Staged
    }
}

impl Default for CapabilityExecutionState {
    fn default() -> Self {
        Self::Requested
    }
}

impl Default for WorkPacketStatus {
    fn default() -> Self {
        Self::Draft
    }
}

impl Default for CellLifecycle {
    fn default() -> Self {
        Self::Proposed
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ExecutionStatus {
    Accepted,
    Denied,
    AwaitingApproval,
    Running,
    Completed,
    Failed,
    Cancelled,
    ResultUnknown,
    Blocked,
}

impl ExecutionStatus {
    pub fn can_transition_to(self, next: Self) -> bool {
        matches!(
            (self, next),
            (Self::Accepted, Self::Running)
                | (Self::Accepted, Self::AwaitingApproval)
                | (Self::Accepted, Self::Denied)
                | (Self::Accepted, Self::Blocked)
                | (Self::Running, Self::AwaitingApproval)
                | (Self::Running, Self::Completed)
                | (Self::Running, Self::Failed)
                | (Self::Running, Self::Cancelled)
                | (Self::Running, Self::ResultUnknown)
                | (Self::Running, Self::Blocked)
                | (Self::AwaitingApproval, Self::Running)
                | (Self::AwaitingApproval, Self::Denied)
                | (Self::AwaitingApproval, Self::Failed)
                | (Self::AwaitingApproval, Self::Cancelled)
                | (Self::AwaitingApproval, Self::ResultUnknown)
                | (Self::AwaitingApproval, Self::Blocked)
        )
    }

    pub fn transition(self, next: Self) -> Result<Self, DomainError> {
        if self.can_transition_to(next) {
            Ok(next)
        } else {
            Err(DomainError::InvalidStateTransition {
                aggregate: "execution",
                from: self.as_str(),
                to: next.as_str(),
            })
        }
    }

    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Accepted => "accepted",
            Self::Denied => "denied",
            Self::AwaitingApproval => "awaiting_approval",
            Self::Running => "running",
            Self::Completed => "completed",
            Self::Failed => "failed",
            Self::Cancelled => "cancelled",
            Self::ResultUnknown => "result_unknown",
            Self::Blocked => "blocked",
        }
    }

    pub const fn is_terminal(self) -> bool {
        matches!(
            self,
            Self::Denied
                | Self::Completed
                | Self::Failed
                | Self::Cancelled
                | Self::ResultUnknown
                | Self::Blocked
        )
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
/// EventLog 中一条带流元数据的不可变运行事件。
pub struct RuntimeEvent {
    /// 事件全局 ID；重复 ID 必须被 EventStore 拒绝。
    pub event_id: EventId,
    /// 关联请求 ID。
    pub request_id: RequestId,
    /// 请求内递增序号，从 1 开始。
    pub sequence: u64,
    /// 稳定事件种类。
    pub kind: String,
    /// 结构化事件载荷。
    pub data: Value,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub aggregate_type: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub aggregate_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stream_version: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub idempotency_key: Option<String>,
}

impl RuntimeEvent {
    /// 创建基础事件；序号 0 会被拒绝。
    pub fn new(
        request_id: RequestId,
        sequence: u64,
        kind: impl Into<String>,
        data: Value,
    ) -> Result<Self, DomainError> {
        if sequence == 0 {
            return Err(DomainError::InvalidEventSequence);
        }
        Ok(Self {
            event_id: EventId::new(),
            request_id,
            sequence,
            kind: kind.into(),
            data,
            aggregate_type: None,
            aggregate_id: None,
            stream_version: None,
            idempotency_key: None,
        })
    }

    /// 附加 aggregate stream 类型、ID 和版本，供 CAS/重放检查使用。
    pub fn with_stream_metadata(
        mut self,
        aggregate_type: impl Into<String>,
        aggregate_id: impl Into<String>,
        stream_version: u64,
    ) -> Self {
        self.aggregate_type = Some(aggregate_type.into());
        self.aggregate_id = Some(aggregate_id.into());
        self.stream_version = Some(stream_version);
        self
    }

    /// 附加幂等键；合法性和载荷一致性由 EventStore 验证。
    pub fn with_idempotency_key(mut self, idempotency_key: impl Into<String>) -> Self {
        self.idempotency_key = Some(idempotency_key.into());
        self
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
/// ControlPlane 返回给 daemon 的统一响应。
pub struct CoreResponse {
    /// 原请求 ID。
    pub request_id: RequestId,
    /// 当前执行状态。
    pub status: ExecutionStatus,
    /// 结构化输出。
    pub output: Value,
    /// 可选错误原因。
    pub error: Option<String>,
}

impl CoreResponse {
    /// 创建 Completed 响应。
    pub fn completed(request_id: RequestId, output: Value) -> Self {
        Self {
            request_id,
            status: ExecutionStatus::Completed,
            output,
            error: None,
        }
    }

    /// 创建 fail-closed 的 Blocked 响应。
    pub fn blocked(request_id: RequestId, reason: impl Into<String>) -> Self {
        let reason = reason.into();
        Self {
            request_id,
            status: ExecutionStatus::Blocked,
            output: Value::Null,
            error: Some(reason),
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq, thiserror::Error)]
/// 领域对象不变量或状态转换失败。
pub enum DomainError {
    /// 授权 ID 为空。
    #[error("authorization_id_required")]
    EmptyAuthorizationId,
    /// 事件序号不是正整数。
    #[error("event_sequence_must_be_positive")]
    InvalidEventSequence,
    /// aggregate 生命周期不允许该状态转换。
    #[error("{aggregate}_invalid_state_transition:{from}->{to}")]
    InvalidStateTransition {
        aggregate: &'static str,
        from: &'static str,
        to: &'static str,
    },
}

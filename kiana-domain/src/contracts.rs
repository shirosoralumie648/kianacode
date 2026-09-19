//! ID 契约注册表与 schema 版本规则。
//!
//! 本模块登记 domain 中公开 ID 类型的唯一 owner 与线协议形状，以及 schema 注册表、
//! 版本策略和 unknown field/event 处理规则。登记不改变任何现有 serde 表示；测试把
//! 注册表与真实类型的 JSON 往返行为锁在一起。

use serde::{Deserialize, Serialize};

/// ID 在 JSON 线协议中的基础形态。
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum IdWireShape {
    /// UUID 型 ID，线协议上表现为 JSON 字符串。
    Uuid,
    /// 非 UUID 字符串型 ID，线协议上表现为 JSON 字符串。
    String,
}

/// 一个公开 ID 类型的唯一契约。
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct IdContract {
    /// Rust 类型名，例如 `RequestId`。
    pub type_name: &'static str,
    /// 定义该 canonical 类型的 crate。
    pub owner_crate: &'static str,
    /// 线协议字段名，例如 `request_id`。
    pub wire_name: &'static str,
    /// 线协议基础形态。
    pub wire_shape: IdWireShape,
}

/// `kiana-domain` 当前公开的全部 ID 契约。
pub const ID_CONTRACTS: &[IdContract] = &[
    IdContract {
        type_name: "RequestId",
        owner_crate: env!("CARGO_PKG_NAME"),
        wire_name: "request_id",
        wire_shape: IdWireShape::Uuid,
    },
    IdContract {
        type_name: "InputId",
        owner_crate: env!("CARGO_PKG_NAME"),
        wire_name: "input_id",
        wire_shape: IdWireShape::Uuid,
    },
    IdContract {
        type_name: "InteractionId",
        owner_crate: env!("CARGO_PKG_NAME"),
        wire_name: "interaction_id",
        wire_shape: IdWireShape::Uuid,
    },
    IdContract {
        type_name: "RunId",
        owner_crate: env!("CARGO_PKG_NAME"),
        wire_name: "run_id",
        wire_shape: IdWireShape::Uuid,
    },
    IdContract {
        type_name: "TurnId",
        owner_crate: env!("CARGO_PKG_NAME"),
        wire_name: "turn_id",
        wire_shape: IdWireShape::Uuid,
    },
    IdContract {
        type_name: "StepId",
        owner_crate: env!("CARGO_PKG_NAME"),
        wire_name: "step_id",
        wire_shape: IdWireShape::Uuid,
    },
    IdContract {
        type_name: "CellId",
        owner_crate: env!("CARGO_PKG_NAME"),
        wire_name: "cell_id",
        wire_shape: IdWireShape::Uuid,
    },
    IdContract {
        type_name: "EventId",
        owner_crate: env!("CARGO_PKG_NAME"),
        wire_name: "event_id",
        wire_shape: IdWireShape::Uuid,
    },
    IdContract {
        type_name: "ExecutionId",
        owner_crate: env!("CARGO_PKG_NAME"),
        wire_name: "execution_id",
        wire_shape: IdWireShape::Uuid,
    },
    IdContract {
        type_name: "InvocationId",
        owner_crate: env!("CARGO_PKG_NAME"),
        wire_name: "invocation_id",
        wire_shape: IdWireShape::Uuid,
    },
    IdContract {
        type_name: "ModelAttemptId",
        owner_crate: env!("CARGO_PKG_NAME"),
        wire_name: "model_attempt_id",
        wire_shape: IdWireShape::Uuid,
    },
    IdContract {
        type_name: "ApprovalId",
        owner_crate: env!("CARGO_PKG_NAME"),
        wire_name: "approval_id",
        wire_shape: IdWireShape::Uuid,
    },
    IdContract {
        type_name: "ArtifactId",
        owner_crate: env!("CARGO_PKG_NAME"),
        wire_name: "artifact_id",
        wire_shape: IdWireShape::Uuid,
    },
    IdContract {
        type_name: "ReceiptId",
        owner_crate: env!("CARGO_PKG_NAME"),
        wire_name: "receipt_id",
        wire_shape: IdWireShape::Uuid,
    },
    IdContract {
        type_name: "OrganizationId",
        owner_crate: env!("CARGO_PKG_NAME"),
        wire_name: "organization_id",
        wire_shape: IdWireShape::Uuid,
    },
    IdContract {
        type_name: "PrincipalId",
        owner_crate: env!("CARGO_PKG_NAME"),
        wire_name: "principal_id",
        wire_shape: IdWireShape::Uuid,
    },
    IdContract {
        type_name: "ProviderAccountId",
        owner_crate: env!("CARGO_PKG_NAME"),
        wire_name: "provider_account_id",
        wire_shape: IdWireShape::Uuid,
    },
    IdContract {
        type_name: "ServiceIdentityId",
        owner_crate: env!("CARGO_PKG_NAME"),
        wire_name: "service_identity_id",
        wire_shape: IdWireShape::Uuid,
    },
    IdContract {
        type_name: "PolicyProfileId",
        owner_crate: env!("CARGO_PKG_NAME"),
        wire_name: "policy_profile_id",
        wire_shape: IdWireShape::Uuid,
    },
    IdContract {
        type_name: "DataBoundaryId",
        owner_crate: env!("CARGO_PKG_NAME"),
        wire_name: "data_boundary_id",
        wire_shape: IdWireShape::Uuid,
    },
    IdContract {
        type_name: "SharingGrantId",
        owner_crate: env!("CARGO_PKG_NAME"),
        wire_name: "sharing_grant_id",
        wire_shape: IdWireShape::Uuid,
    },
    IdContract {
        type_name: "SwarmPlanId",
        owner_crate: env!("CARGO_PKG_NAME"),
        wire_name: "swarm_plan_id",
        wire_shape: IdWireShape::Uuid,
    },
    IdContract {
        type_name: "PartitionId",
        owner_crate: env!("CARGO_PKG_NAME"),
        wire_name: "partition_id",
        wire_shape: IdWireShape::Uuid,
    },
    IdContract {
        type_name: "ChildCellId",
        owner_crate: env!("CARGO_PKG_NAME"),
        wire_name: "child_cell_id",
        wire_shape: IdWireShape::Uuid,
    },
    IdContract {
        type_name: "AttemptId",
        owner_crate: env!("CARGO_PKG_NAME"),
        wire_name: "attempt_id",
        wire_shape: IdWireShape::Uuid,
    },
    IdContract {
        type_name: "DispatchIntentId",
        owner_crate: env!("CARGO_PKG_NAME"),
        wire_name: "dispatch_intent_id",
        wire_shape: IdWireShape::Uuid,
    },
    IdContract {
        type_name: "QueueEntryId",
        owner_crate: env!("CARGO_PKG_NAME"),
        wire_name: "queue_entry_id",
        wire_shape: IdWireShape::Uuid,
    },
    IdContract {
        type_name: "MergeDecisionId",
        owner_crate: env!("CARGO_PKG_NAME"),
        wire_name: "merge_decision_id",
        wire_shape: IdWireShape::Uuid,
    },
    IdContract {
        type_name: "MessageId",
        owner_crate: env!("CARGO_PKG_NAME"),
        wire_name: "message_id",
        wire_shape: IdWireShape::Uuid,
    },
    IdContract {
        type_name: "NotificationId",
        owner_crate: env!("CARGO_PKG_NAME"),
        wire_name: "notification_id",
        wire_shape: IdWireShape::Uuid,
    },
    IdContract {
        type_name: "SubscriptionId",
        owner_crate: env!("CARGO_PKG_NAME"),
        wire_name: "subscription_id",
        wire_shape: IdWireShape::Uuid,
    },
    IdContract {
        type_name: "DeliveryAttemptId",
        owner_crate: env!("CARGO_PKG_NAME"),
        wire_name: "delivery_attempt_id",
        wire_shape: IdWireShape::Uuid,
    },
    IdContract {
        type_name: "ActionRefId",
        owner_crate: env!("CARGO_PKG_NAME"),
        wire_name: "action_ref_id",
        wire_shape: IdWireShape::Uuid,
    },
    IdContract {
        type_name: "DeliveryReceiptId",
        owner_crate: env!("CARGO_PKG_NAME"),
        wire_name: "delivery_receipt_id",
        wire_shape: IdWireShape::Uuid,
    },
    IdContract {
        type_name: "QualityArtifactId",
        owner_crate: env!("CARGO_PKG_NAME"),
        wire_name: "quality_artifact_id",
        wire_shape: IdWireShape::Uuid,
    },
    IdContract {
        type_name: "QualityTransitionId",
        owner_crate: env!("CARGO_PKG_NAME"),
        wire_name: "quality_transition_id",
        wire_shape: IdWireShape::Uuid,
    },
    IdContract {
        type_name: "EvalDatasetId",
        owner_crate: env!("CARGO_PKG_NAME"),
        wire_name: "eval_dataset_id",
        wire_shape: IdWireShape::Uuid,
    },
    IdContract {
        type_name: "EvalSuiteId",
        owner_crate: env!("CARGO_PKG_NAME"),
        wire_name: "eval_suite_id",
        wire_shape: IdWireShape::Uuid,
    },
    IdContract {
        type_name: "EvalCaseId",
        owner_crate: env!("CARGO_PKG_NAME"),
        wire_name: "eval_case_id",
        wire_shape: IdWireShape::Uuid,
    },
    IdContract {
        type_name: "GoldenTraceId",
        owner_crate: env!("CARGO_PKG_NAME"),
        wire_name: "golden_trace_id",
        wire_shape: IdWireShape::Uuid,
    },
    IdContract {
        type_name: "EvalExperimentId",
        owner_crate: env!("CARGO_PKG_NAME"),
        wire_name: "eval_experiment_id",
        wire_shape: IdWireShape::Uuid,
    },
    IdContract {
        type_name: "EvalResultId",
        owner_crate: env!("CARGO_PKG_NAME"),
        wire_name: "eval_result_id",
        wire_shape: IdWireShape::Uuid,
    },
    IdContract {
        type_name: "QualityCandidateId",
        owner_crate: env!("CARGO_PKG_NAME"),
        wire_name: "quality_candidate_id",
        wire_shape: IdWireShape::Uuid,
    },
    IdContract {
        type_name: "QualityGateId",
        owner_crate: env!("CARGO_PKG_NAME"),
        wire_name: "quality_gate_id",
        wire_shape: IdWireShape::Uuid,
    },
    IdContract {
        type_name: "QualityGateDecisionId",
        owner_crate: env!("CARGO_PKG_NAME"),
        wire_name: "quality_gate_decision_id",
        wire_shape: IdWireShape::Uuid,
    },
    IdContract {
        type_name: "FeedbackId",
        owner_crate: env!("CARGO_PKG_NAME"),
        wire_name: "feedback_id",
        wire_shape: IdWireShape::Uuid,
    },
    IdContract {
        type_name: "DriftAlertId",
        owner_crate: env!("CARGO_PKG_NAME"),
        wire_name: "drift_alert_id",
        wire_shape: IdWireShape::Uuid,
    },
    IdContract {
        type_name: "StorageRootId",
        owner_crate: env!("CARGO_PKG_NAME"),
        wire_name: "storage_root_id",
        wire_shape: IdWireShape::Uuid,
    },
    IdContract {
        type_name: "StoreIdentityId",
        owner_crate: env!("CARGO_PKG_NAME"),
        wire_name: "store_identity_id",
        wire_shape: IdWireShape::Uuid,
    },
    IdContract {
        type_name: "StorageLockId",
        owner_crate: env!("CARGO_PKG_NAME"),
        wire_name: "storage_lock_id",
        wire_shape: IdWireShape::Uuid,
    },
    IdContract {
        type_name: "StorageErrorId",
        owner_crate: env!("CARGO_PKG_NAME"),
        wire_name: "storage_error_id",
        wire_shape: IdWireShape::Uuid,
    },
    IdContract {
        type_name: "StorageHealthId",
        owner_crate: env!("CARGO_PKG_NAME"),
        wire_name: "storage_health_id",
        wire_shape: IdWireShape::Uuid,
    },
    IdContract {
        type_name: "StorageIntegrityIncidentId",
        owner_crate: env!("CARGO_PKG_NAME"),
        wire_name: "storage_integrity_incident_id",
        wire_shape: IdWireShape::Uuid,
    },
    IdContract {
        type_name: "ProjectId",
        owner_crate: env!("CARGO_PKG_NAME"),
        wire_name: "project_id",
        wire_shape: IdWireShape::Uuid,
    },
    IdContract {
        type_name: "TemplateId",
        owner_crate: env!("CARGO_PKG_NAME"),
        wire_name: "template_id",
        wire_shape: IdWireShape::Uuid,
    },
    IdContract {
        type_name: "SpawnPlanId",
        owner_crate: env!("CARGO_PKG_NAME"),
        wire_name: "spawn_plan_id",
        wire_shape: IdWireShape::Uuid,
    },
    IdContract {
        type_name: "BudgetLeaseId",
        owner_crate: env!("CARGO_PKG_NAME"),
        wire_name: "budget_lease_id",
        wire_shape: IdWireShape::Uuid,
    },
    IdContract {
        type_name: "CapabilityGrantId",
        owner_crate: env!("CARGO_PKG_NAME"),
        wire_name: "capability_grant_id",
        wire_shape: IdWireShape::Uuid,
    },
    IdContract {
        type_name: "SupervisionLeaseId",
        owner_crate: env!("CARGO_PKG_NAME"),
        wire_name: "supervision_lease_id",
        wire_shape: IdWireShape::Uuid,
    },
    IdContract {
        type_name: "DelegationId",
        owner_crate: env!("CARGO_PKG_NAME"),
        wire_name: "delegation_id",
        wire_shape: IdWireShape::Uuid,
    },
    IdContract {
        type_name: "ExtensionId",
        owner_crate: env!("CARGO_PKG_NAME"),
        wire_name: "extension_id",
        wire_shape: IdWireShape::Uuid,
    },
    IdContract {
        type_name: "ComponentId",
        owner_crate: env!("CARGO_PKG_NAME"),
        wire_name: "component_id",
        wire_shape: IdWireShape::Uuid,
    },
    IdContract {
        type_name: "SnapshotId",
        owner_crate: env!("CARGO_PKG_NAME"),
        wire_name: "snapshot_id",
        wire_shape: IdWireShape::Uuid,
    },
    IdContract {
        type_name: "HookRunId",
        owner_crate: env!("CARGO_PKG_NAME"),
        wire_name: "hook_run_id",
        wire_shape: IdWireShape::Uuid,
    },
    IdContract {
        type_name: "WorkspaceId",
        owner_crate: env!("CARGO_PKG_NAME"),
        wire_name: "workspace_id",
        wire_shape: IdWireShape::Uuid,
    },
    IdContract {
        type_name: "MembershipId",
        owner_crate: env!("CARGO_PKG_NAME"),
        wire_name: "membership_id",
        wire_shape: IdWireShape::Uuid,
    },
    IdContract {
        type_name: "AssignmentId",
        owner_crate: env!("CARGO_PKG_NAME"),
        wire_name: "assignment_id",
        wire_shape: IdWireShape::Uuid,
    },
    IdContract {
        type_name: "ProjectAssignmentId",
        owner_crate: env!("CARGO_PKG_NAME"),
        wire_name: "project_assignment_id",
        wire_shape: IdWireShape::Uuid,
    },
    IdContract {
        type_name: "EvidenceId",
        owner_crate: env!("CARGO_PKG_NAME"),
        wire_name: "evidence_id",
        wire_shape: IdWireShape::Uuid,
    },
    IdContract {
        type_name: "CriterionId",
        owner_crate: env!("CARGO_PKG_NAME"),
        wire_name: "criterion_id",
        wire_shape: IdWireShape::Uuid,
    },
    IdContract {
        type_name: "SecurityRegistryId",
        owner_crate: env!("CARGO_PKG_NAME"),
        wire_name: "security_registry_id",
        wire_shape: IdWireShape::Uuid,
    },
    IdContract {
        type_name: "SecurityContextId",
        owner_crate: env!("CARGO_PKG_NAME"),
        wire_name: "security_context_id",
        wire_shape: IdWireShape::Uuid,
    },
    IdContract {
        type_name: "SecurityPolicyId",
        owner_crate: env!("CARGO_PKG_NAME"),
        wire_name: "security_policy_id",
        wire_shape: IdWireShape::Uuid,
    },
    IdContract {
        type_name: "SecurityDecisionId",
        owner_crate: env!("CARGO_PKG_NAME"),
        wire_name: "security_decision_id",
        wire_shape: IdWireShape::Uuid,
    },
    IdContract {
        type_name: "SecurityEventId",
        owner_crate: env!("CARGO_PKG_NAME"),
        wire_name: "security_event_id",
        wire_shape: IdWireShape::Uuid,
    },
    IdContract {
        type_name: "GrantId",
        owner_crate: env!("CARGO_PKG_NAME"),
        wire_name: "grant_id",
        wire_shape: IdWireShape::Uuid,
    },
    IdContract {
        type_name: "OperationId",
        owner_crate: env!("CARGO_PKG_NAME"),
        wire_name: "operation_id",
        wire_shape: IdWireShape::Uuid,
    },
    IdContract {
        type_name: "AuditId",
        owner_crate: env!("CARGO_PKG_NAME"),
        wire_name: "audit_id",
        wire_shape: IdWireShape::Uuid,
    },
    IdContract {
        type_name: "SecretRefId",
        owner_crate: env!("CARGO_PKG_NAME"),
        wire_name: "secret_ref_id",
        wire_shape: IdWireShape::Uuid,
    },
    IdContract {
        type_name: "EvidenceRefId",
        owner_crate: env!("CARGO_PKG_NAME"),
        wire_name: "evidence_ref_id",
        wire_shape: IdWireShape::Uuid,
    },
    IdContract {
        type_name: "FenceTokenId",
        owner_crate: env!("CARGO_PKG_NAME"),
        wire_name: "fence_token_id",
        wire_shape: IdWireShape::Uuid,
    },
    IdContract {
        type_name: "SessionId",
        owner_crate: env!("CARGO_PKG_NAME"),
        wire_name: "session_id",
        wire_shape: IdWireShape::String,
    },
    IdContract {
        type_name: "WorkFingerprint",
        owner_crate: env!("CARGO_PKG_NAME"),
        wire_name: "work_fingerprint",
        wire_shape: IdWireShape::String,
    },
];

/// Schema 的层级分类。
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SchemaLayer {
    /// Canonical domain schema：业务对象字段与不变量。
    Domain,
    /// Wire protocol schema：跨进程传输的 envelope 与 DTO。
    Wire,
    /// Runtime event schema：执行事实与账本事件。
    RuntimeEvent,
    /// Projection/receipt schema：派生状态与收据。
    Projection,
}

/// Schema 版本号。
#[derive(Clone, Copy, Debug, Eq, PartialEq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SchemaVersion {
    pub major: u32,
    pub minor: u32,
}

impl SchemaVersion {
    pub const fn new(major: u32, minor: u32) -> Self {
        Self { major, minor }
    }

    /// 检查当前运行时版本是否与给定版本兼容。
    ///
    /// 规则：
    /// - major 不匹配 → fail-closed（未知 major 必须拒绝）
    /// - major 匹配、minor 更新 → 兼容（additive 字段可升 minor）
    pub fn is_compatible_with(&self, other: &SchemaVersion) -> bool {
        self.major == other.major
    }
}

/// Schema 的兼容性策略。
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CompatibilityPolicy {
    /// 严格向后兼容：major 不变时只能增加 optional 字段。
    BackwardCompatible,
    /// 破坏性变更：需要 major 升级与显式迁移。
    Breaking,
}

/// Unknown events remain inspectable facts; they confer no state transition or execution authority.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum UnknownEventPolicy {
    PreserveOpaqueWithoutExecution,
}
pub const UNKNOWN_EVENT_POLICY: UnknownEventPolicy =
    UnknownEventPolicy::PreserveOpaqueWithoutExecution;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SchemaMigration {
    pub from: &'static str,
    pub to: &'static str,
    pub adapter: &'static str,
}
pub const SCHEMA_MIGRATIONS: &[SchemaMigration] = &[SchemaMigration {
    from: "kiana.memory-record.v1",
    to: "kiana.memory-record.v2",
    adapter: "legacy_origin_to_candidate_draft",
}];

/// Schema 变更类型。
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SchemaChange {
    /// 增加 optional 字段（可升 minor）。
    AddOptionalField,
    /// 删除字段（必须升 major）。
    RemoveField,
    /// 改变字段类型（必须升 major）。
    ChangeFieldType,
    /// 改变 required 语义（必须升 major）。
    ChangeRequired,
    /// 改变状态语义（必须升 major）。
    ChangeStateSemantics,
}

impl SchemaChange {
    /// 此变更是否需要 major 版本升级。
    pub const fn requires_major_bump(&self) -> bool {
        matches!(
            self,
            SchemaChange::RemoveField
                | SchemaChange::ChangeFieldType
                | SchemaChange::ChangeRequired
                | SchemaChange::ChangeStateSemantics
        )
    }
}

/// 一个 schema 的注册契约。
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SchemaContract {
    /// Schema 名称，例如 `kiana.protocol.v1`。
    pub name: &'static str,
    /// 当前版本。
    pub version: SchemaVersion,
    /// Schema 层级。
    pub layer: SchemaLayer,
    /// Canonical owner crate。
    pub owner_crate: &'static str,
    /// 兼容性策略。
    pub compatibility: CompatibilityPolicy,
    /// Unknown field 处理：`true` 表示允许 unknown fields（宽松模式），`false` 表示拒绝。
    pub allow_unknown_fields: bool,
}

/// `kiana-domain` 和 `kiana-protocol` 当前注册的 schema 契约。
pub const SCHEMA_CONTRACTS: &[SchemaContract] = &[
    SchemaContract {
        name: "kiana.protocol.v1",
        version: SchemaVersion::new(1, 0),
        layer: SchemaLayer::Wire,
        owner_crate: "kiana-protocol",
        compatibility: CompatibilityPolicy::BackwardCompatible,
        allow_unknown_fields: true,
    },
    SchemaContract {
        name: "kiana.work-packet.v1",
        version: SchemaVersion::new(1, 0),
        layer: SchemaLayer::Domain,
        owner_crate: "kiana-domain",
        compatibility: CompatibilityPolicy::BackwardCompatible,
        allow_unknown_fields: false,
    },
    SchemaContract {
        name: "kiana.communication-message.v1",
        version: SchemaVersion::new(1, 0),
        layer: SchemaLayer::Domain,
        owner_crate: "kiana-domain",
        compatibility: CompatibilityPolicy::BackwardCompatible,
        allow_unknown_fields: false,
    },
    SchemaContract {
        name: "kiana.communication-lifecycle.v1",
        version: SchemaVersion::new(1, 0),
        layer: SchemaLayer::RuntimeEvent,
        owner_crate: "kiana-domain",
        compatibility: CompatibilityPolicy::BackwardCompatible,
        allow_unknown_fields: false,
    },
    SchemaContract {
        name: "kiana.notification-event-registry.v1",
        version: SchemaVersion::new(1, 0),
        layer: SchemaLayer::Domain,
        owner_crate: "kiana-domain",
        compatibility: CompatibilityPolicy::Breaking,
        allow_unknown_fields: false,
    },
    SchemaContract {
        name: "kiana.notification-dedup-request.v1",
        version: SchemaVersion::new(1, 0),
        layer: SchemaLayer::Domain,
        owner_crate: "kiana-domain",
        compatibility: CompatibilityPolicy::Breaking,
        allow_unknown_fields: false,
    },
    SchemaContract {
        name: "kiana.notification-dedup-record.v1",
        version: SchemaVersion::new(1, 0),
        layer: SchemaLayer::Projection,
        owner_crate: "kiana-domain",
        compatibility: CompatibilityPolicy::Breaking,
        allow_unknown_fields: false,
    },
    SchemaContract {
        name: "kiana.quality-artifact.v1",
        version: SchemaVersion::new(1, 0),
        layer: SchemaLayer::Domain,
        owner_crate: "kiana-domain",
        compatibility: CompatibilityPolicy::Breaking,
        allow_unknown_fields: false,
    },
    SchemaContract {
        name: "kiana.quality-transition.v1",
        version: SchemaVersion::new(1, 0),
        layer: SchemaLayer::Domain,
        owner_crate: "kiana-domain",
        compatibility: CompatibilityPolicy::Breaking,
        allow_unknown_fields: false,
    },
    SchemaContract {
        name: "kiana.quality-command.v1",
        version: SchemaVersion::new(1, 0),
        layer: SchemaLayer::Wire,
        owner_crate: "kiana-protocol",
        compatibility: CompatibilityPolicy::BackwardCompatible,
        allow_unknown_fields: false,
    },
    SchemaContract {
        name: "kiana.eval-dataset.v1",
        version: SchemaVersion::new(1, 0),
        layer: SchemaLayer::Domain,
        owner_crate: "kiana-domain",
        compatibility: CompatibilityPolicy::Breaking,
        allow_unknown_fields: false,
    },
    SchemaContract {
        name: "kiana.storage-root.v1",
        version: SchemaVersion::new(1, 0),
        layer: SchemaLayer::Domain,
        owner_crate: "kiana-domain",
        compatibility: CompatibilityPolicy::Breaking,
        allow_unknown_fields: false,
    },
    SchemaContract {
        name: "kiana.storage-owner-scope.v1",
        version: SchemaVersion::new(1, 0),
        layer: SchemaLayer::Domain,
        owner_crate: "kiana-domain",
        compatibility: CompatibilityPolicy::Breaking,
        allow_unknown_fields: false,
    },
    SchemaContract {
        name: "kiana.store-identity.v1",
        version: SchemaVersion::new(1, 0),
        layer: SchemaLayer::Domain,
        owner_crate: "kiana-domain",
        compatibility: CompatibilityPolicy::Breaking,
        allow_unknown_fields: false,
    },
    SchemaContract {
        name: "kiana.storage-lock.v1",
        version: SchemaVersion::new(1, 0),
        layer: SchemaLayer::Domain,
        owner_crate: "kiana-domain",
        compatibility: CompatibilityPolicy::Breaking,
        allow_unknown_fields: false,
    },
    SchemaContract {
        name: "kiana.resource-lease.v1",
        version: SchemaVersion::new(1, 0),
        layer: SchemaLayer::Domain,
        owner_crate: "kiana-domain",
        compatibility: CompatibilityPolicy::Breaking,
        allow_unknown_fields: false,
    },
    SchemaContract {
        name: "kiana.storage-schema-registry.v1",
        version: SchemaVersion::new(1, 0),
        layer: SchemaLayer::Domain,
        owner_crate: "kiana-domain",
        compatibility: CompatibilityPolicy::Breaking,
        allow_unknown_fields: false,
    },
    SchemaContract {
        name: "kiana.security-schema-registry.v1",
        version: SchemaVersion::new(1, 0),
        layer: SchemaLayer::Domain,
        owner_crate: "kiana-domain",
        compatibility: CompatibilityPolicy::Breaking,
        allow_unknown_fields: false,
    },
    SchemaContract {
        name: "kiana.security-object.v1",
        version: SchemaVersion::new(1, 0),
        layer: SchemaLayer::Domain,
        owner_crate: "kiana-domain",
        compatibility: CompatibilityPolicy::Breaking,
        allow_unknown_fields: false,
    },
    SchemaContract {
        name: "kiana.security-reason.v1",
        version: SchemaVersion::new(1, 0),
        layer: SchemaLayer::Domain,
        owner_crate: "kiana-domain",
        compatibility: CompatibilityPolicy::Breaking,
        allow_unknown_fields: false,
    },
    SchemaContract {
        name: "kiana.security-context.v1",
        version: SchemaVersion::new(1, 0),
        layer: SchemaLayer::Domain,
        owner_crate: "kiana-core",
        compatibility: CompatibilityPolicy::Breaking,
        allow_unknown_fields: false,
    },
    SchemaContract {
        name: "kiana.session-assertion.v1",
        version: SchemaVersion::new(1, 0),
        layer: SchemaLayer::Domain,
        owner_crate: "kiana-domain",
        compatibility: CompatibilityPolicy::Breaking,
        allow_unknown_fields: false,
    },
    SchemaContract {
        name: "kiana.project-trust-snapshot.v1",
        version: SchemaVersion::new(1, 0),
        layer: SchemaLayer::Domain,
        owner_crate: "kiana-domain",
        compatibility: CompatibilityPolicy::Breaking,
        allow_unknown_fields: false,
    },
    SchemaContract {
        name: "kiana.department-snapshot.v1",
        version: SchemaVersion::new(1, 0),
        layer: SchemaLayer::Domain,
        owner_crate: "kiana-domain",
        compatibility: CompatibilityPolicy::Breaking,
        allow_unknown_fields: false,
    },
    SchemaContract {
        name: "kiana.security-authority-snapshot.v1",
        version: SchemaVersion::new(1, 0),
        layer: SchemaLayer::Domain,
        owner_crate: "kiana-core",
        compatibility: CompatibilityPolicy::Breaking,
        allow_unknown_fields: false,
    },
    SchemaContract {
        name: "kiana.authority-fence.v1",
        version: SchemaVersion::new(1, 0),
        layer: SchemaLayer::RuntimeEvent,
        owner_crate: "kiana-domain",
        compatibility: CompatibilityPolicy::Breaking,
        allow_unknown_fields: false,
    },
    SchemaContract {
        name: "kiana.grant-authority.v1",
        version: SchemaVersion::new(1, 0),
        layer: SchemaLayer::Domain,
        owner_crate: "kiana-domain",
        compatibility: CompatibilityPolicy::Breaking,
        allow_unknown_fields: false,
    },
    SchemaContract {
        name: "kiana.grant-ledger-snapshot.v1",
        version: SchemaVersion::new(1, 0),
        layer: SchemaLayer::Projection,
        owner_crate: "kiana-domain",
        compatibility: CompatibilityPolicy::Breaking,
        allow_unknown_fields: false,
    },
    SchemaContract {
        name: "kiana.grant-scope.v1",
        version: SchemaVersion::new(1, 0),
        layer: SchemaLayer::Domain,
        owner_crate: "kiana-policy",
        compatibility: CompatibilityPolicy::Breaking,
        allow_unknown_fields: false,
    },
    SchemaContract {
        name: "kiana.budget-reservation-fact.v1",
        version: SchemaVersion::new(1, 0),
        layer: SchemaLayer::Domain,
        owner_crate: "kiana-domain",
        compatibility: CompatibilityPolicy::Breaking,
        allow_unknown_fields: false,
    },
    SchemaContract {
        name: "kiana.budget-settlement-fact.v1",
        version: SchemaVersion::new(1, 0),
        layer: SchemaLayer::Domain,
        owner_crate: "kiana-domain",
        compatibility: CompatibilityPolicy::Breaking,
        allow_unknown_fields: false,
    },
    SchemaContract {
        name: "kiana.approval-binding.v1",
        version: SchemaVersion::new(1, 0),
        layer: SchemaLayer::Domain,
        owner_crate: "kiana-core",
        compatibility: CompatibilityPolicy::Breaking,
        allow_unknown_fields: false,
    },
    SchemaContract {
        name: "kiana.invocation-resume-binding.v1",
        version: SchemaVersion::new(1, 0),
        layer: SchemaLayer::Domain,
        owner_crate: "kiana-domain",
        compatibility: CompatibilityPolicy::Breaking,
        allow_unknown_fields: false,
    },
    SchemaContract {
        name: "kiana.execution-output-ref.v1",
        version: SchemaVersion::new(1, 0),
        layer: SchemaLayer::Domain,
        owner_crate: "kiana-domain",
        compatibility: CompatibilityPolicy::Breaking,
        allow_unknown_fields: false,
    },
    SchemaContract {
        name: "kiana.execution-output-budget.v1",
        version: SchemaVersion::new(1, 0),
        layer: SchemaLayer::Domain,
        owner_crate: "kiana-domain",
        compatibility: CompatibilityPolicy::Breaking,
        allow_unknown_fields: false,
    },
    SchemaContract {
        name: "kiana.job-handle.v1",
        version: SchemaVersion::new(1, 0),
        layer: SchemaLayer::Domain,
        owner_crate: "kiana-domain",
        compatibility: CompatibilityPolicy::Breaking,
        allow_unknown_fields: false,
    },
    SchemaContract {
        name: "kiana.process-resource-budget.v1",
        version: SchemaVersion::new(1, 0),
        layer: SchemaLayer::Domain,
        owner_crate: "kiana-domain",
        compatibility: CompatibilityPolicy::Breaking,
        allow_unknown_fields: false,
    },
    SchemaContract {
        name: "kiana.stop-report.v1",
        version: SchemaVersion::new(1, 0),
        layer: SchemaLayer::Domain,
        owner_crate: "kiana-domain",
        compatibility: CompatibilityPolicy::Breaking,
        allow_unknown_fields: false,
    },
    SchemaContract {
        name: "kiana.input-receipt.v1",
        version: SchemaVersion::new(1, 0),
        layer: SchemaLayer::Projection,
        owner_crate: "kiana-domain",
        compatibility: CompatibilityPolicy::Breaking,
        allow_unknown_fields: false,
    },
    SchemaContract {
        name: "kiana.clarification-request.v1",
        version: SchemaVersion::new(1, 0),
        layer: SchemaLayer::Domain,
        owner_crate: "kiana-domain",
        compatibility: CompatibilityPolicy::Breaking,
        allow_unknown_fields: false,
    },
    SchemaContract {
        name: "kiana.clarification-answer.v1",
        version: SchemaVersion::new(1, 0),
        layer: SchemaLayer::Domain,
        owner_crate: "kiana-domain",
        compatibility: CompatibilityPolicy::Breaking,
        allow_unknown_fields: false,
    },
    SchemaContract {
        name: "kiana.clarification-resolution.v1",
        version: SchemaVersion::new(1, 0),
        layer: SchemaLayer::Domain,
        owner_crate: "kiana-domain",
        compatibility: CompatibilityPolicy::Breaking,
        allow_unknown_fields: false,
    },
    SchemaContract {
        name: "kiana.clarification-wait.v1",
        version: SchemaVersion::new(1, 0),
        layer: SchemaLayer::Projection,
        owner_crate: "kiana-domain",
        compatibility: CompatibilityPolicy::Breaking,
        allow_unknown_fields: false,
    },
    SchemaContract {
        name: "kiana.output-contract.v1",
        version: SchemaVersion::new(1, 0),
        layer: SchemaLayer::Domain,
        owner_crate: "kiana-domain",
        compatibility: CompatibilityPolicy::Breaking,
        allow_unknown_fields: false,
    },
    SchemaContract {
        name: "kiana.turn-outcome.v1",
        version: SchemaVersion::new(1, 0),
        layer: SchemaLayer::Domain,
        owner_crate: "kiana-domain",
        compatibility: CompatibilityPolicy::Breaking,
        allow_unknown_fields: false,
    },
    SchemaContract {
        name: "kiana.progress-evidence.v1",
        version: SchemaVersion::new(1, 0),
        layer: SchemaLayer::Domain,
        owner_crate: "kiana-domain",
        compatibility: CompatibilityPolicy::Breaking,
        allow_unknown_fields: false,
    },
    SchemaContract {
        name: "kiana.progress-tracker.v1",
        version: SchemaVersion::new(1, 0),
        layer: SchemaLayer::Domain,
        owner_crate: "kiana-domain",
        compatibility: CompatibilityPolicy::Breaking,
        allow_unknown_fields: false,
    },
    SchemaContract {
        name: "kiana.progress-decision.v1",
        version: SchemaVersion::new(1, 0),
        layer: SchemaLayer::Domain,
        owner_crate: "kiana-domain",
        compatibility: CompatibilityPolicy::Breaking,
        allow_unknown_fields: false,
    },
    SchemaContract {
        name: "kiana.hook-lifecycle-binding.v1",
        version: SchemaVersion::new(1, 0),
        layer: SchemaLayer::Domain,
        owner_crate: "kiana-domain",
        compatibility: CompatibilityPolicy::Breaking,
        allow_unknown_fields: false,
    },
    SchemaContract {
        name: "kiana.hook-lifecycle-plan.v1",
        version: SchemaVersion::new(1, 0),
        layer: SchemaLayer::Domain,
        owner_crate: "kiana-domain",
        compatibility: CompatibilityPolicy::Breaking,
        allow_unknown_fields: false,
    },
    SchemaContract {
        name: "kiana.hook-lifecycle-input.v1",
        version: SchemaVersion::new(1, 0),
        layer: SchemaLayer::Domain,
        owner_crate: "kiana-domain",
        compatibility: CompatibilityPolicy::Breaking,
        allow_unknown_fields: false,
    },
    SchemaContract {
        name: "kiana.hook-lifecycle-result.v1",
        version: SchemaVersion::new(1, 0),
        layer: SchemaLayer::Domain,
        owner_crate: "kiana-domain",
        compatibility: CompatibilityPolicy::Breaking,
        allow_unknown_fields: false,
    },
    SchemaContract {
        name: "kiana.retrieval-candidate.v1",
        version: SchemaVersion::new(1, 0),
        layer: SchemaLayer::Domain,
        owner_crate: "kiana-domain",
        compatibility: CompatibilityPolicy::Breaking,
        allow_unknown_fields: false,
    },
    SchemaContract {
        name: "kiana.retrieval-pack.v1",
        version: SchemaVersion::new(1, 0),
        layer: SchemaLayer::Domain,
        owner_crate: "kiana-domain",
        compatibility: CompatibilityPolicy::Breaking,
        allow_unknown_fields: false,
    },
    SchemaContract {
        name: "kiana.child-harness-intent.v1",
        version: SchemaVersion::new(1, 0),
        layer: SchemaLayer::Domain,
        owner_crate: "kiana-domain",
        compatibility: CompatibilityPolicy::Breaking,
        allow_unknown_fields: false,
    },
    SchemaContract {
        name: "kiana.child-harness-outcome.v1",
        version: SchemaVersion::new(1, 0),
        layer: SchemaLayer::Domain,
        owner_crate: "kiana-domain",
        compatibility: CompatibilityPolicy::Breaking,
        allow_unknown_fields: false,
    },
    SchemaContract {
        name: "kiana.child-harness-cancel.v1",
        version: SchemaVersion::new(1, 0),
        layer: SchemaLayer::Domain,
        owner_crate: "kiana-domain",
        compatibility: CompatibilityPolicy::Breaking,
        allow_unknown_fields: false,
    },
    SchemaContract {
        name: "kiana.resource-cleanup-plan.v1",
        version: SchemaVersion::new(1, 0),
        layer: SchemaLayer::Domain,
        owner_crate: "kiana-domain",
        compatibility: CompatibilityPolicy::Breaking,
        allow_unknown_fields: false,
    },
    SchemaContract {
        name: "kiana.resource-cleanup-report.v1",
        version: SchemaVersion::new(1, 0),
        layer: SchemaLayer::Domain,
        owner_crate: "kiana-domain",
        compatibility: CompatibilityPolicy::Breaking,
        allow_unknown_fields: false,
    },
    SchemaContract {
        name: "kiana.resource-retention.v1",
        version: SchemaVersion::new(1, 0),
        layer: SchemaLayer::Domain,
        owner_crate: "kiana-domain",
        compatibility: CompatibilityPolicy::Breaking,
        allow_unknown_fields: false,
    },
    SchemaContract {
        name: "kiana.legacy-compatibility.v1",
        version: SchemaVersion::new(1, 0),
        layer: SchemaLayer::Wire,
        owner_crate: "kiana-domain",
        compatibility: CompatibilityPolicy::Breaking,
        allow_unknown_fields: false,
    },
    SchemaContract {
        name: "kiana.legacy-checkpoint-decision.v1",
        version: SchemaVersion::new(1, 0),
        layer: SchemaLayer::Projection,
        owner_crate: "kiana-domain",
        compatibility: CompatibilityPolicy::Breaking,
        allow_unknown_fields: false,
    },
    SchemaContract {
        name: "kiana.approval-execution-material.v1",
        version: SchemaVersion::new(1, 0),
        layer: SchemaLayer::Domain,
        owner_crate: "kiana-domain",
        compatibility: CompatibilityPolicy::Breaking,
        allow_unknown_fields: false,
    },
    SchemaContract {
        name: "kiana.approval-plan-preview.v1",
        version: SchemaVersion::new(1, 0),
        layer: SchemaLayer::Projection,
        owner_crate: "kiana-domain",
        compatibility: CompatibilityPolicy::Breaking,
        allow_unknown_fields: false,
    },
    SchemaContract {
        name: "kiana.approval-decision-fact.v1",
        version: SchemaVersion::new(1, 0),
        layer: SchemaLayer::Domain,
        owner_crate: "kiana-domain",
        compatibility: CompatibilityPolicy::Breaking,
        allow_unknown_fields: false,
    },
    SchemaContract {
        name: "kiana.approval-consumption-fact.v1",
        version: SchemaVersion::new(1, 0),
        layer: SchemaLayer::Domain,
        owner_crate: "kiana-domain",
        compatibility: CompatibilityPolicy::Breaking,
        allow_unknown_fields: false,
    },
    SchemaContract {
        name: "kiana.human-inbox-item.v1",
        version: SchemaVersion::new(1, 0),
        layer: SchemaLayer::Projection,
        owner_crate: "kiana-core",
        compatibility: CompatibilityPolicy::Breaking,
        allow_unknown_fields: false,
    },
    SchemaContract {
        name: "kiana.entrypoint-command.v1",
        version: SchemaVersion::new(1, 0),
        layer: SchemaLayer::Wire,
        owner_crate: "kiana-core",
        compatibility: CompatibilityPolicy::Breaking,
        allow_unknown_fields: false,
    },
    SchemaContract {
        name: "kiana.entrypoint-parity-matrix.v1",
        version: SchemaVersion::new(1, 0),
        layer: SchemaLayer::Projection,
        owner_crate: "kiana-core",
        compatibility: CompatibilityPolicy::Breaking,
        allow_unknown_fields: false,
    },
    SchemaContract {
        name: "kiana.policy-bundle.v1",
        version: SchemaVersion::new(1, 0),
        layer: SchemaLayer::Domain,
        owner_crate: "kiana-policy",
        compatibility: CompatibilityPolicy::Breaking,
        allow_unknown_fields: false,
    },
    SchemaContract {
        name: "kiana.policy-revision.v1",
        version: SchemaVersion::new(1, 0),
        layer: SchemaLayer::Domain,
        owner_crate: "kiana-policy",
        compatibility: CompatibilityPolicy::Breaking,
        allow_unknown_fields: false,
    },
    SchemaContract {
        name: "kiana.policy-decision-trace.v1",
        version: SchemaVersion::new(1, 0),
        layer: SchemaLayer::Projection,
        owner_crate: "kiana-policy",
        compatibility: CompatibilityPolicy::Breaking,
        allow_unknown_fields: false,
    },
    SchemaContract {
        name: "kiana.storage-error.v1",
        version: SchemaVersion::new(1, 0),
        layer: SchemaLayer::Domain,
        owner_crate: "kiana-domain",
        compatibility: CompatibilityPolicy::Breaking,
        allow_unknown_fields: false,
    },
    SchemaContract {
        name: "kiana.storage-capabilities.v1",
        version: SchemaVersion::new(1, 0),
        layer: SchemaLayer::Domain,
        owner_crate: "kiana-domain",
        compatibility: CompatibilityPolicy::Breaking,
        allow_unknown_fields: false,
    },
    SchemaContract {
        name: "kiana.storage-health.v1",
        version: SchemaVersion::new(1, 0),
        layer: SchemaLayer::Projection,
        owner_crate: "kiana-domain",
        compatibility: CompatibilityPolicy::Breaking,
        allow_unknown_fields: false,
    },
    SchemaContract {
        name: "kiana.storage-integrity-incident.v1",
        version: SchemaVersion::new(1, 0),
        layer: SchemaLayer::Projection,
        owner_crate: "kiana-domain",
        compatibility: CompatibilityPolicy::Breaking,
        allow_unknown_fields: false,
    },
    SchemaContract {
        name: "kiana.principal.v1",
        version: SchemaVersion::new(1, 0),
        layer: SchemaLayer::Domain,
        owner_crate: "kiana-domain",
        compatibility: CompatibilityPolicy::BackwardCompatible,
        allow_unknown_fields: false,
    },
    SchemaContract {
        name: "kiana.membership.v1",
        version: SchemaVersion::new(1, 0),
        layer: SchemaLayer::Domain,
        owner_crate: "kiana-domain",
        compatibility: CompatibilityPolicy::BackwardCompatible,
        allow_unknown_fields: false,
    },
    SchemaContract {
        name: "kiana.secret-ref.v1",
        version: SchemaVersion::new(1, 0),
        layer: SchemaLayer::Domain,
        owner_crate: "kiana-domain",
        compatibility: CompatibilityPolicy::Breaking,
        allow_unknown_fields: false,
    },
    SchemaContract {
        name: "kiana.credential-lease.v1",
        version: SchemaVersion::new(1, 0),
        layer: SchemaLayer::Domain,
        owner_crate: "kiana-domain",
        compatibility: CompatibilityPolicy::Breaking,
        allow_unknown_fields: false,
    },
    SchemaContract {
        name: "kiana.clock-observation.v1",
        version: SchemaVersion::new(1, 0),
        layer: SchemaLayer::Domain,
        owner_crate: "kiana-domain",
        compatibility: CompatibilityPolicy::Breaking,
        allow_unknown_fields: false,
    },
    SchemaContract {
        name: "kiana.oauth-authorization.v1",
        version: SchemaVersion::new(1, 0),
        layer: SchemaLayer::Domain,
        owner_crate: "kiana-domain",
        compatibility: CompatibilityPolicy::Breaking,
        allow_unknown_fields: false,
    },
    SchemaContract {
        name: "kiana.oauth-callback.v1",
        version: SchemaVersion::new(1, 0),
        layer: SchemaLayer::Domain,
        owner_crate: "kiana-domain",
        compatibility: CompatibilityPolicy::Breaking,
        allow_unknown_fields: false,
    },
    SchemaContract {
        name: "kiana.oauth-token-metadata.v1",
        version: SchemaVersion::new(1, 0),
        layer: SchemaLayer::Domain,
        owner_crate: "kiana-domain",
        compatibility: CompatibilityPolicy::Breaking,
        allow_unknown_fields: false,
    },
    SchemaContract {
        name: "kiana.oauth-token-file.v1",
        version: SchemaVersion::new(1, 0),
        layer: SchemaLayer::Domain,
        owner_crate: "kiana-domain",
        compatibility: CompatibilityPolicy::Breaking,
        allow_unknown_fields: false,
    },
    SchemaContract {
        name: "kiana.provider-use-policy.v1",
        version: SchemaVersion::new(1, 0),
        layer: SchemaLayer::Domain,
        owner_crate: "kiana-policy",
        compatibility: CompatibilityPolicy::Breaking,
        allow_unknown_fields: false,
    },
    SchemaContract {
        name: "kiana.provider-use-policy-rule.v1",
        version: SchemaVersion::new(1, 0),
        layer: SchemaLayer::Domain,
        owner_crate: "kiana-policy",
        compatibility: CompatibilityPolicy::Breaking,
        allow_unknown_fields: false,
    },
    SchemaContract {
        name: "kiana.provider-use-policy-decision.v1",
        version: SchemaVersion::new(1, 0),
        layer: SchemaLayer::Projection,
        owner_crate: "kiana-policy",
        compatibility: CompatibilityPolicy::Breaking,
        allow_unknown_fields: false,
    },
    SchemaContract {
        name: "kiana.provider-credential-probe-request.v1",
        version: SchemaVersion::new(1, 0),
        layer: SchemaLayer::Wire,
        owner_crate: "kiana-protocol",
        compatibility: CompatibilityPolicy::Breaking,
        allow_unknown_fields: false,
    },
    SchemaContract {
        name: "kiana.provider-credential-probe-response.v1",
        version: SchemaVersion::new(1, 0),
        layer: SchemaLayer::Wire,
        owner_crate: "kiana-protocol",
        compatibility: CompatibilityPolicy::Breaking,
        allow_unknown_fields: false,
    },
    SchemaContract {
        name: "kiana.provider-use-policy-view.v1",
        version: SchemaVersion::new(1, 0),
        layer: SchemaLayer::Wire,
        owner_crate: "kiana-protocol",
        compatibility: CompatibilityPolicy::Breaking,
        allow_unknown_fields: false,
    },
    SchemaContract {
        name: "kiana.provider-account.v1",
        version: SchemaVersion::new(1, 0),
        layer: SchemaLayer::Domain,
        owner_crate: "kiana-domain",
        compatibility: CompatibilityPolicy::BackwardCompatible,
        allow_unknown_fields: false,
    },
    SchemaContract {
        name: "kiana.service-identity.v1",
        version: SchemaVersion::new(1, 0),
        layer: SchemaLayer::Domain,
        owner_crate: "kiana-domain",
        compatibility: CompatibilityPolicy::Breaking,
        allow_unknown_fields: false,
    },
    SchemaContract {
        name: "kiana.config-snapshot.v1",
        version: SchemaVersion::new(1, 0),
        layer: SchemaLayer::Domain,
        owner_crate: "kiana-domain",
        compatibility: CompatibilityPolicy::Breaking,
        allow_unknown_fields: false,
    },
    SchemaContract {
        name: "kiana.authority-snapshot.v1",
        version: SchemaVersion::new(1, 0),
        layer: SchemaLayer::Domain,
        owner_crate: "kiana-domain",
        compatibility: CompatibilityPolicy::Breaking,
        allow_unknown_fields: false,
    },
    SchemaContract {
        name: "kiana.identity-migration.v1",
        version: SchemaVersion::new(1, 0),
        layer: SchemaLayer::Domain,
        owner_crate: "kiana-domain",
        compatibility: CompatibilityPolicy::BackwardCompatible,
        allow_unknown_fields: false,
    },
    SchemaContract {
        name: "kiana.policy-profile.v1",
        version: SchemaVersion::new(1, 0),
        layer: SchemaLayer::Domain,
        owner_crate: "kiana-domain",
        compatibility: CompatibilityPolicy::Breaking,
        allow_unknown_fields: false,
    },
    SchemaContract {
        name: "kiana.data-boundary.v1",
        version: SchemaVersion::new(1, 0),
        layer: SchemaLayer::Domain,
        owner_crate: "kiana-domain",
        compatibility: CompatibilityPolicy::Breaking,
        allow_unknown_fields: false,
    },
    SchemaContract {
        name: "kiana.sharing-grant.v1",
        version: SchemaVersion::new(1, 0),
        layer: SchemaLayer::Domain,
        owner_crate: "kiana-domain",
        compatibility: CompatibilityPolicy::Breaking,
        allow_unknown_fields: false,
    },
    SchemaContract {
        name: "kiana.authority-ledger.v1",
        version: SchemaVersion::new(1, 0),
        layer: SchemaLayer::Domain,
        owner_crate: "kiana-domain",
        compatibility: CompatibilityPolicy::Breaking,
        allow_unknown_fields: false,
    },
    SchemaContract {
        name: "kiana.swarm-lineage.v1",
        version: SchemaVersion::new(1, 0),
        layer: SchemaLayer::Domain,
        owner_crate: "kiana-domain",
        compatibility: CompatibilityPolicy::Breaking,
        allow_unknown_fields: false,
    },
    SchemaContract {
        name: "kiana.swarm-partition.v1",
        version: SchemaVersion::new(1, 0),
        layer: SchemaLayer::Domain,
        owner_crate: "kiana-domain",
        compatibility: CompatibilityPolicy::Breaking,
        allow_unknown_fields: false,
    },
    SchemaContract {
        name: "kiana.swarm-work-graph.v1",
        version: SchemaVersion::new(1, 0),
        layer: SchemaLayer::Domain,
        owner_crate: "kiana-domain",
        compatibility: CompatibilityPolicy::Breaking,
        allow_unknown_fields: false,
    },
    SchemaContract {
        name: "kiana.swarm-transition-event.v1",
        version: SchemaVersion::new(1, 0),
        layer: SchemaLayer::RuntimeEvent,
        owner_crate: "kiana-domain",
        compatibility: CompatibilityPolicy::Breaking,
        allow_unknown_fields: false,
    },
    SchemaContract {
        name: "kiana.message.v1",
        version: SchemaVersion::new(1, 0),
        layer: SchemaLayer::Domain,
        owner_crate: "kiana-domain",
        compatibility: CompatibilityPolicy::BackwardCompatible,
        allow_unknown_fields: false,
    },
    SchemaContract {
        name: "kiana.notification.v1",
        version: SchemaVersion::new(1, 0),
        layer: SchemaLayer::Projection,
        owner_crate: "kiana-domain",
        compatibility: CompatibilityPolicy::Breaking,
        allow_unknown_fields: false,
    },
    SchemaContract {
        name: "kiana.notification-subscription.v1",
        version: SchemaVersion::new(1, 0),
        layer: SchemaLayer::Domain,
        owner_crate: "kiana-domain",
        compatibility: CompatibilityPolicy::Breaking,
        allow_unknown_fields: false,
    },
    SchemaContract {
        name: "kiana.notification-delivery-attempt.v1",
        version: SchemaVersion::new(1, 0),
        layer: SchemaLayer::RuntimeEvent,
        owner_crate: "kiana-domain",
        compatibility: CompatibilityPolicy::Breaking,
        allow_unknown_fields: false,
    },
    SchemaContract {
        name: "kiana.notification-action-ref.v1",
        version: SchemaVersion::new(1, 0),
        layer: SchemaLayer::Domain,
        owner_crate: "kiana-domain",
        compatibility: CompatibilityPolicy::Breaking,
        allow_unknown_fields: false,
    },
    SchemaContract {
        name: "kiana.notification-delivery-receipt.v1",
        version: SchemaVersion::new(1, 0),
        layer: SchemaLayer::Projection,
        owner_crate: "kiana-domain",
        compatibility: CompatibilityPolicy::Breaking,
        allow_unknown_fields: false,
    },
    SchemaContract {
        name: "kiana.tool-authority.v1",
        version: SchemaVersion::new(1, 0),
        layer: SchemaLayer::Domain,
        owner_crate: "kiana-domain",
        compatibility: CompatibilityPolicy::Breaking,
        allow_unknown_fields: false,
    },
    SchemaContract {
        name: "kiana.review-packet.v1",
        version: SchemaVersion::new(1, 0),
        layer: SchemaLayer::Domain,
        owner_crate: "kiana-domain",
        compatibility: CompatibilityPolicy::BackwardCompatible,
        allow_unknown_fields: false,
    },
    SchemaContract {
        name: "kiana.cell.v1",
        version: SchemaVersion::new(1, 0),
        layer: SchemaLayer::Domain,
        owner_crate: "kiana-domain",
        compatibility: CompatibilityPolicy::BackwardCompatible,
        allow_unknown_fields: false,
    },
    SchemaContract {
        name: "kiana.approval-challenge.v1",
        version: SchemaVersion::new(1, 0),
        layer: SchemaLayer::Domain,
        owner_crate: "kiana-domain",
        compatibility: CompatibilityPolicy::BackwardCompatible,
        allow_unknown_fields: false,
    },
    SchemaContract {
        name: "kiana.company-command.v1",
        version: SchemaVersion::new(1, 0),
        layer: SchemaLayer::Domain,
        owner_crate: "kiana-domain",
        compatibility: CompatibilityPolicy::BackwardCompatible,
        allow_unknown_fields: false,
    },
    SchemaContract {
        name: "kiana.company-event.v1",
        version: SchemaVersion::new(1, 0),
        layer: SchemaLayer::RuntimeEvent,
        owner_crate: "kiana-domain",
        compatibility: CompatibilityPolicy::BackwardCompatible,
        allow_unknown_fields: false,
    },
    SchemaContract {
        name: "kiana.company-state.v1",
        version: SchemaVersion::new(1, 0),
        layer: SchemaLayer::Domain,
        owner_crate: "kiana-domain",
        compatibility: CompatibilityPolicy::BackwardCompatible,
        allow_unknown_fields: false,
    },
    SchemaContract {
        name: "kiana.memory-record.v1",
        version: SchemaVersion::new(1, 0),
        layer: SchemaLayer::Domain,
        owner_crate: "kiana-domain",
        compatibility: CompatibilityPolicy::BackwardCompatible,
        allow_unknown_fields: false,
    },
    SchemaContract {
        name: "kiana.memory-record.v2",
        version: SchemaVersion::new(2, 0),
        layer: SchemaLayer::Domain,
        owner_crate: "kiana-domain",
        compatibility: CompatibilityPolicy::BackwardCompatible,
        allow_unknown_fields: false,
    },
    SchemaContract {
        name: "kiana.memory-mutation.v1",
        version: SchemaVersion::new(1, 0),
        layer: SchemaLayer::Domain,
        owner_crate: "kiana-domain",
        compatibility: CompatibilityPolicy::Breaking,
        allow_unknown_fields: false,
    },
    SchemaContract {
        name: "kiana.memory-mutation-receipt.v1",
        version: SchemaVersion::new(1, 0),
        layer: SchemaLayer::Projection,
        owner_crate: "kiana-domain",
        compatibility: CompatibilityPolicy::Breaking,
        allow_unknown_fields: false,
    },
    SchemaContract {
        name: "kiana.prompt-bundle.v1",
        version: SchemaVersion::new(1, 0),
        layer: SchemaLayer::Domain,
        owner_crate: "kiana-domain",
        compatibility: CompatibilityPolicy::BackwardCompatible,
        allow_unknown_fields: false,
    },
    SchemaContract {
        name: "kiana.memory-proposal.v1",
        version: SchemaVersion::new(1, 0),
        layer: SchemaLayer::Domain,
        owner_crate: "kiana-domain",
        compatibility: CompatibilityPolicy::BackwardCompatible,
        allow_unknown_fields: false,
    },
    SchemaContract {
        name: "kiana.skill-descriptor.v1",
        version: SchemaVersion::new(1, 0),
        layer: SchemaLayer::Domain,
        owner_crate: "kiana-domain",
        compatibility: CompatibilityPolicy::Breaking,
        allow_unknown_fields: false,
    },
    SchemaContract {
        name: "kiana.hook-descriptor.v1",
        version: SchemaVersion::new(1, 0),
        layer: SchemaLayer::Domain,
        owner_crate: "kiana-domain",
        compatibility: CompatibilityPolicy::Breaking,
        allow_unknown_fields: false,
    },
    SchemaContract {
        name: "kiana.hook-decision.v1",
        version: SchemaVersion::new(1, 0),
        layer: SchemaLayer::RuntimeEvent,
        owner_crate: "kiana-domain",
        compatibility: CompatibilityPolicy::Breaking,
        allow_unknown_fields: false,
    },
    SchemaContract {
        name: "kiana.plugin-lifecycle.v1",
        version: SchemaVersion::new(1, 0),
        layer: SchemaLayer::Domain,
        owner_crate: "kiana-domain",
        compatibility: CompatibilityPolicy::Breaking,
        allow_unknown_fields: false,
    },
    SchemaContract {
        name: "kiana.extension-snapshot.v1",
        version: SchemaVersion::new(1, 0),
        layer: SchemaLayer::Projection,
        owner_crate: "kiana-domain",
        compatibility: CompatibilityPolicy::Breaking,
        allow_unknown_fields: false,
    },
    SchemaContract {
        name: "kiana.extension-error.v1",
        version: SchemaVersion::new(1, 0),
        layer: SchemaLayer::Domain,
        owner_crate: "kiana-domain",
        compatibility: CompatibilityPolicy::Breaking,
        allow_unknown_fields: false,
    },
    SchemaContract {
        name: "kiana.extension-source-resolution.v1",
        version: SchemaVersion::new(1, 0),
        layer: SchemaLayer::Projection,
        owner_crate: "kiana-domain",
        compatibility: CompatibilityPolicy::Breaking,
        allow_unknown_fields: false,
    },
    SchemaContract {
        name: "kiana.extension-catalog.v1",
        version: SchemaVersion::new(1, 0),
        layer: SchemaLayer::Projection,
        owner_crate: "kiana-domain",
        compatibility: CompatibilityPolicy::Breaking,
        allow_unknown_fields: false,
    },
    SchemaContract {
        name: "kiana.plugin-manifest.v1",
        version: SchemaVersion::new(1, 0),
        layer: SchemaLayer::Domain,
        owner_crate: "kiana-skills",
        compatibility: CompatibilityPolicy::Breaking,
        allow_unknown_fields: false,
    },
    SchemaContract {
        name: "kiana.hook-manifest.v1",
        version: SchemaVersion::new(1, 0),
        layer: SchemaLayer::Domain,
        owner_crate: "kiana-skills",
        compatibility: CompatibilityPolicy::Breaking,
        allow_unknown_fields: false,
    },
    SchemaContract {
        name: "kiana.extension-snapshot-cache-key.v1",
        version: SchemaVersion::new(1, 0),
        layer: SchemaLayer::Projection,
        owner_crate: "kiana-skills",
        compatibility: CompatibilityPolicy::Breaking,
        allow_unknown_fields: false,
    },
    SchemaContract {
        name: "kiana.extension-snapshot-cache-entry.v1",
        version: SchemaVersion::new(1, 0),
        layer: SchemaLayer::Projection,
        owner_crate: "kiana-skills",
        compatibility: CompatibilityPolicy::Breaking,
        allow_unknown_fields: false,
    },
    SchemaContract {
        name: "kiana.ui-snapshot.v1",
        version: SchemaVersion::new(1, 0),
        layer: SchemaLayer::Projection,
        owner_crate: "kiana-protocol",
        compatibility: CompatibilityPolicy::Breaking,
        allow_unknown_fields: false,
    },
    SchemaContract {
        name: "kiana.ui-feed.v1",
        version: SchemaVersion::new(1, 0),
        layer: SchemaLayer::Wire,
        owner_crate: "kiana-protocol",
        compatibility: CompatibilityPolicy::Breaking,
        allow_unknown_fields: false,
    },
    SchemaContract {
        name: "kiana.ui-action.v1",
        version: SchemaVersion::new(1, 0),
        layer: SchemaLayer::Wire,
        owner_crate: "kiana-protocol",
        compatibility: CompatibilityPolicy::Breaking,
        allow_unknown_fields: false,
    },
    SchemaContract {
        name: "kiana.ui-action-result.v1",
        version: SchemaVersion::new(1, 0),
        layer: SchemaLayer::Wire,
        owner_crate: "kiana-protocol",
        compatibility: CompatibilityPolicy::Breaking,
        allow_unknown_fields: false,
    },
    SchemaContract {
        name: "kiana.ui-capability.v1",
        version: SchemaVersion::new(1, 0),
        layer: SchemaLayer::Wire,
        owner_crate: "kiana-protocol",
        compatibility: CompatibilityPolicy::Breaking,
        allow_unknown_fields: false,
    },
    SchemaContract {
        name: "kiana.ui-error.v1",
        version: SchemaVersion::new(1, 0),
        layer: SchemaLayer::Wire,
        owner_crate: "kiana-protocol",
        compatibility: CompatibilityPolicy::Breaking,
        allow_unknown_fields: false,
    },
    SchemaContract {
        name: "kiana.ui-handshake-request.v1",
        version: SchemaVersion::new(1, 0),
        layer: SchemaLayer::Wire,
        owner_crate: "kiana-protocol",
        compatibility: CompatibilityPolicy::Breaking,
        allow_unknown_fields: false,
    },
    SchemaContract {
        name: "kiana.ui-handshake-response.v1",
        version: SchemaVersion::new(1, 0),
        layer: SchemaLayer::Wire,
        owner_crate: "kiana-protocol",
        compatibility: CompatibilityPolicy::Breaking,
        allow_unknown_fields: false,
    },
    SchemaContract {
        name: "kiana.ui-health.v1",
        version: SchemaVersion::new(1, 0),
        layer: SchemaLayer::Wire,
        owner_crate: "kiana-protocol",
        compatibility: CompatibilityPolicy::Breaking,
        allow_unknown_fields: false,
    },
    SchemaContract {
        name: "kiana.ui-instance-record.v1",
        version: SchemaVersion::new(1, 0),
        layer: SchemaLayer::Wire,
        owner_crate: "kiana-protocol",
        compatibility: CompatibilityPolicy::Breaking,
        allow_unknown_fields: false,
    },
    SchemaContract {
        name: "kiana.company-scope.v1",
        version: SchemaVersion::new(1, 0),
        layer: SchemaLayer::Domain,
        owner_crate: "kiana-domain",
        compatibility: CompatibilityPolicy::Breaking,
        allow_unknown_fields: false,
    },
    SchemaContract {
        name: "kiana.company-organization-binding.v1",
        version: SchemaVersion::new(1, 0),
        layer: SchemaLayer::Domain,
        owner_crate: "kiana-domain",
        compatibility: CompatibilityPolicy::Breaking,
        allow_unknown_fields: false,
    },
    SchemaContract {
        name: "kiana.company-workspace-binding.v1",
        version: SchemaVersion::new(1, 0),
        layer: SchemaLayer::Domain,
        owner_crate: "kiana-domain",
        compatibility: CompatibilityPolicy::Breaking,
        allow_unknown_fields: false,
    },
    SchemaContract {
        name: "kiana.company-project-binding.v1",
        version: SchemaVersion::new(1, 0),
        layer: SchemaLayer::Domain,
        owner_crate: "kiana-domain",
        compatibility: CompatibilityPolicy::Breaking,
        allow_unknown_fields: false,
    },
    SchemaContract {
        name: "kiana.artifact-ref.v1",
        version: SchemaVersion::new(1, 0),
        layer: SchemaLayer::Projection,
        owner_crate: "kiana-domain",
        compatibility: CompatibilityPolicy::Breaking,
        allow_unknown_fields: false,
    },
    SchemaContract {
        name: "kiana.artifact-version.v1",
        version: SchemaVersion::new(1, 0),
        layer: SchemaLayer::Projection,
        owner_crate: "kiana-domain",
        compatibility: CompatibilityPolicy::Breaking,
        allow_unknown_fields: false,
    },
    SchemaContract {
        name: "kiana.evidence-ref.v1",
        version: SchemaVersion::new(1, 0),
        layer: SchemaLayer::Projection,
        owner_crate: "kiana-domain",
        compatibility: CompatibilityPolicy::Breaking,
        allow_unknown_fields: false,
    },
    SchemaContract {
        name: "kiana.criterion.v1",
        version: SchemaVersion::new(1, 0),
        layer: SchemaLayer::Domain,
        owner_crate: "kiana-domain",
        compatibility: CompatibilityPolicy::Breaking,
        allow_unknown_fields: false,
    },
    SchemaContract {
        name: "kiana.company-command-receipt.v1",
        version: SchemaVersion::new(1, 0),
        layer: SchemaLayer::Projection,
        owner_crate: "kiana-domain",
        compatibility: CompatibilityPolicy::Breaking,
        allow_unknown_fields: false,
    },
    SchemaContract {
        name: "kiana.company-dispatch-intent.v1",
        version: SchemaVersion::new(1, 0),
        layer: SchemaLayer::RuntimeEvent,
        owner_crate: "kiana-domain",
        compatibility: CompatibilityPolicy::Breaking,
        allow_unknown_fields: false,
    },
    SchemaContract {
        name: "kiana.provider-profile-snapshot.v1",
        version: SchemaVersion::new(1, 0),
        layer: SchemaLayer::Projection,
        owner_crate: "kiana-domain",
        compatibility: CompatibilityPolicy::Breaking,
        allow_unknown_fields: false,
    },
    SchemaContract {
        name: "kiana.provider-config-snapshot.v1",
        version: SchemaVersion::new(1, 0),
        layer: SchemaLayer::Projection,
        owner_crate: "kiana-domain",
        compatibility: CompatibilityPolicy::Breaking,
        allow_unknown_fields: false,
    },
    SchemaContract {
        name: "kiana.model-catalog.v1",
        version: SchemaVersion::new(1, 0),
        layer: SchemaLayer::Projection,
        owner_crate: "kiana-domain",
        compatibility: CompatibilityPolicy::Breaking,
        allow_unknown_fields: false,
    },
    SchemaContract {
        name: "kiana.model-catalog-entry.v1",
        version: SchemaVersion::new(1, 0),
        layer: SchemaLayer::Projection,
        owner_crate: "kiana-domain",
        compatibility: CompatibilityPolicy::Breaking,
        allow_unknown_fields: false,
    },
    SchemaContract {
        name: "kiana.company-command-policy.v1",
        version: SchemaVersion::new(1, 0),
        layer: SchemaLayer::Domain,
        owner_crate: "kiana-domain",
        compatibility: CompatibilityPolicy::Breaking,
        allow_unknown_fields: false,
    },
    SchemaContract {
        name: "kiana.human-task.v1",
        version: SchemaVersion::new(1, 0),
        layer: SchemaLayer::Domain,
        owner_crate: "kiana-domain",
        compatibility: CompatibilityPolicy::Breaking,
        allow_unknown_fields: false,
    },
    SchemaContract {
        name: "kiana.human-decision.v1",
        version: SchemaVersion::new(1, 0),
        layer: SchemaLayer::Domain,
        owner_crate: "kiana-domain",
        compatibility: CompatibilityPolicy::Breaking,
        allow_unknown_fields: false,
    },
    SchemaContract {
        name: "kiana.role-spec.v1",
        version: SchemaVersion::new(1, 0),
        layer: SchemaLayer::Domain,
        owner_crate: "kiana-domain",
        compatibility: CompatibilityPolicy::Breaking,
        allow_unknown_fields: false,
    },
    SchemaContract {
        name: "kiana.role-catalog.v1",
        version: SchemaVersion::new(1, 0),
        layer: SchemaLayer::Projection,
        owner_crate: "kiana-domain",
        compatibility: CompatibilityPolicy::Breaking,
        allow_unknown_fields: false,
    },
    SchemaContract {
        name: "kiana.department-spec.v1",
        version: SchemaVersion::new(1, 0),
        layer: SchemaLayer::Domain,
        owner_crate: "kiana-domain",
        compatibility: CompatibilityPolicy::Breaking,
        allow_unknown_fields: false,
    },
    SchemaContract {
        name: "kiana.department-catalog.v1",
        version: SchemaVersion::new(1, 0),
        layer: SchemaLayer::Projection,
        owner_crate: "kiana-domain",
        compatibility: CompatibilityPolicy::Breaking,
        allow_unknown_fields: false,
    },
    SchemaContract {
        name: "kiana.role-assignment.v1",
        version: SchemaVersion::new(1, 0),
        layer: SchemaLayer::Domain,
        owner_crate: "kiana-domain",
        compatibility: CompatibilityPolicy::Breaking,
        allow_unknown_fields: false,
    },
    SchemaContract {
        name: "kiana.project-assignment.v1",
        version: SchemaVersion::new(1, 0),
        layer: SchemaLayer::Domain,
        owner_crate: "kiana-domain",
        compatibility: CompatibilityPolicy::Breaking,
        allow_unknown_fields: false,
    },
    SchemaContract {
        name: "kiana.resolved-assignment.v1",
        version: SchemaVersion::new(1, 0),
        layer: SchemaLayer::Projection,
        owner_crate: "kiana-domain",
        compatibility: CompatibilityPolicy::Breaking,
        allow_unknown_fields: false,
    },
    SchemaContract {
        name: "kiana.data-policy.v1",
        version: SchemaVersion::new(1, 0),
        layer: SchemaLayer::Domain,
        owner_crate: "kiana-domain",
        compatibility: CompatibilityPolicy::BackwardCompatible,
        allow_unknown_fields: false,
    },
    SchemaContract {
        name: "kiana.data-governance-snapshot.v1",
        version: SchemaVersion::new(1, 0),
        layer: SchemaLayer::Projection,
        owner_crate: "kiana-domain",
        compatibility: CompatibilityPolicy::BackwardCompatible,
        allow_unknown_fields: false,
    },
    SchemaContract {
        name: "kiana.golden-trace.v1",
        version: SchemaVersion::new(1, 0),
        layer: SchemaLayer::Projection,
        owner_crate: "kiana-domain",
        compatibility: CompatibilityPolicy::Breaking,
        allow_unknown_fields: false,
    },
    SchemaContract {
        name: "kiana.observability.v1",
        version: SchemaVersion::new(1, 0),
        layer: SchemaLayer::Projection,
        owner_crate: "kiana-domain",
        compatibility: CompatibilityPolicy::BackwardCompatible,
        allow_unknown_fields: false,
    },
    SchemaContract {
        name: "kiana.audit-record.v1",
        version: SchemaVersion::new(1, 0),
        layer: SchemaLayer::Projection,
        owner_crate: "kiana-domain",
        compatibility: CompatibilityPolicy::BackwardCompatible,
        allow_unknown_fields: false,
    },
    SchemaContract {
        name: "kiana.audit-projection.v1",
        version: SchemaVersion::new(1, 0),
        layer: SchemaLayer::Projection,
        owner_crate: "kiana-domain",
        compatibility: CompatibilityPolicy::BackwardCompatible,
        allow_unknown_fields: false,
    },
    SchemaContract {
        name: "kiana.audit-projection-checkpoint.v1",
        version: SchemaVersion::new(1, 0),
        layer: SchemaLayer::Projection,
        owner_crate: "kiana-domain",
        compatibility: CompatibilityPolicy::BackwardCompatible,
        allow_unknown_fields: false,
    },
    SchemaContract {
        name: "kiana.credential-recovery-projection.v1",
        version: SchemaVersion::new(1, 0),
        layer: SchemaLayer::Projection,
        owner_crate: "kiana-domain",
        compatibility: CompatibilityPolicy::BackwardCompatible,
        allow_unknown_fields: false,
    },
    SchemaContract {
        name: "kiana.credential-recovery-event.v1",
        version: SchemaVersion::new(1, 0),
        layer: SchemaLayer::RuntimeEvent,
        owner_crate: "kiana-domain",
        compatibility: CompatibilityPolicy::Breaking,
        allow_unknown_fields: false,
    },
    SchemaContract {
        name: "kiana.audit-query.v1",
        version: SchemaVersion::new(1, 0),
        layer: SchemaLayer::Wire,
        owner_crate: "kiana-protocol",
        compatibility: CompatibilityPolicy::BackwardCompatible,
        allow_unknown_fields: false,
    },
    SchemaContract {
        name: "kiana.audit-query-cursor.v1",
        version: SchemaVersion::new(1, 0),
        layer: SchemaLayer::Wire,
        owner_crate: "kiana-domain",
        compatibility: CompatibilityPolicy::BackwardCompatible,
        allow_unknown_fields: false,
    },
    SchemaContract {
        name: "kiana.audit-export.v1",
        version: SchemaVersion::new(1, 0),
        layer: SchemaLayer::Wire,
        owner_crate: "kiana-protocol",
        compatibility: CompatibilityPolicy::BackwardCompatible,
        allow_unknown_fields: false,
    },
    SchemaContract {
        name: "kiana.audit-delivery-receipt.v1",
        version: SchemaVersion::new(1, 0),
        layer: SchemaLayer::Projection,
        owner_crate: "kiana-domain",
        compatibility: CompatibilityPolicy::BackwardCompatible,
        allow_unknown_fields: false,
    },
    SchemaContract {
        name: "kiana.observability-alert.v1",
        version: SchemaVersion::new(1, 0),
        layer: SchemaLayer::Projection,
        owner_crate: "kiana-domain",
        compatibility: CompatibilityPolicy::BackwardCompatible,
        allow_unknown_fields: false,
    },
    SchemaContract {
        name: "kiana.observability-incident.v1",
        version: SchemaVersion::new(1, 0),
        layer: SchemaLayer::Projection,
        owner_crate: "kiana-domain",
        compatibility: CompatibilityPolicy::BackwardCompatible,
        allow_unknown_fields: false,
    },
    SchemaContract {
        name: "kiana.observability-incident-snapshot.v1",
        version: SchemaVersion::new(1, 0),
        layer: SchemaLayer::Projection,
        owner_crate: "kiana-domain",
        compatibility: CompatibilityPolicy::BackwardCompatible,
        allow_unknown_fields: false,
    },
    SchemaContract {
        name: "kiana.replay-diagnostic.v1",
        version: SchemaVersion::new(1, 0),
        layer: SchemaLayer::Projection,
        owner_crate: "kiana-domain",
        compatibility: CompatibilityPolicy::BackwardCompatible,
        allow_unknown_fields: false,
    },
    SchemaContract {
        name: "kiana.replay-diagnostic-snapshot.v1",
        version: SchemaVersion::new(1, 0),
        layer: SchemaLayer::Projection,
        owner_crate: "kiana-domain",
        compatibility: CompatibilityPolicy::BackwardCompatible,
        allow_unknown_fields: false,
    },
    SchemaContract {
        name: "kiana.fault-case.v1",
        version: SchemaVersion::new(1, 0),
        layer: SchemaLayer::Projection,
        owner_crate: "kiana-domain",
        compatibility: CompatibilityPolicy::BackwardCompatible,
        allow_unknown_fields: false,
    },
    SchemaContract {
        name: "kiana.eval-case.v1",
        version: SchemaVersion::new(1, 0),
        layer: SchemaLayer::Projection,
        owner_crate: "kiana-domain",
        compatibility: CompatibilityPolicy::BackwardCompatible,
        allow_unknown_fields: false,
    },
    SchemaContract {
        name: "kiana.eval-result.v1",
        version: SchemaVersion::new(1, 0),
        layer: SchemaLayer::Projection,
        owner_crate: "kiana-domain",
        compatibility: CompatibilityPolicy::BackwardCompatible,
        allow_unknown_fields: false,
    },
    SchemaContract {
        name: "kiana.eval-suite.v1",
        version: SchemaVersion::new(1, 0),
        layer: SchemaLayer::Projection,
        owner_crate: "kiana-domain",
        compatibility: CompatibilityPolicy::BackwardCompatible,
        allow_unknown_fields: false,
    },
    SchemaContract {
        name: "kiana.harness-trace-binding.v1",
        version: SchemaVersion::new(1, 0),
        layer: SchemaLayer::Projection,
        owner_crate: "kiana-domain",
        compatibility: CompatibilityPolicy::BackwardCompatible,
        allow_unknown_fields: false,
    },
    SchemaContract {
        name: "kiana.harness-replay-report.v1",
        version: SchemaVersion::new(1, 0),
        layer: SchemaLayer::Projection,
        owner_crate: "kiana-domain",
        compatibility: CompatibilityPolicy::BackwardCompatible,
        allow_unknown_fields: false,
    },
    SchemaContract {
        name: "kiana.harness-metric-set.v1",
        version: SchemaVersion::new(1, 0),
        layer: SchemaLayer::Projection,
        owner_crate: "kiana-domain",
        compatibility: CompatibilityPolicy::BackwardCompatible,
        allow_unknown_fields: false,
    },
    SchemaContract {
        name: "kiana.harness-eval-comparison.v1",
        version: SchemaVersion::new(1, 0),
        layer: SchemaLayer::Projection,
        owner_crate: "kiana-domain",
        compatibility: CompatibilityPolicy::BackwardCompatible,
        allow_unknown_fields: false,
    },
    SchemaContract {
        name: "kiana.entrypoint-parity.v1",
        version: SchemaVersion::new(1, 0),
        layer: SchemaLayer::Projection,
        owner_crate: "kiana-domain",
        compatibility: CompatibilityPolicy::BackwardCompatible,
        allow_unknown_fields: false,
    },
    SchemaContract {
        name: "kiana.performance-baseline.v1",
        version: SchemaVersion::new(1, 0),
        layer: SchemaLayer::Projection,
        owner_crate: "kiana-domain",
        compatibility: CompatibilityPolicy::BackwardCompatible,
        allow_unknown_fields: false,
    },
    SchemaContract {
        name: "kiana.migration-observation.v1",
        version: SchemaVersion::new(1, 0),
        layer: SchemaLayer::Projection,
        owner_crate: "kiana-domain",
        compatibility: CompatibilityPolicy::BackwardCompatible,
        allow_unknown_fields: false,
    },
    SchemaContract {
        name: "kiana.company-governance.v1",
        version: SchemaVersion::new(1, 0),
        layer: SchemaLayer::Projection,
        owner_crate: "kiana-domain",
        compatibility: CompatibilityPolicy::BackwardCompatible,
        allow_unknown_fields: false,
    },
    SchemaContract {
        name: "kiana.live-handoff.v1",
        version: SchemaVersion::new(1, 0),
        layer: SchemaLayer::Projection,
        owner_crate: "kiana-domain",
        compatibility: CompatibilityPolicy::BackwardCompatible,
        allow_unknown_fields: false,
    },
    SchemaContract {
        name: "kiana.authenticated-principal.v1",
        version: SchemaVersion::new(1, 0),
        layer: SchemaLayer::Domain,
        owner_crate: "kiana-domain",
        compatibility: CompatibilityPolicy::BackwardCompatible,
        allow_unknown_fields: false,
    },
    SchemaContract {
        name: "kiana.project-identity.v1",
        version: SchemaVersion::new(1, 0),
        layer: SchemaLayer::Domain,
        owner_crate: "kiana-domain",
        compatibility: CompatibilityPolicy::BackwardCompatible,
        allow_unknown_fields: false,
    },
    SchemaContract {
        name: "kiana.session-assignment.v1",
        version: SchemaVersion::new(1, 0),
        layer: SchemaLayer::Domain,
        owner_crate: "kiana-domain",
        compatibility: CompatibilityPolicy::BackwardCompatible,
        allow_unknown_fields: false,
    },
    SchemaContract {
        name: "kiana.turn-identity.v1",
        version: SchemaVersion::new(1, 0),
        layer: SchemaLayer::RuntimeEvent,
        owner_crate: "kiana-domain",
        compatibility: CompatibilityPolicy::BackwardCompatible,
        allow_unknown_fields: false,
    },
    SchemaContract {
        name: "kiana.invocation-identity.v1",
        version: SchemaVersion::new(1, 0),
        layer: SchemaLayer::RuntimeEvent,
        owner_crate: "kiana-domain",
        compatibility: CompatibilityPolicy::BackwardCompatible,
        allow_unknown_fields: false,
    },
    SchemaContract {
        name: "kiana.step-identity.v1",
        version: SchemaVersion::new(1, 0),
        layer: SchemaLayer::RuntimeEvent,
        owner_crate: "kiana-domain",
        compatibility: CompatibilityPolicy::BackwardCompatible,
        allow_unknown_fields: false,
    },
    SchemaContract {
        name: "kiana.model-attempt-identity.v1",
        version: SchemaVersion::new(1, 0),
        layer: SchemaLayer::RuntimeEvent,
        owner_crate: "kiana-domain",
        compatibility: CompatibilityPolicy::BackwardCompatible,
        allow_unknown_fields: false,
    },
    SchemaContract {
        name: "kiana.model-content.v1",
        version: SchemaVersion::new(1, 0),
        layer: SchemaLayer::Domain,
        owner_crate: "kiana-domain",
        compatibility: CompatibilityPolicy::BackwardCompatible,
        allow_unknown_fields: false,
    },
    SchemaContract {
        name: "kiana.provider-continuation.v1",
        version: SchemaVersion::new(1, 0),
        layer: SchemaLayer::Domain,
        owner_crate: "kiana-domain",
        compatibility: CompatibilityPolicy::BackwardCompatible,
        allow_unknown_fields: false,
    },
    SchemaContract {
        name: "kiana.model-outcome.v1",
        version: SchemaVersion::new(1, 0),
        layer: SchemaLayer::Projection,
        owner_crate: "kiana-domain",
        compatibility: CompatibilityPolicy::BackwardCompatible,
        allow_unknown_fields: false,
    },
    SchemaContract {
        name: "kiana.source-ref.v1",
        version: SchemaVersion::new(1, 0),
        layer: SchemaLayer::Domain,
        owner_crate: "kiana-domain",
        compatibility: CompatibilityPolicy::BackwardCompatible,
        allow_unknown_fields: false,
    },
    SchemaContract {
        name: "kiana.source-snapshot.v1",
        version: SchemaVersion::new(1, 0),
        layer: SchemaLayer::Projection,
        owner_crate: "kiana-domain",
        compatibility: CompatibilityPolicy::BackwardCompatible,
        allow_unknown_fields: false,
    },
    SchemaContract {
        name: "kiana.memory-scope.v1",
        version: SchemaVersion::new(1, 0),
        layer: SchemaLayer::Domain,
        owner_crate: "kiana-domain",
        compatibility: CompatibilityPolicy::BackwardCompatible,
        allow_unknown_fields: false,
    },
    SchemaContract {
        name: "kiana.action-catalog.v1",
        version: crate::ACTION_CATALOG_SCHEMA_VERSION,
        layer: SchemaLayer::Domain,
        owner_crate: "kiana-domain",
        compatibility: CompatibilityPolicy::BackwardCompatible,
        allow_unknown_fields: false,
    },
    SchemaContract {
        name: "kiana.scope-set.v1",
        version: crate::SCOPE_SET_SCHEMA_VERSION,
        layer: SchemaLayer::Domain,
        owner_crate: "kiana-domain",
        compatibility: CompatibilityPolicy::BackwardCompatible,
        allow_unknown_fields: false,
    },
    SchemaContract {
        name: "kiana.journal-header.v2",
        version: SchemaVersion::new(2, 0),
        layer: SchemaLayer::RuntimeEvent,
        owner_crate: "kiana-domain",
        compatibility: CompatibilityPolicy::Breaking,
        allow_unknown_fields: false,
    },
    SchemaContract {
        name: "kiana.event-store-health.v1",
        version: SchemaVersion::new(1, 0),
        layer: SchemaLayer::RuntimeEvent,
        owner_crate: "kiana-domain",
        compatibility: CompatibilityPolicy::Breaking,
        allow_unknown_fields: false,
    },
    SchemaContract {
        name: "kiana.projection-checkpoint.v1",
        version: SchemaVersion::new(1, 0),
        layer: SchemaLayer::Projection,
        owner_crate: "kiana-domain",
        compatibility: CompatibilityPolicy::Breaking,
        allow_unknown_fields: false,
    },
    SchemaContract {
        name: "kiana.recovery-resource-snapshot.v1",
        version: SchemaVersion::new(1, 0),
        layer: SchemaLayer::Projection,
        owner_crate: "kiana-domain",
        compatibility: CompatibilityPolicy::Breaking,
        allow_unknown_fields: false,
    },
    SchemaContract {
        name: "kiana.run-receipt.v1",
        version: SchemaVersion::new(1, 0),
        layer: SchemaLayer::Projection,
        owner_crate: "kiana-domain",
        compatibility: CompatibilityPolicy::Breaking,
        allow_unknown_fields: false,
    },
    SchemaContract {
        name: "kiana.execution-receipt.v1",
        version: SchemaVersion::new(1, 0),
        layer: SchemaLayer::Projection,
        owner_crate: "kiana-domain",
        compatibility: CompatibilityPolicy::Breaking,
        allow_unknown_fields: false,
    },
    SchemaContract {
        name: "kiana.receipt-aggregation.v1",
        version: SchemaVersion::new(1, 0),
        layer: SchemaLayer::Projection,
        owner_crate: "kiana-domain",
        compatibility: CompatibilityPolicy::Breaking,
        allow_unknown_fields: false,
    },
    SchemaContract {
        name: "kiana.effect-observation.v1",
        version: SchemaVersion::new(1, 0),
        layer: SchemaLayer::Projection,
        owner_crate: "kiana-domain",
        compatibility: CompatibilityPolicy::Breaking,
        allow_unknown_fields: false,
    },
    SchemaContract {
        name: "kiana.adapter-result.v1",
        version: SchemaVersion::new(1, 0),
        layer: SchemaLayer::Projection,
        owner_crate: "kiana-domain",
        compatibility: CompatibilityPolicy::Breaking,
        allow_unknown_fields: false,
    },
    SchemaContract {
        name: "kiana.tool-observation.v1",
        version: SchemaVersion::new(1, 0),
        layer: SchemaLayer::Projection,
        owner_crate: "kiana-domain",
        compatibility: CompatibilityPolicy::Breaking,
        allow_unknown_fields: false,
    },
    SchemaContract {
        name: "kiana.transition-frame.v1",
        version: SchemaVersion::new(1, 0),
        layer: SchemaLayer::RuntimeEvent,
        owner_crate: "kiana-domain",
        compatibility: CompatibilityPolicy::BackwardCompatible,
        allow_unknown_fields: false,
    },
    SchemaContract {
        name: "kiana.runtime-event.v1",
        version: crate::RUNTIME_EVENT_SCHEMA_VERSION,
        layer: SchemaLayer::RuntimeEvent,
        owner_crate: "kiana-domain",
        compatibility: CompatibilityPolicy::BackwardCompatible,
        allow_unknown_fields: false,
    },
    SchemaContract {
        name: "kiana.command-receipt.v1",
        version: SchemaVersion::new(1, 0),
        layer: SchemaLayer::Projection,
        owner_crate: "kiana-domain",
        compatibility: CompatibilityPolicy::BackwardCompatible,
        allow_unknown_fields: false,
    },
    SchemaContract {
        name: "kiana.execution-scope.v1",
        version: crate::EXECUTION_SCOPE_SCHEMA_VERSION,
        layer: SchemaLayer::Domain,
        owner_crate: "kiana-domain",
        compatibility: CompatibilityPolicy::BackwardCompatible,
        allow_unknown_fields: false,
    },
    SchemaContract {
        name: "kiana.dispatch-permit.v1",
        version: SchemaVersion::new(1, 0),
        layer: SchemaLayer::RuntimeEvent,
        owner_crate: "kiana-domain",
        compatibility: CompatibilityPolicy::Breaking,
        allow_unknown_fields: false,
    },
    SchemaContract {
        name: "kiana.run-cancellation-fact.v1",
        version: SchemaVersion::new(1, 0),
        layer: SchemaLayer::RuntimeEvent,
        owner_crate: "kiana-domain",
        compatibility: CompatibilityPolicy::Breaking,
        allow_unknown_fields: false,
    },
    SchemaContract {
        name: "kiana.capability-result-dimensions.v1",
        version: SchemaVersion::new(1, 0),
        layer: SchemaLayer::Domain,
        owner_crate: "kiana-domain",
        compatibility: CompatibilityPolicy::BackwardCompatible,
        allow_unknown_fields: false,
    },
    SchemaContract {
        name: "kiana.capability-result-receipt.v1",
        version: SchemaVersion::new(1, 0),
        layer: SchemaLayer::Projection,
        owner_crate: "kiana-domain",
        compatibility: CompatibilityPolicy::Breaking,
        allow_unknown_fields: false,
    },
    SchemaContract {
        name: "kiana.capability-outcome.v1",
        version: SchemaVersion::new(1, 0),
        layer: SchemaLayer::Projection,
        owner_crate: "kiana-domain",
        compatibility: CompatibilityPolicy::BackwardCompatible,
        allow_unknown_fields: true,
    },
    SchemaContract {
        name: "kiana.fault-matrix.v1",
        version: SchemaVersion::new(1, 0),
        layer: SchemaLayer::Projection,
        owner_crate: "kiana-domain",
        compatibility: CompatibilityPolicy::BackwardCompatible,
        allow_unknown_fields: false,
    },
    SchemaContract {
        name: "kiana.metric-catalog.v1",
        version: SchemaVersion::new(1, 0),
        layer: SchemaLayer::Projection,
        owner_crate: "kiana-domain",
        compatibility: CompatibilityPolicy::BackwardCompatible,
        allow_unknown_fields: false,
    },
    SchemaContract {
        name: "kiana.metric-snapshot.v1",
        version: SchemaVersion::new(1, 0),
        layer: SchemaLayer::Projection,
        owner_crate: "kiana-domain",
        compatibility: CompatibilityPolicy::BackwardCompatible,
        allow_unknown_fields: false,
    },
    SchemaContract {
        name: "kiana.trace-summary.v1",
        version: SchemaVersion::new(1, 0),
        layer: SchemaLayer::Projection,
        owner_crate: "kiana-domain",
        compatibility: CompatibilityPolicy::BackwardCompatible,
        allow_unknown_fields: false,
    },
    SchemaContract {
        name: "kiana.trace-export-span.v1",
        version: SchemaVersion::new(1, 0),
        layer: SchemaLayer::Projection,
        owner_crate: "kiana-domain",
        compatibility: CompatibilityPolicy::BackwardCompatible,
        allow_unknown_fields: false,
    },
    SchemaContract {
        name: "kiana.model-attempt.v1",
        version: SchemaVersion::new(1, 0),
        layer: SchemaLayer::Projection,
        owner_crate: "kiana-domain",
        compatibility: CompatibilityPolicy::BackwardCompatible,
        allow_unknown_fields: false,
    },
    SchemaContract {
        name: "kiana.capability-attempt.v1",
        version: SchemaVersion::new(1, 0),
        layer: SchemaLayer::Projection,
        owner_crate: "kiana-domain",
        compatibility: CompatibilityPolicy::BackwardCompatible,
        allow_unknown_fields: false,
    },
    SchemaContract {
        name: "kiana.correlation-context.v1",
        version: SchemaVersion::new(1, 0),
        layer: SchemaLayer::Projection,
        owner_crate: "kiana-domain",
        compatibility: CompatibilityPolicy::BackwardCompatible,
        allow_unknown_fields: false,
    },
    SchemaContract {
        name: "kiana.redaction-profile.v1",
        version: SchemaVersion::new(1, 0),
        layer: SchemaLayer::Projection,
        owner_crate: "kiana-domain",
        compatibility: CompatibilityPolicy::BackwardCompatible,
        allow_unknown_fields: false,
    },
    SchemaContract {
        name: "kiana.health-snapshot.v1",
        version: SchemaVersion::new(1, 0),
        layer: SchemaLayer::Projection,
        owner_crate: "kiana-domain",
        compatibility: CompatibilityPolicy::BackwardCompatible,
        allow_unknown_fields: false,
    },
    SchemaContract {
        name: "kiana.span-lifecycle.v1",
        version: SchemaVersion::new(1, 0),
        layer: SchemaLayer::Projection,
        owner_crate: "kiana-domain",
        compatibility: CompatibilityPolicy::BackwardCompatible,
        allow_unknown_fields: false,
    },
    SchemaContract {
        name: "kiana.workflow-command.v1",
        version: SchemaVersion::new(1, 0),
        layer: SchemaLayer::Domain,
        owner_crate: "kiana-domain",
        compatibility: CompatibilityPolicy::BackwardCompatible,
        allow_unknown_fields: false,
    },
    SchemaContract {
        name: "kiana.workflow-definition.v1",
        version: SchemaVersion::new(1, 0),
        layer: SchemaLayer::Domain,
        owner_crate: "kiana-domain",
        compatibility: CompatibilityPolicy::Breaking,
        allow_unknown_fields: false,
    },
    SchemaContract {
        name: "kiana.trigger-definition.v1",
        version: SchemaVersion::new(1, 0),
        layer: SchemaLayer::Domain,
        owner_crate: "kiana-domain",
        compatibility: CompatibilityPolicy::Breaking,
        allow_unknown_fields: false,
    },
    SchemaContract {
        name: "kiana.workflow-event-envelope.v1",
        version: SchemaVersion::new(1, 0),
        layer: SchemaLayer::RuntimeEvent,
        owner_crate: "kiana-domain",
        compatibility: CompatibilityPolicy::Breaking,
        allow_unknown_fields: false,
    },
    SchemaContract {
        name: "kiana.workflow-plan-intent.v1",
        version: SchemaVersion::new(1, 0),
        layer: SchemaLayer::Domain,
        owner_crate: "kiana-domain",
        compatibility: CompatibilityPolicy::Breaking,
        allow_unknown_fields: false,
    },
    SchemaContract {
        name: "kiana.workflow-event.v1",
        version: SchemaVersion::new(1, 0),
        layer: SchemaLayer::RuntimeEvent,
        owner_crate: "kiana-domain",
        compatibility: CompatibilityPolicy::BackwardCompatible,
        allow_unknown_fields: false,
    },
    SchemaContract {
        name: "kiana.workflow-state.v1",
        version: SchemaVersion::new(1, 0),
        layer: SchemaLayer::Domain,
        owner_crate: "kiana-domain",
        compatibility: CompatibilityPolicy::BackwardCompatible,
        allow_unknown_fields: false,
    },
    SchemaContract {
        name: "kiana.swarm-command.v1",
        version: SchemaVersion::new(1, 0),
        layer: SchemaLayer::Domain,
        owner_crate: "kiana-domain",
        compatibility: CompatibilityPolicy::BackwardCompatible,
        allow_unknown_fields: false,
    },
    SchemaContract {
        name: "kiana.extension-manifest.v1",
        version: SchemaVersion::new(1, 0),
        layer: SchemaLayer::Domain,
        owner_crate: "kiana-domain",
        compatibility: CompatibilityPolicy::BackwardCompatible,
        allow_unknown_fields: false,
    },
    SchemaContract {
        name: "kiana.extension-package.v1",
        version: SchemaVersion::new(1, 0),
        layer: SchemaLayer::Domain,
        owner_crate: "kiana-domain",
        compatibility: CompatibilityPolicy::BackwardCompatible,
        allow_unknown_fields: false,
    },
    SchemaContract {
        name: "kiana.extension-migration.v1",
        version: SchemaVersion::new(1, 0),
        layer: SchemaLayer::Domain,
        owner_crate: "kiana-daemon",
        compatibility: CompatibilityPolicy::BackwardCompatible,
        allow_unknown_fields: false,
    },
    SchemaContract {
        name: "kiana.connector-definition.v1",
        version: SchemaVersion::new(1, 0),
        layer: SchemaLayer::Domain,
        owner_crate: "kiana-domain",
        compatibility: CompatibilityPolicy::BackwardCompatible,
        allow_unknown_fields: false,
    },
    SchemaContract {
        name: "kiana.account-binding.v1",
        version: SchemaVersion::new(1, 0),
        layer: SchemaLayer::Domain,
        owner_crate: "kiana-domain",
        compatibility: CompatibilityPolicy::BackwardCompatible,
        allow_unknown_fields: false,
    },
    SchemaContract {
        name: "kiana.provider-receipt.v1",
        version: SchemaVersion::new(1, 0),
        layer: SchemaLayer::Domain,
        owner_crate: "kiana-domain",
        compatibility: CompatibilityPolicy::BackwardCompatible,
        allow_unknown_fields: false,
    },
    SchemaContract {
        name: "kiana.workspace-checkpoint.v1",
        version: SchemaVersion::new(1, 0),
        layer: SchemaLayer::RuntimeEvent,
        owner_crate: "kiana-domain",
        compatibility: CompatibilityPolicy::BackwardCompatible,
        allow_unknown_fields: true,
    },
    SchemaContract {
        name: "kiana.human-inbox.v1",
        version: SchemaVersion::new(1, 0),
        layer: SchemaLayer::Domain,
        owner_crate: "kiana-domain",
        compatibility: CompatibilityPolicy::BackwardCompatible,
        allow_unknown_fields: false,
    },
    SchemaContract {
        name: "kiana.run-snapshot.v1",
        version: SchemaVersion::new(1, 0),
        layer: SchemaLayer::RuntimeEvent,
        owner_crate: "kiana-domain",
        compatibility: CompatibilityPolicy::BackwardCompatible,
        allow_unknown_fields: true,
    },
];

/// Look up the canonical owner, layer and compatibility policy for a registered schema.
pub fn schema_contract(schema_name: &str) -> Option<&'static SchemaContract> {
    SCHEMA_CONTRACTS
        .iter()
        .find(|contract| contract.name == schema_name)
}

/// 检查给定的 schema 版本是否与当前运行时兼容。
///
/// 返回 `Err` 表示 fail-closed：未知 major 版本必须拒绝。
pub fn check_schema_compatibility(
    schema_name: &str,
    incoming_version: &SchemaVersion,
) -> Result<(), String> {
    let contract =
        schema_contract(schema_name).ok_or_else(|| format!("unknown schema: {}", schema_name))?;

    if !contract.version.is_compatible_with(incoming_version) {
        return Err(format!(
            "incompatible schema version: {} runtime={}.{} incoming={}.{} (unknown major must fail-closed)",
            schema_name,
            contract.version.major,
            contract.version.minor,
            incoming_version.major,
            incoming_version.minor
        ));
    }

    Ok(())
}

/// A canonical SHA-256 fingerprint of structured data, independent of object-key order.
pub fn json_digest(value: &serde_json::Value) -> String {
    use sha2::{Digest, Sha256};
    fn canonical(value: &serde_json::Value, output: &mut String) {
        match value {
            serde_json::Value::Object(object) => {
                output.push('{');
                let mut entries = object.iter().collect::<Vec<_>>();
                entries.sort_by_key(|(key, _)| *key);
                for (index, (key, value)) in entries.into_iter().enumerate() {
                    if index > 0 {
                        output.push(',');
                    }
                    output.push_str(&serde_json::to_string(key).expect("JSON string"));
                    output.push(':');
                    canonical(value, output);
                }
                output.push('}');
            }
            serde_json::Value::Array(array) => {
                output.push('[');
                for (index, value) in array.iter().enumerate() {
                    if index > 0 {
                        output.push(',');
                    }
                    canonical(value, output);
                }
                output.push(']');
            }
            scalar => output.push_str(&scalar.to_string()),
        }
    }
    let mut text = String::new();
    canonical(value, &mut text);
    format!("sha256:{:x}", Sha256::digest(text.as_bytes()))
}

#[cfg(test)]
mod tests {
    use super::{IdContract, IdWireShape, ID_CONTRACTS};
    use serde::de::DeserializeOwned;
    use serde::Serialize;
    use std::collections::HashSet;
    use std::fmt::Debug;

    const WORKSPACE_MANIFEST: &str = include_str!("../../Cargo.toml");
    const IDS_SOURCE: &str = include_str!("ids.rs");

    trait IdRoundTripSample: Serialize + DeserializeOwned + PartialEq + Debug {
        const WIRE_SHAPE: IdWireShape;

        fn sample() -> Self;
    }

    macro_rules! impl_uuid_samples {
        ($($id:ident),+ $(,)?) => {
            $(
                impl IdRoundTripSample for crate::$id {
                    const WIRE_SHAPE: IdWireShape = IdWireShape::Uuid;

                    fn sample() -> Self {
                        Self::new()
                    }
                }
            )+
        };
    }

    impl_uuid_samples!(
        RequestId,
        InputId,
        InteractionId,
        RunId,
        TurnId,
        StepId,
        CellId,
        EventId,
        ExecutionId,
        InvocationId,
        ModelAttemptId,
        ApprovalId,
        ArtifactId,
        ReceiptId,
        OrganizationId,
        PrincipalId,
        ProviderAccountId,
        ServiceIdentityId,
        PolicyProfileId,
        DataBoundaryId,
        SharingGrantId,
        SwarmPlanId,
        PartitionId,
        ChildCellId,
        AttemptId,
        DispatchIntentId,
        QueueEntryId,
        MergeDecisionId,
        MessageId,
        NotificationId,
        SubscriptionId,
        DeliveryAttemptId,
        ActionRefId,
        DeliveryReceiptId,
        QualityArtifactId,
        QualityTransitionId,
        EvalDatasetId,
        EvalSuiteId,
        EvalCaseId,
        GoldenTraceId,
        EvalExperimentId,
        EvalResultId,
        QualityCandidateId,
        QualityGateId,
        QualityGateDecisionId,
        FeedbackId,
        DriftAlertId,
        StorageRootId,
        StoreIdentityId,
        StorageLockId,
        StorageErrorId,
        StorageHealthId,
        StorageIntegrityIncidentId,
        ProjectId,
        TemplateId,
        SpawnPlanId,
        BudgetLeaseId,
        CapabilityGrantId,
        SupervisionLeaseId,
        DelegationId,
        ExtensionId,
        ComponentId,
        SnapshotId,
        HookRunId,
        WorkspaceId,
        MembershipId,
        AssignmentId,
        ProjectAssignmentId,
        EvidenceId,
        CriterionId,
        SecurityRegistryId,
        SecurityContextId,
        SecurityPolicyId,
        SecurityDecisionId,
        SecurityEventId,
        GrantId,
        OperationId,
        AuditId,
        SecretRefId,
        EvidenceRefId,
        FenceTokenId,
    );

    impl IdRoundTripSample for crate::SessionId {
        const WIRE_SHAPE: IdWireShape = IdWireShape::String;

        fn sample() -> Self {
            Self::new("session-round-trip")
        }
    }

    impl IdRoundTripSample for crate::WorkFingerprint {
        const WIRE_SHAPE: IdWireShape = IdWireShape::String;

        fn sample() -> Self {
            Self::from_parts("objective", &[], "partition", "output", "policy")
                .expect("valid work fingerprint sample")
        }
    }

    fn contract_for(type_name: &str) -> &'static IdContract {
        ID_CONTRACTS
            .iter()
            .find(|contract| contract.type_name == type_name)
            .unwrap_or_else(|| panic!("missing ID contract for {type_name}"))
    }

    macro_rules! round_trip {
        ($id:ident) => {
            #[allow(non_snake_case)]
            mod $id {
                use super::{contract_for, IdRoundTripSample, IdWireShape};
                use crate::$id;

                #[test]
                fn round_trip_preserves_wire_shape() {
                    let contract = contract_for(stringify!($id));
                    assert_eq!(contract.type_name, stringify!($id));
                    assert_eq!(contract.wire_shape, <$id as IdRoundTripSample>::WIRE_SHAPE);

                    let value = <$id as IdRoundTripSample>::sample();
                    let encoded = serde_json::to_string(&value).expect("serialize ID");
                    let json: serde_json::Value =
                        serde_json::from_str(&encoded).expect("parse serialized ID");
                    let wire_value = json
                        .as_str()
                        .expect("ID wire representation must be a JSON string");
                    let measured_wire_shape = if uuid::Uuid::parse_str(wire_value).is_ok() {
                        IdWireShape::Uuid
                    } else {
                        IdWireShape::String
                    };
                    assert_eq!(contract.wire_shape, measured_wire_shape);

                    let decoded: $id =
                        serde_json::from_str(&encoded).expect("deserialize round trip");
                    assert_eq!(decoded, value);
                }
            }
        };
    }

    macro_rules! register_round_trip_tests {
        ($($id:ident),+ $(,)?) => {
            const ROUND_TRIP_TYPES: &[&str] = &[$(stringify!($id)),+];

            $(round_trip!($id);)+

            #[test]
            fn id_contract_count_matches_registered_types() {
                assert_eq!(ID_CONTRACTS.len(), ROUND_TRIP_TYPES.len());
                for type_name in ROUND_TRIP_TYPES {
                    assert!(
                        ID_CONTRACTS
                            .iter()
                            .any(|contract| contract.type_name == *type_name),
                        "ID_CONTRACTS is missing {type_name}"
                    );
                }
            }
        };
    }

    register_round_trip_tests!(
        RequestId,
        InputId,
        InteractionId,
        RunId,
        TurnId,
        StepId,
        CellId,
        EventId,
        ExecutionId,
        InvocationId,
        ModelAttemptId,
        ApprovalId,
        ArtifactId,
        ReceiptId,
        OrganizationId,
        PrincipalId,
        ProviderAccountId,
        ServiceIdentityId,
        PolicyProfileId,
        DataBoundaryId,
        SharingGrantId,
        SwarmPlanId,
        PartitionId,
        ChildCellId,
        AttemptId,
        DispatchIntentId,
        QueueEntryId,
        MergeDecisionId,
        MessageId,
        NotificationId,
        SubscriptionId,
        DeliveryAttemptId,
        ActionRefId,
        DeliveryReceiptId,
        QualityArtifactId,
        QualityTransitionId,
        EvalDatasetId,
        EvalSuiteId,
        EvalCaseId,
        GoldenTraceId,
        EvalExperimentId,
        EvalResultId,
        QualityCandidateId,
        QualityGateId,
        QualityGateDecisionId,
        FeedbackId,
        DriftAlertId,
        StorageRootId,
        StoreIdentityId,
        StorageLockId,
        StorageErrorId,
        StorageHealthId,
        StorageIntegrityIncidentId,
        ProjectId,
        TemplateId,
        SpawnPlanId,
        BudgetLeaseId,
        CapabilityGrantId,
        SupervisionLeaseId,
        DelegationId,
        ExtensionId,
        ComponentId,
        SnapshotId,
        HookRunId,
        WorkspaceId,
        MembershipId,
        AssignmentId,
        ProjectAssignmentId,
        EvidenceId,
        CriterionId,
        SecurityRegistryId,
        SecurityContextId,
        SecurityPolicyId,
        SecurityDecisionId,
        SecurityEventId,
        GrantId,
        OperationId,
        AuditId,
        SecretRefId,
        EvidenceRefId,
        FenceTokenId,
        SessionId,
        WorkFingerprint,
    );

    #[test]
    fn every_public_type_has_one_owner_and_a_conversion_test() {
        let workspace_crates = workspace_crate_names();
        let mut type_names: HashSet<&str> = HashSet::new();
        let mut wire_names: HashSet<&str> = HashSet::new();

        for contract in ID_CONTRACTS {
            assert!(
                type_names.insert(contract.type_name),
                "duplicate ID type_name: {}",
                contract.type_name
            );
            assert!(
                wire_names.insert(contract.wire_name),
                "duplicate ID wire_name: {}",
                contract.wire_name
            );
            assert!(
                workspace_crates.contains(&contract.owner_crate),
                "unknown owner crate: {}",
                contract.owner_crate
            );
            assert!(
                ROUND_TRIP_TYPES.contains(&contract.type_name),
                "missing conversion test for: {}",
                contract.type_name
            );
            assert!(
                !contract.wire_name.is_empty(),
                "empty wire_name for: {}",
                contract.type_name
            );
        }
    }

    #[test]
    fn every_id_type_definition_is_registered() {
        let mut declared_types: Vec<&str> = IDS_SOURCE
            .lines()
            .filter_map(|line| {
                let line = line.trim();
                if let Some(rest) = line.strip_prefix("uuid_id!(") {
                    return rest.strip_suffix(");").map(str::trim);
                }
                if let Some(rest) = line.strip_prefix("pub struct ") {
                    let type_name = rest.split(['(', '{']).next()?.trim();
                    if type_name.ends_with("Id") || type_name == "WorkFingerprint" {
                        return Some(type_name);
                    }
                }
                None
            })
            .collect();
        let mut registered_types: Vec<&str> = ID_CONTRACTS
            .iter()
            .map(|contract| contract.type_name)
            .collect();

        declared_types.sort_unstable();
        registered_types.sort_unstable();
        assert_eq!(declared_types, registered_types);
    }

    fn workspace_crate_names() -> Vec<&'static str> {
        let (_, members) = WORKSPACE_MANIFEST
            .split_once("members = [")
            .expect("workspace members declaration");
        let (members, _) = members
            .split_once(']')
            .expect("workspace members terminator");

        members
            .lines()
            .filter_map(|line| {
                let line = line.trim();
                if line.is_empty() || line.starts_with('#') {
                    return None;
                }
                let name = line.strip_suffix(',')?.trim();
                name.strip_prefix('"')?.strip_suffix('"')
            })
            .collect()
    }

    mod schema_registry {
        use super::super::{
            check_schema_compatibility, SchemaChange, SchemaLayer, SchemaVersion, SCHEMA_CONTRACTS,
        };
        use std::collections::HashSet;

        #[test]
        fn unknown_major_version_fails_closed() {
            // 运行时是 v1.0，接收到 v2.0 → 必须拒绝
            let runtime_v1 = SchemaVersion::new(1, 0);
            let incoming_v2 = SchemaVersion::new(2, 0);
            assert!(!runtime_v1.is_compatible_with(&incoming_v2));

            // 通过 check_schema_compatibility 验证 fail-closed
            let result = check_schema_compatibility("kiana.protocol.v1", &incoming_v2);
            assert!(result.is_err());
            let err = result.unwrap_err();
            assert!(err.contains("incompatible schema version"));
            assert!(err.contains("unknown major must fail-closed"));

            // 运行时是 v1.5，接收到 v2.1 → 仍必须拒绝
            let runtime_v1_5 = SchemaVersion::new(1, 5);
            let incoming_v2_1 = SchemaVersion::new(2, 1);
            assert!(!runtime_v1_5.is_compatible_with(&incoming_v2_1));

            // v1.0 运行时接收 v1.2 → 兼容（minor 升级）
            let incoming_v1_2 = SchemaVersion::new(1, 2);
            assert!(runtime_v1.is_compatible_with(&incoming_v1_2));
            assert!(check_schema_compatibility("kiana.protocol.v1", &incoming_v1_2).is_ok());

            // v1.5 运行时接收 v1.0 → 兼容（旧 minor）
            let incoming_v1_0 = SchemaVersion::new(1, 0);
            assert!(runtime_v1_5.is_compatible_with(&incoming_v1_0));
        }

        #[test]
        fn schema_contract_names_are_unique() {
            let mut names: HashSet<&str> = HashSet::new();
            for contract in SCHEMA_CONTRACTS {
                assert!(
                    names.insert(contract.name),
                    "duplicate schema name: {}",
                    contract.name
                );
            }
        }

        #[test]
        fn schema_change_classification() {
            assert!(!SchemaChange::AddOptionalField.requires_major_bump());
            assert!(SchemaChange::RemoveField.requires_major_bump());
            assert!(SchemaChange::ChangeFieldType.requires_major_bump());
            assert!(SchemaChange::ChangeRequired.requires_major_bump());
            assert!(SchemaChange::ChangeStateSemantics.requires_major_bump());
        }

        #[test]
        fn schema_version_ordering() {
            let v1_0 = SchemaVersion::new(1, 0);
            let v1_1 = SchemaVersion::new(1, 1);
            let v2_0 = SchemaVersion::new(2, 0);

            assert!(v1_0 < v1_1);
            assert!(v1_1 < v2_0);
            assert!(v1_0 < v2_0);
        }

        #[test]
        fn registered_schemas_have_valid_owners() {
            let valid_owners = [
                "kiana-domain",
                "kiana-protocol",
                "kiana-eventlog",
                "kiana-core",
                "kiana-daemon",
            ];
            for contract in SCHEMA_CONTRACTS {
                assert!(
                    valid_owners.contains(&contract.owner_crate),
                    "unknown owner crate: {}",
                    contract.owner_crate
                );
            }
        }

        #[test]
        fn protocol_schema_allows_unknown_fields() {
            let protocol_contract = SCHEMA_CONTRACTS
                .iter()
                .find(|c| c.name == "kiana.protocol.v1")
                .expect("kiana.protocol.v1 must be registered");

            assert_eq!(protocol_contract.layer, SchemaLayer::Wire);
            assert!(
                protocol_contract.allow_unknown_fields,
                "wire protocol should allow unknown fields for forward compatibility"
            );
        }

        #[test]
        fn domain_schemas_reject_unknown_fields() {
            for contract in SCHEMA_CONTRACTS {
                if matches!(contract.layer, SchemaLayer::Domain) {
                    assert!(
                        !contract.allow_unknown_fields,
                        "domain schema {} should reject unknown fields",
                        contract.name
                    );
                }
            }
        }

        #[test]
        fn check_compatibility_rejects_unknown_schema() {
            let unknown_version = SchemaVersion::new(1, 0);
            let result = check_schema_compatibility("unknown.schema.v99", &unknown_version);
            assert!(result.is_err());
            assert!(result.unwrap_err().contains("unknown schema"));
        }
    }
}

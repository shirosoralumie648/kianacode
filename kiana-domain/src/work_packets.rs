use crate::roles::{default_department_id, default_role_id};
use crate::{
    normalize_role_path, BudgetLeaseId, CapabilityGrantId, CellId, CellLifecycle, DelegationId,
    DomainError, ProjectId, ReceiptId, RoleSpec, RunId, SessionId, SpawnPlanId, SupervisionLeaseId,
    TemplateId, WorkFingerprint, WorkPacketStatus, AGENT_TEMPLATE_SCHEMA, BUDGET_LEASE_SCHEMA,
    CELL_SCHEMA, CLOSING_RECEIPT_SCHEMA, DELEGATION_PACKET_SCHEMA, DEPARTMENT_EXECUTING,
    DEPARTMENT_PLANNING, MERGE_RECEIPT_SCHEMA, RETIREMENT_RECORD_SCHEMA, ROLE_BUILDER,
    SPAWN_PLAN_SCHEMA, SPAWN_RESULT_SCHEMA, WORK_PACKET_SCHEMA,
};
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
/// 可执行工作的不可变范围、验收和分配描述。
///
/// WorkPacket 是 Builder 获得工作范围的主要契约。`validate` 只检查领域不变量；真正的
/// 文件写集、budget、cell 和审批仍须由 ControlPlane 再次绑定，不能因为 packet 中存在
/// `path_allow` 就直接执行。
pub struct WorkPacket {
    /// packet schema 版本。
    pub schema: String,
    /// packet 的业务标识。
    pub id: String,
    /// 所属项目 ID。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub project_id: Option<ProjectId>,
    /// 父 packet ID。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parent_packet_id: Option<String>,
    /// 执行该 packet 的 cell ID。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub owner_cell_id: Option<CellId>,
    /// 验收者主体标识。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub acceptor_id: Option<String>,
    /// 输入引用列表。
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub inputs: Vec<String>,
    /// 依赖的其他 packet/工件标识。
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub dependencies: Vec<String>,
    /// 数据访问范围声明。
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub data_scope: Vec<String>,
    /// 结构化验收测试列表。
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub acceptance_tests: Vec<String>,
    /// 截止时间 Unix 毫秒。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub deadline_unix_ms: Option<u64>,
    /// 绑定的 budget lease。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub budget_lease_id: Option<BudgetLeaseId>,
    /// packet 生命周期状态。
    #[serde(default, skip_serializing_if = "is_draft_status")]
    pub status: WorkPacketStatus,
    /// 发起部门。
    #[serde(default = "default_from_department")]
    pub from_department: String,
    /// 目标部门。
    #[serde(default = "default_department_id")]
    pub to_department: String,
    /// 被分配的角色。
    #[serde(default = "default_role_id")]
    pub assignee_role: String,
    /// 人类可读目标。
    pub goal: String,
    /// 允许写入的相对路径。
    #[serde(default)]
    pub path_allow: Vec<String>,
    /// 验收条件文字列表。
    #[serde(default)]
    pub acceptance: Vec<String>,
    /// 明确禁止的操作或范围。
    #[serde(default)]
    pub forbidden: Vec<String>,
}

fn is_draft_status(status: &WorkPacketStatus) -> bool {
    *status == WorkPacketStatus::Draft
}

fn default_from_department() -> String {
    DEPARTMENT_PLANNING.to_owned()
}

impl WorkPacket {
    /// 创建默认分配给 executing Builder 的草稿 packet。
    pub fn builder_task(id: impl Into<String>, goal: impl Into<String>) -> Self {
        Self {
            schema: WORK_PACKET_SCHEMA.to_owned(),
            id: id.into(),
            project_id: None,
            parent_packet_id: None,
            owner_cell_id: None,
            acceptor_id: None,
            inputs: Vec::new(),
            dependencies: Vec::new(),
            data_scope: Vec::new(),
            acceptance_tests: Vec::new(),
            deadline_unix_ms: None,
            budget_lease_id: None,
            status: WorkPacketStatus::Draft,
            from_department: default_from_department(),
            to_department: default_department_id(),
            assignee_role: default_role_id(),
            goal: goal.into(),
            path_allow: Vec::new(),
            acceptance: Vec::new(),
            forbidden: Vec::new(),
        }
    }

    /// 设置 packet 的相对路径白名单。
    pub fn with_path_allow(mut self, paths: impl IntoIterator<Item = impl Into<String>>) -> Self {
        self.path_allow = paths.into_iter().map(Into::into).collect();
        self
    }

    /// 设置验收测试列表。
    pub fn with_acceptance_tests(
        mut self,
        tests: impl IntoIterator<Item = impl Into<String>>,
    ) -> Self {
        self.acceptance_tests = tests.into_iter().map(Into::into).collect();
        self
    }

    /// 设置依赖 packet 列表。
    pub fn with_dependencies(
        mut self,
        dependencies: impl IntoIterator<Item = impl Into<String>>,
    ) -> Self {
        self.dependencies = dependencies.into_iter().map(Into::into).collect();
        self
    }

    /// 按 WorkPacketStatus 的领域状态机迁移状态。
    pub fn transition_status(&mut self, next: WorkPacketStatus) -> Result<(), DomainError> {
        self.status = self.status.transition(next)?;
        Ok(())
    }

    /// 校验 schema、标识、角色、部门和终态约束。
    pub fn validate(&self) -> Result<(), &'static str> {
        if self.schema.trim() != WORK_PACKET_SCHEMA {
            return Err("packet_invalid");
        }
        if self.id.trim().is_empty() {
            return Err("packet_id_required");
        }
        if self.goal.trim().is_empty() {
            return Err("packet_goal_required");
        }
        if self.status.is_terminal() {
            return Err("packet_status_terminal");
        }
        let role = RoleSpec::lookup(&self.assignee_role).ok_or("packet_role_unknown")?;
        if role.role_id != ROLE_BUILDER {
            return Err("packet_role_must_be_builder");
        }
        if role.department_id != DEPARTMENT_EXECUTING {
            return Err("packet_department_must_be_executing");
        }
        let to = self.to_department.trim();
        if !to.is_empty() && to != DEPARTMENT_EXECUTING {
            return Err("packet_department_must_be_executing");
        }
        Ok(())
    }

    /// 将 packet 字段编排为注入 Builder 的提示文本。
    ///
    /// 这是上下文展示，不是安全边界；路径、工具和执行权限必须由结构化上下文与策略
    /// 单独传递并验证。
    pub fn as_prompt(&self) -> String {
        let mut lines = vec![
            format!("Work packet {}", self.id.trim()),
            format!(
                "From: {} -> {}/{}",
                self.from_department.trim(),
                self.to_department.trim(),
                self.assignee_role.trim()
            ),
            format!("Goal: {}", self.goal.trim()),
        ];
        push_packet_list(&mut lines, "Inputs", &self.inputs);
        push_packet_list(&mut lines, "Dependencies", &self.dependencies);
        push_packet_list(&mut lines, "Data scope", &self.data_scope);
        push_packet_list(&mut lines, "Path allow", &self.path_allow);
        push_packet_list(&mut lines, "Acceptance tests", &self.acceptance_tests);
        push_packet_list(&mut lines, "Acceptance", &self.acceptance);
        push_packet_list(&mut lines, "Forbidden", &self.forbidden);
        lines.join("\n")
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct AgentTemplate {
    pub schema: String,
    pub template_id: TemplateId,
    pub version: String,
    pub role_id: String,
    pub mission_schema: String,
    pub input_schema: String,
    pub output_schema: String,
    pub default_capabilities: Vec<String>,
    pub sandbox_profile: String,
    pub estimated_cost: u64,
    pub max_children: u32,
    pub max_depth: u32,
    pub ttl_seconds: u64,
    pub heartbeat_interval_seconds: u64,
    pub checkpoint_policy: String,
    pub merge_strategy: String,
    #[serde(default)]
    pub delegation_allowed: bool,
}

impl AgentTemplate {
    pub fn for_role(role: &RoleSpec, version: impl Into<String>) -> Self {
        Self {
            schema: AGENT_TEMPLATE_SCHEMA.to_owned(),
            template_id: TemplateId::new(),
            version: version.into(),
            role_id: role.role_id.clone(),
            mission_schema: "kiana.mission.v1".to_owned(),
            input_schema: "kiana.input.v1".to_owned(),
            output_schema: "kiana.output.v1".to_owned(),
            default_capabilities: role.tools.clone(),
            sandbox_profile: role.sandbox.clone(),
            estimated_cost: u64::from(role.max_steps),
            max_children: 0,
            max_depth: 0,
            ttl_seconds: 300,
            heartbeat_interval_seconds: 15,
            checkpoint_policy: "on_failure".to_owned(),
            merge_strategy: "receipt_only".to_owned(),
            delegation_allowed: false,
        }
    }

    pub fn validate(&self) -> Result<(), &'static str> {
        if self.schema.trim() != AGENT_TEMPLATE_SCHEMA {
            return Err("template_invalid");
        }
        if self.version.trim().is_empty() {
            return Err("template_version_required");
        }
        if RoleSpec::lookup(&self.role_id).is_none() {
            return Err("template_role_unknown");
        }
        if self.mission_schema.trim().is_empty()
            || self.input_schema.trim().is_empty()
            || self.output_schema.trim().is_empty()
        {
            return Err("template_schema_required");
        }
        if self.ttl_seconds == 0 || self.heartbeat_interval_seconds == 0 {
            return Err("template_lifetime_invalid");
        }
        if self.heartbeat_interval_seconds > self.ttl_seconds {
            return Err("template_heartbeat_exceeds_ttl");
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct BudgetLease {
    pub schema: String,
    pub lease_id: BudgetLeaseId,
    pub max_tool_calls: u64,
    pub max_tokens: u64,
    pub max_wall_clock_ms: u64,
    pub max_concurrency: u32,
    pub max_effects: u32,
    #[serde(default)]
    pub max_reserved_budget: u64,
    #[serde(default)]
    pub reserved_budget: u64,
    #[serde(default)]
    pub tool_calls_used: u64,
    #[serde(default)]
    pub tokens_used: u64,
    #[serde(default)]
    pub effects_used: u32,
}

impl BudgetLease {
    pub fn new(
        max_tool_calls: u64,
        max_tokens: u64,
        max_wall_clock_ms: u64,
        max_concurrency: u32,
        max_effects: u32,
    ) -> Self {
        Self {
            schema: BUDGET_LEASE_SCHEMA.to_owned(),
            lease_id: BudgetLeaseId::new(),
            max_tool_calls,
            max_tokens,
            max_wall_clock_ms,
            max_concurrency,
            max_effects,
            max_reserved_budget: max_tool_calls,
            reserved_budget: 0,
            tool_calls_used: 0,
            tokens_used: 0,
            effects_used: 0,
        }
    }

    pub fn validate(&self) -> Result<(), &'static str> {
        if self.schema.trim() != BUDGET_LEASE_SCHEMA {
            return Err("budget_lease_invalid");
        }
        if self.max_tool_calls == 0
            || self.max_tokens == 0
            || self.max_wall_clock_ms == 0
            || self.max_concurrency == 0
        {
            return Err("budget_lease_limit_required");
        }
        if self.tool_calls_used > self.max_tool_calls
            || self.tokens_used > self.max_tokens
            || self.effects_used > self.max_effects
            || self.reserved_budget > self.reservation_limit()
        {
            return Err("budget_lease_exceeded");
        }
        Ok(())
    }

    fn reservation_limit(&self) -> u64 {
        // Legacy JSON did not carry an admission limit. Treat zero as the
        // backwards-compatible tool-call ceiling; newly-created leases pin it.
        if self.max_reserved_budget == 0 {
            self.max_tool_calls
        } else {
            self.max_reserved_budget
        }
    }

    pub fn can_reserve(&self, amount: u64) -> bool {
        amount
            <= self
                .reservation_limit()
                .saturating_sub(self.reserved_budget)
    }

    pub fn reserve(&mut self, amount: u64) -> Result<(), &'static str> {
        if !self.can_reserve(amount) {
            return Err("spawn_budget_exceeded");
        }
        self.reserved_budget = self
            .reserved_budget
            .checked_add(amount)
            .ok_or("spawn_budget_exceeded")?;
        Ok(())
    }

    pub fn release(&mut self, amount: u64) -> Result<(), &'static str> {
        if amount > self.reserved_budget {
            return Err("spawn_budget_release_invalid");
        }
        self.reserved_budget -= amount;
        Ok(())
    }

    pub fn can_consume(&self, tool_calls: u64, tokens: u64, effects: u32) -> bool {
        tool_calls <= self.max_tool_calls.saturating_sub(self.tool_calls_used)
            && tokens <= self.max_tokens.saturating_sub(self.tokens_used)
            && effects <= self.max_effects.saturating_sub(self.effects_used)
    }

    pub fn consume(
        &mut self,
        tool_calls: u64,
        tokens: u64,
        effects: u32,
    ) -> Result<(), &'static str> {
        if !self.can_consume(tool_calls, tokens, effects) {
            return Err("budget_lease_exceeded");
        }
        self.tool_calls_used = self
            .tool_calls_used
            .checked_add(tool_calls)
            .ok_or("budget_lease_exceeded")?;
        self.tokens_used = self
            .tokens_used
            .checked_add(tokens)
            .ok_or("budget_lease_exceeded")?;
        self.effects_used = self
            .effects_used
            .checked_add(effects)
            .ok_or("budget_lease_exceeded")?;
        Ok(())
    }
}
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct CellSpec {
    pub schema: String,
    pub cell_id: CellId,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parent_cell_id: Option<CellId>,
    pub root_run_id: RunId,
    pub template_id: TemplateId,
    pub template_version: String,
    pub role_id: String,
    pub objective: String,
    #[serde(default)]
    pub input_refs: Vec<String>,
    pub output_contract: String,
    pub partition_key: String,
    #[serde(default)]
    pub owned_paths: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub work_packet_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub owner_actor_id: Option<String>,
    pub capability_grant_id: CapabilityGrantId,
    pub budget_lease_id: BudgetLeaseId,
    pub supervision_lease_id: SupervisionLeaseId,
    pub depth: u32,
    pub spawn_quota: u32,
    #[serde(default)]
    pub lifecycle: CellLifecycle,
}

impl CellSpec {
    pub fn validate(&self, template: &AgentTemplate) -> Result<(), &'static str> {
        if self.schema.trim() != CELL_SCHEMA {
            return Err("cell_invalid");
        }
        template.validate()?;
        if self.template_id != template.template_id
            || self.template_version != template.version
            || self.role_id != template.role_id
        {
            return Err("cell_template_mismatch");
        }
        if self.objective.trim().is_empty() || self.output_contract.trim().is_empty() {
            return Err("cell_contract_required");
        }
        if self.depth > template.max_depth {
            return Err("cell_depth_exceeded");
        }
        if self
            .owned_paths
            .iter()
            .any(|path| normalize_role_path(path).is_none())
        {
            return Err("cell_path_invalid");
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct SpawnPlan {
    pub schema: String,
    pub plan_id: SpawnPlanId,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parent_cell_id: Option<CellId>,
    pub reason_code: String,
    pub candidate_templates: Vec<TemplateId>,
    pub count: u32,
    pub partition: String,
    #[serde(default)]
    pub input_refs: Vec<String>,
    pub output_contract: String,
    #[serde(default)]
    pub requested_capabilities: Vec<String>,
    pub budget_reservation: u64,
    pub deadline_unix_ms: u64,
    pub rollback_policy: String,
    pub idempotency_key: String,
    pub expected_utility: i64,
    #[serde(default)]
    pub status: SpawnPlanStatus,
}

impl SpawnPlan {
    pub fn validate(&self) -> Result<(), &'static str> {
        if self.schema.trim() != SPAWN_PLAN_SCHEMA {
            return Err("spawn_plan_invalid");
        }
        if self.count == 0 || self.candidate_templates.is_empty() {
            return Err("spawn_plan_count_invalid");
        }
        if self.reason_code.trim().is_empty()
            || self.output_contract.trim().is_empty()
            || self.idempotency_key.trim().is_empty()
        {
            return Err("spawn_plan_contract_required");
        }
        if self.deadline_unix_ms == 0 {
            return Err("spawn_plan_deadline_required");
        }
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SpawnPlanStatus {
    Proposed,
    Validated,
    Reserved,
    Committed,
    RolledBack,
    Rejected,
}

impl SpawnPlanStatus {
    pub fn can_transition_to(self, next: Self) -> bool {
        matches!(
            (self, next),
            (Self::Proposed, Self::Validated | Self::Rejected)
                | (Self::Validated, Self::Reserved | Self::Rejected)
                | (
                    Self::Reserved,
                    Self::Committed | Self::RolledBack | Self::Rejected
                )
                | (Self::Committed, Self::RolledBack)
        )
    }

    pub fn transition(self, next: Self) -> Result<Self, DomainError> {
        if self.can_transition_to(next) {
            Ok(next)
        } else {
            Err(DomainError::InvalidStateTransition {
                aggregate: "spawn_plan",
                from: self.as_str(),
                to: next.as_str(),
            })
        }
    }

    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Proposed => "proposed",
            Self::Validated => "validated",
            Self::Reserved => "reserved",
            Self::Committed => "committed",
            Self::RolledBack => "rolled_back",
            Self::Rejected => "rejected",
        }
    }

    pub const fn is_terminal(self) -> bool {
        matches!(self, Self::RolledBack | Self::Rejected)
    }
}

impl Default for SpawnPlanStatus {
    fn default() -> Self {
        Self::Proposed
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct SpawnReceipt {
    pub schema: String,
    pub plan_id: SpawnPlanId,
    pub cell_id: CellId,
    pub root_run_id: RunId,
    pub work_packet_id: String,
    pub fingerprint: WorkFingerprint,
    pub lifecycle: CellLifecycle,
    pub replayed: bool,
}

impl SpawnReceipt {
    pub fn validate(&self) -> Result<(), &'static str> {
        if self.schema.trim() != SPAWN_RESULT_SCHEMA {
            return Err("spawn_receipt_invalid");
        }
        if self.work_packet_id.trim().is_empty() || self.fingerprint.as_str().trim().is_empty() {
            return Err("spawn_receipt_contract_required");
        }
        if self.lifecycle == CellLifecycle::Proposed {
            return Err("spawn_receipt_lifecycle_invalid");
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct RetirementRecord {
    pub schema: String,
    pub cell_id: CellId,
    pub grant_id: CapabilityGrantId,
    pub budget_lease_id: BudgetLeaseId,
    pub supervision_lease_id: SupervisionLeaseId,
    #[serde(default)]
    pub released_paths: Vec<String>,
    pub reason: String,
    pub retired_at_unix_ms: u64,
}

impl RetirementRecord {
    pub fn validate(&self) -> Result<(), &'static str> {
        if self.schema.trim() != RETIREMENT_RECORD_SCHEMA {
            return Err("retirement_record_invalid");
        }
        if self.reason.trim().is_empty() || self.retired_at_unix_ms == 0 {
            return Err("retirement_record_contract_required");
        }
        if self
            .released_paths
            .iter()
            .any(|path| normalize_role_path(path).is_none())
        {
            return Err("retirement_record_path_invalid");
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct DelegationPacket {
    pub schema: String,
    pub delegation_id: DelegationId,
    pub parent_cell_id: CellId,
    pub child_cell_id: CellId,
    pub source_packet_id: String,
    #[serde(default)]
    pub capability_scopes: Vec<String>,
    #[serde(default)]
    pub path_scopes: Vec<String>,
    pub budget_lease_id: BudgetLeaseId,
    pub capability_grant_id: CapabilityGrantId,
    pub supervision_lease_id: SupervisionLeaseId,
    pub expires_at_unix_ms: u64,
    #[serde(default)]
    pub delegation_allowed: bool,
}

impl DelegationPacket {
    pub fn validate(&self) -> Result<(), &'static str> {
        if self.schema.trim() != DELEGATION_PACKET_SCHEMA {
            return Err("delegation_packet_invalid");
        }
        if self.parent_cell_id == self.child_cell_id {
            return Err("delegation_packet_self_parent");
        }
        if self.source_packet_id.trim().is_empty() || self.expires_at_unix_ms == 0 {
            return Err("delegation_packet_contract_required");
        }
        if self
            .path_scopes
            .iter()
            .any(|path| normalize_role_path(path).is_none())
        {
            return Err("delegation_packet_path_invalid");
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct MergeReceipt {
    pub schema: String,
    pub receipt_id: ReceiptId,
    pub author_run_id: RunId,
    pub author_session_id: SessionId,
    pub reviewer_session_id: SessionId,
    pub reviewer_verdict: String,
    #[serde(default)]
    pub files: Vec<String>,
    pub accepted: bool,
    #[serde(default)]
    pub provenance: Vec<String>,
}

impl MergeReceipt {
    pub fn validate(&self) -> Result<(), &'static str> {
        if self.schema.trim() != MERGE_RECEIPT_SCHEMA {
            return Err("merge_receipt_invalid");
        }
        if self.author_session_id.is_empty() || self.reviewer_session_id.is_empty() {
            return Err("merge_receipt_identity_required");
        }
        if self.author_session_id == self.reviewer_session_id {
            return Err("merge_receipt_reviewer_author_same");
        }
        if self.reviewer_verdict != "pass" || !self.accepted {
            return Err("merge_receipt_not_accepted");
        }
        if self
            .files
            .iter()
            .any(|path| normalize_role_path(path).is_none())
        {
            return Err("merge_receipt_path_invalid");
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct ClosingReceipt {
    pub schema: String,
    pub receipt_id: ReceiptId,
    pub project_id: Option<ProjectId>,
    pub author_run_id: RunId,
    pub author_session_id: SessionId,
    pub reviewer_session_id: SessionId,
    pub closer_session_id: SessionId,
    pub review_id: String,
    pub verdict: String,
    pub accepted: bool,
    #[serde(default)]
    pub files_verified: Vec<String>,
    #[serde(default)]
    pub exceptions: Vec<String>,
    pub lessons_path: String,
}

impl ClosingReceipt {
    pub fn validate(&self) -> Result<(), &'static str> {
        if self.schema.trim() != CLOSING_RECEIPT_SCHEMA {
            return Err("closing_receipt_invalid");
        }
        if self.author_session_id.is_empty()
            || self.reviewer_session_id.is_empty()
            || self.closer_session_id.is_empty()
        {
            return Err("closing_receipt_identity_required");
        }
        if self.closer_session_id == self.author_session_id
            || self.closer_session_id == self.reviewer_session_id
        {
            return Err("closing_receipt_role_separation_failed");
        }
        if self.review_id.trim().is_empty() || self.lessons_path.trim() != "lessons/LEARNED.md" {
            return Err("closing_receipt_provenance_required");
        }
        if self.verdict != "pass" || !self.accepted {
            return Err("closing_receipt_not_accepted");
        }
        if self
            .files_verified
            .iter()
            .any(|path| normalize_role_path(path).is_none())
        {
            return Err("closing_receipt_path_invalid");
        }
        Ok(())
    }
}

fn push_packet_list(lines: &mut Vec<String>, label: &str, values: &[String]) {
    let values: Vec<_> = values
        .iter()
        .map(|value| value.trim())
        .filter(|value| !value.is_empty())
        .collect();
    if values.is_empty() {
        return;
    }
    lines.push(format!("{label}: {}", values.join(", ")));
}

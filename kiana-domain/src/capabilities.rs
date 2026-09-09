use crate::{
    allow_list_covers, normalize_role_path, ApprovalId, BudgetLeaseId, CapabilityGrantId, CellId,
    DomainError, RequestContext, RequestId, RunId, SupervisionLeaseId, CAPABILITY_GRANT_SCHEMA,
    SUPERVISION_LEASE_SCHEMA,
};
use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct CapabilityGrant {
    pub schema: String,
    pub grant_id: CapabilityGrantId,
    pub capability: CapabilityKind,
    pub operation: String,
    #[serde(default)]
    pub resources: Vec<String>,
    #[serde(default)]
    pub paths: Vec<String>,
    pub expires_at_unix_ms: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub approval_id: Option<ApprovalId>,
    #[serde(default)]
    pub delegation_allowed: bool,
}

impl CapabilityGrant {
    pub fn validate(&self) -> Result<(), &'static str> {
        if self.schema.trim() != CAPABILITY_GRANT_SCHEMA {
            return Err("capability_grant_invalid");
        }
        if self.operation.trim().is_empty() || self.expires_at_unix_ms == 0 {
            return Err("capability_grant_scope_required");
        }
        if self
            .paths
            .iter()
            .any(|path| normalize_role_path(path).is_none())
        {
            return Err("capability_grant_path_invalid");
        }
        Ok(())
    }

    pub fn contains(&self, child: &Self) -> bool {
        self.capability == child.capability
            && self.operation == child.operation
            && child.expires_at_unix_ms <= self.expires_at_unix_ms
            && (!child.delegation_allowed || self.delegation_allowed)
            && child
                .resources
                .iter()
                .all(|resource| self.resources.contains(resource))
            && child
                .paths
                .iter()
                .all(|path| allow_list_covers(&self.paths, path))
    }

    pub fn allows_request(&self, request: &CapabilityRequest) -> bool {
        let exact_scope =
            self.capability == request.capability && self.operation == request.operation;
        let coding_scope = matches!(&self.capability, CapabilityKind::Other(scope) if scope == "coding")
            && self.operation == "builder.packet"
            && matches!(
                request.operation.as_str(),
                "shell.exec" | "apply_patch" | "mcp.call" | "memory.search" | "memory.write"
            );
        if !exact_scope && !coding_scope {
            return false;
        }
        let paths = capability_request_paths(request);
        paths.is_empty()
            || paths
                .iter()
                .all(|path| allow_list_covers(&self.paths, path))
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct SupervisionLease {
    pub schema: String,
    pub lease_id: SupervisionLeaseId,
    pub heartbeat_interval_seconds: u64,
    pub stall_threshold_seconds: u64,
    pub retry_limit: u32,
    #[serde(default)]
    pub retries_used: u32,
}

impl SupervisionLease {
    pub fn validate(&self) -> Result<(), &'static str> {
        if self.schema.trim() != SUPERVISION_LEASE_SCHEMA {
            return Err("supervision_lease_invalid");
        }
        if self.heartbeat_interval_seconds == 0
            || self.stall_threshold_seconds < self.heartbeat_interval_seconds
        {
            return Err("supervision_lease_interval_invalid");
        }
        if self.retries_used > self.retry_limit {
            return Err("supervision_lease_retries_exceeded");
        }
        Ok(())
    }
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
/// 面向模型/工具的结构化命令意图。
pub struct CommandIntent {
    /// 命令注册名。
    pub name: String,
    /// 命令参数 JSON。
    pub arguments: Value,
}

impl CommandIntent {
    /// 创建一个不执行任何副作用的命令意图。
    pub fn new(name: impl Into<String>, arguments: Value) -> Self {
        Self {
            name: name.into(),
            arguments,
        }
    }
}

#[derive(Clone, Debug, Eq, Hash, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
/// 能力请求所属的资源类别。
pub enum CapabilityKind {
    /// 查询、索引或只读上下文能力。
    Query,
    /// 文件读取/写入能力。
    Filesystem,
    /// 进程或 shell 能力。
    Process,
    /// 外部网络或 MCP 能力。
    Network,
    /// 模型调用能力。
    Model,
    /// 机密读取能力，默认高风险。
    Secret,
    /// 沙箱后端或隔离能力。
    Sandbox,
    /// 计算机/桌面交互能力。
    Computer,
    /// 工具包装能力。
    Tool,
    /// 未内置的扩展类别；策略必须显式识别后才能放行。
    Other(String),
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
/// 能力请求的风险标签。
pub enum RiskLevel {
    #[default]
    /// 只读或本地无副作用操作。
    ReadOnly,
    /// 修改本地文件或状态。
    LocalWrite,
    /// 可能影响外部系统的操作。
    ExternalSideEffect,
    /// 关键或不可逆操作。
    Critical,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
/// 交给 policy/gate/broker 链路审核的能力请求。
pub struct CapabilityRequest {
    /// 请求唯一 ID。
    pub request_id: RequestId,
    /// 能力类别。
    pub capability: CapabilityKind,
    /// broker 使用的精确操作键。
    pub operation: String,
    /// 原始结构化参数；策略和 handler 必须分别校验。
    pub arguments: Value,
    /// 请求声明的风险级别，不能由模型单方面提升为授权。
    pub risk: RiskLevel,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    /// 所属 cell ID。
    pub cell_id: Option<CellId>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    /// 绑定的 capability grant ID。
    pub capability_grant_id: Option<CapabilityGrantId>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    /// 绑定的 budget lease ID。
    pub budget_lease_id: Option<BudgetLeaseId>,
}

impl CapabilityRequest {
    /// 创建默认 ReadOnly 风险的能力请求。
    pub fn new(
        request_id: RequestId,
        capability: CapabilityKind,
        operation: impl Into<String>,
        arguments: Value,
    ) -> Self {
        Self {
            request_id,
            capability,
            operation: operation.into(),
            arguments,
            risk: RiskLevel::ReadOnly,
            cell_id: None,
            capability_grant_id: None,
            budget_lease_id: None,
        }
    }

    /// 设置请求风险并返回 builder 风格值。
    pub fn with_risk(mut self, risk: RiskLevel) -> Self {
        self.risk = risk;
        self
    }
}

fn capability_request_paths(request: &CapabilityRequest) -> Vec<String> {
    let mut paths = Vec::new();
    if let Some(path) = request
        .arguments
        .get("path")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|path| !path.is_empty())
    {
        paths.push(path.to_owned());
    }
    if let Some(patch) = request.arguments.get("patch").and_then(Value::as_str) {
        for line in patch.lines() {
            let line = line.trim();
            for prefix in [
                "*** Add File:",
                "*** Update File:",
                "*** Delete File:",
                "*** Move to:",
            ] {
                if let Some(path) = line.strip_prefix(prefix).map(str::trim) {
                    if !path.is_empty() {
                        paths.push(path.to_owned());
                    }
                }
            }
        }
    }
    paths
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
/// 用户对一次待审批请求的决定。
pub enum ApprovalDecision {
    /// 同意当前精确请求。
    Approve,
    /// 拒绝当前请求。
    Deny,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
/// 一次性审批挑战及其绑定证明材料。
pub struct ApprovalChallenge {
    /// 挑战 schema 标识。
    pub schema: String,
    /// 审批 ID。
    pub approval_id: ApprovalId,
    /// 被审批请求 ID。
    pub request_id: RequestId,
    /// 请求内容摘要，防止批准被转用于另一请求。
    pub request_hash: String,
    #[serde(default)]
    /// 签发挑战时由服务端确定的请求风险；旧记录缺失时默认只读，以禁止自动批准。
    pub risk: RiskLevel,
    /// Unix 毫秒过期时间。
    pub expires_at_unix_ms: u64,
    /// 面向用户的审批/拒绝原因。
    pub reason: String,
    #[serde(default)]
    /// 可选一次性 nonce。
    pub nonce: String,
    #[serde(default)]
    /// 生成挑战时的策略版本。
    pub policy_version: String,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
/// 待审批请求及其挑战的组合。
pub struct PendingApproval {
    /// 给 UI/调用方展示并回传的挑战。
    pub challenge: ApprovalChallenge,
    /// 等待决定的能力请求。
    pub request: CapabilityRequest,
}

#[derive(Clone, Debug, PartialEq)]
/// 尚未完成审批的能力请求及其运行上下文。
///
/// 该类型只在 core 内保存，故意不序列化；审批通过后仍需重新绑定 challenge、上下文、
/// sandbox 和事件序号，不能直接把其中的请求交给 broker。
pub struct PendingInvocation {
    /// 待决定的审批 ID。
    pub approval_id: ApprovalId,
    /// 当时签发的审批挑战。
    pub challenge: ApprovalChallenge,
    /// 外层协议请求 ID。
    pub request_id: RequestId,
    /// 记录审批事件所用的请求 ID。
    pub event_request_id: RequestId,
    /// 下一条事件序号。
    pub event_sequence: u64,
    /// 所属 run ID。
    pub run_id: RunId,
    /// 等待授权的能力请求。
    pub request: CapabilityRequest,
    /// 创建 pending 时的请求上下文快照。
    pub context: RequestContext,
    /// 当时请求的沙箱档位。
    pub sandbox: String,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
/// 已由 ControlPlane 生成授权 ID 的能力请求。
pub struct AuthorizedCapabilityRequest {
    /// core 生成的非空授权记录 ID。
    pub authorization_id: String,
    /// 已通过策略、gate 和必要审批的原始请求。
    pub request: CapabilityRequest,
}

impl AuthorizedCapabilityRequest {
    /// 创建授权请求；空授权 ID 直接拒绝，防止 broker 接受匿名执行。
    pub fn new(
        authorization_id: impl Into<String>,
        request: CapabilityRequest,
    ) -> Result<Self, DomainError> {
        let authorization_id = authorization_id.into();
        if authorization_id.trim().is_empty() {
            return Err(DomainError::EmptyAuthorizationId);
        }
        Ok(Self {
            authorization_id,
            request,
        })
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
/// capability handler 返回给控制平面的结构化结果。
pub struct CapabilityResult {
    /// 对应原能力请求 ID。
    pub request_id: RequestId,
    /// handler 是否报告成功；`false` 仍可能伴随部分副作用。
    pub success: bool,
    /// handler 输出或结构化错误。
    pub output: Value,
    /// 可供回执追踪的证据引用。
    pub evidence_refs: Vec<String>,
}

impl CapabilityResult {
    /// 创建成功结果；不会自动添加证据引用。
    pub fn success(request_id: RequestId, output: Value) -> Self {
        Self {
            request_id,
            success: true,
            output,
            evidence_refs: Vec::new(),
        }
    }

    /// 创建失败结果；错误文字被放入结构化 `output.error`。
    pub fn failure(request_id: RequestId, error: impl Into<String>) -> Self {
        Self {
            request_id,
            success: false,
            output: serde_json::json!({ "error": error.into() }),
            evidence_refs: Vec::new(),
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(tag = "decision", rename_all = "snake_case")]
pub enum PolicyDecision {
    Allow { authorization_id: String },
    Ask { reason: String },
    Deny { reason: String },
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(tag = "decision", rename_all = "snake_case")]
pub enum GateDecision {
    Allowed { authorization_id: String },
    AwaitingApproval { reason: String },
    Denied { reason: String },
}

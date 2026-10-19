//! # 文件职责
//!
//! 本文件承载 capability 执行主链中跨 crate 共用的 Domain 数据：请求、授权 envelope、结果、
//! 结果回执、审批快照、监督租约，以及交还模型的工具观察。它定义数据形状和纯校验/映射规则，
//! 不在这里执行 shell、文件写入、网络请求或用户审批。
//!
//! # 核心调用链
//!
//! ```text
//! Runner 将模型工具调用映射成 CapabilityRequest
//!   -> Core 规范化 operation/参数/风险并构造服务端 ExecutionScope
//!   -> Policy / Gate / approval 完成判断，Core 包成 AuthorizedCapabilityRequest
//!   -> Capability Broker 派发 handler 并返回 CapabilityResult
//!   -> Core 规范化结果、构造 CapabilityResultReceipt 并写入 EventLog
//!   -> Harness 从 CapabilityResult 构造 ToolObservation
//!   -> 模型只看到有界、标记为 untrusted 的结果摘要
//! ```
//!
//! 主要上游是 `kiana-runner/src/tools.rs` 与 Core 的 capability 准备/审批/dispatch；主要下游
//! 是 `kiana-capability-broker` 的 handler contract、EventLog 结果投影和 `KianaHarness`。
//! 审批挂起时，`PendingInvocation` 会随 `RunSnapshot` 保存；恢复后必须由 Core 重新绑定，不可
//! 直接将快照里的 request 交给 Broker。`CapabilityGrant` 是旧的轻量 grant 值，复杂授权与
//! 撤销账本还要继续读 `grant_authority.rs` / Policy。
//!
//! # 重要信任边界
//!
//! `CapabilityRequest.risk`、`ExecutionScope`、`AuthorizedCapabilityRequest` 和结果摘要都是
//! 数据字段，不会单靠类型存在就证明已授权或已执行。授权权威在 ControlPlane 的策略、Gate、
//! approval、permit 与 Broker 校验链；执行结果是否成功与外部效果是否确定也分开记录。
//! `ToolObservation` 是模型可读反馈，必须保留 `untrusted=true`，它不能为下一次请求增加权限。
//!
//! Serde 是 Rust 的序列化库：本文件的 `Serialize` / `Deserialize` 在 Rust 结构体和 JSON 等
//! 外部格式间转换；结构校验、身份认证和授权判断是另外的步骤，不能由反序列化替代。

use crate::{
    allow_list_covers, json_digest, normalize_role_path, redact_text, ApprovalId,
    ApprovalPlanPreview, BudgetLeaseId, CapabilityEffectState, CapabilityErrorCode,
    CapabilityExecutionState, CapabilityGrantId, CapabilityStopState, CellId, DomainError,
    ExecutionId, ExecutionScope, InvocationId, InvocationResumeBinding, RequestContext, RequestId,
    RunId, SchemaVersion, SupervisionLeaseId, CAPABILITY_GRANT_SCHEMA, SUPERVISION_LEASE_SCHEMA,
};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

/// 结构化进程/停止/效果维度的 schema 名称。
pub const CAPABILITY_RESULT_DIMENSIONS_SCHEMA: &str = "kiana.capability-result-dimensions.v1";
/// 历史 capability outcome 载荷使用的 schema 名称。
pub const CAPABILITY_OUTCOME_SCHEMA: &str = "kiana.capability-outcome.v1";
/// 结果回执的 schema 名称。
pub const CAPABILITY_RESULT_RECEIPT_SCHEMA: &str = "kiana.capability-result-receipt.v1";
/// 当前结果回执合同版本。
pub const CAPABILITY_RESULT_RECEIPT_VERSION: SchemaVersion = SchemaVersion::new(1, 0);
/// 交给模型的有限工具观察 schema 名称。
pub const TOOL_OBSERVATION_SCHEMA: &str = "kiana.tool-observation.v1";
/// 当前工具观察合同版本。
pub const TOOL_OBSERVATION_VERSION: SchemaVersion = SchemaVersion::new(1, 0);
/// 工具观察摘要的长度上限常量，数值为 8 KiB（8192）。
///
/// `ToolObservation::from_result()` 用它限制 Unicode 字符数量，`validate()` 则用同一常量限制
/// UTF-8 字节数量；字符和字节不是同一单位，因此多字节文本截取后仍可能超过校验上限。
/// 这段代码没有附加“内容已截断”的标记，也没有在源码中说明 8 KiB 的选择依据；这里记录
/// 实际单位和行为，避免把未写明的设计理由当作事实。
pub const TOOL_OBSERVATION_MAX_SUMMARY: usize = 8 * 1024;

/// 单次能力调用对应的直接子进程状态。
///
/// 它只描述进程本身，不代表外部效果状态：进程退出时远端写入仍可能未知；
/// 未确认进程停止也不能据此把取消认定为成功。
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
/// 被执行的那个子进程，现在处于什么状态。
///
/// 【`Unknown` 是一等取值，不是异常】
/// 「我启动了它但不知道它怎么样了」是一个**真实且常见**的状态：
/// 机器重启了、连接断了、查询超时了。
/// 它必须有一个自己的取值，因为把它塞进 `NotStarted` 或 `Exited`
/// 都是在凭空做一个没有证据的判断。
pub enum CapabilityProcessState {
    /// 有证据表明执行尚未启动。
    NotStarted,
    /// 已启动且当前仍在运行。
    Running,
    /// 已观察到进程退出；退出码另由 `CapabilityResultDimensions.exit_code` 表示。
    Exited,
    /// 无法确认进程当前或最终状态；不能把它推断成未启动或已退出。
    Unknown,
}

/// Stable, low-cardinality result dimensions shared by the broker, projections and wire
/// adapters.  The original diagnostic error remains in `CapabilityResult.output`; this value is
/// the machine-readable decision surface and must not be reconstructed with substring matching.
/// 一次能力调用的**低基数**结果维度。
///
/// 【它与 `CapabilityResult.output` 的分工】
/// `output` 里是原始的诊断信息（错误文本、上下文），它**不适合**被程序判断。
/// 而这一组维度是给机器读的**判定面**。
///
/// 【⚠ 上面那句注释里的「不得用子串匹配重建」是本文件最重要的一条规则】
/// 意思是：不允许出现
/// ```text
/// if output.contains("timeout") { 判定为超时 }
/// ```
/// 这类写法。原因有三个，每一个都足以单独禁止它：
/// 1. 错误文本会随版本、语言、甚至本地化而变——子串匹配会在某次升级后静默失效；
/// 2. 一条错误消息里可能同时提到多个原因（"timeout while retrying after X"），
///    子串匹配会挑错那一个；
/// 3. 它把「判定」建立在**给人看的文本**上，于是文案变成了接口——
///    之后没人敢改文案，因为改了会坏功能。
///
/// 有了这一组结构化取值，判定就只依赖枚举，文案可以自由改。
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
/// 将一次能力结果拆成可供机器稳定判断的低基数状态维度。
///
/// 它把“子进程状态、是否请求/确认停止、外部效果是否成功/失败/未知”与可读错误文本分开。
/// 调用方用枚举做状态归约，不能从 `CapabilityResult.output` 的错误文案猜测生命周期。
pub struct CapabilityResultDimensions {
    /// 维度合同名称，必须匹配当前 schema。
    pub schema: String,
    /// 直接进程是否未启动、运行中、已退出或结果未知。
    pub process: CapabilityProcessState,
    /// 是否没有请求停止、已确认停止、停止未确认或停止状态未知。
    pub stop: CapabilityStopState,
    /// 外部效果尚未开始、已开始、成功、失败或结果未知。
    pub effect: CapabilityEffectState,
    /// 子进程退出码；没有可确认退出状态时为空。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub exit_code: Option<i32>,
    /// 机器可识别的失败分类；成功结果通常为空。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub failure_code: Option<CapabilityErrorCode>,
}

impl CapabilityResultDimensions {
    /// 拒绝几个不可能或相互矛盾的维度组合。
    ///
    /// `NotStarted` 不能带退出码；效果未知时通常要求进程也未知，除非失败码明确是
    /// `ResultUnknown`。该校验只检查字段之间的合同，不查询 handler、子进程或现实外部系统。
    pub fn validate(&self) -> Result<(), &'static str> {
        if self.schema != CAPABILITY_RESULT_DIMENSIONS_SCHEMA {
            return Err("capability_result_dimensions_schema_invalid");
        }
        if matches!(self.process, CapabilityProcessState::NotStarted) && self.exit_code.is_some() {
            return Err("capability_result_dimensions_process_conflict");
        }
        if matches!(self.effect, CapabilityEffectState::Unknown)
            && !matches!(self.process, CapabilityProcessState::Unknown)
            && self.failure_code != Some(CapabilityErrorCode::ResultUnknown)
        {
            return Err("capability_result_dimensions_unknown_conflict");
        }
        Ok(())
    }

    /// 把更细的进程/停止/效果维度归并成旧生命周期使用的单一执行状态。
    ///
    /// `effect` 是主判定依据；`NotStarted` 只有在停止已确认或错误码为 Cancelled 时归为
    /// Cancelled，否则归为 Failed。该投影会丢失部分细节，调用方需要审计时应同时保留原维度。
    pub fn execution_state(&self) -> CapabilityExecutionState {
        match self.effect {
            CapabilityEffectState::Succeeded => CapabilityExecutionState::Succeeded,
            CapabilityEffectState::Failed => CapabilityExecutionState::Failed,
            CapabilityEffectState::Unknown => CapabilityExecutionState::Unknown,
            CapabilityEffectState::NotStarted => {
                if self.stop == CapabilityStopState::Confirmed
                    || self.failure_code == Some(CapabilityErrorCode::Cancelled)
                {
                    CapabilityExecutionState::Cancelled
                } else {
                    CapabilityExecutionState::Failed
                }
            }
            CapabilityEffectState::Started => CapabilityExecutionState::Executing,
        }
    }
}

/// 结果回执是跨 Core 结果提交、Harness 和审批恢复流程共用的摘要型证据 DTO。
///
/// 原始业务 output 留在调用方管理的事件载荷中；回执只保存结果摘要、生命周期维度和效果标记，
/// 让不同投影使用同一份结构化分类。它的自摘要可以检测字段变化，但不替代 EventLog 提交回执，
/// 也不单独证明摘要对应的现实效果确已发生。
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CapabilityResultReceipt {
    /// 回执合同名称。
    pub schema: String,
    /// 回执合同版本。
    pub version: SchemaVersion,
    /// 回执所绑定的原 capability 请求 ID。
    pub request_id: RequestId,
    /// 可选执行 ID；某些调用链尚未分配执行记录时为空。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub execution_id: Option<ExecutionId>,
    /// 可选模型/工具 invocation ID。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub invocation_id: Option<InvocationId>,
    /// 此次结果属于第几次 attempt；0 被视为无效。
    pub attempt: u32,
    /// handler 报告是否成功；校验器还会要求成功对应已知的 Succeeded effect。
    pub success: bool,
    /// 从 CapabilityResult 派生的进程/停止/效果分类。
    pub dimensions: CapabilityResultDimensions,
    /// 完整传入 CapabilityResult 的内容摘要；回执不保存其 output 正文。
    pub result_digest: String,
    /// handler 侧是否可能已经开始外部效果。
    pub effect_started: bool,
    /// 外部效果是否可确认。
    pub effect_known: bool,
    /// 是否确认没有产生任何效果。
    pub zero_effect: bool,
    /// 是否已设置防止旧执行继续生效的围栏。
    pub fenced: bool,
    /// 若请求过停止，是否确认停止；没有停止请求时可以为空。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stop_confirmed: Option<bool>,
    /// 调用方声明该结果边界已经提交；本类型自身不访问 EventLog 验证此值。
    pub committed: bool,
    /// 对其余回执字段计算的无密钥摘要。
    pub receipt_digest: String,
}

impl CapabilityResultReceipt {
    /// 从已经由 handler/Core 整理过的 CapabilityResult 构造并自校验回执。
    ///
    /// 【输入/输出】输入为结果、可选 execution/invocation ID、非零 attempt 和调用方的 committed
    /// 声明；成功返回只带摘要和维度的回执。
    ///
    /// 【核心流程】提取并验证 dimensions -> 从 output 的约定布尔字段推导 started/known/zero/fenced
    /// 与 stop 状态 -> 对完整 CapabilityResult 计算 result_digest -> 计算 receipt_digest -> validate。
    ///
    /// 【失败情况】result 无法转成 JSON、dimensions 自相矛盾、ID/attempt/效果字段冲突或摘要无效时
    /// 返回 Err。构造过程不提交事件、不核实 committed、不确认现实世界效果；外层 Core 负责持久化
    /// 并在后续投影中把回执与事件来源绑定。
    pub fn from_result(
        result: &CapabilityResult,
        execution_id: Option<ExecutionId>,
        invocation_id: Option<InvocationId>,
        attempt: u32,
        committed: bool,
    ) -> Result<Self, String> {
        let dimensions = result.dimensions();
        dimensions.validate().map_err(|error| error.to_owned())?;
        let output = result.output.as_object();
        let not_executed = output
            .and_then(|value| value.get("not_executed"))
            .and_then(Value::as_bool)
            .unwrap_or(false);
        let unknown = dimensions.effect == CapabilityEffectState::Unknown;
        let effect_started = output
            .and_then(|value| value.get("effect_started"))
            .and_then(Value::as_bool)
            .unwrap_or(!not_executed);
        let effect_known = output
            .and_then(|value| value.get("effect_known"))
            .and_then(Value::as_bool)
            .unwrap_or(!unknown);
        let zero_effect = output
            .and_then(|value| value.get("zero_effect"))
            .and_then(Value::as_bool)
            .unwrap_or(not_executed);
        let fenced = output
            .and_then(|value| value.get("fenced"))
            .and_then(Value::as_bool)
            .unwrap_or(unknown);
        let stop_confirmed = output
            .and_then(|value| value.get("stop_confirmed"))
            .and_then(Value::as_bool);
        let result_digest = json_digest(
            &serde_json::to_value(result).map_err(|_| "capability_result_receipt_encode_failed")?,
        );
        let mut receipt = Self {
            schema: CAPABILITY_RESULT_RECEIPT_SCHEMA.to_owned(),
            version: CAPABILITY_RESULT_RECEIPT_VERSION,
            request_id: result.request_id,
            execution_id,
            invocation_id,
            attempt,
            success: result.success,
            dimensions,
            result_digest,
            effect_started,
            effect_known,
            zero_effect,
            fenced,
            stop_confirmed,
            committed,
            receipt_digest: String::new(),
        };
        receipt.receipt_digest = receipt.digest();
        receipt.validate()?;
        Ok(receipt)
    }

    /// 将事件中的 JSON 回执反序列化为强类型值并立即校验。
    ///
    /// JSON 仅是持久化/传输表示；未知字段会被 serde 拒绝。这里验证字段和回执自摘要，不会比较
    /// EventLog 事件身份，后者必须由使用回执的 Core/Projection 完成。
    pub fn from_json(value: &Value) -> Result<Self, String> {
        let receipt: Self = serde_json::from_value(value.clone())
            .map_err(|_| "capability_result_receipt_decode_failed".to_owned())?;
        receipt.validate()?;
        Ok(receipt)
    }

    /// 检查回执头部、ID、效果标记、维度、digest 格式及自身摘要。
    ///
    /// 成功要求 effect 已知且为 Succeeded；未知效果必须 fenced；zero_effect 与 effect_started
    /// 不能同时为真。该方法不读取 result_digest 对应的原始结果，因此不能证明原结果内容或提交事实。
    pub fn validate(&self) -> Result<(), String> {
        if self.schema != CAPABILITY_RESULT_RECEIPT_SCHEMA
            || !self
                .version
                .is_compatible_with(&CAPABILITY_RESULT_RECEIPT_VERSION)
            || self.request_id.as_uuid().is_nil()
            || self.attempt == 0
            || self.execution_id.is_some_and(|id| id.as_uuid().is_nil())
            || self.invocation_id.is_some_and(|id| id.as_uuid().is_nil())
            || self.zero_effect && self.effect_started
            || !self.effect_known && !self.fenced
            || self.success
                && (!self.effect_known
                    || self.dimensions.effect != CapabilityEffectState::Succeeded)
        {
            return Err("capability_result_receipt_inconsistent".to_owned());
        }
        self.dimensions
            .validate()
            .map_err(|error| error.to_owned())?;
        validate_digest(
            &self.result_digest,
            "capability_result_receipt_result_digest",
        )?;
        validate_digest(&self.receipt_digest, "capability_result_receipt_digest")?;
        if self.receipt_digest != self.digest() {
            return Err("capability_result_receipt_digest_mismatch".to_owned());
        }
        Ok(())
    }

    /// 计算回执摘要；显式列出参与字段，故 `receipt_digest` 不会递归哈希自身。
    ///
    /// 这是无密钥内容摘要，用于检测回执字段变化，不是服务端签名或真实性证明。
    pub fn digest(&self) -> String {
        json_digest(&serde_json::json!({
            "schema": self.schema,
            "version": self.version,
            "request_id": self.request_id,
            "execution_id": self.execution_id,
            "invocation_id": self.invocation_id,
            "attempt": self.attempt,
            "success": self.success,
            "dimensions": self.dimensions,
            "result_digest": self.result_digest,
            "effect_started": self.effect_started,
            "effect_known": self.effect_known,
            "zero_effect": self.zero_effect,
            "fenced": self.fenced,
            "stop_confirmed": self.stop_confirmed,
            "committed": self.committed,
        }))
    }
}

/// 检查摘要文本是否具有 `sha256:` 加 64 位十六进制的固定形状。
///
/// 这里不重算摘要，也不验证签名；真实性/内容绑定由调用此 helper 的具体 DTO 校验负责。
fn validate_digest(value: &str, field: &str) -> Result<(), String> {
    let Some(hex) = value.strip_prefix("sha256:") else {
        return Err(format!("{field}_invalid"));
    };
    if hex.len() != 64 || !hex.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err(format!("{field}_invalid"));
    }
    Ok(())
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
/// 兼容路径使用的轻量 capability grant。
///
/// 它把能力类别、operation、资源/路径列表、过期时间和可选审批 ID 放在一起；当前派生 grant
/// 的 authority ledger 与多维 scope 在 `grant_authority.rs` / Policy 中有更完整的合同。该值
/// 本身不签名、不查询有效期，也不直接授权 Broker 执行。
pub struct CapabilityGrant {
    /// grant schema 名称，必须匹配固定常量。
    pub schema: String,
    /// 当前 grant 的唯一引用 ID。
    pub grant_id: CapabilityGrantId,
    /// 被授予的能力类别。
    pub capability: CapabilityKind,
    /// 被允许的规范 operation 名称。
    pub operation: String,
    /// 可选的资源名称限制。
    #[serde(default)]
    pub resources: Vec<String>,
    /// 可选的路径允许列表；当前校验只确认每条路径可规范化。
    #[serde(default)]
    pub paths: Vec<String>,
    /// Unix 毫秒过期时间；本 DTO 的 `validate()` 只要求非零，不会对照当前时间。
    pub expires_at_unix_ms: u64,
    /// 可选地记录授予该 grant 的审批 ID。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub approval_id: Option<ApprovalId>,
    /// 子 grant 是否可继续派生。
    #[serde(default)]
    pub delegation_allowed: bool,
}

impl CapabilityGrant {
    /// 检查旧 grant 的 schema、operation、过期时间形状和路径文本是否可规范化。
    ///
    /// 这不是“当前有效”判定：函数没有 `now` 输入，不检查资源列表、grant ID 是否为 nil、
    /// approval 是否真实存在，也不验证签发者；这些必须由持有权威状态的上层完成。
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

    /// 检查 child 在能力、operation、过期时间、委派、资源和路径这些字段上是否受限于 self。
    ///
    /// 这是局部包含判断，不比较主体、项目、父 ID、签发人或 authority epoch；Cell/GrantAuthority
    /// 调用方还要检查自己的身份和父子链合同。返回 false 是普通“不包含”，不会产生错误或副作用。
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

    /// 对单个请求做兼容 grant 的 operation/path 快速匹配。
    ///
    /// 除精确 capability+operation 外，`Other("coding")/builder.packet` 还可匹配固定的五种
    /// coding operation。随后提取 `path`/patch 目标并要求都被 grant 路径列表覆盖；若未提取到
    /// 路径，本函数不会因路径维度拒绝。此 helper 不接收当前时间、不检查 grant 有效期、资源列表、
    /// principal 或 project，也不授予新授权；调用者必须先/同时执行完整 grant 与身份校验。
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
#[serde(deny_unknown_fields)]
/// Cell/agent 运行期间的心跳与重试监督参数。
///
/// Cell registry、swarm 和恢复合同会携带此值；它声明监督节奏与最大重试数，不会自己创建
/// timer、检测进程存活或执行 retry。
pub struct SupervisionLease {
    /// lease schema 名称。
    pub schema: String,
    /// 监督租约的稳定 ID。
    pub lease_id: SupervisionLeaseId,
    /// 预期心跳间隔秒数，不能为 0。
    pub heartbeat_interval_seconds: u64,
    /// 视作 stalled 前的阈值秒数，必须不小于心跳间隔。
    pub stall_threshold_seconds: u64,
    /// 允许的重试总次数；0 表示不允许重试。
    pub retry_limit: u32,
    /// 当前已经消耗的重试次数；不得超过 retry_limit。
    #[serde(default)]
    pub retries_used: u32,
}

impl SupervisionLease {
    /// 验证监督字段之间的基本关系；不实际等待心跳或运行重试。
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
///
/// 这是一个“接下来想做什么”的数据值，只有后续控制面和命令分发器接受后才会触发执行；
/// 构造该对象本身没有权限检查和副作用。
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
///
/// 该枚举帮助 Core/Policy 找到适用规则；枚举值本身不是 grant。`Other` 代表扩展字符串，只有
/// 明确识别并配置策略的上层才可以放行，不能把未知类别当默认允许。
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
///
/// 它随请求进入 Core，是动作声明的一部分；Domain action contract 会检查操作所需风险下限，
/// Core 还会结合 sandbox/上下文规范化，不能因为请求写了 `ReadOnly` 就跳过策略或审批。
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
///
/// Runner 通常从模型工具调用构造它，内部 command/adapter 也可创建；接下来 Core 会先规范化
/// operation 与参数、清除调用方提供的旧 scope，再按服务端身份和 ScopeSet 重新准备。Broker
/// 最终收到的是已完成授权流程的请求副本，并会在派发前重验其规范形状和 scope 绑定。
pub struct CapabilityRequest {
    /// 请求唯一 ID。
    pub request_id: RequestId,
    /// 请求涉及的资源/执行类别；用于路由和策略匹配，不是权限本身。
    pub capability: CapabilityKind,
    /// Broker/动作目录使用的规范 operation 键。
    pub operation: String,
    /// 原始结构化参数；Core 会规范化，策略和具体 handler 仍要检查各自需要的约束。
    pub arguments: Value,
    /// 请求声明的风险级别；较高风险不会自行获得授权，较低风险也不能绕过 action contract。
    pub risk: RiskLevel,
    /// Core 准备阶段生成的服务端执行范围快照；入口可先留空，调用方提供的值不能当授权依据。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub execution_scope: Option<ExecutionScope>,
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
    /// 创建一个尚未准备/授权的请求。
    ///
    /// 【调用者】Runner 的模型工具映射和多种 Core command/adapter 会在进入统一授权链前创建它。
    ///
    /// 【输入/输出】接受请求 ID、能力类别、operation 名称和 JSON 参数；返回风险字段为 ReadOnly、
    /// execution scope / Cell / grant / budget 引用均为空的请求。
    ///
    /// 【重要边界】默认风险只是构造时的占位声明，不表示操作已经经过风险评估，更不表示可执行。
    /// 后续 Core 必须规范化请求、按服务端上下文重新构造 scope，再进入 policy/gate/approval。
    ///
    /// 【副作用】纯内存构造；不读取权限、不登记事件、不调用 Broker。
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
            execution_scope: None,
            cell_id: None,
            capability_grant_id: None,
            budget_lease_id: None,
        }
    }

    /// 设置请求声明的风险标签并返回 builder 风格值。
    ///
    /// 这只修改 DTO 字段，不进行风险推导或授权；Domain action contract/Core 仍须核对操作要求。
    pub fn with_risk(mut self, risk: RiskLevel) -> Self {
        self.risk = risk;
        self
    }
}

/// 从结构化参数中提取可用于路径 scope 求交和策略检查的路径文本。
///
/// 【输入/输出】读取 `arguments.path` 字符串，以及 Patch 中 `*** Add/Update/Delete File:` 和
/// `*** Move to:` 行；返回 trim 后的非空文本，保留发现顺序。
///
/// 【限制】这不是 shell 语法分析器，也不识别任意工具私有字段；未知命令语法不会推断路径。
/// 返回值还未 canonicalize，Core 会继续规范化并在失败时拒绝授权。调用方不得把“没提取到路径”
/// 解释为已证明没有文件访问，因为某些操作目标可能藏在此函数未识别的参数格式中。
pub fn capability_request_paths(request: &CapabilityRequest) -> Vec<String> {
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
    // The same file may legitimately appear twice: once in the optional `path` hint and once as a
    // `*** ... File:` header (or in two patch sections, e.g. a move plus an add). Scope dimensions
    // reject duplicates, so collapse repeats here while keeping the first-seen order; the target
    // set is unchanged, only its textual multiplicity is removed.
    let mut seen = std::collections::BTreeSet::new();
    paths.retain(|path| seen.insert(path.clone()));
    paths
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
/// 用户对一次待审批请求的决定。
///
/// 此枚举仅表示用户选择 Approve 或 Deny；Core 还要验证 challenge、请求摘要、过期时间、nonce
/// 和当前策略状态，收到 Approve 文本本身不等于已经创建 permit。
pub enum ApprovalDecision {
    /// 同意当前精确请求。
    Approve,
    /// 拒绝当前请求。
    Deny,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
/// 一次性审批挑战及其绑定证明材料。
///
/// Core 签发 challenge 时把它绑定到 request ID、request_hash、服务端风险、过期时间和策略版本；
/// 用户返回选择后，Core 需要重新验证这些字段，再决定是否继续授权。它不是可重复使用的 permit。
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
///
/// 这是尚未完成授权的传输/存储对象。UI 可展示 challenge，但不能因为拿到 `request` 就绕过审批
/// 状态机把它送进 Broker。
pub struct PendingApproval {
    /// 给 UI/调用方展示并回传的挑战。
    pub challenge: ApprovalChallenge,
    /// 等待决定的能力请求。
    pub request: CapabilityRequest,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
/// 尚未完成审批的能力请求及其运行上下文。
///
/// 该类型由 core 写入受控 RunSnapshot；审批通过后仍需重新绑定 challenge、上下文、
/// sandbox 和事件序号，不能直接把其中的请求交给 broker。
///
/// 各 ID 分别指向业务 Run、外层协议请求、审批事件流请求和待处理 capability 请求；它们用途
/// 不同，恢复时不能互相替代。`resume_binding` 是服务端续接绑定，可在旧 checkpoint 缺省，
/// 但重建前仍要验证受保护的审批材料。
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
    /// Server-owned continuation binding. Legacy snapshots may omit this and are rebuilt only
    /// after the protected approval material has been revalidated.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub resume_binding: Option<InvocationResumeBinding>,
}

/// ApprovalChallenge 的面向界面展示投影；可选决定由服务端提供。
///
/// 它可以含权限 profile、参数副本和脱敏 plan preview，但这些显示字段都不是可执行 request。
/// `expected_version` 供乐观并发检查使用；如果调用方带入版本，adapter 不能静默忽略它。
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ApprovalView {
    #[serde(default)]
    pub permission_profile: Option<crate::PermissionProfile>,
    pub challenge: ApprovalChallenge,
    pub operation: String,
    pub arguments: Value,
    pub available_decisions: Vec<ApprovalDecision>,
    /// 服务端审批聚合版本，用于并发更新时的乐观版本比较。
    /// 旧 adapter 可不提供；若调用方提供版本，adapter 必须比较它，不能静默忽略。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub expected_version: Option<u64>,
    /// 已脱敏的最终计划展示投影，只用于显示，绝不能作为可执行材料。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub plan_preview: Option<ApprovalPlanPreview>,
}

/// Runner 在安全静止点生成的 checkpoint，由 ControlPlane 独占持久化和恢复。
///
/// 它包含 runner 状态摘要、上下文和可选 pending invocation；恢复前 Core 必须校验 schema、
/// 摘要、身份、事件序号与审批绑定。checkpoint 是恢复输入，不是第二个状态权威，也不能单独
/// 驱动工具执行。
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct RunSnapshot {
    /// 可选的 cell 状态快照，旧 checkpoint 可省略。
    #[serde(default)]
    pub cell_state: Option<Value>,
    /// 可选的数据版本标记；其具体解析和新鲜度由恢复流程负责。
    #[serde(default)]
    pub data_epoch: Option<String>,
    /// 可选的 authority 修订标记，恢复时不能只因字段存在就认定权限仍有效。
    #[serde(default)]
    pub authority_revision: Option<String>,
    /// checkpoint schema 名称。
    pub schema: String,
    /// 被暂停/恢复的 Run 身份。
    pub run_id: RunId,
    /// 当时的请求上下文快照；恢复时需要与当前服务端身份/权限重新绑定。
    pub context: RequestContext,
    /// 当时使用的 sandbox 档位。
    pub sandbox: String,
    /// 当时角色提示内容的摘要，用于检测恢复所依赖提示是否漂移。
    pub role_prompt_hash: String,
    /// Runner 状态数据；须与 runner_state_digest 一起校验。
    pub runner_state: Value,
    /// runner_state 的内容摘要。
    pub runner_state_digest: String,
    /// 若暂停时正在等待审批，则保存对应的待处理 invocation。
    pub pending_invocation: Option<PendingInvocation>,
    /// ControlPlane 标记该快照是否允许恢复；仍需通过完整恢复校验。
    pub resumable: bool,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
/// 已由 ControlPlane 生成授权 ID 的能力请求。
///
/// Core 在授权链成功后创建该 envelope 并交给 Broker。它不是 Rust 类型层面的不可伪造 token：
/// 字段可反序列化，`new()` 只拒绝空 ID；真正授权依赖可信 Core 调用路径、permit/策略结果以及
/// Broker 对请求和执行范围的再次验证。
pub struct AuthorizedCapabilityRequest {
    /// core 生成的非空授权记录 ID。
    pub authorization_id: String,
    /// 已通过策略、gate 和必要审批的原始请求。
    pub request: CapabilityRequest,
}

impl AuthorizedCapabilityRequest {
    /// 创建带非空授权 ID 的传输 envelope。
    ///
    /// 该检查只防止空字符串；它不查询 authorization store、不证明 ID 真实存在，也不验证 request
    /// 已通过 policy/gate/approval。生产调用应限制在 Core 的已授权路径，Broker 仍需执行自己的边界检查。
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
///
/// Broker handler 报告 `success/output/evidence_refs`；Core 会再规范化、结合取消/效果信息并
/// 生成回执。`success=false` 不必然代表零副作用；`output` 是诊断 JSON，不应被下游用子串匹配
/// 代替结构化错误分类。
pub struct CapabilityResult {
    /// 对应原能力请求 ID。
    pub request_id: RequestId,
    /// handler 是否报告成功；`false` 仍可能伴随部分副作用，`true` 也要结合 effect 维度理解。
    pub success: bool,
    /// handler 输出或结构化错误。
    pub output: Value,
    /// 可供回执追踪的证据引用。
    #[serde(default)]
    pub evidence_refs: Vec<String>,
}

impl CapabilityResult {
    /// 将失败结果的 `error_code` 或 `error` 文字映射为稳定机器错误分类。
    ///
    /// 成功结果没有 failure code；失败时优先读取 `error_code`，不存在时再读取 `error`，最后
    /// 用固定的 `execution_failed` 作为 fallback。展示文案仍留在 output，不应反过来充当接口。
    pub fn failure_code(&self) -> Option<crate::CapabilityErrorCode> {
        if self.success {
            return None;
        }
        if let Some(code) = self.output.get("error_code").and_then(Value::as_str) {
            return Some(crate::CapabilityErrorCode::from_reason(code));
        }
        Some(crate::CapabilityErrorCode::from_reason(
            self.output
                .get("error")
                .and_then(Value::as_str)
                .unwrap_or("execution_failed"),
        ))
    }

    /// 创建 handler 报告成功的结果；不会自动添加证据引用或效果证明。
    pub fn success(request_id: RequestId, output: Value) -> Self {
        Self {
            request_id,
            success: true,
            output,
            evidence_refs: Vec::new(),
        }
    }

    /// 从结果中的结构化字段推导进程/停止/效果维度，供回执、投影和 Runner 使用。
    ///
    /// 【核心流程】先确定 failure code、`not_executed` 和取消；再推导 stop 状态与可表示为 i32
    /// 的 exit_code；然后分别归约 process 和 effect。`ResultUnknown` 或 `effect_known=false` 优先
    /// 变成 Unknown；取消时只有“确认停止且明确没有开始效果”才可归为 NotStarted，其余维持 Unknown。
    ///
    /// 【为什么分开状态】子进程退出不代表远端写入/支付等外部效果成功；收到取消也不代表动作已停。
    /// 错误文案只供诊断，不能改变这里的机器分类。此函数只读 output，不确认现实效果或写入事件。
    pub fn dimensions(&self) -> CapabilityResultDimensions {
        let failure_code = self.failure_code();
        let not_executed = self.output.get("not_executed") == Some(&Value::Bool(true));
        let cancelled = self.output.get("cancelled") == Some(&Value::Bool(true))
            || failure_code == Some(CapabilityErrorCode::Cancelled);
        let stop = match self.output.get("stop_confirmed").and_then(Value::as_bool) {
            Some(true) => CapabilityStopState::Confirmed,
            Some(false) => CapabilityStopState::Unconfirmed,
            None if cancelled => CapabilityStopState::Unknown,
            None => CapabilityStopState::NotRequested,
        };
        let exit_code = self
            .output
            .get("exit_code")
            .and_then(Value::as_i64)
            .and_then(|code| i32::try_from(code).ok());
        let process = if not_executed {
            CapabilityProcessState::NotStarted
        } else if failure_code == Some(CapabilityErrorCode::ResultUnknown)
            || self.output.get("effect_known") == Some(&Value::Bool(false))
        {
            CapabilityProcessState::Unknown
        } else if exit_code.is_some() || self.success || !cancelled {
            CapabilityProcessState::Exited
        } else {
            CapabilityProcessState::Unknown
        };
        // cancellation 可能发生在外部动作已经发出之后；只有 adapter 明确确认尚未开始，才能断言零效果。
        let effect = if failure_code == Some(CapabilityErrorCode::ResultUnknown)
            || self.output.get("effect_known") == Some(&Value::Bool(false))
        {
            CapabilityEffectState::Unknown
        } else if not_executed {
            CapabilityEffectState::NotStarted
        } else if cancelled {
            if stop == CapabilityStopState::Confirmed
                && self.output.get("effect_started") == Some(&Value::Bool(false))
            {
                CapabilityEffectState::NotStarted
            } else {
                CapabilityEffectState::Unknown
            }
        } else if self.success {
            CapabilityEffectState::Succeeded
        } else {
            CapabilityEffectState::Failed
        };
        CapabilityResultDimensions {
            schema: CAPABILITY_RESULT_DIMENSIONS_SCHEMA.to_owned(),
            process,
            stop,
            effect,
            exit_code,
            failure_code,
        }
    }

    /// 把结果维度进一步折叠成投影共用的单一生命周期状态。
    ///
    /// 这是有损汇总；需要判断停止确认、退出码或外部效果时应保留并检查 `dimensions()`。
    pub fn execution_state(&self) -> CapabilityExecutionState {
        self.dimensions().execution_state()
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

    /// 构造带稳定错误码前缀的失败结果，并可追加人类可读诊断细节。
    ///
    /// `failure_code()` 可以从 canonical 前缀识别分类；冒号后的 detail 只用于诊断，不应影响
    /// retry 或授权分支。该 helper 不代表 handler 没有产生副作用。
    pub fn failure_with_code(
        request_id: RequestId,
        code: crate::CapabilityErrorCode,
        detail: Option<&str>,
    ) -> Self {
        let error = match detail {
            Some(detail) => format!("{}:{detail}", code.as_str()),
            None => code.as_str().to_owned(),
        };
        Self::failure(request_id, error)
    }
}

/// Harness 发给模型的有限结果状态。
///
/// 它是对一次已返回结果的分类，不是授权决定；未来的工具意图仍需由 Core 和 Broker 重新检查。
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
/// Harness 发给模型的有限结果状态；它描述已观察到的结果，不会批准未来工具调用。
pub enum ToolObservationStatus {
    /// handler 在 `CapabilityResult` 中报告成功；此状态本身不保证 effect 已知。
    /// 当前 Harness 调用 `from_result()` 前会单独拒绝 effect 未知的结果。
    Succeeded,
    /// 已知失败，且允许模型尝试修正输入/计划。
    FailedKnown,
    /// 策略或授权拒绝；模型不能用自动重试绕过授权。
    Denied,
    /// 已取消且确认效果尚未开始。
    CancelledNotStarted,
    /// 外部效果未知或无法确认；应进入恢复/对账边界。
    Unknown,
    /// 结果仍未最终确定。
    Pending,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
/// 模型可采取的受限修复提示；它不是权限或重试许可。
pub enum ToolObservationRepair {
    /// 当前不建议基于此观察自动修复。
    None,
    /// 可以尝试修复已知执行/参数错误，但下一次请求仍须完整返回 Core 授权链。
    ModelRepair,
}

/// 一次模型工具消息对应的有限、纯数据反馈。
///
/// 原始 output 会被转为摘要并标记为不可信；这个 DTO 不携带 grant 或执行 scope，不能为下一次请求
/// 新增权限。下游仍必须把后续请求送回 Core 授权链。
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ToolObservation {
    /// 观察合同名和版本。
    pub schema: String,
    pub version: SchemaVersion,
    /// 对应的 capability request 与模型 tool call ID。
    pub request_id: RequestId,
    pub call_id: String,
    /// 给模型看的稳定结果分类。
    pub status: ToolObservationStatus,
    /// 是否允许模型尝试修正请求。
    pub repair: ToolObservationRepair,
    /// 若结果含可表示的进程退出码则保留。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub exit_code: Option<i32>,
    /// 脱敏并限制长度的 output 片段；仍被标记为 untrusted。
    pub summary: String,
    /// 原始结构化 output 的内容摘要，供比对而不是展示全文。
    pub output_digest: String,
    /// 可选完整输出/证据引用；本 DTO 不自行读取该引用。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub full_output_ref: Option<String>,
    /// 可选机器失败分类。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error_code: Option<CapabilityErrorCode>,
    /// 从 CapabilityResult 派生的外部效果状态。
    pub effect: CapabilityEffectState,
    /// 必须为 true，提醒模型将摘要视为不可信工具输出。
    pub untrusted: bool,
}

impl ToolObservation {
    /// 把一次能力结果压缩成 Harness 可放入模型对话的有界观察。
    ///
    /// 【调用者】`KianaHarness` 收到 capability result、核对当前 pending tool 并确认 effect 可知后调用。
    ///
    /// 【数据流】result.output -> JSON 文本 -> `redact_text()` -> 最多取摘要字符 -> 计算原 output digest
    /// 与第一条 evidence ref -> 计算 status/repair -> `validate()` -> `model_text()` 编码给模型。
    ///
    /// 【失败情况】call_id 为空/过长、JSON 编码失败或最终观察校验失败时返回 Err；Harness 会将其
    /// 转成 run failure，而不会把未验证摘要继续送给模型。
    ///
    /// 【信任边界】摘要来源于工具 output，始终设置 `untrusted=true`；ModelRepair 只允许尝试修正，
    /// 不会跳过下一次 Core policy/gate/approval。Unknown 与 Denied 不给自动修复标志。
    ///
    /// ⚠ 摘要用 `.chars().take(...)` 限制 Unicode 字符数，但 `validate()` 用 `String::len()` 限制
    /// UTF-8 字节数。含多字节字符的长输出可能在这里被截成字符数上限后仍超出字节上限，随后验证失败。
    pub fn from_result(
        call_id: impl Into<String>,
        result: &CapabilityResult,
    ) -> Result<Self, String> {
        let call_id = call_id.into();
        if call_id.trim().is_empty() || call_id.len() > 256 {
            return Err("tool_observation_call_id_invalid".to_owned());
        }
        let dimensions = result.dimensions();
        let error_code = result.failure_code();
        let status = if result.success {
            ToolObservationStatus::Succeeded
        } else if error_code == Some(CapabilityErrorCode::ResultUnknown)
            || dimensions.effect == CapabilityEffectState::Unknown
        {
            ToolObservationStatus::Unknown
        } else if error_code == Some(CapabilityErrorCode::Cancelled)
            && dimensions.effect == CapabilityEffectState::NotStarted
        {
            ToolObservationStatus::CancelledNotStarted
        } else if error_code.is_some_and(|code| {
            matches!(
                code,
                CapabilityErrorCode::ExecutionFailed | CapabilityErrorCode::InvalidArguments
            )
        }) {
            // These bounded, known failures may be shown to the model for one repair attempt;
            // they do not grant permission and the next capability request still re-enters Core.
            ToolObservationStatus::FailedKnown
        } else if error_code.is_some_and(|code| code.policy().requires_new_authorization) {
            ToolObservationStatus::Denied
        } else {
            ToolObservationStatus::FailedKnown
        };
        let repair = if status == ToolObservationStatus::FailedKnown
            && matches!(
                error_code,
                Some(CapabilityErrorCode::ExecutionFailed | CapabilityErrorCode::InvalidArguments)
            ) {
            ToolObservationRepair::ModelRepair
        } else {
            ToolObservationRepair::None
        };
        let encoded = serde_json::to_string(&result.output)
            .map_err(|_| "tool_observation_output_encode_failed".to_owned())?;
        // 先脱敏再截断，避免摘要泄露已识别的敏感文本；截断单位是字符而不是字节。
        let summary = redact_text(&encoded)
            .chars()
            .take(TOOL_OBSERVATION_MAX_SUMMARY)
            .collect::<String>();
        let observation = Self {
            schema: TOOL_OBSERVATION_SCHEMA.to_owned(),
            version: TOOL_OBSERVATION_VERSION,
            request_id: result.request_id,
            call_id,
            status,
            repair,
            exit_code: dimensions.exit_code,
            summary,
            output_digest: json_digest(&result.output),
            full_output_ref: result.evidence_refs.first().cloned(),
            error_code,
            effect: dimensions.effect,
            untrusted: true,
        };
        observation.validate()?;
        Ok(observation)
    }

    /// 校验模型观察的 schema、ID、长度、digest 外形、状态组合与 untrusted 标记。
    ///
    /// 该方法不重算 output_digest、不读取 full_output_ref，也无法证明 summary 已正确脱敏；它只校验
    /// DTO 自身的结构边界。来源可信度仍由 Harness/Core 的调用顺序保证。
    pub fn validate(&self) -> Result<(), String> {
        if self.schema != TOOL_OBSERVATION_SCHEMA
            || self.version != TOOL_OBSERVATION_VERSION
            || self.request_id.as_uuid().is_nil()
            || self.call_id.trim().is_empty()
            || self.call_id.len() > 256
            || self.summary.len() > TOOL_OBSERVATION_MAX_SUMMARY
            || !self.output_digest.starts_with("sha256:")
            || self.output_digest.len() != 71
            || !self.output_digest[7..]
                .bytes()
                .all(|byte| byte.is_ascii_hexdigit())
            || self.full_output_ref.as_ref().is_some_and(|reference| {
                reference.trim().is_empty() || reference.len() > 4_096 || reference.contains('\0')
            })
            || !self.untrusted
        {
            return Err("tool_observation_invalid".to_owned());
        }
        if self.status == ToolObservationStatus::Succeeded && self.error_code.is_some()
            || self.status == ToolObservationStatus::Unknown
                && self.repair != ToolObservationRepair::None
        {
            return Err("tool_observation_status_conflict".to_owned());
        }
        if self.repair == ToolObservationRepair::ModelRepair
            && self.status != ToolObservationStatus::FailedKnown
        {
            return Err("tool_observation_repair_conflict".to_owned());
        }
        Ok(())
    }

    /// 验证后把观察编码成模型可读 JSON 文本。
    ///
    /// 只输出有界摘要和 digest 元数据，不把授权/执行 scope 填入模型上下文；失败时返回编码错误，
    /// 不会自动回退到未经校验的字符串。
    pub fn model_text(&self) -> Result<String, String> {
        self.validate()?;
        serde_json::to_string(&json!({
            "schema": self.schema,
            "version": self.version,
            "request_id": self.request_id,
            "call_id": self.call_id,
            "status": self.status,
            "repair": self.repair,
            "exit_code": self.exit_code,
            "summary": self.summary,
            "output_digest": self.output_digest,
            "full_output_ref": self.full_output_ref,
            "error_code": self.error_code,
            "effect": self.effect,
            "untrusted": true,
        }))
        .map_err(|_| "tool_observation_encode_failed".to_owned())
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(tag = "decision", rename_all = "snake_case")]
/// Policy 对一个能力请求给出的粗粒度结论。
///
/// 它表达策略层的 Allow、Ask 或 Deny；Core 还会组合 Gate、审批和其他约束，只有完整授权链
/// 成功后才可能派发请求。PolicyDecision 本身不执行工具。
pub enum PolicyDecision {
    /// 策略允许继续，并附带供后续授权流程使用的 ID。
    Allow { authorization_id: String },
    /// 策略要求额外处理，通常进入审批或 Gate 分支。
    Ask { reason: String },
    /// 策略拒绝请求，并保留可供上层映射的原因。
    Deny { reason: String },
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(tag = "decision", rename_all = "snake_case")]
/// Gate 与 Policy 合并后，Core 用于选择下一步分支的结果。
///
/// `Allowed` 仍只是授权链中的结论数据；调用方还要构造/验证授权 envelope，并由 Broker 做派发前检查。
pub enum GateDecision {
    /// Gate 允许进入后续授权或执行准备，并携带授权 ID。
    Allowed { authorization_id: String },
    /// 需要等待人工审批或其他 Gate 条件完成。
    AwaitingApproval { reason: String },
    /// Gate 拒绝继续，并保留拒绝原因。
    Denied { reason: String },
}

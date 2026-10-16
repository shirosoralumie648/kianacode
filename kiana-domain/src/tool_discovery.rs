//! # 文件职责
//!
//! 本文件实现 CAP-30 动态工具发现的候选筛选 reducer（按固定规则把输入事实归约成决定）：
//! 接收候选、服务端绑定计划和来源声明，输出可复核的 `ToolDiscoveryReport`。它属于
//! `kiana-domain` 的纯数据合同，不扫描扩展包、不验证签名、不连接 MCP、不执行 handler，也不把
//! 报告变成可执行的 `CapabilityRequest`。
//!
//! # 系统位置与数据流
//!
//! ```text
//! 未来的 Extension/MCP 适配器
//!   -> 验证实际来源并形成 ToolDiscoveryCandidate（当前仓内未发现生产构造者）
//! 服务端给定 ToolDiscoveryPlan（角色、项目、effect ceiling、catalog digest、预算）
//!   -> ToolDiscoveryReport::evaluate(plan, candidates)
//!   -> 逐个检查 DTO 结构、内建名称/operation 冲突、重复项、信任/健康/角色/effect 声明
//!   -> 对候选 schema 计算预算，report 只保存准入名称和字节/token 统计，不保存 schema 本体
//!   -> 后续搜索/提升仍需由其他模块实现，并重新经过 ControlPlane/Broker
//! ```
//!
//! 已有的 schema 搜索函数位于 `kiana-domain/src/tool_catalog.rs`，由
//! `kiana-daemon/src/execution_control.rs` 的 `tool.search` 执行路径使用；该搜索与本文件的
//! admission reducer 是两个不同步骤。当前调用扫描只发现 Domain 合同测试直接构造这些值，
//! 没有生产代码创建候选或调用 `evaluate()`。
//!
//! # 安全边界和未实现部分
//!
//! `ToolDiscoveryCandidate` 中的 trust、health、effect、origin、role binding 都只是传入事实。
//! reducer 按这些值做确定性判定，但不会验证签名、读取项目身份或证明适配器诚实；因此这里的
//! `Verified` 本身不是可伪造性已解决的凭证。plan/report digest 也只是无密钥内容摘要。
//!
//! 此模块自身不会写入角色 allow-list 或执行权限。它检查候选声明的 `bound_roles` 是否包含 plan
//! role，但没有 tool-name-to-role allow-list 字段；它也检查内建名称/operation 冲突，却不验证
//! 新 capability/operation 是否已在权威 action/handler 注册表中。只读候选声明写能力会被拒绝，
//! 但信任 effect 的来源真实性仍由上游负责。项目绑定目前也缺少候选 project ID，详见对应校验注释。
//! 搜索排序、持久化候选索引、实时 trust adapter、Broker promotion 和真实执行均不在本文件中。
//! 维护时应继续阅读 `tool_catalog.rs`、`kiana-daemon/src/execution_control.rs` 和 CAP-30
//! baseline；不要把一个成功的 `ToolDiscoveryReport` 当成 capability grant。

use crate::{
    json_digest, redact_text, tool_spec, ExtensionEffect, ExtensionManifest, SchemaVersion,
    ToolCatalogSnapshot, TOOL_CATALOG_VERSION,
};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::{BTreeMap, BTreeSet};

/// 候选工具 DTO 的稳定 schema 名称。
pub const TOOL_DISCOVERY_SCHEMA: &str = "kiana.tool-discovery-candidate.v1";
/// 单次发现计划 DTO 的稳定 schema 名称。
pub const TOOL_DISCOVERY_PLAN_SCHEMA: &str = "kiana.tool-discovery-plan.v1";
/// 单次发现报告 DTO 的稳定 schema 名称。
pub const TOOL_DISCOVERY_REPORT_SCHEMA: &str = "kiana.tool-discovery-report.v1";
/// 当前候选、计划、报告共同使用的格式版本。
pub const TOOL_DISCOVERY_VERSION: SchemaVersion = SchemaVersion::new(1, 0);
/// 一次判定最多接受 512 个候选，避免调用方提供任意大的列表消耗无界内存和 CPU。
pub const MAX_TOOL_DISCOVERY_CANDIDATES: usize = 512;
/// 单个候选最多携带 32 个标签。
pub const MAX_TOOL_DISCOVERY_TAGS: usize = 32;
/// 单个标签最多 64 字节；Rust `String::len()` 统计 UTF-8 字节数，不是字符数。
pub const MAX_TOOL_DISCOVERY_TAG_BYTES: usize = 64;

// Serde 的 Serialize/Deserialize derive 负责结构与外部数据格式互转；枚举通过 snake_case 编码，
// DTO 上的 deny_unknown_fields 则让未知合同字段显式失败，不被静默忽略。
/// 候选工具声称来自哪个来源类别。
///
/// 设计上该值应由服务端适配器观察后填写，而不是采信扩展自报；但本 enum 和 DTO 可公开构造，
/// reducer 不会验证来源真实性。
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ToolDiscoveryOrigin {
    /// 已通过外部准入校验的本地扩展包；签名校验不在本 enum 内执行。
    Extension,
    /// 从 stdio MCP server 声明中发现的工具；此值本身不建立 MCP 连接。
    McpServer,
    /// 随仓库发布、由代码审查覆盖的内建工具。
    Builtin,
}

impl ToolDiscoveryOrigin {
    /// 返回用于 JSON/日志的稳定 origin 字符串。
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Extension => "extension",
            Self::McpServer => "mcp_server",
            Self::Builtin => "builtin",
        }
    }

    /// 判断来源标签是否为仓库内建工具。
    ///
    /// 只有 `Builtin` 会走内建来源分支；扩展/MCP 的 operation 若与内建 operation 冲突会被拒绝，
    /// 但一个全新的 operation 不会仅因“未在内建目录出现”而在此处被拒绝。该判断依赖候选中的
    /// origin 值，调用方仍有责任保证它来自可信适配器。
    const fn is_repository_local(self) -> bool {
        matches!(self, Self::Builtin)
    }
}

/// 候选来源的信任状态，不是候选自身 metadata 的自我背书。
///
/// 语义上与 `ExtensionVisibilityTrust` 的 Trusted/Untrusted 词汇相近，但这是独立 enum；
/// 它只允许把信任结论作为审计输入传递。当前字段是公开的，反序列化也能产生 `Verified`，
/// 所以真正的签名/信任根校验必须由外部准入适配器承担。
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ToolDiscoveryTrust {
    /// 上游声称发布者签名、包摘要和信任根均已校验。
    Verified,
    /// 来源存在，但尚未通过签名/信任校验。
    Unverified,
    /// 已执行信任校验，但来源不可信。
    Denied,
}

impl ToolDiscoveryTrust {
    /// 返回用于序列化或稳定报告的信任状态名称。
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Verified => "verified",
            Self::Unverified => "unverified",
            Self::Denied => "denied",
        }
    }
}

/// 读取候选的 catalog 或 server 的健康状态。
///
/// `Degraded` / `Stale` 候选会被拒绝，因为升级过程返回的部分目录可能被误认为工具已被正式删除。
/// 该 enum 只传递适配器观察到的状态，本文件不会探测远端健康情况。
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ToolDiscoveryHealth {
    Healthy,
    Degraded,
    Stale,
}

impl ToolDiscoveryHealth {
    /// 返回用于序列化或稳定报告的健康状态名称。
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Healthy => "healthy",
            Self::Degraded => "degraded",
            Self::Stale => "stale",
        }
    }
}

/// 扩展或 server 提交的单个候选工具描述，在准入判定之前的数据传输对象。
///
/// 这里的值都是“候选声明”：tool name、capability、operation 是提交方声称的身份。reducer
/// 会查名称/operation 是否与内建绑定冲突，但当前不会把每个新 capability/operation 与完整
/// server-owned action/handler allow-list 对账。字段通过格式校验不代表来源或语义已被认证。
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ToolDiscoveryCandidate {
    /// 候选 DTO 的合同名称。
    pub schema: String,
    /// 候选 DTO 的格式版本。
    pub version: SchemaVersion,
    /// 提交方声称的工具名；未知或冲突名称不会因出现在这里而自动获得可见性。
    pub tool_name: String,
    /// 适配器应当观察到的来源类别。
    pub origin: ToolDiscoveryOrigin,
    /// 来源标识，例如本地扩展或 MCP server 的稳定 ID；它不是验证过的签名。
    pub source_id: String,
    /// 上游产生的信任判定；本 reducer 不校验产生该判定的证据。
    pub trust: ToolDiscoveryTrust,
    /// 上游观察到的目录/服务健康状态。
    pub health: ToolDiscoveryHealth,
    /// 上游应从已验证 manifest/registry 读取的 effect；不要从 descriptor body 直接采信。
    /// 当前构造器仍接受调用方传入此值，所以该约束依赖上游 adapter。
    pub effect: ExtensionEffect,
    /// 提交方声明的 capability 类别。
    pub capability: String,
    /// 提交方声明的内部 operation 名。
    pub operation: String,
    /// 候选是否要求绑定到某个 project；该 flag 不包含候选绑定的具体 project ID。
    pub project_scoped: bool,
    /// 候选注册时使用的工具目录版本，必须与本次 plan 相同。
    pub catalog_version: SchemaVersion,
    /// 上游声称服务端已绑定该来源的角色集合；只有包含 plan.role_id 才可准入。
    /// 它应由 server adapter 设置，但此 struct 本身不证明绑定真实存在。
    pub bound_roles: BTreeSet<String>,
    /// 可选搜索标签；本 reducer 校验并纳入摘要，但当前不按标签排序。
    #[serde(default)]
    pub tags: BTreeSet<String>,
    /// 候选工具输入参数的 JSON Schema。
    pub argument_schema: Value,
}

impl ToolDiscoveryCandidate {
    /// 使用当前合同版本构造并立即校验一个候选 DTO。
    ///
    /// 【调用者】设计上由扩展/MCP/builtin 适配器在完成来源检查后调用；当前仓内没有生产调用点，
    /// 可见调用来自 Domain 合同测试。
    /// 【输入】候选名、来源/信任/健康/effect、能力与操作声明、project flag、角色绑定、标签和 schema。
    /// 【输出】字段齐全且结构合法时返回 `ToolDiscoveryCandidate`。
    /// 【失败】字段或 schema 无效时返回稳定错误字符串；构造成功不代表候选已准入。
    /// 【副作用/设计】只组装内存 DTO；把 validate 放在构造后可避免本地调用者意外持有格式错误值，
    /// 但不会验证签名（signature）、项目信任（project trust）、来源健康或角色绑定事实的真实性。
    /// 以下 lint 属性只关闭 Clippy 对参数过多的静态提醒，不改变运行时行为；逐项参数使每个候选事实
    /// 在调用点可见，读者也能看清信任、健康、effect 与 schema 分别从哪里传入。
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        tool_name: impl Into<String>,
        origin: ToolDiscoveryOrigin,
        source_id: impl Into<String>,
        trust: ToolDiscoveryTrust,
        health: ToolDiscoveryHealth,
        effect: ExtensionEffect,
        capability: impl Into<String>,
        operation: impl Into<String>,
        project_scoped: bool,
        bound_roles: BTreeSet<String>,
        tags: BTreeSet<String>,
        argument_schema: Value,
    ) -> Result<Self, String> {
        let candidate = Self {
            schema: TOOL_DISCOVERY_SCHEMA.to_owned(),
            version: TOOL_DISCOVERY_VERSION,
            tool_name: tool_name.into(),
            origin,
            source_id: source_id.into(),
            trust,
            health,
            effect,
            capability: capability.into(),
            operation: operation.into(),
            project_scoped,
            catalog_version: TOOL_CATALOG_VERSION,
            bound_roles,
            tags,
            argument_schema,
        };
        candidate.validate()?;
        Ok(candidate)
    }

    /// 检查候选 DTO 的合同版本、字符串边界、角色/标签和 schema 形状。
    ///
    /// 名称/capability/operation 各最长 128 字节，source_id 最长 256 字节，最多 16 个角色且每个
    /// 角色最长 64 字节，最多 32 个标签且每个标签最长 64 字节；字符串长度按 UTF-8 字节计算。
    /// `redact_text()` 仅用于 tool_name、source_id、bound_roles 和 tags；capability、operation
    /// 只检查非空与长度，不做同样的脱敏检查，也不对照权威注册表。失败时返回
    /// `tool_discovery_candidate_*` 错误。`redact_text(value) == value` 只表示当前文本脱敏器不会
    /// 改写该值，不是完整的秘密扫描或来源认证。
    /// 后续 `first_violation()` 只检查有限关系：角色是否出现在候选自带的 bound_roles、ReadOnly
    /// effect 是否与已列举的写能力前缀冲突，以及 plan effect ceiling；它不查询角色 allow-list，
    /// 不验证新 capability/operation 已注册，也不认证 trust/effect 事实的来源。
    pub fn validate(&self) -> Result<(), String> {
        if self.schema != TOOL_DISCOVERY_SCHEMA
            || !self.version.is_compatible_with(&TOOL_DISCOVERY_VERSION)
            || self.tool_name.trim().is_empty()
            || self.tool_name.len() > 128
            || self.tool_name.contains(['\0', '\n', '\r', ' '])
            || redact_text(&self.tool_name) != self.tool_name
            || self.source_id.trim().is_empty()
            || self.source_id.len() > 256
            || self.source_id.contains(['\0', '\n', '\r'])
            || redact_text(&self.source_id) != self.source_id
            || self.capability.trim().is_empty()
            || self.capability.len() > 128
            || self.operation.trim().is_empty()
            || self.operation.len() > 128
            || self.bound_roles.is_empty()
            || self.bound_roles.len() > 16
            || self.tags.len() > MAX_TOOL_DISCOVERY_TAGS
            || self
                .argument_schema
                .get("type")
                .and_then(Value::as_str)
                .is_none()
        {
            return Err("tool_discovery_candidate_invalid".to_owned());
        }
        for role in &self.bound_roles {
            if role.trim().is_empty() || role.len() > 64 || redact_text(role) != *role {
                return Err("tool_discovery_candidate_role_invalid".to_owned());
            }
        }
        for tag in &self.tags {
            if tag.trim().is_empty()
                || tag.len() > MAX_TOOL_DISCOVERY_TAG_BYTES
                || tag.contains(['\0', '\n', '\r'])
                || redact_text(tag) != *tag
            {
                return Err("tool_discovery_candidate_tag_invalid".to_owned());
            }
        }
        crate::validate_schema_contract(&self.argument_schema)
            .map_err(|error| format!("tool_discovery_candidate_schema_invalid:{error}"))?;
        Ok(())
    }

    /// 使用 `serde_json::json!` 组合候选字段，再由 `json_digest()` 生成无密钥 SHA-256 摘要，
    /// 用于报告绑定和内容漂移比较。
    ///
    /// 摘要不签名、不证明 publisher 或 adapter 身份；字段顺序和候选来源真实性仍由上游保证。
    pub fn digest(&self) -> String {
        json_digest(&json!({
            "schema": self.schema,
            "version": self.version,
            "tool_name": self.tool_name,
            "origin": self.origin,
            "source_id": self.source_id,
            "trust": self.trust,
            "health": self.health,
            "effect": self.effect,
            "capability": self.capability,
            "operation": self.operation,
            "project_scoped": self.project_scoped,
            "catalog_version": self.catalog_version,
            "bound_roles": self.bound_roles,
            "tags": self.tags,
            "argument_schema": self.argument_schema,
        }))
    }
}

/// 候选未进入本次可搜索集合的原因。
///
/// 正常拒绝会映射到这里的稳定类别；但整份 plan 无效或候选数超限时，报告状态会是 Rejected，
/// 并且不生成逐候选 denial 条目。
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ToolDiscoveryDenial {
    /// 候选登记时使用的目录版本与本次 plan 不同。
    CatalogVersionStale,
    /// 候选来源未被标记为 Verified。
    SourceUntrusted,
    /// 候选来源的目录或 server 不是 Healthy 状态。
    SourceUnhealthy,
    /// 候选被判定为项目不匹配；当前条件见 `first_violation()`，该分支受限于现有 DTO 字段。
    ProjectMismatch,
    /// 候选的 bound_roles 不包含 plan 中的 role_id。
    RoleNotBound,
    /// plan 的 effect ceiling 低于候选声明的 effect。
    EffectInsufficient,
    /// 候选 effect 为 ReadOnly，却声明了 filesystem/process/secret/computer/network 写能力类别。
    ReadOnlyWriteDenied,
    /// 候选工具名或 operation 与本次计划中已准入候选重复。
    DuplicateToolName,
    /// 候选名称命中内建 ToolSpec，或非 builtin 来源声明了内建 operation。
    BuiltinBindingConflict,
    /// 候选自身结构错误，或计划/候选数量等合同边界不可用。
    PlanInvalid,
}

impl ToolDiscoveryDenial {
    /// 返回给日志、协议或界面使用的稳定拒绝原因字符串。
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::CatalogVersionStale => "catalog_version_stale",
            Self::SourceUntrusted => "source_untrusted",
            Self::SourceUnhealthy => "source_unhealthy",
            Self::ProjectMismatch => "project_mismatch",
            Self::RoleNotBound => "role_not_bound",
            Self::EffectInsufficient => "effect_insufficient",
            Self::ReadOnlyWriteDenied => "read_only_write_denied",
            Self::DuplicateToolName => "duplicate_tool_name",
            Self::BuiltinBindingConflict => "builtin_binding_conflict",
            Self::PlanInvalid => "plan_invalid",
        }
    }

    /// 返回同一候选违反多个规则时使用的固定优先级，数字越小越先报告。
    ///
    /// 该顺序让相同输入稳定得到相同拒绝原因；它表达报告选择顺序，不代表后续检查会修复候选。
    const fn precedence(self) -> u8 {
        match self {
            // 整个 plan 无效时，不会产生逐候选拒绝项。
            Self::PlanInvalid => 0,
            // 结构冲突先于 trust：与内建绑定冲突本身比“来源不可信”更具体，不能用后者掩盖替换风险。
            Self::BuiltinBindingConflict => 1,
            Self::DuplicateToolName => 2,
            // 之后检查信任和身份；来源不可信时不应把它继续解释成可准入工具。
            Self::SourceUntrusted => 3,
            Self::CatalogVersionStale => 4,
            Self::SourceUnhealthy => 5,
            Self::ProjectMismatch => 6,
            Self::RoleNotBound => 7,
            // 最后才比较 effect ceiling；只有候选先通过结构、身份和信任检查，才报告权限范围不够。
            Self::EffectInsufficient => 8,
            Self::ReadOnlyWriteDenied => 9,
        }
    }
}

/// 服务端为一次工具发现提供的筛选计划。
///
/// 它绑定角色、项目、effect ceiling、目录版本/digest 和结果/上下文预算，不包含发现结果。
/// `new()` 会从已校验 catalog 复制当前 digest；反序列化后单独调用 `validate()` 只检查摘要形状
/// 与本 plan 自身 digest，不会证明 project/role 来自真实 authority，也不会重新读取 catalog。
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ToolDiscoveryPlan {
    /// 计划合同名称。
    pub schema: String,
    /// 计划格式版本。
    pub version: SchemaVersion,
    /// 服务端希望为其筛选工具的角色 ID。
    pub role_id: String,
    /// 当前服务范围中的项目 ID。
    pub project_id: String,
    /// 本次结果允许达到的 effect 上限；ReadOnly 计划不能准入写工具。
    pub effect_ceiling: ExtensionEffect,
    /// 计划绑定的目录合同版本。
    pub catalog_version: SchemaVersion,
    /// 计划绑定的目录内容 digest；digest 字符串本身不是签名或 authority token。
    pub catalog_digest: String,
    /// 最多纳入 report 的结果数。
    pub max_results: usize,
    /// 选中 schema 的估算上下文 token 上限；估算方式是 schema 字节数除以 4。
    pub max_context_tokens: usize,
    /// 对上述字段生成的无密钥 plan digest，用于发现字段被修改。
    pub plan_digest: String,
}

impl ToolDiscoveryPlan {
    /// 以已通过校验的工具目录创建发现计划，并立即计算/校验自身摘要。
    ///
    /// 【调用者】设计上由服务端 discovery adapter 在确定当前角色、项目和预算后调用；目前未发现生产调用者。
    /// 【输入】角色/项目 ID、effect ceiling、当前 `ToolCatalogSnapshot`、结果数与上下文预算。
    /// 【输出】绑定目录版本和 digest 的计划值。
    /// 【失败】catalog 校验失败或新计划字段/边界无效时返回稳定错误。
    /// 【副作用】只读 snapshot 并在内存中建值；不读取当前用户/项目 authority，也不开始搜索。
    /// 构造结果的可靠性仍取决于调用方传入的角色、项目和 effect 来源。以下 lint 属性只抑制
    /// 参数数量提醒，不改变运行时行为；逐项参数让目录、身份和预算在调用点分别可见。
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        role_id: impl Into<String>,
        project_id: impl Into<String>,
        effect_ceiling: ExtensionEffect,
        catalog: &ToolCatalogSnapshot,
        max_results: usize,
        max_context_tokens: usize,
    ) -> Result<Self, String> {
        catalog.validate()?;
        let mut plan = Self {
            schema: TOOL_DISCOVERY_PLAN_SCHEMA.to_owned(),
            version: TOOL_DISCOVERY_VERSION,
            role_id: role_id.into(),
            project_id: project_id.into(),
            effect_ceiling,
            catalog_version: catalog.version,
            catalog_digest: catalog.digest.clone(),
            max_results,
            max_context_tokens,
            plan_digest: String::new(),
        };
        plan.plan_digest = plan.digest();
        plan.validate()?;
        Ok(plan)
    }

    /// 检查 plan 的 schema/version、角色/项目文本、目录版本、预算范围和 plan_digest。
    ///
    /// `max_results` 上限为 128，context token 上限为 1,048,576；字符串长度按 UTF-8 字节统计。
    /// 这里仅要求 catalog digest 长得像 `sha256:` 摘要，不会对照实时 catalog 验证其值，也不会
    /// 从外部 authority 查询 role_id/project_id。因此它是 DTO 自校验，不是请求者认证。
    pub fn validate(&self) -> Result<(), String> {
        if self.schema != TOOL_DISCOVERY_PLAN_SCHEMA
            || !self.version.is_compatible_with(&TOOL_DISCOVERY_VERSION)
            || self.role_id.trim().is_empty()
            || self.role_id.len() > 64
            || self.project_id.trim().is_empty()
            || self.project_id.len() > 256
            || redact_text(&self.role_id) != self.role_id
            || redact_text(&self.project_id) != self.project_id
            || self.catalog_version != TOOL_CATALOG_VERSION
            || !self.catalog_digest.starts_with("sha256:")
            || self.max_results == 0
            || self.max_results > 128
            || self.max_context_tokens == 0
            || self.max_context_tokens > 1_048_576
        {
            return Err("tool_discovery_plan_invalid".to_owned());
        }
        if self.plan_digest != self.digest() {
            return Err("tool_discovery_plan_digest_mismatch".to_owned());
        }
        Ok(())
    }

    /// 对除 `plan_digest` 自身以外的 plan 字段生成规范 JSON 内容摘要。
    ///
    /// 摘要可以发现计划字段变化，但没有密钥，不能证明计划由可信服务端签发。
    pub fn digest(&self) -> String {
        json_digest(&json!({
            "schema": self.schema,
            "version": self.version,
            "role_id": self.role_id,
            "project_id": self.project_id,
            "effect_ceiling": self.effect_ceiling,
            "catalog_version": self.catalog_version,
            "catalog_digest": self.catalog_digest,
            "max_results": self.max_results,
            "max_context_tokens": self.max_context_tokens,
        }))
    }
}

/// 本次发现结果能否包含至少一个可展示给模型的 schema。
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ToolDiscoveryStatus {
    /// 至少一个候选通过判定并纳入报告。
    Admitted,
    /// 没有候选被纳入；这可以是正常的空结果，不等同于 plan 解析失败。
    Empty,
    /// plan 本身无效或候选列表超限，因此没有执行逐候选准入判定。
    Rejected,
}

impl ToolDiscoveryStatus {
    /// 返回报告中使用的稳定状态名。
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Admitted => "admitted",
            Self::Empty => "empty",
            Self::Rejected => "rejected",
        }
    }
}

/// 一次 discovery pass 的有序判定报告及其输入绑定信息。
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ToolDiscoveryReport {
    /// 报告合同名称。
    pub schema: String,
    /// 报告格式版本。
    pub version: SchemaVersion,
    /// 生成报告时计划绑定的角色 ID。
    pub role_id: String,
    /// 生成报告时计划绑定的项目 ID。
    pub project_id: String,
    /// Admitted、Empty 或 Rejected；只描述搜索可见性，不表示执行结果。
    pub status: ToolDiscoveryStatus,
    /// 本报告使用的目录合同版本。
    pub catalog_version: SchemaVersion,
    /// 本报告复制的目录 digest；不会验证该目录仍对应当前实时目录。
    pub catalog_digest: String,
    /// 按准入顺序记录的候选名；后续只有这些候选可以扩展成模型 schema。
    pub admitted_tools: Vec<String>,
    /// 按工具名排序的拒绝映射；键是候选提交的名称，即使该名称自身格式不合格也不能视为已认证身份。
    pub denials: BTreeMap<String, ToolDiscoveryDenial>,
    /// 最终纳入报告的 schema JSON 字节数。
    pub expanded_schema_bytes: usize,
    /// 以 schema 字节数除以 4 得到的上下文 token 估值，不是 Provider 实际用量回执。
    pub estimated_context_tokens: usize,
    /// 当前必须为 true：准入 schema 不会授予 executor、capability 或 grant。
    pub does_not_grant_execution: bool,
    /// 报告所依据的 plan digest。
    pub plan_digest: String,
    /// 对报告字段生成的无密钥内容摘要。
    pub report_digest: String,
}

impl ToolDiscoveryReport {
    /// 按 plan 和候选列表重新计算一次准入决策并生成报告。
    ///
    /// 【调用者】设计上由搜索入口在扩展结果展示前调用；当前未发现生产调用者。
    /// 【输入】计划和候选切片；【输出】包含准入名称、拒绝原因、schema 字节数/token 估值和摘要的报告。
    /// 【失败】本地生成后会通过 `validate_against()` 自检，若绑定字段不一致则返回错误。
    /// 【副作用】纯内存判定，不加载候选来源、不访问 broker、不创建 CapabilityRequest、不记录事件。
    /// 即使返回 Admitted，也仅表示满足本文件基于输入事实的筛选规则。
    pub fn evaluate(
        plan: &ToolDiscoveryPlan,
        candidates: &[ToolDiscoveryCandidate],
    ) -> Result<Self, String> {
        let decision = derive(plan, candidates);
        let mut report = Self {
            schema: TOOL_DISCOVERY_REPORT_SCHEMA.to_owned(),
            version: TOOL_DISCOVERY_VERSION,
            role_id: plan.role_id.clone(),
            project_id: plan.project_id.clone(),
            status: decision.status,
            catalog_version: plan.catalog_version,
            catalog_digest: plan.catalog_digest.clone(),
            admitted_tools: decision.admitted,
            denials: decision.denials,
            expanded_schema_bytes: decision.schema_bytes,
            estimated_context_tokens: decision.tokens,
            does_not_grant_execution: true,
            plan_digest: plan.plan_digest.clone(),
            report_digest: String::new(),
        };
        report.report_digest = report.digest();
        report.validate_against(plan, candidates)?;
        Ok(report)
    }

    /// 根据原 plan 与候选列表重新推导并比较报告的每个字段，拒绝被改写的结果。
    ///
    /// 【调用者】`evaluate()` 生成报告后自检；消费者也可在使用缓存/反序列化报告前再次调用。
    /// 【输入】原计划和用于产生报告的候选列表；【输出】完全一致返回 `Ok(())`，否则返回错误码。
    /// 【限制】没有持久化来源证据，也不认证 plan/candidate 的提供者；使用相同伪造输入仍会得到
    /// 一致报告。context 预算检查针对报告中的估算值，不是模型实际 token 使用量。
    pub fn validate_against(
        &self,
        plan: &ToolDiscoveryPlan,
        candidates: &[ToolDiscoveryCandidate],
    ) -> Result<(), String> {
        let decision = derive(plan, candidates);
        if self.schema != TOOL_DISCOVERY_REPORT_SCHEMA
            || !self.version.is_compatible_with(&TOOL_DISCOVERY_VERSION)
            || self.role_id != plan.role_id
            || self.project_id != plan.project_id
            || self.status != decision.status
            || self.catalog_version != plan.catalog_version
            || self.catalog_digest != plan.catalog_digest
            || self.admitted_tools != decision.admitted
            || self.denials != decision.denials
            || self.expanded_schema_bytes != decision.schema_bytes
            || self.estimated_context_tokens != decision.tokens
            || self.plan_digest != plan.plan_digest
        {
            return Err("tool_discovery_report_binding_invalid".to_owned());
        }
        if !self.does_not_grant_execution {
            return Err("tool_discovery_report_grants_execution".to_owned());
        }
        if self.estimated_context_tokens > plan.max_context_tokens {
            return Err("tool_discovery_context_budget_exceeded".to_owned());
        }
        if self.report_digest != self.digest() {
            return Err("tool_discovery_report_digest_mismatch".to_owned());
        }
        Ok(())
    }

    /// 返回状态是否为 Admitted；这是“至少有一个候选进入报告”的便捷判断，不是执行许可。
    pub fn admitted(&self) -> bool {
        self.status == ToolDiscoveryStatus::Admitted
    }

    /// 对报告字段生成无密钥摘要；用于发现序列化报告被改动，不验证来源身份。
    pub fn digest(&self) -> String {
        json_digest(&json!({
            "schema": self.schema,
            "version": self.version,
            "role_id": self.role_id,
            "project_id": self.project_id,
            "status": self.status,
            "catalog_version": self.catalog_version,
            "catalog_digest": self.catalog_digest,
            "admitted_tools": self.admitted_tools,
            "denials": self.denials,
            "expanded_schema_bytes": self.expanded_schema_bytes,
            "estimated_context_tokens": self.estimated_context_tokens,
            "does_not_grant_execution": self.does_not_grant_execution,
            "plan_digest": self.plan_digest,
        }))
    }
}

/// 被本规则视为可能改变外部状态的五类 capability 名称。
///
/// 只读 effect 的候选如果声明其中之一会被拒绝，不会把写能力“降级”成只读能力；列表是字符串
/// 前缀规则，不是完整的 CapabilityKind authority 表。
const WRITE_CAPABILITIES: [&str; 5] = ["filesystem", "process", "secret", "computer", "network"];

/// 判断 capability 名是否等于某个 effect-bearing 类别，或以该类别加下划线开头。
///
/// 先转成 ASCII 小写再匹配，避免大小写差异绕过；允许下划线后缀是为兼容更细的类别名称。
/// 这只是 ReadOnly 候选的拒绝辅助判断，不负责完整 capability 名称解析。
fn is_write_capability(capability: &str) -> bool {
    let lowered = capability.to_ascii_lowercase();
    WRITE_CAPABILITIES
        .iter()
        .any(|known| lowered == *known || lowered.starts_with(&format!("{known}_")))
}

/// `derive()` 暂存的纯内存判定结果，尚未附加协议字段和 digest。
struct Decision {
    /// 由准入数量推导出的 Admitted/Empty/Rejected 状态。
    status: ToolDiscoveryStatus,
    /// 按候选输入顺序收集的准入名称，之后还会经过结果数和上下文预算筛选。
    admitted: Vec<String>,
    /// 以候选声明名为 key 的拒绝原因映射。
    denials: BTreeMap<String, ToolDiscoveryDenial>,
    /// 最终保留的 JSON schema 序列化字节数。
    schema_bytes: usize,
    /// 由 `schema_bytes / 4` 向上取整得到的 token 估值。
    tokens: usize,
}

/// 根据固定顺序计算整批候选的准入、拒绝和 schema 预算结果。
///
/// 【调用者】`ToolDiscoveryReport::evaluate()` 和 `validate_against()` 调用；当前未发现生产调用者。
/// 【输入/输出】输入 plan 与有序 candidate 切片；输出 Decision，不产生 IO。
/// 【失败处理】plan 无效或候选多于 512 个时返回 Rejected 空结果；其他候选逐个使用
/// `first_violation()` 选择一个优先级最高的拒绝原因。准入项之后再按 max_results 和上下文预算筛选。
/// 【副作用】无，所有集合、预算和报告字段都在内存中计算。
/// 【为什么固定顺序】同一输入必须得到稳定原因；内建冲突优先于信任/effect，避免把试图占用
/// 内建 operation 的候选弱化描述成普通权限不足。
fn derive(plan: &ToolDiscoveryPlan, candidates: &[ToolDiscoveryCandidate]) -> Decision {
    let mut decision = Decision {
        status: ToolDiscoveryStatus::Empty,
        admitted: Vec::new(),
        denials: BTreeMap::new(),
        schema_bytes: 0,
        tokens: 0,
    };
    if plan.validate().is_err() || candidates.len() > MAX_TOOL_DISCOVERY_CANDIDATES {
        return Decision {
            status: ToolDiscoveryStatus::Rejected,
            admitted: Vec::new(),
            denials: BTreeMap::new(),
            schema_bytes: 0,
            tokens: 0,
        };
    }

    // 只把已准入项记入 seen 集合；被拒绝的声明不会占用其他候选的名称/operation。
    // ⚠ 因而重复检查针对的是“之前已准入项”，不是整份原始候选列表：前一个同名候选若因其他
    // 原因被拒绝，后续同名项仍可能准入；denials 以名称为 key，之后的同名拒绝还可能覆盖前因。
    // 下方预算阶段也按名称查找第一个原始候选，维护者调整这两段时要一起复核重复名语义。
    let mut seen_names = BTreeSet::new();
    let mut seen_operations = BTreeSet::new();
    for candidate in candidates {
        let name = candidate.tool_name.clone();
        if let Some(denial) = first_violation(plan, candidate, &seen_names, &seen_operations) {
            decision.denials.insert(name, denial);
            continue;
        }
        seen_names.insert(name.clone());
        seen_operations.insert(candidate.operation.clone());
        decision.admitted.push(name);
    }

    // 只有准入名称会扩展成用于估算的 JSON 对象。max_results 限制最终保留数量；上下文预算逐项检查，
    // 超预算候选会被拒绝后继续查看后续候选，因此输入顺序会影响先占用预算的工具。
    // bytes / 4 向上取整只是 schema token 估算方法，与 search_tool_catalog 一致，不是 Provider 回执。
    // mem::take 将已准入名称移出 decision，并把原字段替换为空 Vec；下面筛选完成后再写回 kept。
    let admitted = std::mem::take(&mut decision.admitted);
    let mut kept = Vec::with_capacity(admitted.len().min(plan.max_results));
    let mut budget = 0usize;
    for name in admitted {
        if kept.len() >= plan.max_results {
            // 已达到结果数量上限；该候选之前通过身份准入，但不能再扩展进本次 report。
            decision
                .denials
                .insert(name.clone(), ToolDiscoveryDenial::PlanInvalid);
            continue;
        }
        // serde_json::json! 组装与工具 schema 类似的 name/description/parameters 对象；
        // to_vec 序列化该对象，以 JSON 的实际 UTF-8 字节长度估算上下文占用，不会把此对象存进报告。
        // ⚠ 按名称取原列表第一个匹配项，而不是保留准入时的候选引用；若更早的同名候选被拒绝，
        // 这里可能对那一项的 schema 计费。report 只保存名称和估值，不包含 schema 本体。
        let Some(candidate) = candidates
            .iter()
            .find(|candidate| candidate.tool_name == name)
        else {
            continue;
        };
        let bytes = serde_json::to_vec(&json!({
            "name": candidate.tool_name,
            "description": candidate.operation,
            "parameters": candidate.argument_schema,
        }))
        .map(|bytes| bytes.len())
        // JSON Value 的序列化通常由其受限数据类型保证；若未来此对象加入可失败序列化字段，
        // 这个兜底会把失败当成 0 字节，从而少计预算，需要一并重新审查。
        .unwrap_or(0);
        // admission 比较值额外加 1 个估算 token margin；最终 report.tokens 只记录 ceil(bytes / 4)，
        // 不包含这 1 token，因此报告估值不是“预算检查值”的逐字回显。
        if budget.saturating_add(bytes).div_ceil(4).saturating_add(1) > plan.max_context_tokens {
            decision
                .denials
                .insert(name.clone(), ToolDiscoveryDenial::PlanInvalid);
            continue;
        }
        // 只有通过本项预算检查才增加累计字节数并保留候选 schema。
        budget += bytes;
        kept.push(name);
    }
    decision.admitted = kept;
    decision.schema_bytes = budget;
    decision.tokens = budget.div_ceil(4);
    if decision.admitted.is_empty() {
        decision.status = ToolDiscoveryStatus::Empty;
    } else {
        decision.status = ToolDiscoveryStatus::Admitted;
    }
    decision
}

/// 返回单个候选当前命中的最高优先级拒绝原因。
///
/// 【调用者】`derive()` 对候选列表按输入顺序调用，候选成功后才加入去重集合。
/// 【输入】已校验的 plan、一个 candidate，以及此前已准入的名称和 operation 集合。
/// 【输出】无拒绝时返回 `None`；有拒绝时按 `ToolDiscoveryDenial::precedence()` 返回一个原因。
/// 【失败/副作用】它不抛错误、不读外部状态、不修改集合；candidate 结构错误统一映射为
/// `PlanInvalid`。其他拒绝原因由 `derive()` 放入 report.denials，并不会触发重试、禁用或事件记录。
/// 【核心顺序】先验候选结构，再收集内建冲突、已准入重复、trust/version/health、角色与有限 effect
/// 关系，最后按固定 precedence 取一项；排序决定报告文字，不代表此函数完成完整授权。
/// 【边界】角色是否允许该工具、project 是否真实匹配、capability/operation 是否登记以及这些事实
/// 是否由可信 adapter 提供，均不能由当前候选字段本身证明；报告也不授予执行权限。
fn first_violation(
    plan: &ToolDiscoveryPlan,
    candidate: &ToolDiscoveryCandidate,
    seen_names: &BTreeSet<String>,
    seen_operations: &BTreeSet<String>,
) -> Option<ToolDiscoveryDenial> {
    if candidate.validate().is_err() {
        // Candidate 名称格式也可能有误，不能把 map key 当作认证身份；但调用方目前仍会以
        // candidate.tool_name 作为 denial key 写入 report，只能把它理解为原始声明文本。
        return Some(ToolDiscoveryDenial::PlanInvalid);
    }
    let mut violations = Vec::new();

    // 先检查名字或 operation 是否会占用现有内建绑定；外部来源按 operation 身份冲突拒绝，
    // 而不是只看展示名称。repository-local 候选的规则在 shadows_builtin() 中略有不同。
    if shadows_builtin(candidate) {
        violations.push(ToolDiscoveryDenial::BuiltinBindingConflict);
    }
    if seen_names.contains(&candidate.tool_name) || seen_operations.contains(&candidate.operation) {
        violations.push(ToolDiscoveryDenial::DuplicateToolName);
    }
    if candidate.trust != ToolDiscoveryTrust::Verified {
        violations.push(ToolDiscoveryDenial::SourceUntrusted);
    }
    if candidate.catalog_version != plan.catalog_version {
        violations.push(ToolDiscoveryDenial::CatalogVersionStale);
    }
    if candidate.health != ToolDiscoveryHealth::Healthy {
        violations.push(ToolDiscoveryDenial::SourceUnhealthy);
    }
    // ⚠ 当前 candidate 只有 project_scoped 布尔值，没有 candidate_project_id 可与 plan 对比；
    // 同时有效 plan.validate() 已拒绝空 project_id，因此对有效 plan 该 mismatch 分支不可达，
    // 不能把它当作候选与实际项目绑定一致性的检查。
    if candidate.project_scoped && plan.project_id.is_empty() {
        violations.push(ToolDiscoveryDenial::ProjectMismatch);
    }
    if !candidate.bound_roles.contains(&plan.role_id) {
        violations.push(ToolDiscoveryDenial::RoleNotBound);
    }
    if candidate.effect == ExtensionEffect::ReadOnly && is_write_capability(&candidate.capability) {
        violations.push(ToolDiscoveryDenial::ReadOnlyWriteDenied);
    }
    if plan.effect_ceiling == ExtensionEffect::ReadOnly
        && candidate.effect != ExtensionEffect::ReadOnly
    {
        violations.push(ToolDiscoveryDenial::EffectInsufficient);
    }
    violations.sort_by_key(|denial| denial.precedence());
    violations.into_iter().next()
}

/// 对外复用 reducer 的内建绑定冲突判定。
///
/// 【调用者】未来注册入口可复用该规则；当前未发现生产调用点。
/// 【输入/输出】接收候选并返回是否命中本文件识别的内建名称/operation 冲突。
/// 【失败/副作用】没有错误返回或写操作；候选不会因调用此函数而被校验、注册或修改。
/// 返回 true 只是冲突提示，不是 Broker 查询结果，也不是执行许可。
pub fn shadows_builtin_descriptor(candidate: &ToolDiscoveryCandidate) -> bool {
    shadows_builtin(candidate)
}

/// 判断候选是否命中内建名称或试图复用内建 operation。
///
/// 任何来源只要名称能通过 `tool_spec()` 解析为内建名称/alias 就冲突；仓库内建来源若使用新名，
/// 不再按 operation 与目录比较，因为其整体 surface 应随仓库代码审查。扩展/MCP 来源会与当前
/// ToolCatalogSnapshot 的 operation 名比对。注意：实际代码比较的是 operation，不是 capability/operation 二元组。
///
/// 【调用者】`first_violation()` 在候选准入时调用，公开包装 `shadows_builtin_descriptor()` 也转调此函数。
/// 【输入/输出】输入候选声明，输出命中与否；它不要求候选已通过 `validate()`。
/// 【失败处理】若当前静态 ToolCatalogSnapshot 自身校验失败，外部来源一律按冲突返回 true，
/// 使 discovery 失败关闭；本函数不把 catalog 错误细节返回给调用者。
/// 【限制】只查 ToolSpec 名称/alias 和 operation 字符串，不检查 capability 是否登记，也不验证来源真实性。
fn shadows_builtin(candidate: &ToolDiscoveryCandidate) -> bool {
    if tool_spec(&candidate.tool_name).is_some() {
        return true;
    }
    if candidate.origin.is_repository_local() {
        return false;
    }
    if ToolCatalogSnapshot::current().validate().is_err() {
        return true;
    }
    ToolCatalogSnapshot::current()
        .tools
        .iter()
        .any(|tool| tool.operation == candidate.operation)
}

/// 校验 manifest 声称的 effect 是否足以覆盖当前已识别的写能力类别。
///
/// 【调用者】未来扩展发现 adapter 应在把 manifest 事实复制到候选前调用；当前未发现生产调用点。
/// 【输入/输出】输入 `ExtensionManifest`；字段合法且没有只读/写能力冲突时返回 `Ok(())`。
/// 【失败情况】`manifest.validate()` 失败时将其静态错误码复制成 `String` 返回；若 effect 为
/// ReadOnly 且 required_capabilities 命中本文件列出的 filesystem/process/secret/computer/network
/// 类别（含下划线后缀），返回 `tool_discovery_manifest_read_only_write_scope`。其他未知 capability
/// 名不会仅因未识别而被此函数拒绝。
/// 【副作用/设计】纯 DTO 检查；`manifest.validate()` 会校验签名字段的格式，但这里不做 Ed25519
/// 密码学验签、不读包、不查询 trust store，也不改变 manifest。调用方仍须先完成真实的签名和来源准入。
/// 它避免把已列举的写能力与 ReadOnly 标签同时带入后续 reducer，但不是完整 capability 注册表或执行授权检查。
pub fn check_manifest_effect(manifest: &ExtensionManifest) -> Result<(), String> {
    manifest.validate().map_err(str::to_owned)?;
    if manifest.effect == ExtensionEffect::ReadOnly
        && manifest
            .required_capabilities
            .iter()
            .any(|capability| is_write_capability(capability))
    {
        return Err("tool_discovery_manifest_read_only_write_scope".to_owned());
    }
    Ok(())
}

//! # 文件职责
//!
//! 本文件定义一次 capability 动作可使用的执行范围快照 `ExecutionScope`，并负责检查快照
//! 内部字段是否自洽、它是否仍绑定到当前请求。它属于最底层 `kiana-domain`：保存跨 Core、
//! Broker、策略、审批及 handler adapter 共用的数据合同，不读取数据库、不查询文件系统，也
//! 不执行任何能力。
//!
//! # 调用位置与数据流
//!
//! ```text
//! Runner / adapter 提出 CapabilityRequest
//!   -> Core 规范化请求并从服务端 RequestContext / ScopeSet 构造本快照
//!   -> Domain validate() 检查快照内部约束与自摘要
//!   -> Core 将请求交给策略、Gate、审批和已提交 permit 流程
//!   -> Capability Broker 在 handler 派发前再次核对规范请求、目录摘要和本快照
//!   -> handler 只在授权链成功后执行
//! ```
//!
//! 主要上游是 `kiana-core/src/capabilities.rs::build_execution_scope()`；主要下游是
//! `kiana-capability-broker/src/lib.rs::validate_action()`，并由 Core 的恢复/请求流程读取。
//! `kiana-domain/src/capabilities.rs` 定义请求与 `ScopeSet`，`identity.rs` 定义主体/项目值，
//! `journal.rs` 提供确定性 JSON 编码。应把本文件与这些合同一起阅读。
//!
//! # 权限边界
//!
//! Scope 是服务端构造的输入快照，不是 permit、签名或授权票据。`validate()` 主要检查格式、
//! 必填值、列表边界和摘要自洽；只有 Core 的授权链提交 permit 后，Broker 才能执行副作用。
//! 无密钥 SHA-256 digest 能发现内容变化，却不能证明快照来自可信进程，也不能代替与最新
//! authority/trust/data/cancellation 状态比较。模型传入的 scope 会在 Core 规范化时清除并重建。
//!
//! Serde 是 Rust 的序列化库：`Serialize` 把结构体变为 JSON 等外部数据，`Deserialize` 把
//! 外部数据还原为结构体；`deny_unknown_fields` 会拒绝当前版本不认识的额外字段，避免版本
//! 错配被静默解释。`Option` 与 `serde(default)` 表示某些运行关联可以缺省，但缺省值不代表
//! 已验证对应能力或资源。

use crate::{
    canonical_journal_bytes, json_digest, AuthenticatedPrincipalRef, BudgetLeaseId,
    CapabilityGrantId, CapabilityRequest, CellId, ProjectIdentity, RunId, SchemaVersion, ScopeSet,
    SessionId, TurnId,
};
use serde::{Deserialize, Serialize};

/// ExecutionScope JSON 合同的稳定名称；消费者用它拒绝其他结构的载荷。
pub const EXECUTION_SCOPE_SCHEMA: &str = "kiana.execution-scope.v1";
/// 当前兼容的合同版本；不兼容版本不能仅凭字段碰巧相同而继续使用。
pub const EXECUTION_SCOPE_SCHEMA_VERSION: SchemaVersion = SchemaVersion::new(1, 0);
/// 所有字符串列表共享的元素数上限（256 项），并非仅限制文件路径。
pub const MAX_EXECUTION_SCOPE_PATHS: usize = 256;
/// capability grant 引用列表允许的最大数量（128 项）。
pub const MAX_EXECUTION_SCOPE_REFS: usize = 128;

/// 检查字段是否为 `sha256:` 前缀加 64 位十六进制文本。
///
/// 这里只检查形状，不重新计算被引用内容、不比较外部权威值，也不验证签名；返回 `Ok` 仅
/// 表示字符串长得像 SHA-256 digest。错误会带调用方传入的字段名，供上层映射稳定错误码。
fn digest(value: &str, field: &str) -> Result<(), String> {
    let Some(hex) = value.strip_prefix("sha256:") else {
        return Err(format!("{field}_invalid"));
    };
    if hex.len() != 64 || !hex.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err(format!("{field}_invalid"));
    }
    Ok(())
}

/// 检查任一受限字符串列表的数量和单项格式。
///
/// 每个列表最多 256 项；每项必须非空、最多 4,096 个 UTF-8 字节且不含 NUL。这个检查不把
/// 文本解释成 canonical filesystem path，也不校验项目 trust；路径解析和权限判断属于上层。
fn bounded_list(values: &[String], field: &str) -> Result<(), String> {
    if values.len() > MAX_EXECUTION_SCOPE_PATHS
        || values
            .iter()
            .any(|value| value.trim().is_empty() || value.len() > 4_096 || value.contains('\0'))
    {
        return Err(format!("{field}_invalid"));
    }
    Ok(())
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
/// 一次服务端准备动作可使用的身份、资源范围、版本围栏和摘要快照。
///
/// Core 在授权前由当前 RequestContext、CapabilityRequest 和有效 ScopeSet 构造它；Broker
/// 收到请求时会重新验证它与规范化请求的绑定。该类型本身没有执行权，也不保证每个版本值
/// 都已从持久权威账本读取，具体来源要看构造它的 Core 代码。
pub struct ExecutionScope {
    /// 结构合同名，必须等于当前 `EXECUTION_SCOPE_SCHEMA`。
    pub schema: String,
    /// 结构合同版本，必须与当前版本兼容。
    pub version: SchemaVersion,
    /// 服务端认证主体的稳定引用；不是模型可自报的 actor 字符串。
    pub principal: AuthenticatedPrincipalRef,
    /// 当前动作所属项目的显示路径、canonical 路径和身份摘要。
    pub project: ProjectIdentity,
    /// 当前交互会话 ID；是必填的作用域锚点。
    pub session_id: SessionId,
    /// 可选 Run ID；存在 turn 时必须同时存在。
    #[serde(default)]
    pub run_id: Option<RunId>,
    /// 可选模型/执行轮次 ID；领域校验要求它不能脱离 Run。
    #[serde(default)]
    pub turn_id: Option<TurnId>,
    /// 可选 Cell ID，用于把动作关联到被分配的工作单元。
    #[serde(default)]
    pub cell_id: Option<CellId>,
    /// 此请求关联的 capability grant ID 集合；最多 128 项。
    #[serde(default)]
    pub grant_refs: Vec<CapabilityGrantId>,
    /// 可选预算租约 ID；若请求也带租约，Broker 校验时必须一致。
    #[serde(default)]
    pub budget_lease_id: Option<BudgetLeaseId>,
    /// 可选 WorkPacket ID；若提供，必须是非空且不超过 256 字节。
    #[serde(default)]
    pub work_packet_id: Option<String>,
    /// 本次动作采用的执行环境身份；不是实际环境实现的证明。
    pub environment_id: String,
    /// 可选 workspace 版本/修订标识；当前 Domain 只把它纳入摘要，不解析版本语义。
    #[serde(default)]
    pub workspace_revision: Option<String>,
    /// capability 范围集合，表达适用的 operation/path/namespace/network 维度。
    pub permission_scope: ScopeSet,
    /// 允许读取的根路径文本列表；格式检查不负责路径 canonicalize 或真实文件访问。
    #[serde(default)]
    pub read_roots: Vec<String>,
    /// 允许写入的根路径文本列表；实际写集仍须由上层 PathLock/sandbox 等合同落实。
    #[serde(default)]
    pub write_roots: Vec<String>,
    /// 明确禁止读取的路径文本列表；本类型只保存和摘要这些声明。
    #[serde(default)]
    pub read_denies: Vec<String>,
    /// 明确禁止写入的路径文本列表；本类型不直接拦截文件操作。
    #[serde(default)]
    pub write_denies: Vec<String>,
    /// memory adapter 可访问的 collection/scope 名称列表。
    #[serde(default)]
    pub memory_scopes: Vec<String>,
    /// 内部服务或 connector 的 scope 名称列表。
    #[serde(default)]
    pub server_scopes: Vec<String>,
    /// 网络目的地/host 规则文本列表；实际网络观测由 Broker 的网络合同校验。
    #[serde(default)]
    pub network_policy: Vec<String>,
    /// 授权来源的代次编号，必须非零；本 Domain 校验器不会单独查询其是否为最新代次。
    /// 当前 Core 构造器固定填 `1`，因此不能仅凭字段存在就认为它来自 authority 账本。
    pub authority_epoch: u64,
    /// trust 状态修订的 SHA-256 外形摘要；真实来源绑定需由构造者或后续校验边界提供。
    pub trust_revision: String,
    /// 数据状态代次，必须非零；是否与当前数据账本一致不由本方法查询。
    /// 当前 Core 构造器固定填 `1`，实际版本对齐情况需另查相关 adapter/fence。
    pub data_epoch: u64,
    /// 取消状态代次，必须非零；Broker/调用流程仍需结合实际取消和 fencing 事实判断。
    /// 当前 Core 构造器固定填 `1`，不代表 Domain 已经读取取消账本。
    pub cancellation_epoch: u64,
    /// Unix 毫秒绝对截止时间，必须非零；这里不读取系统时钟验证它是否尚未过期。
    /// 当前 Core 构造器使用 `u64::MAX` 表示这个快照未设置有限截止时间。
    pub deadline_unix_ms: u64,
    /// fencing（围栏）令牌，必须非零；它用于拒绝旧执行世代，但数值新旧由上层比较。
    /// 当前 Core 构造器固定填 `1`，本 Domain 自校验不会判断其他权威存储中的最新令牌。
    pub fencing_token: u64,
    /// capability/action 目录的 digest，`validate_for_request()` 会与当前目录摘要比较。
    pub catalog_digest: String,
    /// 清除 scope 字段后对整个 capability action request 计算的绑定摘要。
    pub action_digest: String,
    /// `permission_scope.digest()` 的副本，用于检查 ScopeSet 与快照字段一致。
    pub permission_scope_digest: String,
    /// 除本字段本身外整份快照的内容摘要；无密钥，不是签名。
    pub scope_digest: String,
}

impl ExecutionScope {
    /// 校验快照内部的版本、必填项、范围列表、标识关系和自摘要。
    ///
    /// 【调用者】Core 构造快照后调用；Broker 的 `validate_for_request()` 也会先调用它。
    ///
    /// 【输入/输出】检查当前 `self`；全部约束满足时返回 `Ok(())`，否则返回字段/阶段错误字符串。
    /// 上层 Core 会把这些字符串映射为 action 错误，Broker 会映射为 `PortError::Failed`。
    ///
    /// 【核心流程】先检查 schema/version/session/environment/epoch/deadline/fencing 头部；
    /// 再委托主体、项目和 ScopeSet 自检；接着检查空的 Restricted 维度、Run/Turn 关系、grant
    /// 数量和相邻重复项、所有受限字符串列表、WorkPacket 长度；最后验证 digest 外形、权限
    /// 范围摘要和整份 scope 摘要。
    ///
    /// 【重要边界】此处不会联网或查询 EventLog，所以非零 epoch、未来 deadline 或 fencing
    /// 值只证明字段非零，不证明它们是当前值；digest 校验是自洽检查，不证明来源可信。
    pub fn validate(&self) -> Result<(), String> {
        if self.schema != EXECUTION_SCOPE_SCHEMA
            || !self
                .version
                .is_compatible_with(&EXECUTION_SCOPE_SCHEMA_VERSION)
            || self.session_id.is_empty()
            || self.environment_id.trim().is_empty()
            || self.authority_epoch == 0
            || self.data_epoch == 0
            || self.cancellation_epoch == 0
            || self.deadline_unix_ms == 0
            || self.fencing_token == 0
        {
            return Err("execution_scope_header_invalid".to_owned());
        }
        // 身份和项目的专门值对象各自掌握格式/摘要规则，避免本结构复制另一份校验逻辑。
        self.principal.validate()?;
        self.project.validate()?;
        self.permission_scope.validate()?;
        if [
            &self.permission_scope.operations,
            &self.permission_scope.paths,
            &self.permission_scope.namespaces,
            &self.permission_scope.network,
        ]
        .iter()
        .any(|dimension| {
            matches!(dimension, crate::ScopeDimension::Restricted(values) if values.is_empty())
        })
        {
            return Err("execution_scope_empty".to_owned());
        }
        if self.run_id.is_none() && self.turn_id.is_some() {
            return Err("execution_scope_turn_requires_run".to_owned());
        }
        // windows(2) 只比较相邻 grant；本函数没有先排序或构造全量集合。
        if self.grant_refs.len() > MAX_EXECUTION_SCOPE_REFS
            || self.grant_refs.windows(2).any(|pair| pair[0] == pair[1])
        {
            return Err("execution_scope_grants_invalid".to_owned());
        }
        for (values, field) in [
            (&self.read_roots, "execution_scope_read_roots"),
            (&self.write_roots, "execution_scope_write_roots"),
            (&self.read_denies, "execution_scope_read_denies"),
            (&self.write_denies, "execution_scope_write_denies"),
            (&self.memory_scopes, "execution_scope_memory_scopes"),
            (&self.server_scopes, "execution_scope_server_scopes"),
            (&self.network_policy, "execution_scope_network_policy"),
        ] {
            bounded_list(values, field)?;
        }
        if self
            .work_packet_id
            .as_deref()
            .is_some_and(|id| id.trim().is_empty() || id.len() > 256)
        {
            return Err("execution_scope_work_packet_invalid".to_owned());
        }
        for (value, field) in [
            (&self.trust_revision, "execution_scope_trust_revision"),
            (&self.catalog_digest, "execution_scope_catalog_digest"),
            (&self.action_digest, "execution_scope_action_digest"),
            (
                &self.permission_scope_digest,
                "execution_scope_permission_scope_digest",
            ),
            (&self.scope_digest, "execution_scope_scope_digest"),
        ] {
            digest(value, field)?;
        }
        if self.permission_scope_digest != self.permission_scope.digest() {
            return Err("execution_scope_permission_scope_digest_mismatch".to_owned());
        }
        if self.scope_digest != self.digest() {
            return Err("execution_scope_digest_mismatch".to_owned());
        }
        Ok(())
    }

    /// 返回可稳定写入 journal 的 JSON 字节表示。
    ///
    /// `canonical_journal_bytes()` 会递归排序 JSON 对象键、保持数组顺序，并拒绝超过 128 层
    /// 的嵌套或无法序列化的值。相同结构不因 map 键迭代顺序而得到不同字节；这仍只是内容
    /// 编码，不会给快照添加签名或授权含义。当前仓内未找到本方法的直接生产调用点。
    pub fn canonical_bytes(&self) -> Result<Vec<u8>, String> {
        canonical_journal_bytes(self)
    }

    /// 校验此快照是否与携带它的 capability request 绑定。
    ///
    /// 【调用者】Capability Broker 的 `validate_action()` 在实际调用 handler 前使用；它先
    /// 确认请求已经规范化，再进入这里核对绑定。此校验不是 permit 提交或批准检查。
    ///
    /// 【输入/输出】请求应是已经规范化、且 execution_scope 指回此对象的请求；一致返回 `Ok(())`，
    /// catalog/action/resource/run/turn 任一检查失败会返回稳定错误字符串。
    ///
    /// 【核心流程】先执行本对象内部校验 -> 比较当前 action catalog digest -> 克隆请求并清空
    /// execution_scope 后重算 action digest -> 比对 grant/budget 引用 -> 若参数中含可解析的
    /// Run/Turn ID，再比对快照 ID。
    ///
    /// 【为什么清空 scope】action digest 的输入请求本身含有 scope 字段会形成“scope 摘要依赖
    /// scope 摘要”的递归，因此 Core 构造端和 Broker 校验端都先清空再计算同一个动作摘要。
    ///
    /// ⚠ Run/Turn 分支只在 request 参数成功解析出 ID 时才比较；无法解析的值在本方法中不会
    /// 触发对应 mismatch，参数合法性需由请求规范化/上游动作合同负责。grant 检查只关注首项。
    pub fn validate_for_request(&self, request: &CapabilityRequest) -> Result<(), String> {
        self.validate()?;
        if self.catalog_digest != crate::capability_action_catalog_digest() {
            return Err("execution_scope_catalog_changed".to_owned());
        }
        // 使用请求副本去掉快照引用；不改调用者传入的 request，也避免自引用摘要。
        let mut without_scope = request.clone();
        without_scope.execution_scope = None;
        if self.action_digest != crate::capability_action_digest(&without_scope) {
            return Err("execution_scope_action_digest_mismatch".to_owned());
        }
        if self
            .grant_refs
            .first()
            .is_some_and(|grant| Some(*grant) != request.capability_grant_id)
            || self.budget_lease_id != request.budget_lease_id
        {
            return Err("execution_scope_resource_mismatch".to_owned());
        }
        if let Some(run_id) = request
            .arguments
            .get("run_id")
            .and_then(serde_json::Value::as_str)
            .and_then(RunId::parse_str)
        {
            if self.run_id != Some(run_id) {
                return Err("execution_scope_run_mismatch".to_owned());
            }
        }
        if let Some(turn_id) = request
            .arguments
            .get("turn_id")
            .and_then(|value| serde_json::from_value::<TurnId>(value.clone()).ok())
        {
            if self.turn_id != Some(turn_id) {
                return Err("execution_scope_turn_mismatch".to_owned());
            }
        }
        Ok(())
    }

    /// 比较本快照在部分权限/身份维度上是否不宽于 parent。
    ///
    /// 【输入/输出】先分别校验 child 与 parent；无效对象返回 Err，两个对象都有效时返回布尔值。
    /// 【比较范围】主体 ID、项目 ID、session、permission_scope 子集、截止时间、authority/data
    /// epoch 相等、cancellation/fencing 不倒退，以及 child 中每个 grant 都存在于 parent。
    ///
    /// 这不是对所有 `ExecutionScope` 字段的逐字段子集比较：例如 environment、读写根/deny
    /// 列表、memory/server/network 列表、trust revision、catalog/action digest 和 budget lease
    /// 不在此处单独比较。不要把返回 true 理解为“所有资源维度均已证明收窄”。仓内当前未找到
    /// 此 Domain 方法的直接生产调用点。
    pub fn is_subset_of(&self, parent: &Self) -> Result<bool, String> {
        self.validate()?;
        parent.validate()?;
        Ok(self.principal.principal_id == parent.principal.principal_id
            && self.project.project_id == parent.project.project_id
            && self.session_id == parent.session_id
            && self
                .permission_scope
                .is_subset_of(&parent.permission_scope)?
            && self.deadline_unix_ms <= parent.deadline_unix_ms
            && self.authority_epoch == parent.authority_epoch
            && self.data_epoch == parent.data_epoch
            && self.cancellation_epoch >= parent.cancellation_epoch
            && self.fencing_token >= parent.fencing_token
            && self
                .grant_refs
                .iter()
                .all(|grant| parent.grant_refs.iter().any(|candidate| candidate == grant)))
    }

    /// 计算整份 scope 的无密钥 SHA-256 内容摘要，但将自身的 `scope_digest` 先置空。
    ///
    /// 先序列化为 JSON Value，再把 digest 字段清空，是为了避免摘要递归引用自身。调用者在
    /// 修改任何其他字段后都必须重新计算摘要；此值可检测不一致/漂移，却不是认证签名。
    pub fn digest(&self) -> String {
        let mut value = serde_json::to_value(self).unwrap_or(serde_json::Value::Null);
        if let Some(object) = value.as_object_mut() {
            object.insert(
                "scope_digest".to_owned(),
                serde_json::Value::String(String::new()),
            );
        }
        json_digest(&value)
    }
}

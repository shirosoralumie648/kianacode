//! 投影器、索引、工作队列与租约修复的只读协调（reconcile）合同，属于 `kiana-domain`。
//!
//! 【系统位置与数据流】
//! ```text
//! 上游装配 projector/index/queue/lease 四类 RepairFact
//!     -> ReconcileInput（绑定同一操作、source cursor、generation、epoch 和 evidence）
//!     -> 本文件 validate() / derive_decision()
//!     -> ReconcileReport（健康、待修复、阻断、未知或 commit_ready）
//!     -> Core 门面 -> DaemonHost::ops_reconcile() -> 调用入口
//! ```
//! 当前没有生产事实采集器或路由调用者；已有测试只用合成 facts。此 reducer 不读 EventLog、投影存储、索引清单或队列租约，
//! 不执行修复、不重跑结果未知的操作、不获取租约，也不写入新 generation。
//! `commit_ready` 只表示输入合同允许把经过审阅的计划送往后续 server-owned 提交边界，不是提交回执。
//!
//! Serde 负责把版本化 DTO 与 JSON 等外部格式互转；`deny_unknown_fields` 拒绝当前版本没有声明的字段。

use crate::{
    json_digest, redact_text, scan_secret_sentinels, OperationId, SchemaVersion, SecretScanChannel,
};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

/// 单个 projector/index/queue/lease 事实的稳定 wire schema 名称。
pub const DEPLOYMENT_REPAIR_FACT_SCHEMA: &str = "kiana.deployment-repair-fact.v1";
/// reconcile input 的稳定 wire schema 名称。
pub const DEPLOYMENT_RECONCILE_INPUT_SCHEMA: &str = "kiana.deployment-reconcile-input.v1";
/// reconcile report 的稳定 wire schema 名称。
pub const DEPLOYMENT_RECONCILE_REPORT_SCHEMA: &str = "kiana.deployment-reconcile-report.v1";
/// 当前 DTO 的 schema 版本。
pub const DEPLOYMENT_RECONCILE_VERSION: SchemaVersion = SchemaVersion::new(1, 0);
/// input 必须恰好包含的目标数：projector、index、queue、lease 各一条。
pub const MAX_RECONCILE_FACTS: usize = 4;
/// actor/reason/remediation 等用户可见文本的最大 UTF-8 字节数。
pub const MAX_RECONCILE_TEXT: usize = 256;

/// 四类需要一起核对的可重建/协调目标；固定声明顺序也是 input wire 合同的一部分。
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RepairTarget {
    /// EventLog 派生读模型（projector）的游标与代数。
    Projector,
    /// Repo map、精确/BM25、dense 等索引 generation。
    Index,
    /// Workflow queue 派生状态及对应 lease/fence。
    Queue,
    /// 协调租约的活动状态和 fencing 条件。
    Lease,
}

impl RepairTarget {
    /// input 必须恰好覆盖且按此顺序排列的目标集合。
    pub const ALL: [Self; 4] = [Self::Projector, Self::Index, Self::Queue, Self::Lease];

    /// 为稳定顺序校验返回 1..=4 的位置号，不是数据库 ID。
    const fn ordinal(self) -> u8 {
        match self {
            Self::Projector => 1,
            Self::Index => 2,
            Self::Queue => 3,
            Self::Lease => 4,
        }
    }

    /// 返回稳定的小写 wire/人类展示名称。
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Projector => "projector",
            Self::Index => "index",
            Self::Queue => "queue",
            Self::Lease => "lease",
        }
    }
}

/// 请求 reducer 评估哪一种运维动作；这三个值只选择决策语义，不直接执行命令。
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ReconcileCommand {
    /// 仅检查并汇报当前事实。
    Inspect,
    /// 计算确定性的下一投影 generation，但不应用计划。
    PlanRepair,
    /// 检查显式审批摘要并判断是否达到可提交状态，仍不执行提交。
    CommitRepair,
}

impl ReconcileCommand {
    /// 返回稳定的小写 wire/展示名称。
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Inspect => "inspect",
            Self::PlanRepair => "plan_repair",
            Self::CommitRepair => "commit_repair",
        }
    }
}

/// 单个目标的状态；它由 `RepairFact::status()` 根据调用方声明的事实派生。
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RepairFactStatus {
    /// cursor 和 generation 与本次输入要求一致，且 lease/fence 有效。
    Healthy,
    /// 当前投影落后于已声明的 source cursor 或期望 generation。
    RepairNeeded,
    /// 游标超前、generation 超前或 lease/fence 不允许继续。
    Blocked,
    /// 关键事实无法确认，例如 projection cursor 缺失或操作结果未知。
    Unknown,
}

impl RepairFactStatus {
    /// 返回稳定的小写状态文本。
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Healthy => "healthy",
            Self::RepairNeeded => "repair_needed",
            Self::Blocked => "blocked",
            Self::Unknown => "unknown",
        }
    }
}

/// 整个 reconcile 请求的聚合结果；CommitReady 仍是计划状态，不代表任何持久化写入完成。
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ReconcileStatus {
    /// 四个目标均匹配输入声明的 cursor 和 generation。
    Healthy,
    /// 至少一个目标落后，当前命令只检查或生成修复计划。
    RepairNeeded,
    /// 至少一个目标违反 cursor/generation/lease/fence 安全条件，拒绝计划提交。
    Blocked,
    /// 存在无法判定的事实；不得自动重跑可能已生效的操作。
    Unknown,
    /// 有落后目标且命令为 CommitRepair、审批摘要格式有效；仅可继续到后续提交边界。
    CommitReady,
}

impl ReconcileStatus {
    /// 返回稳定的小写状态文本。
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Healthy => "healthy",
            Self::RepairNeeded => "repair_needed",
            Self::Blocked => "blocked",
            Self::Unknown => "unknown",
            Self::CommitReady => "commit_ready",
        }
    }
}

/// 上游对某一个 repair target 的事实快照；状态由其 cursor、generation、租约和未知结果字段推导。
///
/// 此类型不持有对应的 EventLog、ProjectionCheckpoint、index manifest 或 queue lease 对象，摘要字段只是调用方声明的引用。
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RepairFact {
    /// 固定 fact schema 名称。
    pub schema: String,
    /// fact schema 版本。
    pub version: SchemaVersion,
    /// 当前事实对应 projector/index/queue/lease 中的哪一类目标。
    pub target: RepairTarget,
    /// 上游认为权威数据到达的位置；四条 fact 必须引用相同值。
    pub source_cursor: u64,
    /// 目标投影已应用到的 source 位置；None 表示缺少可确认的检查点，会派生为 Unknown。
    pub projection_cursor: Option<u64>,
    /// 当前目标声明的 generation；与 expected_generation 比较来判断落后或超前。
    pub projection_generation: u64,
    /// 本次输入要求的目标 generation；必须在四条 fact 中保持一致且非零。
    pub expected_generation: u64,
    /// authority 侧 epoch，防止把不同授权代际的事实混在一个 input；这里只做数值绑定。
    pub authority_epoch: u64,
    /// 数据侧 epoch，防止把不同数据代际的事实混在一个 input；这里只做数值绑定。
    pub data_epoch: u64,
    /// 调用方对 lease 当前有效性的声明，不会由本类型查询 lease store。
    pub lease_active: bool,
    /// 调用方对 fencing token 有效性的声明，不会由本类型比对当前 token。
    pub fence_valid: bool,
    /// 标记原操作是否可能已产生效果但结果未知；为 true 时优先派生 Unknown，禁止自动重跑。
    pub result_unknown: bool,
    /// source snapshot 的 SHA-256 引用；只检查格式并在 facts 间要求相同，不读取源内容。
    pub source_digest: String,
    /// lease/fence 证据摘要；只校验格式，不加载真实租约或验证签发者。
    pub lease_digest: String,
    /// 当前 fact 的 evidence 摘要；须与 input 的 evidence_digest 字符串相同。
    pub evidence_digest: String,
    /// 对该 fact 其余业务字段计算的无密钥 SHA-256 内容摘要。
    pub fact_digest: String,
}

impl RepairFact {
    /// 创建单目标 fact，填充 schema/version，计算自摘要后执行结构校验。
    ///
    /// 【调用者】预期由上游从 projector、索引、队列和 lease 状态汇总器调用；当前生产源码没有这样的构造调用者，Domain 测试使用 fixture。
    /// 【输入/输出】各个游标、代数、epoch、bool 和摘要均由调用方提供；合法时返回可放入 `ReconcileInput::new()` 的 fact。
    /// 【失败情况】版本、零值、cursor 或摘要格式/内容不合法时返回稳定 `String` 错误。
    /// 【副作用/限制】只创建内存 DTO 和摘要，不读取实际状态，也不证明布尔值或 digest 对应真实 adapter 事实。
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        target: RepairTarget,
        source_cursor: u64,
        projection_cursor: Option<u64>,
        projection_generation: u64,
        expected_generation: u64,
        authority_epoch: u64,
        data_epoch: u64,
        lease_active: bool,
        fence_valid: bool,
        result_unknown: bool,
        source_digest: impl Into<String>,
        lease_digest: impl Into<String>,
        evidence_digest: impl Into<String>,
    ) -> Result<Self, String> {
        let mut fact = Self {
            schema: DEPLOYMENT_REPAIR_FACT_SCHEMA.to_owned(),
            version: DEPLOYMENT_RECONCILE_VERSION,
            target,
            source_cursor,
            projection_cursor,
            projection_generation,
            expected_generation,
            authority_epoch,
            data_epoch,
            lease_active,
            fence_valid,
            result_unknown,
            source_digest: source_digest.into(),
            lease_digest: lease_digest.into(),
            evidence_digest: evidence_digest.into(),
            fact_digest: String::new(),
        };
        fact.fact_digest = fact.digest();
        fact.validate()?;
        Ok(fact)
    }

    /// 按固定优先级把调用方声明的单目标事实分类。
    ///
    /// 先把 `result_unknown` 或缺失 cursor 归为 Unknown；再把 lease/fence 失效、cursor 超前或 generation 超前归为 Blocked；
    /// 然后把 cursor/generation 落后归为 RepairNeeded；只有所有比较都相等且 lease/fence 有效才是 Healthy。
    /// 本函数不自行调用 `validate()`，直接使用前应先校验 fact；尤其不要把 Unknown 改为重跑操作的许可。
    pub fn status(&self) -> RepairFactStatus {
        if self.result_unknown || self.projection_cursor.is_none() {
            RepairFactStatus::Unknown
        } else if !self.lease_active
            || !self.fence_valid
            || self
                .projection_cursor
                .is_some_and(|cursor| cursor > self.source_cursor)
            || self.projection_generation > self.expected_generation
        {
            RepairFactStatus::Blocked
        } else if self
            .projection_cursor
            .is_some_and(|cursor| cursor < self.source_cursor)
            || self.projection_generation < self.expected_generation
        {
            RepairFactStatus::RepairNeeded
        } else {
            RepairFactStatus::Healthy
        }
    }

    /// 验证 fact header、所有必需的非零数字、摘要外形及自摘要。
    ///
    /// 成功不代表 lease_active/fence_valid/result_unknown 声明与真实系统一致；这些布尔值和 epoch 没有外部权威读取。
    /// 调用 input 校验时，每条 fact 还会被绑定到共同 cursor、generation、epoch、target 顺序和 evidence。
    pub fn validate(&self) -> Result<(), String> {
        if self.schema != DEPLOYMENT_REPAIR_FACT_SCHEMA
            || self.version != DEPLOYMENT_RECONCILE_VERSION
            || self.source_cursor == 0
            || self.projection_generation == 0
            || self.expected_generation == 0
            || self.authority_epoch == 0
            || self.data_epoch == 0
        {
            return Err("deployment_repair_fact_header_invalid".to_owned());
        }
        if self.projection_cursor.is_some_and(|cursor| cursor == 0) {
            return Err("deployment_repair_fact_cursor_invalid".to_owned());
        }
        for (value, field) in [
            (&self.source_digest, "deployment_repair_source_digest"),
            (&self.lease_digest, "deployment_repair_lease_digest"),
            (&self.evidence_digest, "deployment_repair_evidence_digest"),
        ] {
            valid_digest(value, field)?;
        }
        valid_digest(&self.fact_digest, "deployment_repair_fact_digest")?;
        if self.fact_digest != self.digest() {
            return Err("deployment_repair_fact_digest_mismatch".to_owned());
        }
        Ok(())
    }

    /// 对 fact 除 `fact_digest` 外的全部业务字段计算无密钥 SHA-256 摘要。
    pub fn digest(&self) -> String {
        json_digest(&serde_json::json!({
            "schema": self.schema,
            "version": self.version,
            "target": self.target,
            "source_cursor": self.source_cursor,
            "projection_cursor": self.projection_cursor,
            "projection_generation": self.projection_generation,
            "expected_generation": self.expected_generation,
            "authority_epoch": self.authority_epoch,
            "data_epoch": self.data_epoch,
            "lease_active": self.lease_active,
            "fence_valid": self.fence_valid,
            "result_unknown": self.result_unknown,
            "source_digest": self.source_digest,
            "lease_digest": self.lease_digest,
            "evidence_digest": self.evidence_digest,
        }))
    }
}

/// 一次 projector/index/queue/lease 协调请求的完整输入合同。
///
/// 上游先汇总四种目标事实，再把它们和同一操作 ID、cursor、期望 generation、authority/data epoch、actor、证据及可选审批摘要交给本 reducer。
/// `validate()` 只验证这些字段彼此一致，不加载 EventLog/租约/审批记录，也不证明调用者有权执行提交。
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ReconcileInput {
    /// 稳定 input wire schema 名称。
    pub schema: String,
    /// DTO schema 版本。
    pub version: SchemaVersion,
    /// 本次 reconcile 操作的稳定 ID；它标识本次操作，不是目标资源 ID。
    pub operation_id: OperationId,
    /// inspect / plan_repair / commit_repair 三种请求之一。
    pub command: ReconcileCommand,
    /// 声明发起或负责本次操作的 actor；只做安全文本校验，不查身份系统。
    pub actor_id: String,
    /// 上游用于对齐四条事实的共同 source cursor。
    pub source_cursor: u64,
    /// 本次要求的投影 generation；四条 fact 必须精确匹配。
    pub expected_generation: u64,
    /// 授权代际声明；四条 fact 必须精确匹配，但不会回查当前 ControlPlane epoch。
    pub authority_epoch: u64,
    /// 数据代际声明；四条 fact 必须精确匹配，但不会回查当前数据源 epoch。
    pub data_epoch: u64,
    /// 本次操作 evidence 的摘要引用；所有 fact 必须使用完全相同的字符串。
    pub evidence_digest: String,
    /// commit_repair 必须提供格式正确的审批摘要；它不是审批对象或有效授权证明。
    pub approval_digest: Option<String>,
    /// 必须按 Projector、Index、Queue、Lease 顺序各提供一条事实。
    pub facts: Vec<RepairFact>,
    /// 对除本字段外所有 input 业务字段计算的无密钥摘要。
    pub input_digest: String,
}

impl ReconcileInput {
    /// 组装一次 input，保留调用方传入的 fact 顺序，计算摘要并立即校验。
    ///
    /// 【调用者】预期由运维事实汇总/命令层调用；当前生产源码没有 input builder，测试直接构造 fixture。
    /// 【输入/输出】接收操作标识、命令、actor、版本边界、证据/审批摘要及四条 fact；成功返回可以交给 Core/DaemonHost 的 DTO。
    /// 【失败情况】任何 header、审批与命令关系、fact 顺序/绑定、文本或摘要错误都会返回 `String`；不会自动排序或补齐目标。
    /// 【副作用/设计原因】只构造内存值。固定顺序和共享字段让四个不同目标不能在一次输入中悄悄引用不同事实快照。
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        operation_id: OperationId,
        command: ReconcileCommand,
        actor_id: impl Into<String>,
        source_cursor: u64,
        expected_generation: u64,
        authority_epoch: u64,
        data_epoch: u64,
        evidence_digest: impl Into<String>,
        approval_digest: Option<String>,
        facts: Vec<RepairFact>,
    ) -> Result<Self, String> {
        let mut input = Self {
            schema: DEPLOYMENT_RECONCILE_INPUT_SCHEMA.to_owned(),
            version: DEPLOYMENT_RECONCILE_VERSION,
            operation_id,
            command,
            actor_id: actor_id.into(),
            source_cursor,
            expected_generation,
            authority_epoch,
            data_epoch,
            evidence_digest: evidence_digest.into(),
            approval_digest,
            facts,
            input_digest: String::new(),
        };
        input.input_digest = input.digest();
        input.validate()?;
        Ok(input)
    }

    /// 校验请求 schema、命令与审批字段关系、四目标覆盖顺序、共同事实绑定和 input 自摘要。
    ///
    /// 【核心流程】先检查非零 header/恰好四条 fact；再校验 actor/evidence；CommitRepair 要求 approval digest，其余命令禁止带审批摘要；
    /// 接着逐条校验 RepairFact、共同 cursor/generation/epoch、目标序号唯一性、跨目标 source digest 和 evidence 绑定；最后比较 input digest。
    /// 【失败情况】按首次发现的问题返回稳定 `String` 错误，由 Core 原样传播，Daemon adapter 包装为 `PortError::Failed`。
    /// 【限制】审批只校验 `sha256:` 外形，actor 只校验文本；这里不调用审批服务、权限系统、lease store 或 EventLog。
    pub fn validate(&self) -> Result<(), String> {
        if self.schema != DEPLOYMENT_RECONCILE_INPUT_SCHEMA
            || self.version != DEPLOYMENT_RECONCILE_VERSION
            || self.operation_id.as_uuid().is_nil()
            || self.source_cursor == 0
            || self.expected_generation == 0
            || self.authority_epoch == 0
            || self.data_epoch == 0
            || self.facts.len() != RepairTarget::ALL.len()
            || self.facts.len() > MAX_RECONCILE_FACTS
        {
            return Err("deployment_reconcile_input_header_invalid".to_owned());
        }
        safe_text(&self.actor_id, "deployment_reconcile_actor")?;
        valid_digest(
            &self.evidence_digest,
            "deployment_reconcile_evidence_digest",
        )?;
        match self.command {
            ReconcileCommand::CommitRepair => {
                let approval = self
                    .approval_digest
                    .as_deref()
                    .ok_or_else(|| "deployment_reconcile_approval_required".to_owned())?;
                valid_digest(approval, "deployment_reconcile_approval_digest")?;
            }
            ReconcileCommand::Inspect | ReconcileCommand::PlanRepair => {
                if self.approval_digest.is_some() {
                    return Err("deployment_reconcile_unexpected_approval".to_owned());
                }
            }
        }
        let mut targets = BTreeSet::new();
        // expected_ordinal 与 RepairTarget::ALL 的声明顺序对应，防止不同序列化顺序代表同一逻辑集合。
        let mut expected_ordinal = 1;
        // 跨目标的 source_digest 必须相同；Option 用于把第一条事实作为后续比较基准。
        let mut source_digest: Option<&str> = None;
        for fact in &self.facts {
            fact.validate()?;
            if fact.source_cursor != self.source_cursor
                || fact.expected_generation != self.expected_generation
                || fact.authority_epoch != self.authority_epoch
                || fact.data_epoch != self.data_epoch
                || fact.target.ordinal() != expected_ordinal
                || !targets.insert(fact.target)
            {
                return Err("deployment_reconcile_fact_binding_invalid".to_owned());
            }
            if let Some(previous) = source_digest {
                if previous != fact.source_digest {
                    return Err("deployment_reconcile_source_digest_mismatch".to_owned());
                }
            } else {
                source_digest = Some(fact.source_digest.as_str());
            }
            if fact.evidence_digest != self.evidence_digest {
                return Err("deployment_reconcile_evidence_binding_invalid".to_owned());
            }
            expected_ordinal += 1;
        }
        if targets.len() != RepairTarget::ALL.len() {
            return Err("deployment_reconcile_target_set_invalid".to_owned());
        }
        valid_digest(&self.input_digest, "deployment_reconcile_input_digest")?;
        if self.input_digest != self.digest() {
            return Err("deployment_reconcile_input_digest_mismatch".to_owned());
        }
        Ok(())
    }

    /// 对 input 的全部业务字段计算摘要，不包含 `input_digest` 自身。
    ///
    /// 这是无密钥一致性指纹，不提供身份认证；调用者修改字段后可以重算摘要，所以可信事实仍须来自上游权威边界。
    pub fn digest(&self) -> String {
        json_digest(&serde_json::json!({
            "schema": self.schema,
            "version": self.version,
            "operation_id": self.operation_id,
            "command": self.command,
            "actor_id": self.actor_id,
            "source_cursor": self.source_cursor,
            "expected_generation": self.expected_generation,
            "authority_epoch": self.authority_epoch,
            "data_epoch": self.data_epoch,
            "evidence_digest": self.evidence_digest,
            "approval_digest": self.approval_digest,
            "facts": self.facts,
        }))
    }
}

/// Domain 对一次 reconcile 请求的只读决策输出。
///
/// report 复制 input 的操作、actor、cursor、facts 和 evidence，并附上状态、原因及最多一个计划 generation。
/// 它是审阅/呈现数据，不是 EventLog 事件、队列执行结果或 repair commit receipt。
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ReconcileReport {
    /// 稳定 report wire schema 名称。
    pub schema: String,
    /// DTO schema 版本。
    pub version: SchemaVersion,
    /// 对应 input 的本次 reconcile 操作 ID。
    pub operation_id: OperationId,
    /// 对应 input 请求的命令类别。
    pub command: ReconcileCommand,
    /// 对四条目标事实应用固定优先级后的结果。
    pub status: ReconcileStatus,
    /// 本 reducer 始终输出 true，表示本报告生成过程没有修改目标状态。
    pub read_only: bool,
    /// 表示是否仍需显式提交；只有 CommitReady 状态才为 false。
    pub explicit_commit_required: bool,
    /// 从 input 复制的 actor 声明，不表示通过了身份/权限检查。
    pub actor_id: String,
    /// 从 input 复制的共同 source cursor，不表示现场 EventLog 查询结果。
    pub source_cursor: u64,
    /// 若需要修复则为 expected_generation + 1；None 表示当前不提供可应用的计划代数。
    pub planned_projection_generation: Option<u64>,
    /// 从 input 复制的四条目标事实及其派生状态。
    pub diagnostics: Vec<RepairFact>,
    /// 稳定状态原因文本，用于操作者理解为什么健康、待修复或被阻断。
    pub reason: String,
    /// 建议的下一步；本模块只展示文字，不执行它。
    pub remediation: String,
    /// 对应 input 的 evidence 摘要引用。
    pub evidence_digest: String,
    /// 对 report 其余业务字段计算的无密钥 SHA-256 摘要。
    pub report_digest: String,
}

impl ReconcileReport {
    /// 验证 input 后生成纯只读 reconcile report。
    ///
    /// 【调用者】经 `kiana_core::evaluate_reconcile()`、Daemon adapter 和 `DaemonHost::ops_reconcile()` 静态委托；目前没有生产调用者。
    /// 【输入/输出】读取 `ReconcileInput`，成功时返回四目标诊断、整体状态和可选的下一 generation。
    /// 【核心流程】input 校验 → 派生四目标状态 → 计算 command 决策 → 复制绑定字段 → 生成 report digest → 再用 `validate_against()` 自检。
    /// 【失败情况】输入不合法、generation 加一溢出、或输出绑定校验失败时返回 `String`；Unknown/Blocked 是成功返回的状态，不是 Rust 错误。
    /// 【副作用/为什么这样设计】纯 reducer 让多入口共享同一决策顺序；实际采集和提交留给其他权威组件，本函数永远不写投影或 lease。
    pub fn evaluate(input: &ReconcileInput) -> Result<Self, String> {
        input.validate()?;
        let (status, planned_generation, reason, remediation, allow_commit) =
            derive_decision(input)?;
        let mut report = Self {
            schema: DEPLOYMENT_RECONCILE_REPORT_SCHEMA.to_owned(),
            version: DEPLOYMENT_RECONCILE_VERSION,
            operation_id: input.operation_id,
            command: input.command,
            status,
            read_only: true,
            explicit_commit_required: !allow_commit,
            actor_id: input.actor_id.clone(),
            source_cursor: input.source_cursor,
            planned_projection_generation: planned_generation,
            diagnostics: input.facts.clone(),
            reason: reason.to_owned(),
            remediation: remediation.to_owned(),
            evidence_digest: input.evidence_digest.clone(),
            report_digest: String::new(),
        };
        report.report_digest = report.digest();
        report.validate_against(input)?;
        Ok(report)
    }

    /// 用原 input 重新派生决策，检查 report 是否仍与该 input 完全一致。
    ///
    /// 【调用者】`evaluate()` 返回前调用；Core 的 `validate_reconcile_report()` 也提供给需要再次复核的上层。
    /// 【检查内容】比较 schema/version、操作/命令、状态、read_only/commit 标志、actor/cursor、计划代数、facts、原因、remediation、evidence 和摘要。
    /// 【失败处理】首个绑定/文本/摘要错误以 `String` 返回；不重建 report、不重试、不记录日志。
    /// 【边界】所有外部事实仍来自 input；本函数不会确认 actor/审批/epoch/lease/source digest 的真实性。
    pub fn validate_against(&self, input: &ReconcileInput) -> Result<(), String> {
        input.validate()?;
        let (status, planned_generation, reason, remediation, allow_commit) =
            derive_decision(input)?;
        if self.schema != DEPLOYMENT_RECONCILE_REPORT_SCHEMA
            || self.version != DEPLOYMENT_RECONCILE_VERSION
            || self.operation_id != input.operation_id
            || self.command != input.command
            || self.status != status
            || !self.read_only
            || self.explicit_commit_required != !allow_commit
            || self.actor_id != input.actor_id
            || self.source_cursor != input.source_cursor
            || self.planned_projection_generation != planned_generation
            || self.diagnostics != input.facts
            || self.reason != reason
            || self.remediation != remediation
            || self.evidence_digest != input.evidence_digest
        {
            return Err("deployment_reconcile_report_binding_invalid".to_owned());
        }
        safe_text(&self.actor_id, "deployment_reconcile_report_actor")?;
        safe_text(&self.reason, "deployment_reconcile_report_reason")?;
        safe_text(&self.remediation, "deployment_reconcile_report_remediation")?;
        valid_digest(
            &self.evidence_digest,
            "deployment_reconcile_report_evidence_digest",
        )?;
        valid_digest(&self.report_digest, "deployment_reconcile_report_digest")?;
        if self.report_digest != self.digest() {
            return Err("deployment_reconcile_report_digest_mismatch".to_owned());
        }
        Ok(())
    }

    /// 对 report 除 `report_digest` 外的全部业务字段计算无密钥摘要。
    pub fn digest(&self) -> String {
        json_digest(&serde_json::json!({
            "schema": self.schema,
            "version": self.version,
            "operation_id": self.operation_id,
            "command": self.command,
            "status": self.status,
            "read_only": self.read_only,
            "explicit_commit_required": self.explicit_commit_required,
            "actor_id": self.actor_id,
            "source_cursor": self.source_cursor,
            "planned_projection_generation": self.planned_projection_generation,
            "diagnostics": self.diagnostics,
            "reason": self.reason,
            "remediation": self.remediation,
            "evidence_digest": self.evidence_digest,
        }))
    }

    /// 在没有原始 input 的情况下校验 report 自身后，生成人类可读的多行摘要。
    ///
    /// 【调用者】预期由 ops CLI/UI 展示层调用；仓库当前没有生产调用者。
    /// 【输出内容】命令、状态、read_only、source cursor、原因/建议、可选计划代数，以及每个目标的派生状态/cursor/generation。
    /// 【限制】不会执行 remediation；自校验不等于重新与原始 input 绑定。
    pub fn render_human(&self) -> Result<String, String> {
        self.validate_digest_only()?;
        let mut lines = vec![
            format!("command: {}", self.command.as_str()),
            format!("status: {}", self.status.as_str()),
            format!("read_only: {}", self.read_only),
            format!("source_cursor: {}", self.source_cursor),
            format!("reason: {}", self.reason),
            format!("remediation: {}", self.remediation),
        ];
        if let Some(generation) = self.planned_projection_generation {
            lines.push(format!("planned_projection_generation: {generation}"));
        }
        for fact in &self.diagnostics {
            lines.push(format!(
                "{}: {} cursor={} generation={}",
                fact.target.as_str(),
                fact.status().as_str(),
                fact.projection_cursor
                    .map_or_else(|| "unknown".to_owned(), |cursor| cursor.to_string()),
                fact.projection_generation
            ));
        }
        Ok(lines.join("\n"))
    }

    /// 自校验后把 report 序列化为格式化 JSON。
    ///
    /// Serde 把结构体写成外部 DTO；自校验失败或 JSON 编码失败时返回稳定错误字符串，不输出部分报告。
    pub fn render_json(&self) -> Result<String, String> {
        self.validate_digest_only()?;
        serde_json::to_string_pretty(self)
            .map_err(|_| "deployment_reconcile_json_encode_failed".to_owned())
    }

    /// 渲染前可执行的有限自校验；没有原 input，因此无法验证 input/report 绑定。
    ///
    /// 此处检查固定 schema/version、只读标记、四条 fact、每条 fact 的自校验、安全文本及 report digest。
    /// 目标唯一/顺序、所有 fact 的共同 cursor/epoch、command 决策是否正确等跨字段关系，须由 `validate_against()` 才能重算。
    fn validate_digest_only(&self) -> Result<(), String> {
        if self.schema != DEPLOYMENT_RECONCILE_REPORT_SCHEMA
            || self.version != DEPLOYMENT_RECONCILE_VERSION
            || !self.read_only
            || self.diagnostics.len() != MAX_RECONCILE_FACTS
        {
            return Err("deployment_reconcile_report_shape_invalid".to_owned());
        }
        for fact in &self.diagnostics {
            fact.validate()?;
        }
        safe_text(&self.actor_id, "deployment_reconcile_report_actor")?;
        safe_text(&self.reason, "deployment_reconcile_report_reason")?;
        safe_text(&self.remediation, "deployment_reconcile_report_remediation")?;
        valid_digest(
            &self.evidence_digest,
            "deployment_reconcile_report_evidence_digest",
        )?;
        valid_digest(&self.report_digest, "deployment_reconcile_report_digest")?;
        if self.report_digest != self.digest() {
            return Err("deployment_reconcile_report_digest_mismatch".to_owned());
        }
        Ok(())
    }
}

/// 把四条目标事实与请求命令归并为 `(整体状态, 计划代数, 原因, 建议, 是否允许进入下一提交边界)`。
///
/// 【调用者】`ReconcileReport::evaluate()` 和 `validate_against()`；两处都要求 input 已通过完整校验。
/// 【优先级】任何 Unknown → 任一 Blocked → 无需修复（但 CommitRepair 拒绝空提交）→ 有修复且 CommitRepair 时 CommitReady → 普通修复计划。
/// 这样不确定或违反 fence 的事实不会被后续“可修复”状态覆盖，也不会让未知外部效果自动重跑。
/// 【失败情况】函数一进入就计算 `expected_generation + 1`；若它溢出则返回 `deployment_reconcile_generation_overflow`。
/// 因为这一步早于状态判断，即使 Inspect 输入本来健康或无需修复，generation 为 `u64::MAX` 也会失败；这是当前实现的边界行为。
/// 【副作用/限制】只计算报告数据，不执行修复；CommitReady 仍要求其他受控提交边界重新验证 actor、审批和当前事实。
fn derive_decision(
    input: &ReconcileInput,
) -> Result<
    (
        ReconcileStatus,
        Option<u64>,
        &'static str,
        &'static str,
        bool,
    ),
    String,
> {
    let statuses = input
        .facts
        .iter()
        .map(RepairFact::status)
        .collect::<Vec<_>>();
    // 计划 generation 始终比本次期望 generation 前进一步。
    // checked_add 不允许整数回绕；但当前实现即使稍后的分支不会返回计划，也会先做这个检查。
    let planned_generation = input
        .expected_generation
        .checked_add(1)
        .ok_or_else(|| "deployment_reconcile_generation_overflow".to_owned())?;
    // Unknown 优先：结果未知时无法判断重建是否会重复一个已经产生外部效果的操作。
    if statuses.contains(&RepairFactStatus::Unknown) {
        return Ok((
            ReconcileStatus::Unknown,
            None,
            "reconcile_result_unknown",
            "query the original effect and record verified external evidence",
            false,
        ));
    }
    // Blocked 表示游标/代数超前或 lease/fence 不匹配，不能靠重新生成投影计划掩盖权威性冲突。
    if statuses.contains(&RepairFactStatus::Blocked) {
        return Ok((
            ReconcileStatus::Blocked,
            None,
            "reconcile_fence_or_cursor_blocked",
            "restore the matching lease, fence, epoch and source cursor before repair",
            false,
        ));
    }
    let needs_repair = statuses.contains(&RepairFactStatus::RepairNeeded);
    if !needs_repair {
        // 即使当前状态健康，CommitRepair 也不能成为空操作提交；调用方应回到 inspect。
        if input.command == ReconcileCommand::CommitRepair {
            return Ok((
                ReconcileStatus::Blocked,
                None,
                "reconcile_no_repair_needed",
                "inspect the current projection and do not submit an empty repair",
                false,
            ));
        }
        return Ok((ReconcileStatus::Healthy, None, "ok", "none", false));
    }
    if input.command == ReconcileCommand::CommitRepair {
        // 审批摘要已在 input.validate() 中检查外形，但这里仍不执行审批，也不写 generation。
        return Ok((
            ReconcileStatus::CommitReady,
            Some(planned_generation),
            "reconcile_commit_explicitly_ready",
            "submit the reviewed repair through the server-owned commit boundary",
            true,
        ));
    }
    // Inspect 和 PlanRepair 都只返回待处理计划；二者的状态类别相同，reason 文本区分操作者意图。
    Ok((
        ReconcileStatus::RepairNeeded,
        Some(planned_generation),
        if input.command == ReconcileCommand::PlanRepair {
            "reconcile_repair_plan_ready"
        } else {
            "reconcile_repair_needed"
        },
        "review actor, evidence, lease and approval before an explicit commit",
        false,
    ))
}

/// 校验 actor/reason/remediation 等面对运维入口的文字，避免报告携带明显路径或未脱敏秘密。
///
/// 先要求非空且不超过 256 UTF-8 字节，再拒绝控制/换行、常见绝对路径、路径穿越和 file URL；
/// 接着要求通用 redactor 不再改变文本，并通过 Receipt secret sentinel 扫描。
/// 这是有限的字符串安全检查，不验证 actor 身份，也不能证明上游 facts 中不存在所有秘密。
fn safe_text(value: &str, field: &str) -> Result<(), String> {
    if value.trim().is_empty()
        || value.len() > MAX_RECONCILE_TEXT
        || value.contains(['\0', '\r', '\n'])
        || value.contains("..")
        || value.contains("file://")
        || value.split_whitespace().any(|token| {
            token.starts_with('/')
                || token.starts_with('\\')
                || (token.len() >= 3
                    && token.as_bytes()[0].is_ascii_alphabetic()
                    && token.as_bytes()[1] == b':'
                    && matches!(token.as_bytes()[2], b'/' | b'\\'))
        })
    {
        return Err(format!("{field}_invalid"));
    }
    if redact_text(value) != value {
        return Err(format!("{field}_not_redacted"));
    }
    scan_secret_sentinels(SecretScanChannel::Receipt, value)
        .map_err(|_| format!("{field}_secret_detected"))
}

/// 检查 `sha256:` 前缀和 64 个十六进制字符的格式；不读取被摘要对象或认证摘要来源。
fn valid_digest(value: &str, field: &str) -> Result<(), String> {
    let Some(hex) = value.strip_prefix("sha256:") else {
        return Err(format!("{field}_invalid"));
    };
    if hex.len() != 64 || !hex.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err(format!("{field}_invalid"));
    }
    Ok(())
}

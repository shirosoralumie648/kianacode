//! DEP-10 确定性部署启动阶段顺序与就绪报告合同。
//!
//! 【系统位置与调用链】
//! ```text
//! adapter 预先收集 manifest/root/trust/lease/store/migration/projector/capacity facts
//!     -> DaemonHost::evaluate_startup()
//!     -> kiana-daemon::startup_coordinator::evaluate()
//!     -> kiana-core::evaluate_startup()
//!     -> StartupCoordinatorReport::evaluate()
//!     -> derive_result() 按八个 StartupStage 折叠
//!     -> 返回 Ready / Blocked / Unknown 和原因
//! ```
//!
//! 本 Domain 纯聚合器（reducer）只验证并折叠调用方准备好的摘要/fact，不会打开存储、取得 lease、运行
//! migration、重建 projector、探测 capacity、持久化启动状态或实际阻止命令。缺阶段、旧
//! epoch、摘要不匹配、未信任或 Unknown 证据会阻止 Ready；pending operation 不会被静默
//! resume。当前调用链仍是 `partial/source` 合同，不能误读成已经接入真实进程启动流程。
//!
//! 这里的 SHA-256 digest 是字段一致性指纹，不是签名；对输入的自摘要校验不代表摘要引用的
//! release/storage/lease/recovery 对象已经由本模块重新读取或证明真实。

use crate::{json_digest, SchemaVersion};
use serde::{Deserialize, Serialize};

/// 单个启动阶段 fact 在序列化数据中的固定 schema 标识。
pub const DEPLOYMENT_STARTUP_FACT_SCHEMA: &str = "kiana.deployment-startup-fact.v1";
/// 启动 request 在序列化数据中的固定 schema 标识。
pub const DEPLOYMENT_STARTUP_REQUEST_SCHEMA: &str = "kiana.deployment-startup-request.v1";
/// 启动结果 report 在序列化数据中的固定 schema 标识。
pub const DEPLOYMENT_STARTUP_REPORT_SCHEMA: &str = "kiana.deployment-startup-report.v1";
/// 这组三类启动 DTO 当前共同使用的 major/minor 版本。
pub const DEPLOYMENT_STARTUP_VERSION: SchemaVersion = SchemaVersion::new(1, 0);
/// 固定启动链包含的阶段数量：Manifest、Root、Trust、Lease、Store、Migration、Projector、Capacity。
pub const MAX_STARTUP_FACTS: usize = 8;
/// `reason` / `remediation` 文本的字节上限（256 bytes，约 256 个 ASCII 字符）。
/// `String::len()` 按 UTF-8 字节计数，多字节中文字符会占用多个字节；同时拒绝空白、NUL 和换行。
pub const MAX_STARTUP_TEXT_BYTES: usize = 256;

/// 启动前必须依次确认的八类证据。
///
/// 顺序既是启动依赖，也是 `derive_result()` 的拒绝优先级：上游阶段不 Ready 时不会继续
/// 把下游阶段标成已完成。`ALL` 和 `ordinal()` 让 reducer、构造器与 wire 校验共享同一顺序。
/// Serde 的 `rename_all = "snake_case"` 将这些 Rust 变体写成稳定的小写下划线 wire 值。
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StartupStage {
    /// Release manifest 与目标 build/artifact 的身份摘要。
    Manifest,
    /// 目标存储根的身份/解析结果摘要。
    Root,
    /// 项目是否通过信任检查及其证据摘要。
    Trust,
    /// 当前启动实例使用的 operation lease / fencing 摘要。
    Lease,
    /// EventStore / journal 可用性和完整性摘要。
    Store,
    /// 迁移注册表及目标 schema 兼容性摘要。
    Migration,
    /// 投影/checkpoint 当前状态摘要。
    Projector,
    /// 当前容量与资源限制检查摘要。
    Capacity,
}

impl StartupStage {
    /// 启动 reducer 使用的规范顺序表；每个 stage 恰好出现一次。
    pub const ALL: [Self; MAX_STARTUP_FACTS] = [
        Self::Manifest,
        Self::Root,
        Self::Trust,
        Self::Lease,
        Self::Store,
        Self::Migration,
        Self::Projector,
        Self::Capacity,
    ];

    /// 将阶段映射为从 1 开始的固定序号，供请求构造器排序和反序列化后校验顺序。
    ///
    /// 这里的数字只是规范顺序编号，不是数据库 ID、优先级分数或可配置阶段位置；修改它会
    /// 改变 request 的规范排序和已序列化 facts 的校验结果。
    pub const fn ordinal(self) -> u8 {
        match self {
            Self::Manifest => 1,
            Self::Root => 2,
            Self::Trust => 3,
            Self::Lease => 4,
            Self::Store => 5,
            Self::Migration => 6,
            Self::Projector => 7,
            Self::Capacity => 8,
        }
    }
}

/// 单个启动阶段提交的事实状态。
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StartupFactStatus {
    /// 检查尚未得出可用结论；不能提升为 Ready。
    Pending,
    /// 调用方报告该阶段满足当前 reducer 合同；真实源对象是否可信仍由上游负责。
    Ready,
    /// 已确定不满足条件，需要按 remediation 处理。
    Blocked,
    /// 证据不足或结果不确定；整个启动报告保留 Unknown。
    Unknown,
}

/// 对检测到未完成旧 operation 时所采用的启动策略。
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StartupResumeMode {
    /// 以全新运行启动；若仍有 pending operation，则阻止 Ready。
    Fresh,
    /// 调用方明确声明走恢复路径，并在 pending operation 时提供 recovery evidence digest。
    ExplicitRecovery,
}

/// 八阶段 reducer 的最终摘要状态。
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StartupCoordinatorStatus {
    /// 所有阶段事实均与 request 的摘要、epoch、generation 匹配且为 Ready，也没有 Fresh 模式下的遗留 pending operation。
    Ready,
    /// 存在可确定的阻塞原因，例如缺阶段、版本漂移、Pending 或 Blocked fact。
    Blocked,
    /// 至少一个阶段明确报告事实/效果不确定，不能把它降为普通拒绝或成功。
    Unknown,
}

/// 某个固定启动阶段的 adapter-supplied fact。
///
/// 该对象本身不是 lease、manifest 或 migration report；`source_digest` 只是声称与相应
/// request 字段相同的摘要。reducer 会比较摘要、epoch、generation 和状态，但不会回读源对象。
/// `Serialize`/`Deserialize` 支持 wire 表示，`deny_unknown_fields` 拒绝当前 schema 不认识的字段。
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StartupStageFact {
    /// 固定 fact schema 名称。
    pub schema: String,
    /// 当前 fact schema 版本。
    pub version: SchemaVersion,
    /// 此记录说明八阶段中的哪一个阶段。
    pub stage: StartupStage,
    /// 阶段当前结果；只有 Ready 会推进 completed_stages。
    pub status: StartupFactStatus,
    /// 该阶段所依据对象的 SHA-256 摘要，稍后与 request 中同阶段摘要精确比较。
    pub source_digest: String,
    /// 事实采集时使用的 authority epoch；必须与 request 当前 epoch 相同。
    pub authority_epoch: u64,
    /// 事实采集时使用的数据 epoch；必须与 request 当前 epoch 相同。
    pub data_epoch: u64,
    /// 事实生成代数；必须与 request generation 相同，用于拒绝上一轮启动留下的陈旧事实。
    pub generation: u64,
    /// 事实来源游标。它进入 `fact_digest`，但当前 `derive_result()` 不读取或比较该值。
    pub source_cursor: u64,
    /// 面向调用方的稳定状态原因；Ready 必须为 `ok`，其他状态不能为 `ok`。
    pub reason: String,
    /// 建议的人工/上层修复动作；Ready 必须为 `none`，其他状态必须提供非 `none` 值。
    pub remediation: String,
    /// 对本 fact 其余字段计算的摘要，用于发现载荷被修改。
    pub fact_digest: String,
}

impl StartupStageFact {
    /// 构造一个带当前 schema 和自摘要的阶段事实。
    ///
    /// 调用方传入阶段来源摘要、epoch/generation 和解释文案；函数先组装 payload，再计算
    /// `fact_digest`，最后执行同一套 validate。字段不合法或摘要形状错误时返回稳定字符串，
    /// 不会查询 manifest、root、lease 或其他外部组件。
    pub fn new(
        stage: StartupStage,
        status: StartupFactStatus,
        source_digest: impl Into<String>,
        authority_epoch: u64,
        data_epoch: u64,
        generation: u64,
        source_cursor: u64,
        reason: impl Into<String>,
        remediation: impl Into<String>,
    ) -> Result<Self, String> {
        let mut fact = Self {
            schema: DEPLOYMENT_STARTUP_FACT_SCHEMA.to_owned(),
            version: DEPLOYMENT_STARTUP_VERSION,
            stage,
            status,
            source_digest: source_digest.into(),
            authority_epoch,
            data_epoch,
            generation,
            source_cursor,
            reason: reason.into(),
            remediation: remediation.into(),
            fact_digest: String::new(),
        };
        // 摘要先于 validate 生成，因此调用方无需自行同步维护 fact_digest；它仍只是内容
        // 指纹，不能证明 `source_digest` 指向的对象真实存在或已被可信 adapter 检查。
        fact.fact_digest = fact.digest();
        fact.validate()?;
        Ok(fact)
    }

    /// 校验 fact 的版本、epoch、摘要形状、解释字段和自摘要。
    ///
    /// Ready 必须写成 `reason="ok"`、`remediation="none"`；Pending/Blocked/Unknown 必须
    /// 同时提供非成功原因和非空 remediation。失败返回字段专属字符串，由 Startup request
    /// 校验逐条传播。这里没有检查 source_cursor 是否非零或当前，只把它纳入 fact 摘要。
    pub fn validate(&self) -> Result<(), String> {
        if self.schema != DEPLOYMENT_STARTUP_FACT_SCHEMA
            || self.version != DEPLOYMENT_STARTUP_VERSION
            || self.authority_epoch == 0
            || self.data_epoch == 0
            || self.generation == 0
        {
            return Err("deployment_startup_fact_header_invalid".to_owned());
        }
        valid_digest(&self.source_digest, "deployment_startup_fact_source_digest")?;
        valid_digest(&self.fact_digest, "deployment_startup_fact_digest")?;
        bounded(&self.reason, "deployment_startup_fact_reason")?;
        bounded(&self.remediation, "deployment_startup_fact_remediation")?;
        // 状态和面向人的解释必须一致，避免 Ready 仍携带阻塞原因，或非 Ready 被描述成成功。
        match self.status {
            StartupFactStatus::Ready if self.reason != "ok" || self.remediation != "none" => {
                return Err("deployment_startup_ready_fact_explanation_invalid".to_owned());
            }
            StartupFactStatus::Pending
            | StartupFactStatus::Blocked
            | StartupFactStatus::Unknown
                if self.reason == "ok" || self.remediation == "none" =>
            {
                return Err("deployment_startup_nonready_fact_explanation_invalid".to_owned());
            }
            _ => {}
        }
        if self.fact_digest != self.digest() {
            return Err("deployment_startup_fact_digest_mismatch".to_owned());
        }
        Ok(())
    }

    /// 对阶段状态及其版本/来源/解释字段计算指纹。
    ///
    /// `json_digest` 会先对 JSON 对象键排序再计算 SHA-256，因此同一结构不受键插入顺序影响。
    /// `fact_digest` 本身不进入输入以避免自引用；摘要只提供内容一致性检查，不是签名。
    pub fn digest(&self) -> String {
        json_digest(&serde_json::json!({
            "schema": self.schema,
            "version": self.version,
            "stage": self.stage,
            "status": self.status,
            "source_digest": self.source_digest,
            "authority_epoch": self.authority_epoch,
            "data_epoch": self.data_epoch,
            "generation": self.generation,
            "source_cursor": self.source_cursor,
            "reason": self.reason,
            "remediation": self.remediation,
        }))
    }
}

/// 适配器在构造 wire request 前提供的启动证据集合。
///
/// 它与 `StartupCoordinatorRequest` 分离，避免把 wire JSON 的默认值当成已收集的源证据。
/// 该中间结构由调用方填充；本模块不提供从真实 filesystem/EventStore/lease/provider 等
/// adapter 读取这些值的生产采集器。
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct StartupCoordinatorInput {
    /// 目标 release manifest 的内容摘要。
    pub release_manifest_digest: String,
    /// 目标 storage root 身份/预检结果的摘要。
    pub storage_root_digest: String,
    /// 项目 trust 检查输入/结果对应的摘要。
    pub project_trust_digest: String,
    /// 项目是否已经通过信任判定；Trust stage 会显式要求 true。
    pub project_trusted: bool,
    /// 当前 operation lease/fencing 摘要。
    pub lease_digest: String,
    /// EventStore/journal 检查证据摘要。
    pub store_digest: String,
    /// migration registry/schema preflight 证据摘要。
    pub migration_registry_digest: String,
    /// projector/checkpoint 预检证据摘要。
    pub projector_digest: String,
    /// capacity 预检证据摘要。
    pub capacity_digest: String,
    /// 配置 revision 的摘要；当前只在 request digest 和格式校验中使用，不是阶段门槛。
    pub config_revision: String,
    /// 本次启动当前 authority epoch。
    pub authority_epoch: u64,
    /// 本次启动当前 data epoch。
    pub data_epoch: u64,
    /// 本次启动 generation，必须与每个 fact 相同。
    pub generation: u64,
    /// 上一轮是否留下未完成 operation；Fresh 模式下会阻止最终 Ready。
    pub pending_operation: bool,
    /// 调用方对 operation journal 完整性的布尔结论；Store 阶段会要求为 true。
    pub journal_valid: bool,
    /// operation journal 的摘要；当前做格式和 request digest 校验，不读取 journal 内容。
    pub journal_digest: String,
    /// 遇到 pending operation 时采用 Fresh 拒绝还是显式恢复证据模式。
    pub resume_mode: StartupResumeMode,
    /// 可选恢复证据摘要；pending + ExplicitRecovery 时必须提供并通过格式校验。本 reducer
    /// 不执行恢复，也不验证摘要指向的 recovery object。
    pub recovery_evidence_digest: Option<String>,
    /// 八个有序启动阶段的 facts；构造 request 时会按 stage ordinal 排序。
    pub facts: Vec<StartupStageFact>,
}

/// DEP-10 reducer 接收的 versioned wire request。
///
/// 它承载 `StartupCoordinatorInput` 的摘要与事实，并额外携带 schema/version/request digest。
/// JSON `Deserialize` 可直接构造此类型，但不会调用 `new()` 的排序逻辑；外部 wire 输入必须
/// 已按 `StartupStage::ordinal()` 严格递增，否则 `validate()` 会拒绝。未知字段被 Serde 拒绝。
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StartupCoordinatorRequest {
    /// 固定 request schema 名称。
    pub schema: String,
    /// request 版本。
    pub version: SchemaVersion,
    /// 目标 release manifest 的摘要。
    pub release_manifest_digest: String,
    /// 目标 storage root 的摘要。
    pub storage_root_digest: String,
    /// 项目 trust 输入/结果的摘要。
    pub project_trust_digest: String,
    /// 项目是否已通过 trust 判定。
    pub project_trusted: bool,
    /// operation lease/fencing 的摘要。
    pub lease_digest: String,
    /// EventStore/journal 检查的摘要。
    pub store_digest: String,
    /// migration registry/schema preflight 的摘要。
    pub migration_registry_digest: String,
    /// projector/checkpoint 预检的摘要。
    pub projector_digest: String,
    /// capacity 预检的摘要。
    pub capacity_digest: String,
    /// config revision digest；只参与格式和 request digest 校验，当前不参与阶段 reducer 条件。
    pub config_revision: String,
    /// 所有阶段 fact 必须匹配的当前 authority epoch。
    pub authority_epoch: u64,
    /// 所有阶段 fact 必须匹配的当前 data epoch。
    pub data_epoch: u64,
    /// 所有阶段 fact 必须匹配的启动 generation。
    pub generation: u64,
    /// 是否存在需要显式处理的旧 operation。
    pub pending_operation: bool,
    /// Store 阶段是否可以信任调用方报告的 journal 完整性。
    pub journal_valid: bool,
    /// journal digest 只校验格式并绑定在 request digest 内，本 reducer 不读取 journal。
    pub journal_digest: String,
    /// Fresh 禁止 pending operation 隐式续跑；ExplicitRecovery 要求有恢复证据摘要。
    pub resume_mode: StartupResumeMode,
    /// 可选恢复证据摘要。只验证格式并纳入 request digest，不在此执行恢复或验证对象内容。
    pub recovery_evidence_digest: Option<String>,
    /// 阶段 facts 必须无重复、按固定顺序递增，且数量不超过 `MAX_STARTUP_FACTS`。
    pub facts: Vec<StartupStageFact>,
    /// request 除自身摘要外所有字段的 SHA-256 指纹。
    pub request_digest: String,
}

impl StartupCoordinatorRequest {
    /// 从适配器输入构造规范化 request。
    ///
    /// 【作用与调用关系】设计上由 adapter 在把 facts 交给 Core 前使用此构造器；之后
    /// `StartupCoordinatorReport::evaluate()` 才会按阶段顺序折叠它们。当前工作区中实际调用
    /// 本构造器的是 Domain fixtures，尚未发现生产 adapter 调用点，因此这里描述的是合同入口，
    /// 不是已经接通的进程启动采集流程。
    ///
    /// 【核心流程】把 input 字段搬入 wire DTO → 按 `StartupStage::ordinal()` 排序 facts →
    /// 计算 request digest → 调用 `validate()`。排序只发生在这个构造器；从 JSON 直接
    /// `Deserialize` 的 request 不会自动排序，必须已经符合严格递增顺序。
    ///
    /// 【失败/副作用】摘要/字段/顺序错误返回稳定字符串；没有存储、网络或恢复副作用。
    pub fn new(input: StartupCoordinatorInput) -> Result<Self, String> {
        let mut request = Self {
            schema: DEPLOYMENT_STARTUP_REQUEST_SCHEMA.to_owned(),
            version: DEPLOYMENT_STARTUP_VERSION,
            release_manifest_digest: input.release_manifest_digest,
            storage_root_digest: input.storage_root_digest,
            project_trust_digest: input.project_trust_digest,
            project_trusted: input.project_trusted,
            lease_digest: input.lease_digest,
            store_digest: input.store_digest,
            migration_registry_digest: input.migration_registry_digest,
            projector_digest: input.projector_digest,
            capacity_digest: input.capacity_digest,
            config_revision: input.config_revision,
            authority_epoch: input.authority_epoch,
            data_epoch: input.data_epoch,
            generation: input.generation,
            pending_operation: input.pending_operation,
            journal_valid: input.journal_valid,
            journal_digest: input.journal_digest,
            resume_mode: input.resume_mode,
            recovery_evidence_digest: input.recovery_evidence_digest,
            facts: input.facts,
            request_digest: String::new(),
        };
        // 规范排序让同一组逻辑 facts 不因 adapter 收集顺序不同而得到不同摘要。
        // 排序不会删除重复 stage；重复阶段仍会在后续校验中被拒绝。
        request.facts.sort_by_key(|fact| fact.stage.ordinal());
        request.request_digest = request.digest();
        request.validate()?;
        Ok(request)
    }

    /// 检查 request 的 schema、阶段顺序、摘要形状、恢复模式和自摘要。
    ///
    /// 外部 wire request 不会自动规范化，因此阶段必须严格递增且不能超出八项上限。每个
    /// fact 还会独立校验。`recovery_evidence_digest` 只验证格式并绑定 request；不会检查
    /// 恢复对象或实际恢复结果。所有失败都以字段/条件专属字符串返回。
    pub fn validate(&self) -> Result<(), String> {
        // 这是 wire 边界的顺序拒绝。重复/逆序阶段会在 reducer 运行前失败，避免相同阶段
        // 被多次折叠或将调用方数组顺序误当成依赖顺序。
        if self.schema != DEPLOYMENT_STARTUP_REQUEST_SCHEMA
            || self.version != DEPLOYMENT_STARTUP_VERSION
            || self.authority_epoch == 0
            || self.data_epoch == 0
            || self.generation == 0
            || self.facts.len() > MAX_STARTUP_FACTS
            || self
                .facts
                .windows(2)
                .any(|pair| pair[0].stage.ordinal() >= pair[1].stage.ordinal())
        {
            return Err("deployment_startup_request_header_invalid".to_owned());
        }
        for (digest, field) in [
            (
                &self.release_manifest_digest,
                "deployment_startup_manifest_digest",
            ),
            (
                &self.storage_root_digest,
                "deployment_startup_storage_root_digest",
            ),
            (
                &self.project_trust_digest,
                "deployment_startup_project_trust_digest",
            ),
            (&self.lease_digest, "deployment_startup_lease_digest"),
            (&self.store_digest, "deployment_startup_store_digest"),
            (
                &self.migration_registry_digest,
                "deployment_startup_migration_digest",
            ),
            (
                &self.projector_digest,
                "deployment_startup_projector_digest",
            ),
            (&self.capacity_digest, "deployment_startup_capacity_digest"),
            (&self.config_revision, "deployment_startup_config_revision"),
            (&self.journal_digest, "deployment_startup_journal_digest"),
            (&self.request_digest, "deployment_startup_request_digest"),
        ] {
            valid_digest(digest, field)?;
        }
        // pending operation 只有在调用方明确选择 ExplicitRecovery 并给出摘要时，才可继续
        // 到事实折叠；这仍不执行恢复。Fresh 携带恢复摘要则自相矛盾，直接拒绝。
        if self.pending_operation
            && self.resume_mode == StartupResumeMode::ExplicitRecovery
            && self.recovery_evidence_digest.is_none()
        {
            return Err("deployment_startup_recovery_evidence_required".to_owned());
        }
        match (&self.resume_mode, &self.recovery_evidence_digest) {
            (StartupResumeMode::Fresh, Some(_)) => {
                return Err("deployment_startup_fresh_recovery_evidence_invalid".to_owned())
            }
            (_, Some(digest)) => valid_digest(digest, "deployment_startup_recovery_digest")?,
            _ => {}
        }
        for fact in &self.facts {
            fact.validate()?;
        }
        // 严格递增校验已经会拒绝任意重复 stage；这里保留集合检查作为额外防线。当前重复
        // 输入通常先返回 request_header_invalid，因此下面的 duplicate_stage 错误分支可能
        // 无法从正常调用路径到达。
        if self
            .facts
            .iter()
            .map(|fact| fact.stage)
            .collect::<std::collections::BTreeSet<_>>()
            .len()
            != self.facts.len()
        {
            return Err("deployment_startup_duplicate_stage".to_owned());
        }
        if self.request_digest != self.digest() {
            return Err("deployment_startup_request_digest_mismatch".to_owned());
        }
        Ok(())
    }

    /// 计算绑定所有 request 输入（包括 facts 顺序）的 SHA-256 指纹。
    ///
    /// `request_digest` 自身不进入哈希输入。构造器先规范化 stage 顺序再调用本方法；
    /// 直接构造/反序列化者必须保证相同规范顺序，且此 digest 不是 adapter 身份签名。
    pub fn digest(&self) -> String {
        json_digest(&serde_json::json!({
            "schema": self.schema,
            "version": self.version,
            "release_manifest_digest": self.release_manifest_digest,
            "storage_root_digest": self.storage_root_digest,
            "project_trust_digest": self.project_trust_digest,
            "project_trusted": self.project_trusted,
            "lease_digest": self.lease_digest,
            "store_digest": self.store_digest,
            "migration_registry_digest": self.migration_registry_digest,
            "projector_digest": self.projector_digest,
            "capacity_digest": self.capacity_digest,
            "config_revision": self.config_revision,
            "authority_epoch": self.authority_epoch,
            "data_epoch": self.data_epoch,
            "generation": self.generation,
            "pending_operation": self.pending_operation,
            "journal_valid": self.journal_valid,
            "journal_digest": self.journal_digest,
            "resume_mode": self.resume_mode,
            "recovery_evidence_digest": self.recovery_evidence_digest,
            "facts": self.facts,
        }))
    }
}

/// 启动协调器对一份 request 计算出的只读结果。
///
/// `completed_stages` 保存按固定顺序已经通过的前缀；`blocked_stage` 指向最早不能继续的
/// 阶段（或遗留 operation 对应的 Lease 阶段）；`reason` 与 `remediation` 给出机器可读/可
/// 展示原因。Unknown 必须原样保留，不能渲染成 Ready。报告摘要只能校验字段一致性。
///
/// `Serialize`/`Deserialize` 支持 API/fixture 传输，`deny_unknown_fields` 拒绝未知 schema 字段。
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StartupCoordinatorReport {
    /// 固定 report schema 名称。
    pub schema: String,
    /// report 版本。
    pub version: SchemaVersion,
    /// 生成此报告的 request digest。
    pub request_digest: String,
    /// 整个启动流程的最终 Ready/Blocked/Unknown 状态。
    pub status: StartupCoordinatorStatus,
    /// 从 Manifest 开始按序通过的阶段列表；阶段 fact 失败时只包含它之前的通过项。
    pub completed_stages: Vec<StartupStage>,
    /// 首个失败/不确定阶段；全部 fact Ready 时通常为 None。Fresh pending operation 是循环后
    /// 的额外门槛，因此可能在八个 completed stage 全部列出时仍指向 Lease。
    pub blocked_stage: Option<StartupStage>,
    /// 最终稳定原因，例如 `ok` 或首个失败 fact 的 reason。
    pub reason: String,
    /// 调用方可采取的下一步，例如补齐某 stage fact 或显式记录 recovery evidence。
    pub remediation: String,
    /// 对报告其余字段计算的摘要；不是签名或外部证据认证。
    pub report_digest: String,
}

impl StartupCoordinatorReport {
    /// 校验 request、按八阶段 reducer 生成报告，再校验报告与 request 的绑定。
    ///
    /// 首个缺失/不匹配/非 Ready fact 会确定返回状态和最早阻塞 stage；只有八阶段全部 Ready
    /// 且无 Fresh 模式的 pending operation 才返回 Ready。失败分成 request 校验错误、报告
    /// 字段错误或绑定错误，均以稳定 `String` 错误返回。函数不实际执行启动动作。
    pub fn evaluate(request: &StartupCoordinatorRequest) -> Result<Self, String> {
        request.validate()?;
        let (status, completed_stages, blocked_stage, reason, remediation) = derive_result(request);
        let mut report = Self {
            schema: DEPLOYMENT_STARTUP_REPORT_SCHEMA.to_owned(),
            version: DEPLOYMENT_STARTUP_VERSION,
            request_digest: request.request_digest.clone(),
            status,
            completed_stages,
            blocked_stage,
            reason,
            remediation,
            report_digest: String::new(),
        };
        report.report_digest = report.digest();
        report.validate_against(request)?;
        Ok(report)
    }

    /// 检查报告 schema、阶段序列、文案长度、摘要格式与自摘要。
    ///
    /// 不带 request 时只能校验报告内部结构，无法判断状态是否真由某组 facts 算出；需要
    /// 调用 `validate_against()` 完成输入绑定。
    pub fn validate(&self) -> Result<(), String> {
        if self.schema != DEPLOYMENT_STARTUP_REPORT_SCHEMA
            || self.version != DEPLOYMENT_STARTUP_VERSION
            || self.completed_stages.len() > MAX_STARTUP_FACTS
            || self
                .completed_stages
                .windows(2)
                .any(|pair| pair[0].ordinal() >= pair[1].ordinal())
        {
            return Err("deployment_startup_report_header_invalid".to_owned());
        }
        valid_digest(
            &self.request_digest,
            "deployment_startup_report_request_digest",
        )?;
        valid_digest(&self.report_digest, "deployment_startup_report_digest")?;
        bounded(&self.reason, "deployment_startup_report_reason")?;
        bounded(&self.remediation, "deployment_startup_report_remediation")?;
        if self.report_digest != self.digest() {
            return Err("deployment_startup_report_digest_mismatch".to_owned());
        }
        Ok(())
    }

    /// 重算 request 的期望结果，并校验当前报告的 request digest 与全部派生字段一致。
    ///
    /// 这能发现 report 自摘要正确但内容与输入 facts 不符的情况；它仍不读取摘要引用对象，
    /// 不证明适配器采集的事实为真。
    pub fn validate_against(&self, request: &StartupCoordinatorRequest) -> Result<(), String> {
        request.validate()?;
        self.validate()?;
        let expected = derive_result(request);
        if self.request_digest != request.request_digest
            || (
                self.status,
                &self.completed_stages,
                self.blocked_stage,
                &self.reason,
                &self.remediation,
            ) != (
                expected.0,
                &expected.1,
                expected.2,
                &expected.3,
                &expected.4,
            )
        {
            return Err("deployment_startup_report_binding_mismatch".to_owned());
        }
        Ok(())
    }

    /// 计算绑定 request、状态、已完成阶段、阻塞点和解释文案的报告指纹。
    ///
    /// `report_digest` 不放入输入以避免递归；此无密钥摘要只用于检测载荷漂移。
    pub fn digest(&self) -> String {
        json_digest(&serde_json::json!({
            "schema": self.schema,
            "version": self.version,
            "request_digest": self.request_digest,
            "status": self.status,
            "completed_stages": self.completed_stages,
            "blocked_stage": self.blocked_stage,
            "reason": self.reason,
            "remediation": self.remediation,
        }))
    }
}

/// 按固定的八阶段顺序将 facts 折叠为启动结果。
///
/// 【输入/输出】调用方已通过 `StartupCoordinatorRequest::validate()` 的 request；函数返回
/// `(status, completed_stages, blocked_stage, reason, remediation)`，不构造或写入持久化记录。
///
/// 【核心顺序】依次处理 Manifest → Root → Trust → Lease → Store → Migration → Projector →
/// Capacity。每个阶段按“必须存在 → authority/data epoch 与 generation 匹配 → source digest
/// 匹配 → 阶段专属门槛 → fact.status 为 Ready”检查。第一项失败就立即返回，因此后续阶段
/// 不会被假装为已完成；Unknown 保持 Unknown，Pending/Blocked 转为 Blocked。
///
/// ⚠ 这里使用的是 request 里已经提供的布尔值和 digest，不会重查源事实。`source_cursor`、
/// `config_revision`、`journal_digest` 与 recovery digest 虽然被摘要/格式校验绑定，当前不
/// 参与阶段状态比较；journal 只在 Store 阶段读取 `journal_valid` 布尔值。ExplicitRecovery
/// 也只是允许有 pending operation 的输入继续折叠，并不真的恢复旧 operation。
fn derive_result(
    request: &StartupCoordinatorRequest,
) -> (
    StartupCoordinatorStatus,
    Vec<StartupStage>,
    Option<StartupStage>,
    String,
    String,
) {
    let mut completed = Vec::new();
    for stage in StartupStage::ALL {
        // 缺少阶段不能当作默认 Ready。返回此前已经按顺序通过的前缀，并指明最早缺失项。
        let Some(fact) = request.facts.iter().find(|fact| fact.stage == stage) else {
            return (
                StartupCoordinatorStatus::Blocked,
                completed,
                Some(stage),
                "startup_stage_missing".to_owned(),
                format!("supply_verified_{stage:?}_fact"),
            );
        };
        // facts 必须来自同一轮启动身份。旧 authority/data epoch 或 generation 即使内容
        // 摘要仍合法，也不能与当前 request 混合使用。
        if fact.authority_epoch != request.authority_epoch
            || fact.data_epoch != request.data_epoch
            || fact.generation != request.generation
        {
            return (
                StartupCoordinatorStatus::Blocked,
                completed,
                Some(stage),
                "startup_epoch_or_generation_mismatch".to_owned(),
                "rebuild_facts_for_current_startup_epoch".to_owned(),
            );
        }
        // 每个 stage 对应 request 中一项 server-supplied digest。这里只做字符串相等比较，
        // 不重新打开 manifest、storage root、lease 或其他来源对象。
        let expected_digest = match stage {
            StartupStage::Manifest => &request.release_manifest_digest,
            StartupStage::Root => &request.storage_root_digest,
            StartupStage::Trust => &request.project_trust_digest,
            StartupStage::Lease => &request.lease_digest,
            StartupStage::Store => &request.store_digest,
            StartupStage::Migration => &request.migration_registry_digest,
            StartupStage::Projector => &request.projector_digest,
            StartupStage::Capacity => &request.capacity_digest,
        };
        if &fact.source_digest != expected_digest {
            return (
                StartupCoordinatorStatus::Blocked,
                completed,
                Some(stage),
                "startup_stage_digest_mismatch".to_owned(),
                "recompute_stage_from_current_revision".to_owned(),
            );
        }
        // trust 和 journal 是显式的额外门槛：两者的 digest match 不会替代布尔结论。
        if stage == StartupStage::Trust && !request.project_trusted {
            return (
                StartupCoordinatorStatus::Blocked,
                completed,
                Some(stage),
                "startup_project_untrusted".to_owned(),
                "trust_project_before_startup".to_owned(),
            );
        }
        if stage == StartupStage::Store && !request.journal_valid {
            return (
                StartupCoordinatorStatus::Blocked,
                completed,
                Some(stage),
                "startup_journal_invalid".to_owned(),
                "quarantine_and_reconcile_journal".to_owned(),
            );
        }
        if fact.status != StartupFactStatus::Ready {
            // Unknown 表示结果本身不确定，必须和可确定的 Blocked 分开保留；Pending 在本
            // coordinator 中按尚未通过门槛处理，所以归为 Blocked。
            return (
                if fact.status == StartupFactStatus::Unknown {
                    StartupCoordinatorStatus::Unknown
                } else {
                    StartupCoordinatorStatus::Blocked
                },
                completed,
                Some(stage),
                fact.reason.clone(),
                fact.remediation.clone(),
            );
        }
        completed.push(stage);
    }
    // 旧 operation 不能因“八项新启动预检都通过”而静默续跑。Fresh 明确阻塞；
    // ExplicitRecovery 的摘要格式已在 request.validate() 检查，但这里不执行恢复动作。
    if request.pending_operation && request.resume_mode == StartupResumeMode::Fresh {
        return (
            StartupCoordinatorStatus::Blocked,
            completed,
            Some(StartupStage::Lease),
            "startup_resume_requires_explicit_recovery".to_owned(),
            "record_recovery_evidence_before_resume".to_owned(),
        );
    }
    (
        StartupCoordinatorStatus::Ready,
        completed,
        None,
        "ok".to_owned(),
        "none".to_owned(),
    )
}

/// 校验 reason/remediation 是非空白、有限字节长度且不含 NUL/CR/LF 的单行文本。
///
/// 这些字段会进入 fact/report digest 并由上层展示；限制长度和控制字符可避免输出被截断或
/// 注入额外日志行。失败原因通过传入的稳定 field key 后缀表达。
fn bounded(value: &str, field: &str) -> Result<(), String> {
    if value.trim().is_empty()
        || value.len() > MAX_STARTUP_TEXT_BYTES
        || value.contains(['\0', '\r', '\n'])
    {
        return Err(format!("{field}_invalid"));
    }
    Ok(())
}

/// 检查摘要字符串外观为精确小写前缀 `sha256:` 加 64 位十六进制文本。
///
/// 只检查格式，不读取或验证被摘要引用的外部内容；十六进制部分允许大小写。调用者字段
/// 名会成为稳定错误码的一部分，方便上层区分哪类输入不合法。
fn valid_digest(value: &str, field: &str) -> Result<(), String> {
    let Some(hex) = value.strip_prefix("sha256:") else {
        return Err(format!("{field}_invalid"));
    };
    if hex.len() != 64 || !hex.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err(format!("{field}_invalid"));
    }
    Ok(())
}

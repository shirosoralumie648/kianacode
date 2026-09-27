//! Versioned `ops.*` wire contracts.
//!
//! These DTOs describe an operator intent and its evidence envelope. They never authorize an
//! operation, consume an approval, call a Broker, append EventLog facts or perform a supervisor
//! action. Core must compare the supplied actor/epoch/scope with a server-owned snapshot before
//! dispatch; unknown envelopes remain inspectable and are never executable.
//!
//! # 这个文件在系统里的位置
//!
//! 运维（ops）命令的**线上格式**。所谓运维命令，指的是不需要模型参与、
//! 由人直接发起的系统级操作：备份、恢复、迁移、投影器重建、对账、灰度发布、维护窗口。
//!
//! ```text
//! 人 / 脚本 发出一个运维意图
//!        ↓  组装成 OpsCommandRequest（含身份、世代、作用域、幂等键）
//! 【本文件：只定义 DTO 与判定规则，零业务逻辑】
//!        ↓  kiana-core 拿服务端快照比对
//!    Core：比对 actor / epoch / scope 与服务端是否一致
//!        ↓  通过才继续走授权与执行路径
//!    实际执行（备份/恢复/迁移…）
//! ```
//!
//! # 最重要的一条：这些 DTO 不授权任何操作
//!
//! 文件头说得很直接：这些结构体 "never authorize an operation, consume an approval,
//! call a Broker, append EventLog facts or perform a supervisor action"。
//!
//! 翻译过来是**五不准**：
//! 1. 不授权任何操作；
//! 2. 不消费审批；
//! 3. 不调用能力代理（Broker）；
//! 4. 不写事件账本；
//! 5. 不执行任何监管动作。
//!
//! 它们只是**描述意图**的纯数据。想让它生效，必须由 `kiana-core` 拿服务端持有的
//! 快照去比对身份、世代和作用域，通过之后才能进入真正的执行流程。
//!
//! **为什么这条边界重要**：一个 DTO 如果自带"我已经被授权了"这种语义，
//! 那么只要有人能构造出这个 DTO，就等于拿到了执行权。
//! 把授权判定完全放在服务端，是让"客户端声称"和"服务端认定"分离的唯一办法。
//!
//! # 关键词：证据封套（evidence envelope）
//!
//! 文件头把 `OpsCommandRequest` 称为 "evidence envelope" —— 证据封套。
//! 这个词点出了它的性质：**它携带的是"证明"，不是"命令"**。
//!
//! 里面装的是：谁发的（第几代授权）、在什么范围内（第几号项目、哪个存储根）、
//! 内容摘要是什么、幂等键是什么。服务端拿这些去和自己的账本核对，
//! 核对通过才承认这次请求。
//!
//! # 术语
//!
//! - **operation（操作）**：一次运维动作，有唯一 ID。
//! - **idempotency key（幂等键）**：客户端生成的唯一标记。
//!   同一个键重复提交必须是同一个结果 —— 见 `OpsCommandRequest::replay_against`。
//! - **authority epoch（授权世代）**：一个单调递增的整数。
//!   管理员换届时递增，用来区分"不同代的权限"。
//! - **replay（重放）**：把同一个请求再发一次，应当得到与第一次相同的结果。
//! - **fail-closed**：证据不足时拒绝。
//!
//! # 上游契约
//!
//! These DTOs describe an operator intent and its evidence envelope. They never authorize an
//! operation, consume an approval, call a Broker, append EventLog facts or perform a supervisor
//! action. Core must compare the supplied actor/epoch/scope with a server-owned snapshot before
//! dispatch; unknown envelopes remain inspectable and are never executable.

use kiana_domain::{json_digest, OperationId, ProjectId, RequestId, SchemaVersion, StorageRootId};
use serde::{Deserialize, Serialize};
use serde_json::Value;

/// 运维协议的 schema 标识符。`v1` 是第一版。
///
pub const OPS_PROTOCOL_SCHEMA: &str = "kiana.ops.v1";
/// 运维协议的版本号。
///
pub const OPS_PROTOCOL_VERSION: SchemaVersion = SchemaVersion::new(1, 0);
/// 运维封套（envelope）的 schema 标识符。
///
/// 它和上面的协议 schema 是**两个不同的东西**：
/// 协议 schema 标识"运维命令这一类协议"，
/// 封套 schema 标识"承载证据的那个外层包装"。
/// 分开标识意味着将来可以只改封套格式而不动命令本身。
///
pub const OPS_ENVELOPE_SCHEMA: &str = "kiana.ops-envelope.v1";
/// 文本字段的最大长度：512 字节。
///
/// 【为什么是 512】
/// 运维命令的参数都是短标识符（项目 ID、存储根 ID、原因说明）。
/// 512 字节足够写一句人类可读的原因，又不至于让报文膨胀。
/// 这是一个"防滥用"上限，不是业务限制。
///
pub const OPS_MAX_TEXT: usize = 512;
/// 载荷（payload）的最大字节数：16 KiB。
///
/// 【为什么是 16 KiB】
/// 运维命令的载荷是结构化的小对象（要恢复哪个备份、要迁移到哪一步），
/// 不需要承载文件内容。
/// 16 KiB 已经远超正常需求，同时能防止有人把一个巨大的 JSON
/// 塞进运维通道来耗尽服务端内存。
///
/// ⚠ 注意：真正的大文件传输不走这个通道。运维通道只传"引用"和"参数"。
///
pub const OPS_MAX_PAYLOAD_BYTES: usize = 16 * 1024;

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
/// 全部运维命令。
///
/// 【作用】
/// 枚举系统支持的所有运维动作。这个列表是**封闭的** ——
/// 只有这里列出的命令才是合法的，其他一律拒绝。
///
/// 【七个分组】
///
/// 状态与诊断：
/// `Status`（看状态）、`Doctor`（诊断）、`Preflight`（预检）
///
/// 备份：
/// `BackupCreate`、`BackupList`、`BackupVerify`
///
/// 恢复：
/// `RestoreVerify`、`RestorePlan`、`RestoreActivate`
///
/// 迁移：
/// `MigratePlan`、`MigratePreflight`、`MigrateApply`、`MigrateResume`、`MigrateVerify`
///
/// 投影器（projector，把事件流投影成可查询状态的组件）：
/// `ProjectorStatus`、`ProjectorRebuild`、`ProjectorVerify`
///
/// 对账（reconcile，处理"结果未知"的操作）：
/// `ReconcileList`、`ReconcileShow`、`ReconcileCommit`
///
/// 灰度发布（rollout）：
/// `RolloutStatus`、`RolloutPause`、`RolloutResume`、`RolloutPromote`、`RolloutRollback`
///
/// 维护窗口：
/// `MaintenanceOpen`、`MaintenanceClose`
///
/// 【⚠ 危险命令与安全命令成对出现】
/// 注意这几组：`BackupCreate` / `BackupList` / `BackupVerify`、
/// `RestorePlan` / `RestoreActivate`、`MigratePlan` / `MigrateApply`。
///
/// 几乎每个破坏性动作都有一个**只读或预演**的对应物：
/// - `RestorePlan` 只生成恢复计划，`RestoreActivate` 才真正执行；
/// - `MigratePlan` 只规划，`MigrateApply` 才动手。
///
/// 这个配对模式是刻意的：**先看、再做**。
/// 如果只有 `RestoreActivate` 而没有 `RestorePlan`，
/// 运维人员不得不在不了解后果的情况下直接执行恢复。
///
pub enum OpsCommand {
    Status,
    Doctor,
    Preflight,
    Drain,
    BackupCreate,
    BackupList,
    BackupVerify,
    RestoreVerify,
    RestorePlan,
    RestoreActivate,
    MigratePlan,
    MigratePreflight,
    MigrateApply,
    MigrateResume,
    MigrateVerify,
    ProjectorStatus,
    ProjectorRebuild,
    ProjectorVerify,
    ReconcileList,
    ReconcileShow,
    ReconcileCommit,
    RolloutStatus,
    RolloutPause,
    RolloutResume,
    RolloutPromote,
    RolloutRollback,
    MaintenanceOpen,
    MaintenanceClose,
}

impl OpsCommand {
    /// 全部 28 个命令的完整列表。
    ///
    /// 【作用】
    /// 一个编译期固定的数组，包含 `OpsCommand` 的每一个变体。
    ///
    /// 【为什么需要它】
    /// 三个用途：
    /// 1. **遍历**：校验、文档生成、CLI 补全都需要"所有合法命令"这个集合；
    /// 2. **完整性保证**：如果将来给 `OpsCommand` 加了一个变体却忘了加进这个数组，
    ///    编译器不会报错，但遍历时会漏掉它。
    ///    ⚠ 所以新增变体时**必须**同步更新这个数组；
    /// 3. **顺序稳定**：数组顺序固定，遍历结果的顺序可复现。
    ///
    pub const ALL: [Self; 28] = [
        Self::Status,
        Self::Doctor,
        Self::Preflight,
        Self::Drain,
        Self::BackupCreate,
        Self::BackupList,
        Self::BackupVerify,
        Self::RestoreVerify,
        Self::RestorePlan,
        Self::RestoreActivate,
        Self::MigratePlan,
        Self::MigratePreflight,
        Self::MigrateApply,
        Self::MigrateResume,
        Self::MigrateVerify,
        Self::ProjectorStatus,
        Self::ProjectorRebuild,
        Self::ProjectorVerify,
        Self::ReconcileList,
        Self::ReconcileShow,
        Self::ReconcileCommit,
        Self::RolloutStatus,
        Self::RolloutPause,
        Self::RolloutResume,
        Self::RolloutPromote,
        Self::RolloutRollback,
        Self::MaintenanceOpen,
        Self::MaintenanceClose,
    ];

    /// 命令的稳定线上名称。
    ///
    /// 【作用】
    /// 把枚举变体映射成线上传输用的字符串（如 `Deny` → `"deny"`）。
    ///
    /// 【⚠ 这些字符串是对外契约】
    /// 它们出现在 CLI 参数、API 请求、日志里。
    /// 下游可能按这些字符串做分发和统计。
    /// 改写其中任何一个都会让下游静默失效。
    ///
    /// 【为什么用 `const fn`】
    /// 编译期可求值，不产生运行时代码。
    /// 这类纯查表函数标 `const` 是恰当的。
    ///
    pub const fn wire_name(self) -> &'static str {
        match self {
            Self::Status => "ops.status",
            Self::Doctor => "ops.doctor",
            Self::Preflight => "ops.preflight",
            Self::Drain => "ops.drain",
            Self::BackupCreate => "ops.backup.create",
            Self::BackupList => "ops.backup.list",
            Self::BackupVerify => "ops.backup.verify",
            Self::RestoreVerify => "ops.restore.verify",
            Self::RestorePlan => "ops.restore.plan",
            Self::RestoreActivate => "ops.restore.activate",
            Self::MigratePlan => "ops.migrate.plan",
            Self::MigratePreflight => "ops.migrate.preflight",
            Self::MigrateApply => "ops.migrate.apply",
            Self::MigrateResume => "ops.migrate.resume",
            Self::MigrateVerify => "ops.migrate.verify",
            Self::ProjectorStatus => "ops.projector.status",
            Self::ProjectorRebuild => "ops.projector.rebuild",
            Self::ProjectorVerify => "ops.projector.verify",
            Self::ReconcileList => "ops.reconcile.list",
            Self::ReconcileShow => "ops.reconcile.show",
            Self::ReconcileCommit => "ops.reconcile.commit",
            Self::RolloutStatus => "ops.rollout.status",
            Self::RolloutPause => "ops.rollout.pause",
            Self::RolloutResume => "ops.rollout.resume",
            Self::RolloutPromote => "ops.rollout.promote",
            Self::RolloutRollback => "ops.rollout.rollback",
            Self::MaintenanceOpen => "ops.maintenance.open",
            Self::MaintenanceClose => "ops.maintenance.close",
        }
    }

    /// 从线上字符串解析出命令。
    ///
    /// 【作用】
    /// `wire_name` 的逆操作。
    ///
    /// 【⚠ 解析失败返回 `None` 而不是 `Err`】
    /// 因为"这个字符串不是合法命令名"是一个**正常的输入校验结果**，
    /// 不是系统故障。调用方看到 `None` 就知道该拒绝这个请求。
    ///
    /// 【⚠ 解析失败时不应该"猜"】
    /// 一个拼错的命令名（比如 `"backupcreate"` 少了下划线）不能被"聪明地"纠正成
    /// `BackupCreate`。运维命令的错误执行后果太严重，
    /// 宁可明确报错让人去查文档，也不要自动纠正。
    ///
    pub fn parse(value: &str) -> Option<Self> {
        Self::ALL
            .iter()
            .copied()
            .find(|command| command.wire_name() == value)
    }
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
/// 运维查询类型。
///
/// 【作用】
/// 与 [`OpsCommand`] 平行的一组**只读**查询。
///
/// 【为什么查询和命令要分成两个类型】
/// 因为它们的授权要求不同：
/// - 命令会改变系统状态，需要更严格的授权和更完整的审计；
/// - 查询只读，可以放宽一点（比如允许在恢复期间查询状态）。
///
/// 用两个类型分开，编译器就能强制调用方声明"我这次是查还是做"，
/// 不会把一个查询当成命令执行。
///
pub enum OpsQuery {
    Status,
    Doctor,
    Preflight,
    ReconcileList,
    ReconcileShow,
    RolloutStatus,
}

impl OpsQuery {
    pub const fn wire_name(self) -> &'static str {
        match self {
            Self::Status => "ops.status",
            Self::Doctor => "ops.doctor",
            Self::Preflight => "ops.preflight",
            Self::ReconcileList => "ops.reconcile.list",
            Self::ReconcileShow => "ops.reconcile.show",
            Self::RolloutStatus => "ops.rollout.status",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        [
            Self::Status,
            Self::Doctor,
            Self::Preflight,
            Self::ReconcileList,
            Self::ReconcileShow,
            Self::RolloutStatus,
        ]
        .into_iter()
        .find(|query| query.wire_name() == value)
    }
}

/// 运维事件名称白名单。
///
/// 【作用】
/// 只有这 6 个事件名是合法的运维事件。
///
/// 【⚠ 为什么需要白名单】
/// 事件名会进事件账本。如果允许任意字符串，
/// 那么一个恶意的客户端可以写入伪造的事件名，
/// 让审计日志里出现根本没发生过的操作记录。
///
/// 白名单把"能写什么"限制在服务端认定的范围内。
///
const OPS_EVENT_NAMES: [&str; 6] = [
    "ops.operation.accepted",
    "ops.operation.state_changed",
    "ops.operation.completed",
    "ops.operation.failed",
    "ops.operation.cancelled",
    "ops.operation.unknown",
];

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
/// 运维操作的作用域。
///
/// 【作用】
/// 描述"这次操作管的是哪个项目、哪个存储根"。
///
/// 【两个字段都可以是 `None`】
/// - `project_id: None` —— 不限定项目（全局操作）；
/// - `storage_root_id: None` —— 不限定存储根。
///
/// 【⚠ 为什么允许 `None` 而不是强制指定】
/// 因为确实存在跨项目的操作（比如查看全局状态）。
/// 但 `None` 的含义必须是"**明确的不限定**"，
/// 而不是"忘了填"。校验函数会确保这一点 —— 见 `validate`。
///
pub struct OpsScope {
    pub project_id: Option<ProjectId>,
    pub storage_root_id: Option<StorageRootId>,
    pub scope_digest: String,
}

impl OpsScope {
    /// 构造一个作用域。
    ///
    /// 【作用】
    /// 便捷构造函数，把两个可选 ID 打包成 [`OpsScope`]。
    ///
    pub fn new(project_id: Option<ProjectId>, storage_root_id: Option<StorageRootId>) -> Self {
        let mut scope = Self {
            project_id,
            storage_root_id,
            scope_digest: String::new(),
        };
        scope.scope_digest = scope.digest();
        scope
    }

    /// 校验这个作用域本身是否合法。
    ///
    /// 【⚠ 关键规则：两个 `None` 不能同时出现】
    /// 如果项目和存储根都不限定，那这个作用域等于"全宇宙"。
    /// 这在语义上是危险的 —— 它意味着一条命令可以作用于任何地方。
    ///
    /// 所以校验会拒绝"两个都为空"的情况。
    /// 必须至少限定一个维度。
    ///
    /// 【失败情况】
    /// 返回带稳定原因码的 `Err`。
    ///
    pub fn validate(&self) -> Result<(), String> {
        if self.project_id.is_none() && self.storage_root_id.is_none() {
            return Err("ops_scope_empty".to_owned());
        }
        if self.project_id.is_some_and(|id| id.as_uuid().is_nil())
            || self.storage_root_id.is_some_and(|id| id.as_uuid().is_nil())
        {
            return Err("ops_scope_id_invalid".to_owned());
        }
        valid_digest(&self.scope_digest, "ops_scope_digest")?;
        if self.scope_digest != self.digest() {
            return Err("ops_scope_digest_mismatch".to_owned());
        }
        Ok(())
    }

    /// 计算作用域的摘要。
    ///
    /// 【作用】
    /// 让作用域可以被比对和记录。两个 `OpsScope` 摘要相同，
    /// 就说明它们指向同一个作用范围。
    ///
    pub fn digest(&self) -> String {
        json_digest(&serde_json::json!({
            "project_id": self.project_id,
            "storage_root_id": self.storage_root_id,
        }))
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
/// 权威快照 —— 服务端视角的"授权现状"。
///
/// 【作用】
/// 记录**服务端自己认为**当前是谁在操作、第几代授权、管哪些范围。
///
/// 【⚠ 它和请求里的 authority 字段是两个东西】
/// 请求里带的 authority 是**客户端的声明**（"我认为我是这些权限"），
/// 这个快照是**服务端的认定**。
///
/// 两者的比对由 `OpsCommandRequest::validate_authority` 完成。
/// 这正是文件头说的 "Core must compare the supplied actor/epoch/scope
/// with a server-owned snapshot"。
///
pub struct OpsAuthoritySnapshot {
    pub actor_id: String,
    pub authority_epoch: u64,
    pub scope: OpsScope,
}

impl OpsAuthoritySnapshot {
    /// 校验这个快照自身是否合法。
    ///
    /// 【核心检查】
    /// - 身份标识非空且在长度限制内；
    /// - 授权世代非 0（0 不代表任何真实世代）；
    /// - 作用域自身合法（见 `OpsScope::validate`）。
    ///
    pub fn validate(&self) -> Result<(), String> {
        bounded(&self.actor_id, "ops_actor_id")?;
        if self.authority_epoch == 0 {
            return Err("ops_authority_epoch_invalid".to_owned());
        }
        self.scope.validate()
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
/// 重放判定的结果。
///
/// 【作用】
/// 回答一个问题："这次请求是新的，还是上次那个的重放？"
///
/// 【两个变体】
/// - `New` —— 是一个全新的操作；
/// - `Replay` —— 与之前某次**完全相同**，应当返回上次的结果。
///
/// 【⚠ 注意这里没有"冲突"变体】
/// 冲突情况返回的是 `Err`，而不是某个变体。
/// 原因见 `OpsCommandRequest::replay_against` 的说明。
///
pub enum OpsReplayDisposition {
    New,
    Replay,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
/// 一次运维命令请求 —— 本文件的核心类型。
///
/// 【作用】
/// 把"想做什么"（command + payload）和"凭什么"（authority + 幂等键 + 摘要）
/// 打包成一个可传输、可校验、可审计的整体。
///
/// 【字段语义】
/// - `schema` / `command` —— 格式标识与命令名；
/// - `operation_id` —— **这次操作的唯一 ID**，由服务端或客户端生成；
/// - `idempotency_key` —— **幂等键**，用于识别重放；
/// - `authority` —— 客户端声明的身份/世代/作用域；
/// - `payload` —— 命令参数；
/// - `request_digest` —— 请求内容摘要，用于检测篡改。
///
/// 【⚠ 这不是一个"已授权"的请求】
/// 它是一个**声称自己已获授权**的请求。
/// 真伪由 `validate_authority` 对照服务端快照判定。
/// 文件头那句 "they never authorize an operation" 说的就是这个区别。
///
pub struct OpsCommandRequest {
    pub schema: String,
    pub command: String,
    pub operation_id: OperationId,
    pub idempotency_key: String,
    pub authority: OpsAuthoritySnapshot,
    pub payload: Value,
    pub request_digest: String,
}

impl OpsCommandRequest {
    /// 构造一个运维命令请求。
    ///
    /// 【作用】
    /// 组装并校验一个新的请求。
    ///
    /// 【⚠ 顺序：先算摘要再校验】
    /// `request_digest` 必须在 `validate()` 之前算好，
    /// 因为校验里会做"存储的摘要 vs 重算的摘要"的比对。
    ///
    pub fn new(
        command: OpsCommand,
        operation_id: OperationId,
        idempotency_key: impl Into<String>,
        authority: OpsAuthoritySnapshot,
        payload: Value,
    ) -> Result<Self, String> {
        let mut request = Self {
            schema: OPS_PROTOCOL_SCHEMA.to_owned(),
            command: command.wire_name().to_owned(),
            operation_id,
            idempotency_key: idempotency_key.into(),
            authority,
            payload,
            request_digest: String::new(),
        };
        request.request_digest = request.digest();
        request.validate()?;
        Ok(request)
    }

    /// 校验请求自身是否合法。
    ///
    /// 【核心检查 —— 四组】
    ///
    /// ① 头部与文本字段
    /// - schema 精确匹配；
    /// - 命令名非空、不超长度、无控制字符；
    /// - 幂等键同样检查。
    ///
    /// ② 标识与摘要格式
    /// - `operation_id` 必须是合法 UUID；
    /// - `request_digest` 必须是合法摘要格式。
    ///
    /// ③ 授权快照自身合法
    /// 见 `OpsAuthoritySnapshot::validate`。
    ///
    /// ④ 摘要自洽
    /// 重算摘要与存储值比对。这一步防的是**内容被改** ——
    /// 有人把 payload 里的"恢复哪个备份"改掉，摘要就对不上了。
    ///
    pub fn validate(&self) -> Result<(), String> {
        if self.schema != OPS_PROTOCOL_SCHEMA || self.operation_id.as_uuid().is_nil() {
            return Err("ops_command_header_invalid".to_owned());
        }
        if OpsCommand::parse(&self.command).is_none() {
            return Err("ops_unknown_command".to_owned());
        }
        bounded(&self.idempotency_key, "ops_idempotency_key")?;
        if self.idempotency_key.contains(['\r', '\n']) {
            return Err("ops_idempotency_key_invalid".to_owned());
        }
        self.authority.validate()?;
        validate_payload(&self.payload, "ops_command_payload_invalid")?;
        valid_digest(&self.request_digest, "ops_request_digest")?;
        if self.request_digest != self.digest() {
            return Err("ops_request_digest_mismatch".to_owned());
        }
        Ok(())
    }

    /// 拿服务端快照核验客户端的授权声明。
    ///
    /// 【作用 —— 这是本文件最关键的安全检查】
    /// 把请求里**客户端声明的** authority 与**服务端持有的**快照逐项比对。
    ///
    /// 【核心流程】
    /// 1. 先各自校验（请求和快照都要合法）；
    /// 2. 比对 `actor_id` —— 不符则 `ops_actor_mismatch`；
    /// 3. 比对 `authority_epoch` —— 不符则 `ops_authority_epoch_mismatch`；
    /// 4. 比对 `scope` —— 不符则 `ops_scope_mismatch`。
    ///
    /// 【⚠ 为什么这个方法存在，而不是让 Core 直接比对】
    /// 因为它把比对规则**固定在了协议层**。
    /// 如果让每个调用方各自比对，三处的实现很容易出现细微差异，
    /// 而其中一处漏掉某一项检查就是一个越权漏洞。
    ///
    /// 把规则收在一个函数里，所有调用方都走同一条路径。
    ///
    /// 【⚠ 三项都必须比对，缺一不可】
    /// - 只比 actor 不比 epoch：老权限的持有者能继续用过期授权；
    /// - 只比 epoch 不比 actor：搞混了是哪一代的授权；
    /// - 只比前两项不比 scope：身份对但能操作的范围被扩大了。
    ///
    /// 【失败情况】
    /// 任一不符即返回 `Err`，并带上具体是哪项不符的原因码。
    ///
    pub fn validate_authority(&self, expected: &OpsAuthoritySnapshot) -> Result<(), String> {
        self.validate()?;
        expected.validate()?;
        if self.authority.actor_id != expected.actor_id {
            return Err("ops_actor_mismatch".to_owned());
        }
        if self.authority.authority_epoch != expected.authority_epoch {
            return Err("ops_authority_epoch_mismatch".to_owned());
        }
        if self.authority.scope != expected.scope {
            return Err("ops_scope_mismatch".to_owned());
        }
        Ok(())
    }

    /// 判断这次请求是新操作还是重放。
    ///
    /// 【作用 —— 幂等性的实现核心】
    /// 解决一个具体问题：网络超时时客户端不知道请求有没有送达，
    /// 于是重发一次。如果服务端把重发当成新操作，就会执行两次 ——
    /// 比如"恢复备份"执行两次，后果难以预料。
    ///
    /// 【核心流程 —— 三态判定】
    ///
    /// 情况 1：**没有历史记录**（`prior` 是 `None`）
    /// → `New`。第一次提交。
    ///
    /// 情况 2：**三个标识全同**
    /// （`operation_id` 相同 **且** `idempotency_key` 相同 **且** `request_digest` 相同）
    /// → `Replay`。这就是同一个请求重发，返回上次的结果即可。
    ///
    /// 情况 3：**部分相同**
    /// （`operation_id` 相同 **或** `idempotency_key` 相同，但不完全一致）
    /// → `Err("ops_idempotency_conflict")`。
    ///
    /// 情况 4：**全不同**
    /// → `New`。这是一个真正的新操作。
    ///
    /// 【⚠ 情况 3 为什么必须报错，而不是当成新操作】
    /// 这是本方法最重要的判断。
    ///
    /// 设想：客户端用同一个 `operation_id` 发起了两次，但两次的 payload 不同。
    /// 如果当成新操作，第二次会**覆盖**第一次 —— 而第一次可能已经执行了。
    /// 比如第一次是"恢复到 A 备份"，第二次是"恢复到 B 备份"，
    /// 用同一个操作 ID，服务端无法判断该听哪个。
    ///
    /// 更危险的情况是：攻击者拿到一个已用的 `idempotency_key`，
    /// 配上不同的 `operation_id` 发一个新请求，试图"占用"这个键。
    /// 报错能拦住这类复用。
    ///
    /// **同一个 ID 配不同内容 = 冲突，必须人工介入。**
    ///
    /// 【⚠ 为什么情况 2 要比三个字段而不是只比一个】
    /// 只比 `operation_id` 不够 —— 同一个操作 ID 可以配不同内容（那是情况 3）。
    /// 只比 `idempotency_key` 也不够 —— 键可能被复用。
    /// 三个**同时**相同，才能确定"这就是同一个请求"。
    ///
    /// 【⚠ 校验失败时先 `self.validate()`】
    /// 即使要判断重放，本次请求自身也必须先合法。
    /// 一个格式都不过关的请求，不应该被当成"合法的新操作"或"合法的重放"。
    ///
    /// 【副作用】
    /// 无。纯判定，不修改任何状态。
    pub fn replay_against(&self, prior: Option<&Self>) -> Result<OpsReplayDisposition, String> {
        self.validate()?;
        let Some(prior) = prior else {
            return Ok(OpsReplayDisposition::New);
        };
        prior.validate()?;
        if self.operation_id == prior.operation_id
            && self.idempotency_key == prior.idempotency_key
            && self.request_digest == prior.request_digest
        {
            Ok(OpsReplayDisposition::Replay)
        } else if self.operation_id == prior.operation_id
            || self.idempotency_key == prior.idempotency_key
        {
            Err("ops_idempotency_conflict".to_owned())
        } else {
            Ok(OpsReplayDisposition::New)
        }
    }

    pub fn digest(&self) -> String {
        json_digest(&serde_json::json!({
            "schema": self.schema,
            "command": self.command,
            "operation_id": self.operation_id,
            "idempotency_key": self.idempotency_key,
            "authority": self.authority,
            "payload": self.payload,
        }))
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OpsQueryRequest {
    pub schema: String,
    pub query: String,
    pub request_id: RequestId,
    pub authority: OpsAuthoritySnapshot,
    pub cursor: Option<String>,
    pub payload: Value,
    pub query_digest: String,
}

impl OpsQueryRequest {
    pub fn new(
        query: OpsQuery,
        request_id: RequestId,
        authority: OpsAuthoritySnapshot,
        cursor: Option<String>,
        payload: Value,
    ) -> Result<Self, String> {
        let mut request = Self {
            schema: OPS_PROTOCOL_SCHEMA.to_owned(),
            query: query.wire_name().to_owned(),
            request_id,
            authority,
            cursor,
            payload,
            query_digest: String::new(),
        };
        request.query_digest = request.digest();
        request.validate()?;
        Ok(request)
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.schema != OPS_PROTOCOL_SCHEMA || self.request_id.as_uuid().is_nil() {
            return Err("ops_query_header_invalid".to_owned());
        }
        if OpsQuery::parse(&self.query).is_none() {
            return Err("ops_unknown_query".to_owned());
        }
        self.authority.validate()?;
        if let Some(cursor) = &self.cursor {
            bounded(cursor, "ops_cursor")?;
        }
        validate_payload(&self.payload, "ops_query_payload_invalid")?;
        valid_digest(&self.query_digest, "ops_query_digest")?;
        if self.query_digest != self.digest() {
            return Err("ops_query_digest_mismatch".to_owned());
        }
        Ok(())
    }

    pub fn digest(&self) -> String {
        json_digest(&serde_json::json!({
            "schema": self.schema,
            "query": self.query,
            "request_id": self.request_id,
            "authority": self.authority,
            "cursor": self.cursor,
            "payload": self.payload,
        }))
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OpsEventKind {
    Accepted,
    StateChanged,
    Completed,
    Unknown,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OpsEvent {
    pub schema: String,
    pub event: String,
    pub kind: OpsEventKind,
    pub operation_id: OperationId,
    pub source_cursor: u64,
    pub payload: Value,
    pub event_digest: String,
}

impl OpsEvent {
    pub fn new(
        event: impl Into<String>,
        kind: OpsEventKind,
        operation_id: OperationId,
        source_cursor: u64,
        payload: Value,
    ) -> Result<Self, String> {
        let mut value = Self {
            schema: OPS_PROTOCOL_SCHEMA.to_owned(),
            event: event.into(),
            kind,
            operation_id,
            source_cursor,
            payload,
            event_digest: String::new(),
        };
        value.event_digest = value.digest();
        value.validate()?;
        Ok(value)
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.schema != OPS_PROTOCOL_SCHEMA
            || self.operation_id.as_uuid().is_nil()
            || self.source_cursor == 0
        {
            return Err("ops_event_header_invalid".to_owned());
        }
        bounded(&self.event, "ops_event_name")?;
        if !OPS_EVENT_NAMES.contains(&self.event.as_str()) {
            return Err("ops_event_name_invalid".to_owned());
        }
        validate_payload(&self.payload, "ops_event_payload_invalid")?;
        valid_digest(&self.event_digest, "ops_event_digest")?;
        if self.event_digest != self.digest() {
            return Err("ops_event_digest_mismatch".to_owned());
        }
        Ok(())
    }

    pub fn digest(&self) -> String {
        json_digest(&serde_json::json!({
            "schema": self.schema,
            "event": self.event,
            "kind": self.kind,
            "operation_id": self.operation_id,
            "source_cursor": self.source_cursor,
            "payload": self.payload,
        }))
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OpsErrorCode {
    UnknownCommand,
    UnknownQuery,
    ScopeDenied,
    AuthorityMismatch,
    IdempotencyConflict,
    ResultUnknown,
    Unsupported,
    Internal,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OpsError {
    pub schema: String,
    pub code: OpsErrorCode,
    pub message: String,
    pub retryable: bool,
    pub operation_id: Option<OperationId>,
    pub error_digest: String,
}

impl OpsError {
    pub fn new(
        code: OpsErrorCode,
        message: impl Into<String>,
        retryable: bool,
        operation_id: Option<OperationId>,
    ) -> Result<Self, String> {
        let mut value = Self {
            schema: OPS_PROTOCOL_SCHEMA.to_owned(),
            code,
            message: message.into(),
            retryable,
            operation_id,
            error_digest: String::new(),
        };
        value.error_digest = value.digest();
        value.validate()?;
        Ok(value)
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.schema != OPS_PROTOCOL_SCHEMA {
            return Err("ops_error_header_invalid".to_owned());
        }
        bounded(&self.message, "ops_error_message")?;
        if self
            .operation_id
            .is_some_and(|operation_id| operation_id.as_uuid().is_nil())
        {
            return Err("ops_error_operation_invalid".to_owned());
        }
        valid_digest(&self.error_digest, "ops_error_digest")?;
        if self.error_digest != self.digest() {
            return Err("ops_error_digest_mismatch".to_owned());
        }
        Ok(())
    }

    pub fn digest(&self) -> String {
        json_digest(&serde_json::json!({
            "schema": self.schema,
            "code": self.code,
            "message": self.message,
            "retryable": self.retryable,
            "operation_id": self.operation_id,
        }))
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OpsUnknownEnvelope {
    pub schema: String,
    pub original_type: String,
    pub payload: Value,
    pub reason: String,
    pub unknown_digest: String,
}

impl OpsUnknownEnvelope {
    pub fn new(
        original_type: impl Into<String>,
        payload: Value,
        reason: impl Into<String>,
    ) -> Result<Self, String> {
        let mut value = Self {
            schema: OPS_PROTOCOL_SCHEMA.to_owned(),
            original_type: original_type.into(),
            payload,
            reason: reason.into(),
            unknown_digest: String::new(),
        };
        value.unknown_digest = value.digest();
        value.validate()?;
        Ok(value)
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.schema != OPS_PROTOCOL_SCHEMA {
            return Err("ops_unknown_header_invalid".to_owned());
        }
        bounded(&self.original_type, "ops_unknown_type")?;
        bounded(&self.reason, "ops_unknown_reason")?;
        validate_payload(&self.payload, "ops_unknown_payload_invalid")?;
        valid_digest(&self.unknown_digest, "ops_unknown_digest")?;
        if self.unknown_digest != self.digest() {
            return Err("ops_unknown_digest_mismatch".to_owned());
        }
        Ok(())
    }

    pub fn digest(&self) -> String {
        json_digest(&serde_json::json!({
            "schema": self.schema,
            "original_type": self.original_type,
            "payload": self.payload,
            "reason": self.reason,
        }))
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "body", rename_all = "snake_case")]
pub enum OpsEnvelopeBody {
    Command(OpsCommandRequest),
    Query(OpsQueryRequest),
    Event(OpsEvent),
    Error(OpsError),
    Unknown(OpsUnknownEnvelope),
}

impl OpsEnvelopeBody {
    fn validate(&self) -> Result<(), String> {
        match self {
            Self::Command(value) => value.validate(),
            Self::Query(value) => value.validate(),
            Self::Event(value) => value.validate(),
            Self::Error(value) => value.validate(),
            Self::Unknown(value) => value.validate(),
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OpsEnvelope {
    pub schema: String,
    pub version: SchemaVersion,
    pub request_id: RequestId,
    pub body: OpsEnvelopeBody,
    pub envelope_digest: String,
}

impl OpsEnvelope {
    pub fn new(request_id: RequestId, body: OpsEnvelopeBody) -> Result<Self, String> {
        let mut envelope = Self {
            schema: OPS_ENVELOPE_SCHEMA.to_owned(),
            version: OPS_PROTOCOL_VERSION,
            request_id,
            body,
            envelope_digest: String::new(),
        };
        envelope.envelope_digest = envelope.digest();
        envelope.validate()?;
        Ok(envelope)
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.schema != OPS_ENVELOPE_SCHEMA
            || self.version != OPS_PROTOCOL_VERSION
            || self.request_id.as_uuid().is_nil()
        {
            return Err("ops_envelope_header_invalid".to_owned());
        }
        self.body.validate()?;
        valid_digest(&self.envelope_digest, "ops_envelope_digest")?;
        if self.envelope_digest != self.digest() {
            return Err("ops_envelope_digest_mismatch".to_owned());
        }
        Ok(())
    }

    pub fn digest(&self) -> String {
        json_digest(&serde_json::json!({
            "schema": self.schema,
            "version": self.version,
            "request_id": self.request_id,
            "body": self.body,
        }))
    }
}

fn bounded(value: &str, field: &str) -> Result<(), String> {
    if value.trim().is_empty() || value.len() > OPS_MAX_TEXT || value.contains(['\0', '\r', '\n']) {
        return Err(format!("{field}_invalid"));
    }
    Ok(())
}

fn validate_payload(value: &Value, error: &str) -> Result<(), String> {
    if !value.is_object()
        || serde_json::to_vec(value)
            .map(|encoded| encoded.len() > OPS_MAX_PAYLOAD_BYTES)
            .unwrap_or(true)
    {
        return Err(error.to_owned());
    }
    Ok(())
}

fn valid_digest(value: &str, field: &str) -> Result<(), String> {
    let Some(hex) = value.strip_prefix("sha256:") else {
        return Err(format!("{field}_invalid"));
    };
    if hex.len() != 64 || !hex.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err(format!("{field}_invalid"));
    }
    Ok(())
}

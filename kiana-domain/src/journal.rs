//! 本地 EventStore 原子事务日志使用的领域契约。
//!
//! 【本模块在系统中的位置】
//! ```text
//! ControlPlane 组装 TransitionBatch（命令意图、期望版本、事件）
//!       ↓ EventStorePort::commit_transition
//! MemoryEventLog / JsonlEventLog 调用这里的契约校验
//!       ↓ JSONL 编码 JournalFrame；内存实现更新 JournalState
//!       ↓ adapter 返回 CommitOutcome / CommandReceipt
//! ControlPlane 根据结果继续派发、返回冲突，或在 Unknown 时先查询原命令收据
//! ```
//!
//! 本文件定义共享的事务输入、提交回执、JSONL 帧格式和健康快照。它位于 `kiana-domain`，
//! 不依赖 Core 或具体存储 adapter，使内存和文件实现可按同一规则校验。真正的状态计划和
//! 版本比较在 `kiana-eventlog/src/journal_core.rs`；文件编码/重放在 `jsonl.rs`；调用入口在
//! `kiana-core`。这里的 command receipt 表示 EventStore 已记录哪个命令及哪些事件，**不是**
//! Broker 的 dispatch receipt，也不能单独证明外部副作用真实发生。
//!
//! 结构上的 `Serialize` / `Deserialize` derive（派生实现）由 serde 生成 JSON 等格式的转换代码。
//! 多个日志 DTO 使用 `deny_unknown_fields`，让当前版本遇到未声明字段时明确失败，而不是静默
//! 丢掉可能承载新语义的字段；格式升级因此需要同步更新版本/schema 和重放规则。
use crate::{json_digest, EventId, RequestId, RuntimeEvent, SchemaVersion};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::{BTreeSet, HashSet};

/// 日志头 JSON 结构的 schema 名；它与 writer_version 分开校验。
pub const JOURNAL_HEADER_SCHEMA: &str = "kiana.journal-header.v2";
/// 帧 JSON 结构的 schema 名；`v1` 是 frame payload 契约，不等于文件 writer_version。
pub const JOURNAL_FRAME_SCHEMA: &str = "kiana.transition-frame.v1";
/// EventStore 健康快照的 schema 名。
pub const EVENT_STORE_HEALTH_SCHEMA: &str = "kiana.event-store-health.v1";
/// 健康快照的独立协议版本。
pub const EVENT_STORE_HEALTH_VERSION: SchemaVersion = SchemaVersion::new(1, 0);
/// command receipt 外部表示使用的 schema 名。
pub const COMMAND_RECEIPT_SCHEMA: &str = "kiana.command-receipt.v1";
/// JSONL writer 使用的当前文件格式版本；升级时必须保留旧格式重放兼容。
pub const JOURNAL_WRITER_VERSION: u32 = 2;
/// 单条完整事务帧的编码上限，4 MiB；限制重放和提交时的单帧内存占用。
pub const MAX_JOURNAL_FRAME_BYTES: usize = 4 * 1024 * 1024;
/// 兼容单事件追加时，一条事件本体的编码上限，1 MiB。
pub const MAX_JOURNAL_EVENT_BYTES: usize = 1024 * 1024;
/// 一个原子转换最多携带的事件数；超过时整个 batch 在提交前被拒绝。
pub const MAX_TRANSITION_EVENTS: usize = 256;
/// 一个原子转换最多读取/写入的聚合版本数，防止 CAS read set 无上限增长。
pub const MAX_TRANSITION_READ_SET: usize = 1024;
/// 单次分页最多返回的逻辑事件数，避免一次读取克隆无限大的事件集合。
pub const MAX_JOURNAL_PAGE_EVENTS: usize = 4096;
/// 一个 JSONL 日志文件可接受的最大字节数，512 MiB。
pub const MAX_JOURNAL_LOG_BYTES: u64 = 512 * 1024 * 1024;
/// 一个 EventStore 最多索引的逻辑事件数；达到边界后拒绝新追加。
pub const MAX_JOURNAL_EVENTS: usize = 1_000_000;
/// 已提交逻辑事件的游标类型；0 表示日志开头，不是 JSONL 文件的字节偏移。
pub type EventCursor = u64;

/// 一个聚合（aggregate，即由同一 ID 标识的一组可版本化领域状态）的当前版本。
///
/// EventStore 用 `(aggregate_type, aggregate_id)` 找到版本，再和 batch 中的期望版本比较；
/// 不匹配时返回冲突，避免两个并发命令基于同一旧状态都成功覆盖。
#[derive(Clone, Debug, Eq, PartialEq, Ord, PartialOrd, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AggregateVersion {
    /// 聚合所属领域类型，例如 Core 约定的 authority 或 run 类型。
    pub aggregate_type: String,
    /// 该类型下聚合的稳定 ID；和 aggregate_type 一起构成版本流键。
    pub aggregate_id: String,
    /// 版本号：在 batch.read set 中是提交前预期值，在 receipt.versions 中是提交后的版本；
    /// EventStore 比较时不存在的聚合按 0 处理。
    pub version: u64,
}
impl AggregateVersion {
    /// 构造一个聚合版本值；真正的非空和长度校验由 `validate` 执行。
    pub fn new(
        aggregate_type: impl Into<String>,
        aggregate_id: impl Into<String>,
        version: u64,
    ) -> Self {
        Self {
            aggregate_type: aggregate_type.into(),
            aggregate_id: aggregate_id.into(),
            version,
        }
    }
    /// 检查聚合类型和 ID 是否非空且未超过字段长度上限。
    ///
    /// 空值无法唯一标识版本流；过长标识会放大每条 read set 和日志记录的资源占用。
    /// `String::len()` 按 UTF-8 字节计数，因此这里的 128 和 4096 是字节上限，不是字符数。
    /// 失败时返回稳定的 `journal_aggregate_invalid` 原因，供上层转换成结构化存储错误。
    pub fn validate(&self) -> Result<(), &'static str> {
        if self.aggregate_type.trim().is_empty()
            || self.aggregate_type.len() > 128
            || self.aggregate_id.trim().is_empty()
            || self.aggregate_id.len() > 4096
        {
            return Err("journal_aggregate_invalid");
        }
        Ok(())
    }
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
/// 一次 EventStore 原子提交所需的命令身份、CAS 预期版本和事件集合。
///
/// Core 在状态转换已计算、准备提交事件时构造它；EventStore 再以 `expected_versions` 检查
/// 每个相关聚合仍处于预期版本。`events` 中每个写入聚合必须也出现在这个 read set，并带有
/// 连续的 `stream_version`。JSONL adapter 会把事件和回执编码到同一 `JournalFrame`；
/// 内存 adapter 则在锁保护下整体更新索引。两者都必须遵守“整批提交或整批拒绝”的端口语义。
pub struct TransitionBatch {
    /// 本次逻辑命令的稳定身份；EventStore 以它查重并返回原提交回执。
    pub command_id: RequestId,
    /// 对不可变命令意图的摘要，用于识别同一命令重放；不是重新计算的授权决定摘要。
    pub command_digest: String,
    /// 提交前必须核对的聚合版本集合；包含本次写入对象及其权限/状态依赖，每个聚合只能出现一次。
    pub expected_versions: Vec<AggregateVersion>,
    /// 本次命令要一起提交的领域事件；不能拆成若干独立 append，否则部分成功会留下半次转换。
    pub events: Vec<RuntimeEvent>,
}
impl TransitionBatch {
    /// 验证 command_id 和 command_digest 的格式，不执行版本比较或写盘。
    ///
    /// EventStore 的规划器先使用此轻量检查识别已提交命令的重放，再由 `validate` 检查整批
    /// 事件；frame 反序列化时也会再次校验。摘要必须是 SHA-256 的 64 位十六进制本体，允许
    /// 使用带 `sha256:` 前缀或裸十六进制表示。
    /// 【为什么 `sha256:` 前缀可有可无】
    ///
    /// 本仓库对「digest 长什么样」并不统一，而 journal 是两种写法的交汇点：
    ///
    /// - `kiana-domain::json_digest` 返回 `sha256:<64 hex>`。`kiana-core` 的
    ///   `authority.rs` / `lifecycle.rs` / `model_budget.rs` 都用它构造 `command_digest`。
    /// - `kiana-core` 的 `connector_quota.rs` 与 `ui_actions.rs` 则先经过本地的
    ///   `journal_digest()`——它做的正是 `value.strip_prefix("sha256:").unwrap_or(value)`，
    ///   即**主动剥掉前缀**，把裸 64 位 hex 交给 journal。
    ///
    /// 只接受其中一种格式会打断使用另一格式的生产者，所以校验兼容两种既有表示。
    ///
    /// 因此这里接受两种写法，但**强度没有任何放宽**：无论带不带前缀，摘要本体都必须
    /// 恰好 64 位且全为 hex，否则一律 `journal_command_digest_invalid`。与 domain 内
    /// `adapter_result` / `approval_journal` / `artifact_contracts` 等使用相同前缀处理的契约
    /// 相比，这里的兼容点是同时接受前缀和无前缀形式；摘要长度和字符集要求没有改变。
    pub fn validate_identity(&self) -> Result<(), &'static str> {
        if self.command_id.as_uuid().is_nil() {
            return Err("journal_command_id_invalid");
        }
        let hex = self
            .command_digest
            .strip_prefix("sha256:")
            .unwrap_or(&self.command_digest);
        if hex.len() != 64 || !hex.bytes().all(|byte| byte.is_ascii_hexdigit()) {
            return Err("journal_command_digest_invalid");
        }
        Ok(())
    }
    /// 验证整批事件在结构、身份、read set 和体积上的静态约束。
    ///
    /// 【调用者】EventStore 规划转换时调用；frame 构造和重放校验会沿用这套约束。
    /// 【检查内容】事件数/read set 上限、聚合唯一性、事件 ID 和 RequestId、身份链接、写入
    /// 聚合是否已在 read set 中、stream_version 是否非零、幂等键格式以及序列化后的大小。
    /// 【失败情况】返回 `journal_*` 错误码；EventStore 不会把不合格 batch 写入日志。
    ///
    /// 这里不查询“当前实际版本”也不检查版本是否连续；那需要共享状态，交给
    /// `JournalState::plan_transition` 在锁保护下完成。
    pub fn validate(&self) -> Result<(), &'static str> {
        self.validate_identity()?;
        if self.events.is_empty() || self.events.len() > MAX_TRANSITION_EVENTS {
            return Err("journal_batch_event_limit");
        }
        if self.expected_versions.is_empty()
            || self.expected_versions.len() > MAX_TRANSITION_READ_SET
        {
            return Err("journal_read_set_limit");
        }
        let mut seen = BTreeSet::new();
        for version in &self.expected_versions {
            version.validate()?;
            if !seen.insert((&version.aggregate_type, &version.aggregate_id)) {
                return Err("journal_read_set_duplicate");
            }
        }
        for event in &self.events {
            // sequence=0 表示没有有效顺序；kind 必须非空且最多 256 个 UTF-8 字节，避免将
            // 无界文本放进索引键和持久日志。后续失败仍使用稳定错误码，不让无效事件入账。
            if event.sequence == 0 || event.kind.trim().is_empty() || event.kind.len() > 256 {
                return Err("journal_event_invalid");
            }
            if event.event_id.as_uuid().is_nil() || event.request_id.as_uuid().is_nil() {
                return Err("journal_event_identity_invalid");
            }
            event
                .validate_identity_links()
                .map_err(|_| "journal_event_identity_invalid")?;
            let Some(kind) = event.aggregate_type.as_ref() else {
                return Err("journal_event_aggregate_required");
            };
            let Some(id) = event.aggregate_id.as_ref() else {
                return Err("journal_event_aggregate_required");
            };
            // 写事件的聚合必须先列入 expected_versions 才能参与同一 CAS 检查；缺失版本或 0
            // 不是有效的已分配流版本。真实的“是否等于当前版本 + 1”在共享 JournalState 中检查。
            if !seen.contains(&(kind, id)) || event.stream_version.is_none_or(|v| v == 0) {
                return Err("journal_write_not_in_read_set");
            }
            if event
                .idempotency_key
                .as_ref()
                .is_some_and(|key| key.trim().is_empty() || key.len() > 4096)
            {
                // 幂等键用于识别同一事件请求的重放；空白键无稳定含义，超长键会无谓放大索引。
                // String::len() 的单位是 UTF-8 字节，4096 不是字符数。
                return Err("journal_event_idempotency_invalid");
            }
            if canonical_journal_bytes(event)
                .map_err(|_| "journal_event_encode_failed")?
                .len()
                > MAX_JOURNAL_EVENT_BYTES
            {
                return Err("journal_event_size_limit");
            }
        }
        if canonical_journal_bytes(self)
            .map_err(|_| "journal_batch_encode_failed")?
            .len()
            > MAX_JOURNAL_FRAME_BYTES
        {
            return Err("journal_frame_size_limit");
        }
        Ok(())
    }
}
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
/// EventStore 对某个 `TransitionBatch` 提交结果的可验证回执。
///
/// 它把命令身份、连续事件游标、事件 ID 顺序和提交后聚合版本绑在一起，供重放、查询和
/// Core 对未知提交进行核对。它只证明日志契约中的提交事实，不代表 dispatch 已发生。
pub struct CommandReceipt {
    /// 与原 TransitionBatch 一致的命令 ID。
    pub command_id: RequestId,
    /// 原始不可变命令意图摘要，用于区分同 ID 的重放和身份冲突。
    pub command_digest: String,
    /// 此次 EventStore 提交记录的稳定 ID；它不是某个执行器或 Broker 的 dispatch ID。
    pub commit_id: EventId,
    /// 这批事件占用的第一个一基逻辑事件游标。
    pub first_cursor: EventCursor,
    /// 这批事件占用的最后一个逻辑事件游标，也是提交后日志游标。
    pub cursor: EventCursor,
    /// 与 batch.events 顺序对应的事件 ID 列表。
    pub event_ids: Vec<EventId>,
    /// 本次提交后的版本快照，包含未变化但作为读取依赖检查过的聚合。
    pub versions: Vec<AggregateVersion>,
}

impl CommandReceipt {
    /// 检查回执是否对应给定 batch 的命令、事件和提交游标。
    ///
    /// 该校验由重放/收据消费方调用，不会访问文件或当前聚合状态。它比较 command ID 和摘要、
    /// 连续游标区间、按顺序排列的 event ID，并确认回执没有遗漏或回退 read set 版本。失败时
    /// 返回具体的不一致原因，调用方应把回执视为无效，不能据此恢复或继续派发。
    pub fn validate_against(&self, batch: &TransitionBatch) -> Result<(), String> {
        batch.validate().map_err(|error| error.to_owned())?;
        if self.commit_id.as_uuid().is_nil() {
            return Err("command_receipt_commit_id_invalid".to_owned());
        }
        if self.command_id != batch.command_id || self.command_digest != batch.command_digest {
            return Err("command_receipt_identity_mismatch".to_owned());
        }
        if self.first_cursor == 0
            || self.cursor < self.first_cursor
            || self.cursor - self.first_cursor + 1 != batch.events.len() as u64
        {
            return Err("command_receipt_cursor_invalid".to_owned());
        }
        let expected_ids = batch
            .events
            .iter()
            .map(|event| event.event_id)
            .collect::<Vec<_>>();
        if self.event_ids != expected_ids {
            return Err("command_receipt_event_ids_mismatch".to_owned());
        }
        if self.event_ids.iter().copied().collect::<HashSet<_>>().len() != self.event_ids.len() {
            return Err("command_receipt_event_ids_duplicate".to_owned());
        }
        for expected in &batch.expected_versions {
            let Some(actual) = self.versions.iter().find(|version| {
                version.aggregate_type == expected.aggregate_type
                    && version.aggregate_id == expected.aggregate_id
            }) else {
                return Err("command_receipt_read_set_missing".to_owned());
            };
            if actual.version < expected.version {
                return Err("command_receipt_version_regressed".to_owned());
            }
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(tag = "status", rename_all = "snake_case", deny_unknown_fields)]
/// EventStore 对一次事务计划的结果分类。
///
/// `Committed` 是本次新增成功；`Replayed` 是同一 command ID/digest 已存在并返回原回执；
/// `Conflict` 表示 read set 版本变化；`Unknown` 表示当前无法确定提交是否完成。Core 遇到
/// `Unknown` 必须先用 command ID 查原回执，不能直接再次派发可能已经发生的副作用。
pub enum CommitOutcome {
    Committed {
        receipt: CommandReceipt,
    },
    Replayed {
        original: CommandReceipt,
    },
    Conflict {
        changed: Vec<AggregateVersion>,
    },
    /// 提交结果未知；在 read_command 核对原命令前，不允许据此开始 dispatch。
    Unknown {
        command_id: RequestId,
        reason: String,
    },
}
impl CommitOutcome {
    /// 仅当本次已提交或识别为历史重放时返回可用的 command receipt。
    /// 冲突和未知结果都没有足以安全继续的回执。
    pub fn receipt(&self) -> Option<&CommandReceipt> {
        match self {
            Self::Committed { receipt } => Some(receipt),
            Self::Replayed { original } => Some(original),
            _ => None,
        }
    }
}
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
/// 一个具体 EventStore adapter 声明支持的操作能力和资源上限。
///
/// Core/Daemon 可据此选择能力路径；该结构是实现声明，不是外部测试或生产环境验证回执。
pub struct EventStoreCapabilities {
    /// 是否支持把 `TransitionBatch` 作为单个逻辑事务提交。
    pub atomic_transitions: bool,
    /// adapter 是否声明提交经过其持久化屏障。
    pub durable_commits: bool,
    /// 是否支持按 command ID 查询提交回执。
    pub command_receipts: bool,
    /// 是否支持按逻辑游标读取事件页。
    pub cursor_reads: bool,
    /// adapter 当前能读取/写入的日志格式版本。
    pub writer_format_version: u32,
    /// 最大帧编码字节数。
    pub max_frame_bytes: usize,
    /// 单个事务允许提交的最大事件数。
    pub max_batch_events: usize,
}
impl Default for EventStoreCapabilities {
    fn default() -> Self {
        Self {
            atomic_transitions: false,
            durable_commits: false,
            command_receipts: false,
            cursor_reads: false,
            writer_format_version: 0,
            max_frame_bytes: 0,
            max_batch_events: 0,
        }
    }
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
/// 按逻辑事件游标读取的一页结果。
pub struct JournalPage {
    /// 本页事件；原子事务不会为了凑满 limit 而被拆成不完整的一页。
    pub events: Vec<RuntimeEvent>,
    /// 本页结束的逻辑事件游标，下一页从此位置继续。
    pub cursor: EventCursor,
    /// 日志中是否还有未返回的事件。
    pub has_more: bool,
}
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
/// EventStore 的格式化健康快照。
///
/// 它用于把 adapter 自报的健康状态、格式和已同步游标交给上层；快照摘要可检测字段内容被
/// 意外改动，但不是带密钥的签名，也不能取代 `CURRENT_STATUS.md` 中系统级的验证证据。
pub struct EventStoreHealth {
    /// 此快照的协议 schema 名称。
    pub schema: String,
    /// 快照 schema 版本，用于调用方判断能否兼容解析。
    pub version: SchemaVersion,
    /// 底层 writer 当前使用的日志格式版本。
    pub writer_version: u32,
    /// adapter 是否报告当前存储操作可用；不等于完整产品路径已验证。
    pub healthy: bool,
    /// adapter 对自身持久化屏障的报告；不能直接等同于整个 Kiana 功能已获 durable 证明。
    pub durable: bool,
    /// adapter 报告已经同步确认的逻辑事件游标。
    pub last_durable_cursor: EventCursor,
    /// adapter 是否已关闭；关闭后新的读写请求会被拒绝。
    pub closed: bool,
    /// 对上述健康字段的摘要，便于发现传输或保存过程中的意外改动。
    pub health_digest: String,
}
impl EventStoreHealth {
    /// 创建带 schema/version 和字段摘要的健康快照。
    ///
    /// 参数值来自具体 adapter；本函数只组装对象并计算摘要，不执行 flush 或探测磁盘。
    pub fn new(
        writer_version: u32,
        healthy: bool,
        durable: bool,
        last_durable_cursor: EventCursor,
        closed: bool,
    ) -> Self {
        let mut health = Self {
            schema: EVENT_STORE_HEALTH_SCHEMA.to_owned(),
            version: EVENT_STORE_HEALTH_VERSION,
            writer_version,
            healthy,
            durable,
            last_durable_cursor,
            closed,
            health_digest: String::new(),
        };
        health.health_digest = health.digest();
        health
    }

    /// 验证快照 schema、兼容版本及字段摘要。
    ///
    /// 健康摘要采用带 `sha256:` 前缀的 `json_digest` 格式；这和 journal frame 的裸十六进制
    /// body_sha256 是两套不同字段契约。格式或摘要不匹配时拒绝该快照。
    pub fn validate(&self) -> Result<(), String> {
        // 健康快照使用 json_digest 的带前缀格式；帧校验和仍使用独立的裸十六进制契约。
        let health_hex = self.health_digest.strip_prefix("sha256:");
        if self.schema != EVENT_STORE_HEALTH_SCHEMA
            || !self.version.is_compatible_with(&EVENT_STORE_HEALTH_VERSION)
            || !health_hex.is_some_and(valid_journal_digest)
        {
            return Err("event_store_health_header_invalid".to_owned());
        }
        if self.health_digest != self.digest() {
            return Err("event_store_health_digest_mismatch".to_owned());
        }
        Ok(())
    }

    /// 对 schema、版本、writer 状态、健康/持久标志、游标和关闭标志求摘要。
    /// `health_digest` 本身不放进输入，避免摘要递归依赖自身。
    pub fn digest(&self) -> String {
        json_digest(&serde_json::json!({
            "schema": self.schema,
            "version": self.version,
            "writer_version": self.writer_version,
            "healthy": self.healthy,
            "durable": self.durable,
            "last_durable_cursor": self.last_durable_cursor,
            "closed": self.closed,
        }))
    }
}
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct JournalHeader {
    /// 固定格式标识；不认识的 schema 必须拒绝重放。
    pub schema: String,
    /// 必需格式标记；为 false 时当前 writer 不会把该 header 当作有效 v2 头。
    pub required: bool,
    /// 写入器版本，必须与当前支持的 v2 完全匹配。
    pub writer_version: u32,
}
impl Default for JournalHeader {
    fn default() -> Self {
        Self {
            schema: JOURNAL_HEADER_SCHEMA.into(),
            required: true,
            writer_version: JOURNAL_WRITER_VERSION,
        }
    }
}
impl JournalHeader {
    /// 检查日志头 schema、必需标志和 writer 版本。
    ///
    /// 在 EventStore 开始重放后，若 header 声明了当前实现不认识的格式，就拒绝打开日志，
    /// 避免把新格式静默解释成旧事件。
    pub fn validate(&self) -> Result<(), String> {
        if self.schema != JOURNAL_HEADER_SCHEMA || !self.required {
            return Err("journal_header_invalid".to_owned());
        }
        if self.writer_version != JOURNAL_WRITER_VERSION {
            return Err("journal_writer_version_unsupported".to_owned());
        }
        Ok(())
    }
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum JournalFramePayload {
    /// 一条原子事务：原命令输入与对应的提交回执必须在同一帧中一致。
    Transition {
        batch: TransitionBatch,
        receipt: CommandReceipt,
    },
    /// 兼容的单事件追加仍保留旧事件/CAS 语义，但在升级后的日志中完整放入一个 v2 帧。
    Event { event: RuntimeEvent },
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
/// JSONL 中一条 v2 记录的领域表示。
///
/// `body_len` 和 `body_sha256` 用规范化后的 body 字节计算，帮助重放时发现截断、意外修改或
/// 编码差异。SHA-256 未使用密钥，因此它提供完整性校验信息，不是抵抗有意重写日志的签名。
pub struct JournalFrame {
    /// 帧 schema，和 header schema 分别描述记录类型与文件 writer 版本。
    pub schema: String,
    /// 当前实现要求必须理解此帧；必需帧若无法校验就不能跳过。
    pub required: bool,
    /// writer 格式版本。
    pub writer_version: u32,
    /// 规范化 body 的字节长度，而非包含外层字段的整帧长度。
    pub body_len: u64,
    /// 规范化 body 的 SHA-256 十六进制摘要。
    pub body_sha256: String,
    /// 事务 batch+receipt 或兼容单事件内容。
    pub body: JournalFramePayload,
}
impl JournalFrame {
    /// 将 payload 包装成当前版本帧，并计算 body 长度和校验和。
    ///
    /// 调用方通常是 EventStore 的事务规划器或 JSONL encoder。序列化失败或完整外层帧超过
    /// `MAX_JOURNAL_FRAME_BYTES` 时返回错误，调用方应在写文件前停止提交。
    pub fn new(body: JournalFramePayload) -> Result<Self, String> {
        let encoded = canonical_journal_bytes(&body)?;
        let frame = Self {
            schema: JOURNAL_FRAME_SCHEMA.into(),
            required: true,
            writer_version: JOURNAL_WRITER_VERSION,
            body_len: encoded.len() as u64,
            body_sha256: journal_sha256(&encoded),
            body,
        };
        if canonical_journal_bytes(&frame)?.len() > MAX_JOURNAL_FRAME_BYTES {
            return Err("journal_frame_size_limit".into());
        }
        Ok(frame)
    }
    /// 重放前验证帧版本、body 长度、SHA-256 和载荷内部契约。
    ///
    /// 对事务载荷还会验证 batch 与 receipt 的对应关系；对单事件载荷则检查事件身份链接。
    /// 任何不匹配均返回错误，EventStore 不会把这一帧应用到当前 JournalState。
    pub fn validate(&self) -> Result<(), String> {
        if self.schema != JOURNAL_FRAME_SCHEMA
            || !self.required
            || self.writer_version != JOURNAL_WRITER_VERSION
        {
            return Err("journal_writer_version_unsupported".into());
        }
        let encoded = canonical_journal_bytes(&self.body)?;
        if self.body_len > MAX_JOURNAL_FRAME_BYTES as u64
            || !valid_journal_digest(&self.body_sha256)
            || encoded.len() as u64 != self.body_len
            || journal_sha256(&encoded) != self.body_sha256
        {
            return Err("journal_frame_integrity_failed".into());
        }
        match &self.body {
            JournalFramePayload::Transition { batch, receipt } => {
                batch.validate().map_err(|error| error.to_owned())?;
                receipt.validate_against(batch)?;
            }
            JournalFramePayload::Event { event } => {
                if event.event_id.as_uuid().is_nil()
                    || event.request_id.as_uuid().is_nil()
                    || event.sequence == 0
                    || event.kind.trim().is_empty()
                {
                    return Err("journal_frame_event_invalid".into());
                }
                event
                    .validate_identity_links()
                    .map_err(|error| format!("journal_frame_event_identity_invalid:{error}"))?;
            }
        }
        if canonical_journal_bytes(self)?.len() > MAX_JOURNAL_FRAME_BYTES {
            return Err("journal_frame_size_limit".into());
        }
        Ok(())
    }

    /// 从帧载荷提取逻辑事件副本，供重放和观察代码统一展开两类 body。
    ///
    /// 本方法本身不调用 `validate`；读取不可信字节时必须先完成帧校验，再使用返回的事件。
    pub fn logical_events(&self) -> Vec<RuntimeEvent> {
        match &self.body {
            JournalFramePayload::Transition { batch, .. } => batch.events.clone(),
            JournalFramePayload::Event { event } => vec![event.clone()],
        }
    }
}
/// 检查帧校验和是否是恰好 64 个十六进制字符的裸摘要（不含 `sha256:` 前缀）。
fn valid_journal_digest(value: &str) -> bool {
    value.len() == 64 && value.bytes().all(|byte| byte.is_ascii_hexdigit())
}
/// 对给定原始字节计算小写 SHA-256 十六进制文本；此 journal 字段不添加算法前缀。
pub fn journal_sha256(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}
/// 将可序列化对象转为确定性的 JSON 字节表示。
///
/// 【数据流】DTO → serde_json::Value → 递归排序所有对象键（数组顺序保持不变）→ UTF-8 JSON 字节。
/// `JournalFrame::new/validate` 用同一表示计算长度与 SHA-256，因此字段顺序不会因 map 类型或
/// serde_json feature 配置变化而改变摘要。
///
/// 为控制极端嵌套输入的递归和内存成本，最大嵌套深度是 128 层；超过限制或无法序列化时返回
/// 错误。此规范编码描述 JSON 值，不是原始输入文本的逐字节签名。
pub fn canonical_journal_bytes(value: &impl Serialize) -> Result<Vec<u8>, String> {
    fn sorted(value: serde_json::Value, depth: usize) -> Result<serde_json::Value, String> {
        if depth > 128 {
            return Err("journal_payload_depth_limit".into());
        }
        Ok(match value {
            serde_json::Value::Object(fields) => {
                let mut ordered: Vec<_> = fields.into_iter().collect();
                ordered.sort_by(|a, b| a.0.cmp(&b.0));
                let mut result = serde_json::Map::new();
                for (key, value) in ordered {
                    result.insert(key, sorted(value, depth + 1)?);
                }
                serde_json::Value::Object(result)
            }
            serde_json::Value::Array(values) => serde_json::Value::Array(
                values
                    .into_iter()
                    .map(|v| sorted(v, depth + 1))
                    .collect::<Result<_, _>>()?,
            ),
            other => other,
        })
    }
    let value = serde_json::to_value(value).map_err(|e| format!("journal_encode_failed:{e}"))?;
    serde_json::to_vec(&sorted(value, 0)?).map_err(|e| format!("journal_encode_failed:{e}"))
}

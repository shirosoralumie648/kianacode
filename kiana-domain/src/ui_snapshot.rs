//! UI-05 的只读 UI 快照投影与分页合同。
//!
//! DaemonHost 从账本适配器取得一次有序事件切片后调用 [`project_ui_snapshot()`]。本文件按查询里的
//! owner/session 过滤、检查源游标/投影游标/保留下界、把适合 UI 展示的事件折叠为条目，再稳定排序、
//! 分页并生成快照摘要：
//!
//! ```text
//! 调用方提供 UiSnapshotQuery
//!     -> DaemonHost 单次读取 persisted_events
//!     -> 本文件验证 cursor / generation / retention / owner-session 边界
//!     -> RuntimeEvent 识别与去重 -> 稳定排序 -> page + next_page + digest
//!     -> DaemonHost 把结构化错误映射为 PortError，或返回 UiSnapshotPage
//! ```
//!
//! 投影只读取传入的数据，不读取 transcript、文件系统投影或 stream cache，也不写回新的事实。owner 过滤
//! 是按 query.owner_id 字符串筛选事件，不等于身份认证；可信入口必须自行把 owner_id 绑定到已认证主体。
//! 页面游标 digest 是无密钥内容摘要，不是签名。当前实现只把 epoch/source cursor/generation/offset
//! 放入游标摘要，因此调用方不应把它当作已绑定用户、session、数据 epoch 或查询条件的凭证。
//!
//! 同一份事件切片还可用于两条只读读路径：
//!
//! - [`project_ui_snapshot_health()`] 报告 projector 的 source/projection/lag 状态，把「读模型落后」
//!   与「read model 进度未知」显式区分，不把它们伪装成空列表；
//! - [`rebuild_ui_snapshot_projection()`] 是 read-model rebuild 入口：重新从账本切片生成页面并附带
//!   与同一游标一致的 health，用于新进程重建与重放核对。
//!
//! 三条读路径都不写事件、不占资源租约，也不构成第二事实源：重建只读取 EventLog，任何状态判断都来自
//! 调用方给出的 cursor 标签，而不是本模块自己保存的进度。

use crate::{json_digest, RuntimeEvent};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::{BTreeMap, BTreeSet};

pub const UI_SNAPSHOT_PROJECTOR_SCHEMA: &str = "kiana.ui-snapshot-projector.v1";
pub const UI_SNAPSHOT_PAGE_SCHEMA: &str = "kiana.ui-snapshot-page.v1";
pub const UI_SNAPSHOT_CURSOR_SCHEMA: &str = "kiana.ui-snapshot-cursor.v1";
pub const UI_SNAPSHOT_PROJECTOR_HEALTH_SCHEMA: &str = "kiana.ui-snapshot-projector-health.v1";
pub const UI_SNAPSHOT_REBUILD_SCHEMA: &str = "kiana.ui-snapshot-rebuild.v1";
/// 页面 limitation：本页只是完整 owner/session 结果的一个切片，调用方必须继续跟随 `next_page`。
pub const UI_SNAPSHOT_LIMITATION_PARTIAL_PAGE: &str = "ui_snapshot_page_partial";
/// 页面/health limitation：retention floor 之前的历史可能已被清理，不能当作完整历史展示。
pub const UI_SNAPSHOT_LIMITATION_RETENTION_FLOOR: &str = "ui_snapshot_retention_floor_applied";
/// health limitation：read model 明确落后于 source cursor；此时禁止渲染为「已是最新」。
pub const UI_SNAPSHOT_LIMITATION_PROJECTION_LAGGING: &str = "ui_snapshot_projection_lagging";
/// health limitation：read model 的处理进度未知；它与「落后」不同，必须单独可见。
pub const UI_SNAPSHOT_LIMITATION_PROJECTION_UNKNOWN: &str = "ui_snapshot_projection_unknown";
/// 单页允许返回的最大条目数，用来约束一次响应体的规模。
pub const UI_SNAPSHOT_MAX_PAGE_SIZE: usize = 256;
/// 一次完整投影最多保留的去重条目数；超过后拒绝整次查询，不返回截断且看似完整的首页。
pub const UI_SNAPSHOT_MAX_ENTRIES: usize = 4_096;

/// 检查必填文本非空、不含 NUL 且不超过给定 UTF-8 字节上限。
///
/// 【调用者】查询、游标、条目、页面和 limitation 的 validator。
/// 【输入/输出】文本、用于错误码的字段名和字节数上限；失败返回 `<field>_invalid`。
/// 【边界】只检查字符串形状，不验证 ID 是否存在或属于当前身份。
fn required(value: &str, field: &str, max: usize) -> Result<(), String> {
    if value.trim().is_empty() || value.len() > max || value.contains('\0') {
        return Err(format!("{field}_invalid"));
    }
    Ok(())
}

/// 检查摘要文本外形为 `sha256:` 前缀加 64 位十六进制字符。
///
/// 【调用者】游标和页面 validator。
/// 【失败】格式不合要求时返回 `<field>_invalid`。
/// 【重要边界】不读取原始 payload、不重算摘要，也不验证摘要签发者；内容一致性由各 DTO 另行比较。
fn digest(value: &str, field: &str) -> Result<(), String> {
    let Some(hex) = value.strip_prefix("sha256:") else {
        return Err(format!("{field}_invalid"));
    };
    if hex.len() != 64 || !hex.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err(format!("{field}_invalid"));
    }
    Ok(())
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum UiSnapshotEntryKind {
    /// 会话摘要。
    Session,
    /// Run 摘要。
    Run,
    /// UI action 的最新已识别状态。
    Action,
    /// Artifact 引用摘要，不包含 Artifact 正文。
    Artifact,
    /// Receipt 引用摘要。
    Receipt,
}

impl UiSnapshotEntryKind {
    /// 为快照排序指定固定的类别优先级：Session → Run → Action → Artifact → Receipt。
    /// 这是展示顺序，不是事件发生顺序或权限优先级。
    const fn rank(self) -> u8 {
        match self {
            Self::Session => 0,
            Self::Run => 1,
            Self::Action => 2,
            Self::Artifact => 3,
            Self::Receipt => 4,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct UiSnapshotEntry {
    /// projector 从事件内容识别出的展示类别。
    pub kind: UiSnapshotEntryKind,
    /// 该类别内的实体 ID；来源字段随类别而异。
    pub id: String,
    /// 事件里识别到的 owner 标签；页面校验要求与页面 owner 一致。
    pub owner_id: String,
    /// 优先取 stream version，其次事件 sequence；用于描述该实体的修订位置。
    pub revision: u64,
    /// 该条目在当前输入事件切片中的一基位置，不等同于 `RuntimeEvent.sequence`。
    pub source_cursor: u64,
    /// 从事件载荷复制出的 JSON 数据；本 projector 不在这里重新脱敏或校验业务 schema。
    pub payload: Value,
}

impl UiSnapshotEntry {
    /// 验证条目标识和非零位置字段。
    ///
    /// 【调用者】`UiSnapshotPage::validate()`。
    /// 【失败】缺少 ID/owner、文本过长或 revision/source cursor 为 0 时返回稳定字段错误。
    /// 【边界】不查询对应实体，也不验证 payload 与 kind/id 的关系；owner 是否经过认证由上游负责。
    fn validate(&self) -> Result<(), String> {
        required(&self.id, "ui_snapshot_entry_id", 512)?;
        required(&self.owner_id, "ui_snapshot_entry_owner", 256)?;
        if self.revision == 0 || self.source_cursor == 0 {
            return Err("ui_snapshot_entry_revision_invalid".to_owned());
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct UiSnapshotPageCursor {
    /// 游标合同 schema。
    pub schema: String,
    /// authority/instance epoch 标签；epoch 改变时旧分页位置应失效。
    pub epoch: String,
    /// 此页所基于的完整事件切片游标。
    pub source_cursor: u64,
    /// 排序后去重列表中的下一个起始位置，而不是页码。
    pub offset: usize,
    /// 当前 DTO 用它携带 projection generation。
    pub revision: u64,
    /// 游标前五项字段的无密钥 SHA-256 自摘要。
    pub cursor_digest: String,
}

impl UiSnapshotPageCursor {
    /// 创建并立即校验一个 next-page 游标。
    ///
    /// 【调用者】`project_ui_snapshot()` 在当前页之后仍有条目时调用。
    /// 【输入】epoch、完整 source cursor、下页 offset 和 projection generation（字段名为 revision）。
    /// 【输出】digest 已计算并通过 validator 的游标；不会访问账本或保存状态。
    /// 【容易误改】摘要不含 owner、session、page_size、data_epoch 或 retention_floor，也没有密钥；
    /// 不能将它当作不可伪造的身份凭证。
    pub fn new(
        epoch: impl Into<String>,
        source_cursor: u64,
        offset: usize,
        revision: u64,
    ) -> Result<Self, String> {
        let mut cursor = Self {
            schema: UI_SNAPSHOT_CURSOR_SCHEMA.to_owned(),
            epoch: epoch.into(),
            source_cursor,
            offset,
            revision,
            cursor_digest: String::new(),
        };
        cursor.cursor_digest = cursor.digest();
        cursor.validate()?;
        Ok(cursor)
    }

    /// 校验后把 cursor 序列化为 JSON 字符串，供下一次 Query 的 `after` 字段使用。
    /// 【失败】cursor 不合法或 JSON 序列化失败时返回稳定错误；输出大小由调用者 decode 时限制。
    pub fn encode(&self) -> Result<String, String> {
        self.validate()?;
        serde_json::to_string(self).map_err(|_| "ui_snapshot_cursor_encode_failed".to_owned())
    }

    /// 将 Query 的 `after` 字符串解码并校验为分页 cursor。
    /// 【失败】编码超过 2,048 字节、JSON 格式无效或内部合同不匹配时拒绝，不把坏 cursor 当作首页。
    pub fn decode(value: &str) -> Result<Self, String> {
        if value.len() > 2_048 {
            return Err("ui_snapshot_cursor_too_large".to_owned());
        }
        let cursor: Self = serde_json::from_str(value)
            .map_err(|_| "ui_snapshot_cursor_decode_failed".to_owned())?;
        cursor.validate()?;
        Ok(cursor)
    }

    /// 检查游标 schema、revision/offset 范围、epoch 以及自摘要。
    ///
    /// 【调用者】构造、编码、解码游标时；projector 还会把有效游标与当前 query 的部分字段比较。
    /// 【失败】返回 cursor header、epoch、digest 或 digest mismatch 错误。
    /// 【边界】offset 不超过 4,096；这里不要求 source_cursor 非零，也不认证自摘要，实际 offset 是否
    /// 落在条目范围内要等 projector 根据完整结果列表再次检查。
    pub fn validate(&self) -> Result<(), String> {
        if self.schema != UI_SNAPSHOT_CURSOR_SCHEMA
            || self.revision == 0
            || self.offset > UI_SNAPSHOT_MAX_ENTRIES
        {
            return Err("ui_snapshot_cursor_invalid".to_owned());
        }
        required(&self.epoch, "ui_snapshot_cursor_epoch", 256)?;
        digest(&self.cursor_digest, "ui_snapshot_cursor_digest")?;
        if self.cursor_digest != self.digest() {
            return Err("ui_snapshot_cursor_digest_mismatch".to_owned());
        }
        Ok(())
    }

    /// 只对 schema、epoch、source cursor、offset 和 revision 生成内容摘要。
    fn digest(&self) -> String {
        json_digest(&json!({
            "schema": self.schema,
            "epoch": self.epoch,
            "source_cursor": self.source_cursor,
            "offset": self.offset,
            "revision": self.revision,
        }))
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct UiSnapshotQuery {
    /// 查询合同 schema。
    pub schema: String,
    /// 要筛选的 owner 文本；必须由可信调用边界绑定到已认证主体。
    pub owner_id: String,
    /// 可选 session 过滤条件；为空表示查询该 owner 的所有可识别 session 条目。
    #[serde(default)]
    pub session_id: Option<String>,
    /// 当前权威/实例 epoch 标签，分页期间必须保持一致。
    pub epoch: String,
    /// 调用方声明这次传入事件切片末端的位置，必须等于切片长度。
    pub source_cursor: u64,
    /// read model 已处理到的位置；缺失代表未知，落后代表 projection lag。
    #[serde(default)]
    pub projection_cursor: Option<u64>,
    /// projector 版本代数，旧 cursor 不能跨代继续用。
    pub projection_generation: u64,
    /// 数据治理 epoch 标签，会进入结果页，但当前没有放入 page cursor。
    pub data_epoch: u64,
    /// 已清理到的最早边界；其前仍 pending 的 action 会阻止 snapshot 返回。
    pub retention_floor: u64,
    /// 当前页最多返回多少条，范围为 1 到 256。
    pub page_size: usize,
    /// 可选的 JSON 编码 UiSnapshotPageCursor。
    #[serde(default)]
    pub after: Option<String>,
    /// 调用方提供的 Unix 毫秒时间标签；当前 projector 只校验非零，不读取系统时钟对账。
    pub generated_at_unix_ms: u64,
}

impl UiSnapshotQuery {
    /// 创建并校验 projector query。
    ///
    /// 【调用者】外层 Daemon/API adapter 应从认证上下文和同一 read snapshot 准备这些字段；仓内没有
    /// 发现 `ui_snapshot_page()` 之外的生产构造路径。
    /// 【输出】字段完整的 Query；失败返回 header、owner/session、projection cursor 或 after 错误。
    /// 【设计原因】把查询条件固定在一个 DTO 中，让 projector 对游标、generation、数据 epoch 与保留边界
    /// 做一致性检查，而不是在多个 UI 请求参数中散落判断。
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        owner_id: impl Into<String>,
        session_id: Option<String>,
        epoch: impl Into<String>,
        source_cursor: u64,
        projection_cursor: Option<u64>,
        projection_generation: u64,
        data_epoch: u64,
        retention_floor: u64,
        page_size: usize,
        after: Option<String>,
        generated_at_unix_ms: u64,
    ) -> Result<Self, String> {
        let query = Self {
            schema: UI_SNAPSHOT_PROJECTOR_SCHEMA.to_owned(),
            owner_id: owner_id.into(),
            session_id,
            epoch: epoch.into(),
            source_cursor,
            projection_cursor,
            projection_generation,
            data_epoch,
            retention_floor,
            page_size,
            after,
            generated_at_unix_ms,
        };
        query.validate()?;
        Ok(query)
    }

    /// 检查 Query 的 schema、范围和必填标签。
    ///
    /// 【调用者】`new()` 与 `project_ui_snapshot()`。
    /// 【关键拒绝】generation/data epoch/时间为 0，retention 越过 source，page size 不在 1..=256，
    /// projection cursor 超前或 after 是空字符串都会失败；projection cursor 落后留给 projector 返回
    /// 专门的 lag 错误，缺失也由 projector 区分为 unknown。
    /// 【边界】允许 projection cursor 暂时落后；这里不认证 owner，也不校验非空 after 的长度/JSON，
    /// 那一步由游标 decode 完成。
    pub fn validate(&self) -> Result<(), String> {
        if self.schema != UI_SNAPSHOT_PROJECTOR_SCHEMA
            || self.projection_generation == 0
            || self.data_epoch == 0
            || self.retention_floor > self.source_cursor
            || self.page_size == 0
            || self.page_size > UI_SNAPSHOT_MAX_PAGE_SIZE
            || self.generated_at_unix_ms == 0
        {
            return Err("ui_snapshot_query_header_invalid".to_owned());
        }
        required(&self.owner_id, "ui_snapshot_query_owner", 256)?;
        required(&self.epoch, "ui_snapshot_query_epoch", 256)?;
        if let Some(session_id) = &self.session_id {
            required(session_id, "ui_snapshot_query_session", 256)?;
        }
        if self
            .projection_cursor
            .is_some_and(|cursor| cursor > self.source_cursor)
        {
            return Err("ui_snapshot_projection_cursor_ahead".to_owned());
        }
        if self.after.as_deref().is_some_and(str::is_empty) {
            return Err("ui_snapshot_after_cursor_invalid".to_owned());
        }
        Ok(())
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct UiSnapshotPage {
    /// page wire schema。
    pub schema: String,
    /// 创建本页时采用的 epoch。
    pub epoch: String,
    /// 本页代表的完整切片位置；当前合同要求它等于 source_cursor。
    pub snapshot_cursor: u64,
    /// 当前 query 的 owner 标签，所有条目都必须一致。
    pub owner_id: String,
    /// 生成此页时传入事件切片的末端位置。
    pub source_cursor: u64,
    /// read model 的处理位置；返回页要求已追平 source cursor。
    pub projection_cursor: u64,
    /// snapshot projector 的 generation。
    pub projection_generation: u64,
    /// 数据治理 epoch 标签。
    pub data_epoch: u64,
    /// source 之前的 retention 清理边界。
    pub retention_floor: u64,
    /// 当前排序结果的一个切片，不超过 256 条。
    pub entries: Vec<UiSnapshotEntry>,
    /// 下一页的 JSON cursor；最后一页为空。
    #[serde(default)]
    pub next_page: Option<String>,
    /// 已知投影限制：retention 清理边界与「本页只是完整结果切片」等稳定编码，按字典序去重排列。
    #[serde(default)]
    pub limitations: Vec<String>,
    /// 对本页所有业务字段计算的无密钥自摘要。
    pub snapshot_digest: String,
}

impl UiSnapshotPage {
    /// 校验返回页的 cursor 对齐、owner 范围、条目、limitations、next cursor 和自摘要。
    ///
    /// 【调用者】`project_ui_snapshot()` 组装完页面前；接收端也可自行重验。
    /// 【失败】header/cursor 不对齐、条目 owner 不同、文本或摘要不合法时返回结构化 String 错误。
    /// 【设计原因】页面是一个固定 source cursor 下的只读投影；source 与 projection 不一致时不能把部分
    /// 新旧状态拼成看似完整的 snapshot。
    /// 【边界】校验不能回到原始事件重建条目，也不验证摘要签发者；`next_page` 只绑定 epoch/source/
    /// projection generation，未绑定 query owner/session/data epoch/page size/retention floor。
    pub fn validate(&self) -> Result<(), String> {
        if self.schema != UI_SNAPSHOT_PAGE_SCHEMA
            || self.snapshot_cursor != self.source_cursor
            || self.projection_cursor != self.source_cursor
            || self.projection_generation == 0
            || self.data_epoch == 0
            || self.retention_floor > self.source_cursor
            || self.entries.len() > UI_SNAPSHOT_MAX_PAGE_SIZE
            || self.limitations.len() > 32
        {
            return Err("ui_snapshot_page_header_invalid".to_owned());
        }
        required(&self.epoch, "ui_snapshot_page_epoch", 256)?;
        required(&self.owner_id, "ui_snapshot_page_owner", 256)?;
        for entry in &self.entries {
            entry.validate()?;
            if entry.owner_id != self.owner_id {
                return Err("ui_snapshot_page_owner_leak".to_owned());
            }
        }
        for limitation in &self.limitations {
            required(limitation, "ui_snapshot_limitation", 512)?;
        }
        digest(&self.snapshot_digest, "ui_snapshot_digest")?;
        if self.snapshot_digest != self.digest() {
            return Err("ui_snapshot_digest_mismatch".to_owned());
        }
        if let Some(next) = &self.next_page {
            let cursor = UiSnapshotPageCursor::decode(next)?;
            if cursor.epoch != self.epoch
                || cursor.source_cursor != self.source_cursor
                || cursor.revision != self.projection_generation
            {
                return Err("ui_snapshot_next_cursor_mismatch".to_owned());
            }
        }
        Ok(())
    }

    /// 对页面公开字段生成确定性摘要；`snapshot_digest` 本身不参与摘要输入。
    fn digest(&self) -> String {
        json_digest(&json!({
            "schema": self.schema,
            "epoch": self.epoch,
            "snapshot_cursor": self.snapshot_cursor,
            "owner_id": self.owner_id,
            "source_cursor": self.source_cursor,
            "projection_cursor": self.projection_cursor,
            "projection_generation": self.projection_generation,
            "data_epoch": self.data_epoch,
            "retention_floor": self.retention_floor,
            "entries": self.entries,
            "next_page": self.next_page,
            "limitations": self.limitations,
        }))
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct ProjectedItem {
    /// 从事件字段识别出的展示类别。
    kind: UiSnapshotEntryKind,
    /// 该类别下用来折叠重复事件的 ID。
    id: String,
    /// 从顶层或兼容的嵌套字段识别出的 owner 标签。
    owner_id: String,
    /// stream revision/event sequence 标签。
    revision: u64,
    /// 当前输入切片内的一基位置。
    source_cursor: u64,
    /// 原事件的 JSON payload 副本。
    payload: Value,
}

/// 按兼容字段顺序从事件中提取 owner 标签。
///
/// 【调用者】session owner 冲突检测与条目识别。
/// 【优先级】顶层 `owner_id` → 顶层 `actor_id` → `action.owner_id` → `record.owner_id`；
/// 找到非字符串/缺失时继续尝试后续位置。
/// 【失败/边界】全部没有可用字符串时返回 None，事件不会生成条目；这只是事件过滤标签，不是认证身份。
fn event_owner(event: &RuntimeEvent) -> Option<&str> {
    event
        .data
        .get("owner_id")
        .and_then(Value::as_str)
        .or_else(|| event.data.get("actor_id").and_then(Value::as_str))
        .or_else(|| {
            event
                .data
                .get("action")
                .and_then(|action| action.get("owner_id"))
                .and_then(Value::as_str)
        })
        .or_else(|| {
            event
                .data
                .get("record")
                .and_then(|record| record.get("owner_id"))
                .and_then(Value::as_str)
        })
}

/// 从事件中提取 session 标签，兼容旧的顶层和嵌套事件形状。
///
/// 【调用者】请求 session 的 owner 冲突检测和 session filter。
/// 【优先级】顶层 `session_id` → 顶层 `session` → `action.session_id` → `record.session_id`。
/// 找不到字符串时返回 None；带 session filter 的普通条目会因无法匹配而被跳过。
fn event_session(event: &RuntimeEvent) -> Option<&str> {
    event
        .data
        .get("session_id")
        .and_then(Value::as_str)
        .or_else(|| event.data.get("session").and_then(Value::as_str))
        .or_else(|| {
            event
                .data
                .get("action")
                .and_then(|action| action.get("session_id"))
                .and_then(Value::as_str)
        })
        .or_else(|| {
            event
                .data
                .get("record")
                .and_then(|record| record.get("session_id"))
                .and_then(Value::as_str)
        })
}

/// 从一个事件识别可展示的 UI 实体，并保留原 payload 作为条目内容。
///
/// 【调用者】`project_ui_snapshot()` 扫描事件时。
/// 【输入】RuntimeEvent 与在本切片中的一基 source position。
/// 【输出】有 owner 且能按类别找到实体 ID 时返回 ProjectedItem；缺少必要 owner/ID 时跳过该事件。
/// 【分类优先级】`ui.action.*` → artifact ID/ref → receipt ID/digest → run_id → session_id，第一项匹配即停止。
/// 【修订值】优先用 stream_version，再用 event.sequence；当前 `sequence` 本身非 Option，因此最后的
/// source_cursor fallback 实际不会触发。
/// 【边界】不按事件种类白名单验证 payload，也不重新脱敏/校验 payload；只要 owner 与某个识别字段存在，
/// 该事件即可被投影成对应 kind。上游事实边界需确保这些字段的含义可靠。
fn event_item(event: &RuntimeEvent, source_cursor: u64) -> Option<ProjectedItem> {
    let owner_id = event_owner(event)?.to_owned();
    let (kind, id) = if event.kind.starts_with("ui.action.") {
        let id = event.aggregate_id.clone().or_else(|| {
            event
                .data
                .get("action_id")
                .and_then(Value::as_str)
                .map(str::to_owned)
        })?;
        (UiSnapshotEntryKind::Action, id)
    } else if let Some(id) = event
        .data
        .get("artifact_id")
        .and_then(Value::as_str)
        .or_else(|| event.data.get("artifact_ref").and_then(Value::as_str))
    {
        (UiSnapshotEntryKind::Artifact, id.to_owned())
    } else if let Some(id) = event
        .data
        .get("receipt_id")
        .and_then(Value::as_str)
        .or_else(|| event.data.get("receipt_digest").and_then(Value::as_str))
    {
        (UiSnapshotEntryKind::Receipt, id.to_owned())
    } else if let Some(id) = event.data.get("run_id").and_then(Value::as_str) {
        (UiSnapshotEntryKind::Run, id.to_owned())
    } else if let Some(id) = event.data.get("session_id").and_then(Value::as_str) {
        (UiSnapshotEntryKind::Session, id.to_owned())
    } else {
        return None;
    };
    Some(ProjectedItem {
        kind,
        id,
        owner_id,
        revision: event
            .stream_version
            .or(Some(event.sequence))
            .unwrap_or(source_cursor),
        source_cursor,
        payload: event.data.clone(),
    })
}

/// 读取 UI action 当前状态标签，兼容顶层 `state` 与 `record.state`。
///
/// 【调用者】判断 retention floor 之前接受、之后已终态的 action 是否仍然 pending。
/// 【失败】不是字符串或字段缺失时返回 None；这些值不会被误认作已完成状态。
fn state_string(event: &RuntimeEvent) -> Option<&str> {
    event.data.get("state").and_then(Value::as_str).or_else(|| {
        event
            .data
            .get("record")
            .and_then(|record| record.get("state"))
            .and_then(Value::as_str)
    })
}

/// 从调用方提供的一次有序事件切片构造 owner/session 范围内的分页快照。
///
/// 【调用者】当前仓内生产桥接点是 `DaemonHost::ui_snapshot_page()`：它调用 `persisted_events()` 一次，
/// 再把同一 Vec 和 query 传入本函数。全仓没有找到这个公开 facade 的下游产品调用点；Domain UI-05
/// fixture 直接调用 projector。
/// 【输入】事件 Vec 与 `UiSnapshotQuery`。调用方应确保 query 的 owner、epoch、source/projection cursor、
/// generation、data epoch 和 retention floor 来自与这次读取相符的可信上下文；本函数只做合同比较。
/// 【输出】对同一输入切片生成的 `UiSnapshotPage`，其中条目过滤后按 kind/id/revision/source position
/// 排序并按 offset/page_size 截取。
/// 【副作用】纯内存投影，不写事件、不修改游标权威状态、不读取外部快照；错误由 Daemon facade 映射成
/// `PortError::Failed`。
/// 【失败情况】query 不合法、source cursor 与切片长度不符、projection 未知/落后、after cursor 与
/// query epoch/source/generation 不同、跨 owner session、保留边界仍有 pending action、条目过量或分页
/// offset 越界时返回明确错误，不返回部分页。
/// 【为什么要求 cursor 对齐】没有完整且一致的读取位置时，UI 可能把不同时间点的 session/run/action
/// 混在一页里；这里选择拒绝，而不是把缺失投影解释成空列表。
/// 【安全边界】owner_id 只用于过滤，不在 Domain 内做主体认证；cursor 摘要也不是签名。当前 cursor
/// 只检查 epoch/source/generation，不绑定 owner/session，所以相同切片参数下复用到不同查询会改变 offset，
/// 可能造成分页遗漏。调用方还必须确保 owner 与授权主体一致。
pub fn project_ui_snapshot(
    events: &[RuntimeEvent],
    query: &UiSnapshotQuery,
) -> Result<UiSnapshotPage, String> {
    query.validate()?;
    // 此合同把完整事件切片长度当作 source cursor。它不会逐条核对 RuntimeEvent.sequence 是否连续，
    // 所以调用方必须传入完整、按事实顺序排列的快照，而不是任意子集或重新排序后的事件。
    if query.source_cursor != events.len() as u64 {
        return Err("ui_snapshot_source_cursor_mismatch".to_owned());
    }
    let projection_cursor = query
        .projection_cursor
        .ok_or_else(|| "ui_snapshot_projection_lag_unknown".to_owned())?;
    if projection_cursor < query.source_cursor {
        return Err("ui_snapshot_projection_lag".to_owned());
    }
    // Query::validate() 已提前拒绝 projection_cursor > source_cursor；保留此显式分支可让投影边界自身
    // 仍能读出 cursor-ahead 语义，但对通过 validate 的 query 当前不可达。
    if projection_cursor > query.source_cursor {
        return Err("ui_snapshot_projection_cursor_ahead".to_owned());
    }
    let after = query
        .after
        .as_deref()
        .map(UiSnapshotPageCursor::decode)
        .transpose()?;
    // 游标绑定当前 epoch、完整 source cursor 和 generation；cursor digest 不是认证签名，也不绑定 owner
    // 或 session。offset 只在下面重建当前 query 的过滤后排序列表时解释。
    let offset = if let Some(cursor) = &after {
        if cursor.epoch != query.epoch || cursor.source_cursor != query.source_cursor {
            return Err("ui_snapshot_cursor_source_mismatch".to_owned());
        }
        if cursor.revision != query.projection_generation {
            return Err("ui_snapshot_cursor_generation_mismatch".to_owned());
        }
        cursor.offset
    } else {
        0
    };

    let requested_session = query.session_id.as_deref();
    let mut session_owner_mismatch = false;
    let mut pending_before_retention = BTreeSet::new();
    let mut actions = BTreeMap::<String, ProjectedItem>::new();
    let mut items = BTreeMap::<(u8, String), ProjectedItem>::new();
    // 这里使用切片位置生成一基 cursor，而不是 event.sequence。事件遍历顺序同时决定同 ID 条目采用哪一版：
    // BTreeMap::insert 会让后遇到的同 kind/id 项覆盖先前值，所以不能在调用前重排 source events。
    for (index, event) in events.iter().enumerate() {
        let source_cursor = index as u64 + 1;
        // 若请求 session 在事件里属于另一个有明确 owner 的主体，则拒绝整页，而不是只过滤掉冲突事件；
        // 这样调用方错误指定 session 时不会得到看似有效但来源混杂的结果。
        if let Some(session_id) = requested_session {
            if event_session(event) == Some(session_id)
                && event_owner(event).is_some_and(|owner| owner != query.owner_id)
            {
                session_owner_mismatch = true;
            }
        }
        if event.kind.starts_with("ui.action.") {
            // Action 事件先按 action ID 折叠为最新可识别状态，同时单独追踪 retention floor 之前
            // 接受、但尚未观测到 applied/rejected/unknown 终态的条目。
            let Some(action_id) = event.aggregate_id.clone().or_else(|| {
                event
                    .data
                    .get("action_id")
                    .and_then(Value::as_str)
                    .map(str::to_owned)
            }) else {
                continue;
            };
            if let Some(item) = event_item(event, source_cursor) {
                if item.owner_id == query.owner_id
                    && requested_session.is_none_or(|session| event_session(event) == Some(session))
                {
                    if state_string(event) == Some("accepted")
                        && source_cursor <= query.retention_floor
                    {
                        pending_before_retention.insert(action_id.clone());
                    }
                    if matches!(
                        state_string(event),
                        Some("applied" | "rejected" | "unknown")
                    ) {
                        pending_before_retention.remove(&action_id);
                    }
                    actions.insert(action_id, item);
                }
            }
            continue;
        }
        let Some(item) = event_item(event, source_cursor) else {
            continue;
        };
        if item.owner_id != query.owner_id
            || requested_session.is_some_and(|session| event_session(event) != Some(session))
        {
            continue;
        }
        items.insert((item.kind.rank(), item.id.clone()), item);
    }
    // 拒绝优先级：先阻止跨 owner session，再阻止删除仍可能代表未完成操作的历史 action。
    if session_owner_mismatch {
        return Err("ui_snapshot_session_owner_mismatch".to_owned());
    }
    if !pending_before_retention.is_empty() {
        return Err("ui_snapshot_retention_protected_pending".to_owned());
    }
    // Action 使用单独 map 完成状态折叠后再并入普通实体集合，使同一个 action 只保留一个当前展示条目。
    for action in actions.into_values() {
        items.insert((action.kind.rank(), action.id.clone()), action);
    }
    let mut all = items.into_values().collect::<Vec<_>>();
    if all.len() > UI_SNAPSHOT_MAX_ENTRIES {
        return Err("ui_snapshot_entry_limit".to_owned());
    }
    // 固定类别、ID、实体 revision、切片位置的排序次序，确保同一输入和 query 得到同一页边界。
    all.sort_by(|left, right| {
        (
            left.kind.rank(),
            &left.id,
            left.revision,
            left.source_cursor,
        )
            .cmp(&(
                right.kind.rank(),
                &right.id,
                right.revision,
                right.source_cursor,
            ))
    });
    if offset > all.len() {
        return Err("ui_snapshot_cursor_offset_invalid".to_owned());
    }
    let end = (offset + query.page_size).min(all.len());
    // 页游标 offset 指向此处排序后的完整 owner/session 结果列表，而不是原始事件位置。
    let entries = all[offset..end]
        .iter()
        .map(|item| UiSnapshotEntry {
            kind: item.kind,
            id: item.id.clone(),
            owner_id: item.owner_id.clone(),
            revision: item.revision,
            source_cursor: item.source_cursor,
            payload: item.payload.clone(),
        })
        .collect::<Vec<_>>();
    // 只有还有条目时才产生下一游标；游标从本页 end 继续，并绑定本次 epoch/source/generation。
    let next_page = if end < all.len() {
        Some(
            UiSnapshotPageCursor::new(
                query.epoch.clone(),
                query.source_cursor,
                end,
                query.projection_generation,
            )?
            .encode()?,
        )
    } else {
        None
    };
    // limitation 由本次切片的实际边界产生，而不是常量文案：retention floor 之前的历史可能已被清理，
    // 只有 `next_page` 存在时本页才是完整结果的切片。调用方据此决定「继续翻页」还是「历史不完整」。
    let mut limitations = Vec::new();
    if query.retention_floor > 0 {
        limitations.push(UI_SNAPSHOT_LIMITATION_RETENTION_FLOOR.to_owned());
    }
    if next_page.is_some() {
        limitations.push(UI_SNAPSHOT_LIMITATION_PARTIAL_PAGE.to_owned());
    }
    limitations.sort();
    limitations.dedup();
    let mut page = UiSnapshotPage {
        schema: UI_SNAPSHOT_PAGE_SCHEMA.to_owned(),
        epoch: query.epoch.clone(),
        snapshot_cursor: query.source_cursor,
        owner_id: query.owner_id.clone(),
        source_cursor: query.source_cursor,
        projection_cursor,
        projection_generation: query.projection_generation,
        data_epoch: query.data_epoch,
        retention_floor: query.retention_floor,
        entries,
        next_page,
        limitations,
        snapshot_digest: String::new(),
    };
    // 先填充所有页面字段，再计算摘要并走公开 validator，避免返回尚未自洽的结构体。
    page.snapshot_digest = page.digest();
    page.validate()?;
    Ok(page)
}

/// projector 相对 source cursor 的处理状态。
///
/// `UpToDate` 只表示 read model 已处理到本次切片末端；`Lagging` 表示还差一段；`Unknown` 表示调用方
/// 没有给出处理位置。三者都不是权限或业务终态，且任何一项都不能被渲染成「空结果」。
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum UiSnapshotProjectorStatus {
    /// read model 已处理到 source cursor。
    UpToDate,
    /// read model 落后于 source cursor，`lag` 字段给出差值。
    Lagging,
    /// 处理位置未知；不能当作已追平。
    Unknown,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct UiSnapshotProjectorHealthQuery {
    /// health 合同 schema。
    pub schema: String,
    /// authority/instance epoch 标签。
    pub epoch: String,
    /// 调用方声明这次传入事件切片末端的位置，必须等于切片长度。
    pub source_cursor: u64,
    /// read model 已处理到的位置；缺失代表未知，落后代表 lag。
    pub projection_cursor: Option<u64>,
    /// projector 版本代数。
    pub projection_generation: u64,
    /// 数据治理 epoch 标签。
    pub data_epoch: u64,
    /// 已清理到的最早边界。
    pub retention_floor: u64,
    /// 调用方提供的观测时间标签；projector 只校验非零，不读取系统时钟。
    pub observed_at_unix_ms: u64,
}

impl UiSnapshotProjectorHealthQuery {
    /// 创建并校验 projector health 查询。
    ///
    /// 【调用者】外层 Daemon/API adapter 与 `rebuild_ui_snapshot_projection()` 的派生构造。
    /// 【输出】字段完整的 health 查询；失败返回 health header 或 cursor-ahead 错误。
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        epoch: impl Into<String>,
        source_cursor: u64,
        projection_cursor: Option<u64>,
        projection_generation: u64,
        data_epoch: u64,
        retention_floor: u64,
        observed_at_unix_ms: u64,
    ) -> Result<Self, String> {
        let query = Self {
            schema: UI_SNAPSHOT_PROJECTOR_HEALTH_SCHEMA.to_owned(),
            epoch: epoch.into(),
            source_cursor,
            projection_cursor,
            projection_generation,
            data_epoch,
            retention_floor,
            observed_at_unix_ms,
        };
        query.validate()?;
        Ok(query)
    }

    /// 检查 health query 的 schema、范围与必填标签。
    ///
    /// 【关键拒绝】generation/data epoch/时间为 0、retention 越过 source、epoch 为空、projection cursor
    /// 超前（谎报处理位置）都会失败；落后的 projection cursor 是合法输入，其结果会被标为 `Lagging`。
    /// 【边界】不认证 owner/epoch，也不读取账本；这些由调用边界负责。
    pub fn validate(&self) -> Result<(), String> {
        if self.schema != UI_SNAPSHOT_PROJECTOR_HEALTH_SCHEMA
            || self.projection_generation == 0
            || self.data_epoch == 0
            || self.retention_floor > self.source_cursor
            || self.observed_at_unix_ms == 0
        {
            return Err("ui_snapshot_health_query_header_invalid".to_owned());
        }
        required(&self.epoch, "ui_snapshot_health_query_epoch", 256)?;
        if self
            .projection_cursor
            .is_some_and(|cursor| cursor > self.source_cursor)
        {
            return Err("ui_snapshot_projection_cursor_ahead".to_owned());
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct UiSnapshotProjectorHealth {
    /// health wire schema。
    pub schema: String,
    /// 观测时的 epoch 标签。
    pub epoch: String,
    /// 处理状态：已追平 / 落后 / 未知。
    pub status: UiSnapshotProjectorStatus,
    /// 本次观测的事件切片末端位置。
    pub source_cursor: u64,
    /// read model 处理位置；`Unknown` 状态为空。
    pub projection_cursor: Option<u64>,
    /// `source_cursor - projection_cursor`；仅 `Lagging`/`UpToDate` 有值。
    pub lag: Option<u64>,
    /// 当前 projector 版本代数。
    pub projection_generation: u64,
    /// 数据治理 epoch 标签。
    pub data_epoch: u64,
    /// retention 清理边界。
    pub retention_floor: u64,
    /// 调用方给出的观测时间标签。
    pub observed_at_unix_ms: u64,
    /// 稳定 limitation 编码；未知进度与落后状态必须在这里可见。
    #[serde(default)]
    pub limitations: Vec<String>,
    /// 对本结构公开字段计算的无密钥自摘要。
    pub health_digest: String,
}

impl UiSnapshotProjectorHealth {
    /// 校验 status 与 cursor/lag 的自洽性、文本边界和自摘要。
    ///
    /// 【失败】未知字段在反序列化阶段已拒绝；这里对 `UpToDate` 要求 lag=0、对 `Lagging` 要求
    /// `lag = source - projection > 0`、对 `Unknown` 要求两个 cursor 字段都为空，并重算摘要。
    /// 【边界】健康快照只描述 projector 的读进度，不证明事件事实正确，也不授权任何动作。
    pub fn validate(&self) -> Result<(), String> {
        if self.schema != UI_SNAPSHOT_PROJECTOR_HEALTH_SCHEMA
            || self.projection_generation == 0
            || self.data_epoch == 0
            || self.retention_floor > self.source_cursor
            || self.observed_at_unix_ms == 0
            || self.limitations.len() > 32
        {
            return Err("ui_snapshot_health_header_invalid".to_owned());
        }
        required(&self.epoch, "ui_snapshot_health_epoch", 256)?;
        match self.status {
            UiSnapshotProjectorStatus::UpToDate => {
                if self.projection_cursor != Some(self.source_cursor) || self.lag != Some(0) {
                    return Err("ui_snapshot_health_status_inconsistent".to_owned());
                }
            }
            UiSnapshotProjectorStatus::Lagging => {
                let Some(projection_cursor) = self.projection_cursor else {
                    return Err("ui_snapshot_health_status_inconsistent".to_owned());
                };
                if projection_cursor >= self.source_cursor
                    || self.lag != Some(self.source_cursor - projection_cursor)
                {
                    return Err("ui_snapshot_health_status_inconsistent".to_owned());
                }
            }
            UiSnapshotProjectorStatus::Unknown => {
                if self.projection_cursor.is_some() || self.lag.is_some() {
                    return Err("ui_snapshot_health_status_inconsistent".to_owned());
                }
            }
        }
        for limitation in &self.limitations {
            required(limitation, "ui_snapshot_health_limitation", 512)?;
        }
        digest(&self.health_digest, "ui_snapshot_health_digest")?;
        if self.health_digest != self.digest() {
            return Err("ui_snapshot_health_digest_mismatch".to_owned());
        }
        Ok(())
    }

    /// 对 health 的所有公开字段生成确定性摘要；`health_digest` 自身不参与摘要输入。
    fn digest(&self) -> String {
        json_digest(&json!({
            "schema": self.schema,
            "epoch": self.epoch,
            "status": self.status,
            "source_cursor": self.source_cursor,
            "projection_cursor": self.projection_cursor,
            "lag": self.lag,
            "projection_generation": self.projection_generation,
            "data_epoch": self.data_epoch,
            "retention_floor": self.retention_floor,
            "observed_at_unix_ms": self.observed_at_unix_ms,
            "limitations": self.limitations,
        }))
    }
}

/// 从调用方提供的一次有序事件切片生成 projector health/lag 快照。
///
/// 【调用者】`DaemonHost::ui_snapshot_projector_health()`；语义上与页面投影共享「source cursor 等于
/// 切片长度」这同一条单游标规则。
/// 【输出】`UpToDate` / `Lagging` / `Unknown` 三态之一，落后差值写入 `lag`，并把对应稳定 limitation
/// 放进列表。
/// 【副作用】纯内存读取；不写事件、不推进任何游标、不接触 UI 状态。
/// 【失败情况】query 不合法、source cursor 与切片长度不符时拒绝。
/// 【为什么不能返回空列表】UI 若在 lag/unknown 时渲染「没有待办」，就会把投影落后伪装成业务空态；
/// 这里让状态先可见，由调用方决定显示的是 lag 指标而不是空结果。
/// 【安全边界】owner 过滤不在本函数内；它只做切片/游标一致性判断，不认证主体。
pub fn project_ui_snapshot_health(
    events: &[RuntimeEvent],
    query: &UiSnapshotProjectorHealthQuery,
) -> Result<UiSnapshotProjectorHealth, String> {
    query.validate()?;
    if query.source_cursor != events.len() as u64 {
        return Err("ui_snapshot_source_cursor_mismatch".to_owned());
    }
    let (status, lag) = match query.projection_cursor {
        None => (UiSnapshotProjectorStatus::Unknown, None),
        Some(projection_cursor) if projection_cursor < query.source_cursor => (
            UiSnapshotProjectorStatus::Lagging,
            Some(query.source_cursor - projection_cursor),
        ),
        // validate() 已拒绝 projection_cursor > source_cursor，因此这里只剩已追平一种情况。
        Some(_) => (UiSnapshotProjectorStatus::UpToDate, Some(0)),
    };
    let mut limitations = Vec::new();
    match status {
        UiSnapshotProjectorStatus::Unknown => {
            limitations.push(UI_SNAPSHOT_LIMITATION_PROJECTION_UNKNOWN.to_owned());
        }
        UiSnapshotProjectorStatus::Lagging => {
            limitations.push(UI_SNAPSHOT_LIMITATION_PROJECTION_LAGGING.to_owned());
        }
        UiSnapshotProjectorStatus::UpToDate => {}
    }
    if query.retention_floor > 0 {
        limitations.push(UI_SNAPSHOT_LIMITATION_RETENTION_FLOOR.to_owned());
    }
    limitations.sort();
    limitations.dedup();
    let mut health = UiSnapshotProjectorHealth {
        schema: UI_SNAPSHOT_PROJECTOR_HEALTH_SCHEMA.to_owned(),
        epoch: query.epoch.clone(),
        status,
        source_cursor: query.source_cursor,
        projection_cursor: query.projection_cursor,
        lag,
        projection_generation: query.projection_generation,
        data_epoch: query.data_epoch,
        retention_floor: query.retention_floor,
        observed_at_unix_ms: query.observed_at_unix_ms,
        limitations,
        health_digest: String::new(),
    };
    health.health_digest = health.digest();
    health.validate()?;
    Ok(health)
}

/// read-model rebuild 的只读结果：同一游标下的页面与 projector health。
///
/// 这是 UI-05 的「read-model rebuild 命令」产物：重建永远从 EventLog 切片重新计算，不加载、也不
/// 覆盖任何持久化 read model；`page` 与 `health` 必须指向同一次读取。
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct UiSnapshotRebuild {
    /// rebuild 合同 schema。
    pub schema: String,
    /// 本次重建采用的 epoch。
    pub epoch: String,
    /// 本次重建读取的完整切片位置。
    pub source_cursor: u64,
    /// 采用的 projector 版本代数。
    pub projection_generation: u64,
    /// 数据治理 epoch 标签。
    pub data_epoch: u64,
    /// 从账本切片重建的页面。
    pub page: UiSnapshotPage,
    /// 与页面同一游标、同一读取的 projector health。
    pub health: UiSnapshotProjectorHealth,
    /// 绑定页面与 health 摘要的重建自摘要。
    pub rebuild_digest: String,
}

impl UiSnapshotRebuild {
    /// 校验页面与 health 指向同一次读取，并且状态已追平。
    ///
    /// 【失败】任一侧 validate 失败、epoch/source/generation/data epoch 不一致、health 不是
    /// `UpToDate`、或 rebuild 摘要不匹配时拒绝。
    /// 【边界】重建结果仍然只是读模型；它的摘要不证明事件事实或业务结果正确。
    pub fn validate(&self) -> Result<(), String> {
        self.page.validate()?;
        self.health.validate()?;
        if self.schema != UI_SNAPSHOT_REBUILD_SCHEMA
            || self.epoch != self.page.epoch
            || self.epoch != self.health.epoch
            || self.source_cursor != self.page.source_cursor
            || self.source_cursor != self.health.source_cursor
            || self.projection_generation != self.page.projection_generation
            || self.projection_generation != self.health.projection_generation
            || self.data_epoch != self.page.data_epoch
            || self.data_epoch != self.health.data_epoch
            || self.health.status != UiSnapshotProjectorStatus::UpToDate
        {
            return Err("ui_snapshot_rebuild_inconsistent".to_owned());
        }
        digest(&self.rebuild_digest, "ui_snapshot_rebuild_digest")?;
        if self.rebuild_digest != self.digest() {
            return Err("ui_snapshot_rebuild_digest_mismatch".to_owned());
        }
        Ok(())
    }

    /// 通过页面与 health 的两个子摘要生成重建摘要，避免再造第三份业务字段快照。
    fn digest(&self) -> String {
        json_digest(&json!({
            "schema": self.schema,
            "epoch": self.epoch,
            "source_cursor": self.source_cursor,
            "projection_generation": self.projection_generation,
            "data_epoch": self.data_epoch,
            "page": self.page.snapshot_digest,
            "health": self.health.health_digest,
        }))
    }
}

/// 从同一事件切片重建页面与 projector health。
///
/// 【调用者】`DaemonHost::rebuild_ui_snapshot_projection()`：一次 `persisted_events()` 读取同时用于页面
/// 与 health，保证两者指向同一游标。
/// 【输入】事件 Vec 与页面 query；health 查询由 query 派生（同一 epoch/source/generation/data epoch/
/// retention floor/时间标签），因此重建结果可重复且不引入新的时间源。
/// 【输出】`UiSnapshotRebuild`：页面通过全部页边界/retention/cursor 校验，health 状态为 `UpToDate`。
/// 【失败情况】页面投影的任何拒绝（lag、未知进度、retention pending、跨 owner session、cursor 拼接）
/// 都会让重建失败，不返回半新半旧的组合。
/// 【为什么需要它】新进程或恢复后的调用方需要一条显式路径证明「读模型可以从账本重建」，而不是依赖
/// 进程内缓存；本函数与页面投影共享同一验证器，不存在第二套重建逻辑。
/// 【安全边界】与 `project_ui_snapshot()` 相同：owner 过滤不等于认证，摘要不是签名。
pub fn rebuild_ui_snapshot_projection(
    events: &[RuntimeEvent],
    query: &UiSnapshotQuery,
) -> Result<UiSnapshotRebuild, String> {
    let page = project_ui_snapshot(events, query)?;
    let health_query = UiSnapshotProjectorHealthQuery::new(
        query.epoch.clone(),
        query.source_cursor,
        query.projection_cursor,
        query.projection_generation,
        query.data_epoch,
        query.retention_floor,
        query.generated_at_unix_ms,
    )?;
    let health = project_ui_snapshot_health(events, &health_query)?;
    let mut rebuild = UiSnapshotRebuild {
        schema: UI_SNAPSHOT_REBUILD_SCHEMA.to_owned(),
        epoch: page.epoch.clone(),
        source_cursor: page.source_cursor,
        projection_generation: page.projection_generation,
        data_epoch: page.data_epoch,
        page,
        health,
        rebuild_digest: String::new(),
    };
    rebuild.rebuild_digest = rebuild.digest();
    rebuild.validate()?;
    Ok(rebuild)
}

//! 可重建读模型（projection）检查点的领域合同。
//!
//! 检查点保存 projector 已折叠出的状态、来源 EventLog 游标和一组事件 ID，使后续重放可以从该状态接着处理尾部事件，
//! 而不必每次都从零折叠全部历史。检查点只是可丢弃、可重建的读模型加速数据，不是授权或业务事实的权威来源。
//!
//! 【典型数据流】EventLog/上游读取器提供已提交事件和游标 → Core `ReplayProjection` 调用领域状态 fold →
//! 本结构封装 projector 状态并计算摘要 → 调用者决定是否持久化/缓存；本 DTO 自身不读写 EventLog 或磁盘。
//! Core 源码中的直接调用点是 `notification_projector.rs` 的 `NotificationProjection::apply_committed()`；全仓库未找到该类型的生产实例化调用者，
//! 当前可见使用来自测试。通用重放器收到的事件、游标及 fold 都由调用者提供，因此不能据此说投影已接入运行时 EventStore 路径。
//!
//! Serde 将结构体与 JSON 等外部格式互转；`deny_unknown_fields` 拒绝当前合同未声明的额外字段。

use crate::{json_digest, EventCursor, EventId, SchemaVersion, MAX_SOURCE_EVENT_IDS};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

/// 检查点 JSON 对应的稳定 schema 名称。
pub const PROJECTION_CHECKPOINT_SCHEMA: &str = "kiana.projection-checkpoint.v1";
/// 当前检查点合同版本。
pub const PROJECTION_CHECKPOINT_VERSION: SchemaVersion = SchemaVersion::new(1, 0);

/// 某个读模型 projector 的可序列化重放锚点。
///
/// checkpoint 包含的是 projector 自己定义的 JSON state；Domain 不知道其中的业务字段含义。
/// `state_digest` 用于先检查状态本身，`checkpoint_digest` 再绑定 schema、projector、游标、事件 ID 列表和 state 摘要。
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProjectionCheckpoint {
    /// 固定 checkpoint schema 名称。
    pub schema: String,
    /// 可兼容演进的 schema 版本。
    pub version: SchemaVersion,
    /// 产生该状态的 projector 稳定名称；恢复时必须与当前 projector 相同。
    pub projector: String,
    /// 该读模型已处理到的 EventLog 全局游标；0 可表示从零开始的空锚点。
    pub source_cursor: EventCursor,
    /// 已折叠事件的身份集合；用于识别重放片段中重复出现的事件 ID，不代表这些事件内容。
    #[serde(default)]
    pub source_event_ids: Vec<EventId>,
    /// projector 业务状态的 JSON 表示；具体 schema 由 projector 的 fold 函数负责。
    pub state: Value,
    /// 对 JSON state 单独计算的 SHA-256 摘要。
    pub state_digest: String,
    /// 对 checkpoint 其余字段计算的 SHA-256 摘要。
    pub checkpoint_digest: String,
}

impl ProjectionCheckpoint {
    /// 创建并校验 checkpoint，先规范化 event ID 列表，再计算状态和整体摘要。
    ///
    /// 【调用者】Core `ReplayProjection::checkpoint()`，或需要将 projector 状态包装成 DTO 的调用者。
    /// 【输入/输出】接收 projector 名称、EventLog 游标、已处理事件 ID 和 JSON state；成功返回可序列化 checkpoint。
    /// 【规范化】构造器会排序并去重 event IDs；直接反序列化出的重复/乱序列表则由 `validate()` 拒绝。
    /// 【失败情况】名称空/过长、event ID 数量越界或生成后的字段校验失败时返回稳定错误。
    /// 【副作用与限制】只在内存中计算摘要；它不保存 checkpoint，也不核对游标是否真的对应这些事件。
    pub fn new(
        projector: &str,
        source_cursor: EventCursor,
        mut source_event_ids: Vec<EventId>,
        state: Value,
    ) -> Result<Self, String> {
        if projector.trim().is_empty() || projector.len() > 256 {
            return Err("projection_checkpoint_projector_invalid".to_owned());
        }
        source_event_ids.sort();
        source_event_ids.dedup();
        let state_digest = json_digest(&state);
        let mut checkpoint = Self {
            schema: PROJECTION_CHECKPOINT_SCHEMA.to_owned(),
            version: PROJECTION_CHECKPOINT_VERSION,
            projector: projector.to_owned(),
            source_cursor,
            source_event_ids,
            state,
            state_digest,
            checkpoint_digest: String::new(),
        };
        checkpoint.checkpoint_digest = checkpoint.digest();
        checkpoint.validate()?;
        Ok(checkpoint)
    }

    /// 把外部 JSON `Value` 解码为 checkpoint，并立即检查结构与摘要。
    ///
    /// Serde 在这里把 JSON 字段映射为 Rust 类型；未知字段、字段类型错误会统一变成 decode 错误，
    /// 解码成功后仍须通过 `validate()`，所以“能解析”不等于“可安全用于恢复”。
    pub fn from_json(value: &Value) -> Result<Self, String> {
        let checkpoint: Self = serde_json::from_value(value.clone())
            .map_err(|_| "projection_checkpoint_decode_failed".to_owned())?;
        checkpoint.validate()?;
        Ok(checkpoint)
    }

    /// 把 checkpoint 编码为 JSON `Value`，供持久化 adapter 或其他传输层继续处理。
    ///
    /// 本方法只转换格式，不写文件/数据库；持久化责任属于调用者。
    pub fn to_json(&self) -> Result<Value, String> {
        serde_json::to_value(self).map_err(|_| "projection_checkpoint_encode_failed".to_owned())
    }

    /// 验证 wire header、projector 名称、event ID 集合和两层内容摘要。
    ///
    /// 【顺序要求】事件 ID 数组必须严格递增，因此重复和乱序 checkpoint 都会被拒绝；构造器创建时会先规范化。
    /// 【兼容性】版本使用 `is_compatible_with()`，而不是只接受完全相同的数字版本。
    /// 【限制】这里不验证 `source_cursor` 与事件序号/ID 的外部关系，也不认证摘要来源；checkpoint 始终只是可重建缓存。
    pub fn validate(&self) -> Result<(), String> {
        if self.schema != PROJECTION_CHECKPOINT_SCHEMA
            || !self
                .version
                .is_compatible_with(&PROJECTION_CHECKPOINT_VERSION)
            || self.projector.trim().is_empty()
            || self.projector.len() > 256
            || self.source_event_ids.len() > MAX_SOURCE_EVENT_IDS
            || self
                .source_event_ids
                .windows(2)
                .any(|pair| pair[0] >= pair[1])
            || self.source_event_ids.iter().any(|id| id.as_uuid().is_nil())
            || !valid_digest(&self.state_digest)
            || !valid_digest(&self.checkpoint_digest)
        {
            return Err("projection_checkpoint_header_invalid".to_owned());
        }
        if self.state_digest != json_digest(&self.state) {
            return Err("projection_checkpoint_state_digest_mismatch".to_owned());
        }
        if self.checkpoint_digest != self.digest() {
            return Err("projection_checkpoint_digest_mismatch".to_owned());
        }
        Ok(())
    }

    /// 对除 `checkpoint_digest` 外的 checkpoint 字段计算内容摘要。
    ///
    /// `state_digest` 本身也进入这个摘要；摘要可发现非协同改动，但没有密钥，不能证明数据由可信 projector 生成。
    pub fn digest(&self) -> String {
        json_digest(&json!({
            "schema": self.schema,
            "version": self.version,
            "projector": self.projector,
            "source_cursor": self.source_cursor,
            "source_event_ids": self.source_event_ids,
            "state": self.state,
            "state_digest": self.state_digest,
        }))
    }
}

/// 判断摘要字符串是否符合 `sha256:` 加 64 位十六进制字符的外形。
///
/// 此 helper 不重算摘要，也不认证写入者；内容比较由 checkpoint 的 `validate()` 完成。
fn valid_digest(value: &str) -> bool {
    let Some(hex) = value.strip_prefix("sha256:") else {
        return false;
    };
    hex.len() == 64 && hex.bytes().all(|byte| byte.is_ascii_hexdigit())
}

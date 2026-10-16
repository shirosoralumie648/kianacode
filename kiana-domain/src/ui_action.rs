//! UI action 命令与状态 journal（操作记录簿）的 Domain 合同。
//!
//! 界面发来的 [`UiActionCommand`] 只是一个带前置条件的请求，不代表操作已经执行。ControlPlane 先将
//! command 与服务端当前 authority snapshot 比较，再由 [`UiActionJournal`] reducer 计算
//! Accepted/Applied/Rejected/Unknown 状态；Core 的 action facade 才负责把各状态作为聚合事件通过
//! EventStore CAS 持久化：
//!
//! ```text
//! UI/CLI/Workbench action intent
//!     -> UiActionCommand（稳定身份 + 目标/owner/scope + epoch/cursor/revision + deadline + payload digest）
//!     -> ControlPlane 重查 owner/scope/permit/当前版本，并按原 idempotency_key 读取 journal
//!     -> 本文件 UiActionJournal 校验并转换状态（纯内存、确定性）
//!     -> Core 通过 atomic transition 持久化 accepted/applied/rejected/unknown 事实
//!     -> surface 使用原幂等键查询丢失 ACK 的原结果
//! ```
//!
//! 本文件不执行 UI effect、不打开事务存储、不发放 permit，也不认证调用者提供的摘要。Accepted 只表示
//! 命令满足给定快照条件；Applied 需要上游提供 receipt digest；Unknown 表示效果结果无法确认，必须保留
//! 原 command/idempotency key 对账，不能生成新 key 重做。

use crate::{json_digest, RequestId, SchemaVersion};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::BTreeMap;

pub const UI_ACTION_COMMAND_SCHEMA: &str = "kiana.ui-action-command.v1";
pub const UI_ACTION_RECORD_SCHEMA: &str = "kiana.ui-action-record.v1";
pub const UI_ACTION_JOURNAL_VERSION: SchemaVersion = SchemaVersion::new(1, 0);
pub const UI_ACTION_AGGREGATE_TYPE: &str = "ui_action";

/// 检查必填文本非空、不含 NUL 且不超过给定 UTF-8 字节数。
///
/// 【调用者】command validator 检查 key/target/owner/epoch。
/// 【失败】返回由 field 名构造的稳定错误字符串；`len()` 计字节，不计字符。
/// 【边界】只验证形状，不能确认 owner/target 是否存在或当前用户能否操作它们。
fn required(value: &str, field: &str, max: usize) -> Result<(), String> {
    if value.trim().is_empty() || value.len() > max || value.contains('\0') {
        return Err(format!("{field}_invalid"));
    }
    Ok(())
}

/// 检查摘要形状是否为 `sha256:` 加 64 个十六进制字符。
///
/// 【调用者】命令、记录和 receipt validator。
/// 【失败】格式不符返回 `<field>_invalid`。
/// 【边界】不从原始数据重算、不认证签名者；各 DTO 只有在持有摘要输入时才能再做内容比较。
fn digest(value: &str, field: &str) -> Result<(), String> {
    let Some(hex) = value.strip_prefix("sha256:") else {
        return Err(format!("{field}_invalid"));
    };
    if hex.len() != 64 || !hex.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err(format!("{field}_invalid"));
    }
    Ok(())
}

/// 去掉 json_digest 使用的 `sha256:` 展示前缀，保留摘要十六进制部分。
/// 调用方应先验证格式；本 helper 对缺少前缀的任意字符串会原样复制。
fn bare_digest(value: &str) -> String {
    value.strip_prefix("sha256:").unwrap_or(value).to_owned()
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum UiActionState {
    /// 命令已被接受进 journal，effect 还没有被记录为完成。
    Accepted,
    /// 上游报告 effect 已有 receipt；这是唯一允许 effect_count=1 的状态。
    Applied,
    /// 命令被明确拒绝，reason 说明稳定拒绝原因。
    Rejected,
    /// effect 可能已发生，但调用方没有得到可验证结果；保持此状态等待用原 key 对账。
    Unknown,
}

impl UiActionState {
    /// 判断该状态是否禁止再走普通 Accepted→Applied 流程。
    /// Unknown 也属于终态，因为“结果未知”不能被当作“未执行”然后自动重做。
    pub const fn terminal(self) -> bool {
        matches!(self, Self::Applied | Self::Rejected | Self::Unknown)
    }
}

/// Surface adapter 生成并提交的一次 UI 操作请求身份。
///
/// 【系统位置】入口把用户动作编码成该 DTO；Core 将它与 authority snapshot、已有幂等记录和 EventStore
/// aggregate 对账。
/// 【下游】`UiActionJournal::accept()` 检查字段后生成 Accepted record；后续 apply/reject/unknown 必须
/// 引用同一 command digest 和 idempotency key。
/// 【重要边界】DTO 自带 digest 只是内容一致性标签；scope/owner/epoch/cursor 是客户端声明的 expected 值，
/// 仍须由 ControlPlane 与服务端当前权威事实比较。当前合同没有 permit_digest 字段，不能把 command digest
/// 解释成包含 permit 的授权证明。
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct UiActionCommand {
    /// 命令 wire schema。
    pub schema: String,
    /// command/journal 合同版本；当前兼容版本为 1.0。
    pub version: SchemaVersion,
    /// 命令 ID，用于跨事件追踪同一次 UI 意图。
    pub command_id: RequestId,
    /// 调用方为重试/ACK 对账复用的键；同一键不能代表不同 command digest。
    pub idempotency_key: String,
    /// 被操作的目标标识；本 Domain 只检查格式，不查询目标。
    pub target_id: String,
    /// 发起动作的 owner 标识；必须由 Core 对照可信 authority snapshot。
    pub owner_id: String,
    /// 目标/操作 scope 的摘要标签；服务端须与当前 scope digest 比较。
    pub scope_digest: String,
    /// 客户端看到的 authority/instance epoch，检测身份或授权实例更替。
    pub expected_epoch: String,
    /// 客户端看到的源状态 cursor，用于检测快照过期。
    pub expected_cursor: u64,
    /// 可选目标修订版。这里与服务端当前 Option 做精确相等比较；None 不是无条件跳过比较。
    #[serde(default)]
    pub expected_revision: Option<u64>,
    /// 最迟接受时间（Unix 毫秒）；新命令在 `now > deadline` 时拒绝，已有幂等记录重放不重新过期判定。
    pub deadline_unix_ms: u64,
    /// UI 操作参数 JSON；真正解释 payload 的目标 handler 不在本 Domain journal。
    pub payload: Value,
    /// 对 payload JSON 的内容摘要。
    pub payload_digest: String,
    /// 对除 payload 正文外的不可变 command identity 生成的摘要。
    pub command_digest: String,
}

impl UiActionCommand {
    /// 从 surface 提供的字段构造 command，先算两个摘要再运行完整验证。
    ///
    /// 【调用者】入口 adapter 创建 UI action intent 时。
    /// 【输入/输出】接收 command ID、幂等 key、target/owner/scope、expected 版本字段、期限和 JSON payload；
    /// 成功返回 digest 已填充的 DTO。
    /// 【失败】字段/版本/摘要不符合合同则返回稳定错误；不访问当前权限、目标或 EventStore。
    /// 【设计原因】先构造摘要再统一调用 `validate()`，让 caller 和接收端使用同一套字段集合与摘要规则。
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        command_id: RequestId,
        idempotency_key: impl Into<String>,
        target_id: impl Into<String>,
        owner_id: impl Into<String>,
        scope_digest: impl Into<String>,
        expected_epoch: impl Into<String>,
        expected_cursor: u64,
        expected_revision: Option<u64>,
        deadline_unix_ms: u64,
        payload: Value,
    ) -> Result<Self, String> {
        let payload_digest = json_digest(&payload);
        let mut command = Self {
            schema: UI_ACTION_COMMAND_SCHEMA.to_owned(),
            version: UI_ACTION_JOURNAL_VERSION,
            command_id,
            idempotency_key: idempotency_key.into(),
            target_id: target_id.into(),
            owner_id: owner_id.into(),
            scope_digest: scope_digest.into(),
            expected_epoch: expected_epoch.into(),
            expected_cursor,
            expected_revision,
            deadline_unix_ms,
            payload,
            payload_digest,
            command_digest: String::new(),
        };
        command.command_digest = command.digest();
        command.validate()?;
        Ok(command)
    }

    /// 检查 command header、文本/摘要形状以及 payload/command 自摘要。
    ///
    /// 【调用者】`new()` 和 `UiActionJournal::accept()`；Core facade 也会在读取 journal 前验证。
    /// 【成功条件】schema/version/id/期限/Option revision 合法，必填标识有界，scope/payload/command
    /// 摘要形状正确，payload digest 与 JSON 内容相等，command digest 与不可变字段重算值相等。
    /// 【失败影响】Core 将错误映射为 ui action 拒绝，不写 accepted 事件，也不触发 effect。
    /// 【边界】没有当前 authority snapshot，因此不能验证 epoch/cursor/revision 是否最新；无密钥 hash 也
    /// 不能证明 payload 来源或授权者身份。
    pub fn validate(&self) -> Result<(), String> {
        if self.schema != UI_ACTION_COMMAND_SCHEMA
            || !self.version.is_compatible_with(&UI_ACTION_JOURNAL_VERSION)
            || self.command_id.as_uuid().is_nil()
            || self.expected_revision == Some(0)
            || self.deadline_unix_ms == 0
        {
            return Err("ui_action_command_header_invalid".to_owned());
        }
        required(&self.idempotency_key, "ui_action_idempotency_key", 256)?;
        required(&self.target_id, "ui_action_target", 512)?;
        required(&self.owner_id, "ui_action_owner", 256)?;
        required(&self.expected_epoch, "ui_action_epoch", 256)?;
        digest(&self.scope_digest, "ui_action_scope_digest")?;
        digest(&self.payload_digest, "ui_action_payload_digest")?;
        digest(&self.command_digest, "ui_action_command_digest")?;
        if self.payload_digest != json_digest(&self.payload) {
            return Err("ui_action_payload_digest_mismatch".to_owned());
        }
        if self.command_digest != self.digest() {
            return Err("ui_action_command_digest_mismatch".to_owned());
        }
        Ok(())
    }

    /// 只摘要不可变 command 身份；journal state、服务端时间和 receipt 不参与。
    /// 因而状态变化时 command digest 保持不变，record digest 才随状态重算。
    pub fn digest(&self) -> String {
        json_digest(&json!({
            "schema": self.schema,
            "version": self.version,
            "command_id": self.command_id,
            "idempotency_key": self.idempotency_key,
            "target_id": self.target_id,
            "owner_id": self.owner_id,
            "scope_digest": self.scope_digest,
            "expected_epoch": self.expected_epoch,
            "expected_cursor": self.expected_cursor,
            "expected_revision": self.expected_revision,
            "deadline_unix_ms": self.deadline_unix_ms,
            "payload_digest": self.payload_digest,
        }))
    }

    /// 返回不含 `sha256:` 前缀的 command 摘要，供 EventStore transition batch 的 digest 字段使用。
    /// 本方法只去前缀，不做 `validate()`；存储调用者应先确认 command 合法。
    pub fn journal_digest(&self) -> String {
        bare_digest(&self.command_digest)
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct UiActionRecord {
    /// record wire schema。
    pub schema: String,
    /// 记录格式版本。
    pub version: SchemaVersion,
    /// 与初始 command 相同的请求追踪 ID。
    pub command_id: RequestId,
    /// journal 的主索引键；Core 同时用它定位 EventStore 聚合。
    pub idempotency_key: String,
    /// 原始 command 的不可变摘要，状态迁移时不变。
    pub command_digest: String,
    /// 原始 payload 摘要。
    pub payload_digest: String,
    /// 目标和 owner 标签的服务端 record 副本。
    pub target_id: String,
    pub owner_id: String,
    /// 初始命令声明并由 Core 对照的 scope 摘要。
    pub scope_digest: String,
    /// 当前记录状态；从 Accepted 迁移到唯一一种终态。
    pub state: UiActionState,
    /// Core 接受命令时记录的 Unix 毫秒时间。
    pub accepted_at_unix_ms: u64,
    /// 只有 Applied 才应存在的效果确认时间。
    #[serde(default)]
    pub applied_at_unix_ms: Option<u64>,
    /// Rejected/Unknown 的原因标签；Applied/Accepted 通常为空。
    #[serde(default)]
    pub reason: Option<String>,
    /// Domain 记录的 effect 次数；合同最多 1，只有 Applied 才能为 1。
    pub effect_count: u32,
    /// 只有 Applied 才允许关联的 receipt 摘要。
    #[serde(default)]
    pub receipt_digest: Option<String>,
    /// 对此 record 当前所有字段计算的摘要；每次状态迁移后重算。
    pub record_digest: String,
}

impl UiActionRecord {
    /// 从通过 Core admission 的 command 建立初始 Accepted record。
    ///
    /// 【调用者】只由同文件 `UiActionJournal::accept()` 调用。
    /// 【输入/输出】command 的身份字段加当前接受时间；得到 effect_count=0、无 receipt、无 applied time
    /// 的 Accepted 记录，并马上填好 record digest。
    /// 【副作用】仅构造内存值；Durable 事件由 Core 另行提交。
    fn from_command(command: &UiActionCommand, now_unix_ms: u64) -> Self {
        let mut record = Self {
            schema: UI_ACTION_RECORD_SCHEMA.to_owned(),
            version: UI_ACTION_JOURNAL_VERSION,
            command_id: command.command_id,
            idempotency_key: command.idempotency_key.clone(),
            command_digest: command.command_digest.clone(),
            payload_digest: command.payload_digest.clone(),
            target_id: command.target_id.clone(),
            owner_id: command.owner_id.clone(),
            scope_digest: command.scope_digest.clone(),
            state: UiActionState::Accepted,
            accepted_at_unix_ms: now_unix_ms,
            applied_at_unix_ms: None,
            reason: None,
            effect_count: 0,
            receipt_digest: None,
            record_digest: String::new(),
        };
        record.record_digest = record.digest();
        record
    }

    /// 验证 record 的 schema、摘要外形和 state/receipt/effect 组合关系。
    ///
    /// 【调用者】从存储载荷解码、journal 装载和状态迁移前后；Core 的 query helper 通过
    /// `validate_for_query()` 间接调用。
    /// 【关键状态不变量】Applied 必须有 receipt、effect_count=1 和 applied timestamp；其它状态不能有
    /// receipt/applied timestamp，effect_count 必须为 0；Rejected/Unknown 还必须有 reason。
    /// 【失败】状态与 receipt 不匹配、缺 Applied receipt、effect 计数不符或 record digest 过期时返回
    /// 对应稳定错误，不会将结构异常解释成成功。
    /// 【边界】这里只比较字段形状和无密钥自摘要；不重新查询实际执行 receipt，也没有校验
    /// `applied_at_unix_ms >= accepted_at_unix_ms`。
    fn validate(&self) -> Result<(), String> {
        if self.schema != UI_ACTION_RECORD_SCHEMA
            || !self.version.is_compatible_with(&UI_ACTION_JOURNAL_VERSION)
            || self.command_id.as_uuid().is_nil()
            || self.accepted_at_unix_ms == 0
            || self.effect_count > 1
        {
            return Err("ui_action_record_header_invalid".to_owned());
        }
        required(&self.idempotency_key, "ui_action_idempotency_key", 256)?;
        digest(&self.command_digest, "ui_action_command_digest")?;
        digest(&self.payload_digest, "ui_action_payload_digest")?;
        digest(&self.scope_digest, "ui_action_scope_digest")?;
        digest(&self.record_digest, "ui_action_record_digest")?;
        match (&self.state, &self.receipt_digest) {
            (UiActionState::Applied, Some(receipt_digest)) => {
                digest(receipt_digest, "ui_action_receipt_digest")?;
            }
            (UiActionState::Applied, None) => {
                return Err("ui_action_applied_receipt_missing".to_owned());
            }
            (
                UiActionState::Accepted | UiActionState::Rejected | UiActionState::Unknown,
                Some(_),
            ) => {
                return Err("ui_action_receipt_state_mismatch".to_owned());
            }
            (_, None) => {}
        }
        if self.state == UiActionState::Applied
            && (self.effect_count != 1 || self.applied_at_unix_ms.is_none())
        {
            return Err("ui_action_applied_effect_missing".to_owned());
        }
        if self.state != UiActionState::Applied
            && (self.effect_count != 0 || self.applied_at_unix_ms.is_some())
        {
            return Err("ui_action_effect_state_mismatch".to_owned());
        }
        if self.state.terminal() && self.reason.is_none() && self.state != UiActionState::Applied {
            return Err("ui_action_terminal_reason_missing".to_owned());
        }
        if self.record_digest != self.digest() {
            return Err("ui_action_record_digest_mismatch".to_owned());
        }
        Ok(())
    }

    /// 对完整 record 状态生成内容摘要；`record_digest` 自身不参与输入。
    fn digest(&self) -> String {
        json_digest(&json!({
            "schema": self.schema,
            "version": self.version,
            "command_id": self.command_id,
            "idempotency_key": self.idempotency_key,
            "command_digest": self.command_digest,
            "payload_digest": self.payload_digest,
            "target_id": self.target_id,
            "owner_id": self.owner_id,
            "scope_digest": self.scope_digest,
            "state": self.state,
            "accepted_at_unix_ms": self.accepted_at_unix_ms,
            "applied_at_unix_ms": self.applied_at_unix_ms,
            "reason": self.reason,
            "effect_count": self.effect_count,
            "receipt_digest": self.receipt_digest,
        }))
    }

    /// 给 Core 查询持久记录使用的公开 validator 转发方法。
    /// 它只做 record 合同校验，不按 caller/owner/scope 过滤查询结果。
    pub fn validate_for_query(&self) -> Result<(), String> {
        self.validate()
    }

    /// 以不可变 record 为起点，复制到临时单条 journal 后尝试 Applied 转换。
    ///
    /// 【调用者】Core 读取已接受 record、确认有 receipt 后使用；成功值之后还需经存储 CAS 持久化。
    /// 【输入/输出】receipt digest 和当前时间，成功返回新 record；当前对象本身不变。
    /// 【失败】原记录无效、状态为 Rejected/Unknown、receipt 格式错误或时钟早于接受时间时返回错误。
    /// 【为什么复制成临时 journal】让单记录转换复用 `UiActionJournal::apply()` 的同一规则，避免第二套状态逻辑。
    /// 【重放】若原状态已经 Applied，journal 会返回原记录，不比较重试时传入的 receipt 是否与原 receipt 相同。
    pub fn apply_transition(
        &self,
        receipt_digest: impl Into<String>,
        now_unix_ms: u64,
    ) -> Result<Self, String> {
        let mut journal = UiActionJournal::from_record(self.clone())?;
        journal.apply(&self.idempotency_key, receipt_digest, now_unix_ms)
    }

    /// 复制该记录并尝试进入 Rejected；不持久化，不改变原 record。
    /// Accepted 可转为 Rejected；Applied 返回原记录；Unknown 与 Rejected 冲突。
    pub fn reject_transition(&self, reason: impl Into<String>) -> Result<Self, String> {
        let mut journal = UiActionJournal::from_record(self.clone())?;
        journal.reject(&self.idempotency_key, reason)
    }

    /// 复制该记录并进入 Unknown，用于结果无法确认时保留原幂等身份供 reconciliation。
    /// 返回 Unknown 不表示 effect 一定没发生，不能据此使用新 key 自动重试。
    pub fn unknown_transition(&self, reason: impl Into<String>) -> Result<Self, String> {
        let mut journal = UiActionJournal::from_record(self.clone())?;
        journal.unknown(&self.idempotency_key, reason)
    }
}

#[derive(Clone, Debug, Default)]
/// 按 idempotency key 索引的确定性内存 reducer，不是持久化 journal 本身。
///
/// Core facade 在第一次 admission 时临时创建它计算下一条 record；持久事实仍由 Core 的原子
/// EventStore transition 写入。测试也可直接用它验证同一状态规则，而不启动存储 adapter。
pub struct UiActionJournal {
    records: BTreeMap<String, UiActionRecord>,
}

impl UiActionJournal {
    /// 创建没有任何记录的空内存 journal。
    pub fn new() -> Self {
        Self::default()
    }

    /// 从一条已存在且合法的 record 重建最小 journal。
    ///
    /// 【调用者】单记录 transition helper 将不可变 record 复制成临时 reducer 时。
    /// 【输入/输出】先验证 record，再按其 idempotency key 插入；失败时不返回部分 journal。
    pub fn from_record(record: UiActionRecord) -> Result<Self, String> {
        record.validate()?;
        let mut journal = Self::new();
        journal
            .records
            .insert(record.idempotency_key.clone(), record);
        Ok(journal)
    }

    /// 接受一次 UI action，或按完全相同的幂等身份返回已有记录。
    ///
    /// 【调用者】Core `admit_ui_action()` 在 authority/permit 检查及读取原 action stream 后调用；
    /// Core 当前为单个 key 创建新 journal，所以持久化仍由 Core 完成。
    /// 【输入】command、服务端 authority epoch/cursor/revision/owner/scope 快照和当前 Unix 毫秒时间。
    /// 【输出】`(record, replayed)`：首次成功返回 Accepted 与 `false`；相同 key+digest 重放返回原记录与
    /// `true`，不增加 effect_count。
    /// 【失败】同 key 不同 digest、owner/scope 不同、deadline 已过、epoch/cursor/revision 过期时返回明确错误。
    /// Core 把拒绝变成 command error；只有成功的新记录才继续写 accepted aggregate event。
    /// 【容易误改】幂等分支在 epoch/cursor/revision/deadline 新鲜度检查之前执行：已存在的原命令即使
    /// 当前快照已变化或期限已过，也应返回原结果供对账，不能把它当成第二次新操作。
    /// 【边界】当前 journal 只以 idempotency_key 作唯一索引，不拒绝不同 key 复用 command_id；
    /// owner/scope/epoch 等输入本身必须由 Core 从可信 snapshot 提供，本 reducer 不查询权威来源。
    pub fn accept(
        &mut self,
        command: &UiActionCommand,
        authority_epoch: &str,
        current_cursor: u64,
        current_revision: Option<u64>,
        owner_id: &str,
        scope_digest: &str,
        now_unix_ms: u64,
    ) -> Result<(UiActionRecord, bool), String> {
        command.validate()?;
        if let Some(original) = self.records.get(&command.idempotency_key) {
            if original.command_digest != command.command_digest {
                return Err("ui_action_idempotency_digest_mismatch".to_owned());
            }
            if original.owner_id != command.owner_id || original.owner_id != owner_id {
                return Err("ui_action_owner_mismatch".to_owned());
            }
            if original.scope_digest != command.scope_digest
                || original.scope_digest != scope_digest
            {
                return Err("ui_action_scope_mismatch".to_owned());
            }
            return Ok((original.clone(), true));
        }
        if now_unix_ms > command.deadline_unix_ms {
            return Err("ui_action_deadline_expired".to_owned());
        }
        if command.expected_epoch != authority_epoch {
            return Err("ui_action_stale_epoch".to_owned());
        }
        if command.expected_cursor != current_cursor {
            return Err("ui_action_stale_cursor".to_owned());
        }
        if command.expected_revision != current_revision {
            return Err("ui_action_stale_revision".to_owned());
        }
        if command.owner_id != owner_id {
            return Err("ui_action_owner_mismatch".to_owned());
        }
        if command.scope_digest != scope_digest {
            return Err("ui_action_scope_mismatch".to_owned());
        }
        let record = UiActionRecord::from_command(command, now_unix_ms);
        self.records
            .insert(command.idempotency_key.clone(), record.clone());
        Ok((record, false))
    }

    /// 把 Accepted 记录推进为 Applied，并记录 receipt 摘要和一次 effect 计数。
    ///
    /// 【调用者】Core 在目标 handler 已取得 effect receipt 后调用；不能在 effect 执行前标记 Applied。
    /// 【输入】原 idempotency key、receipt digest、当前服务端毫秒时间。
    /// 【输出】新 Applied record；Core 随后负责通过 atomic transition 持久化。
    /// 【失败】key 未接受、record 无效、状态为 Rejected/Unknown、时钟早于 accepted_at 或 receipt 格式无效时拒绝。
    /// 【重放】已 Applied 时先返回原 record，不再增加 effect_count，也不核对本次新传入的 receipt；这表示
    /// 返回第一次确认结果，不表示接受第二个 receipt。
    /// 【副作用边界】只修改内存 journal，不在此函数执行 UI effect。
    pub fn apply(
        &mut self,
        idempotency_key: &str,
        receipt_digest: impl Into<String>,
        now_unix_ms: u64,
    ) -> Result<UiActionRecord, String> {
        let record = self
            .records
            .get_mut(idempotency_key)
            .ok_or_else(|| "ui_action_not_accepted".to_owned())?;
        record.validate()?;
        match record.state {
            UiActionState::Applied => return Ok(record.clone()),
            UiActionState::Rejected => return Err("ui_action_rejected".to_owned()),
            UiActionState::Unknown => return Err("ui_action_result_unknown".to_owned()),
            UiActionState::Accepted => {}
        }
        if now_unix_ms < record.accepted_at_unix_ms {
            return Err("ui_action_clock_regressed".to_owned());
        }
        let receipt_digest = receipt_digest.into();
        digest(&receipt_digest, "ui_action_receipt_digest")?;
        record.state = UiActionState::Applied;
        record.applied_at_unix_ms = Some(now_unix_ms);
        record.effect_count = 1;
        record.receipt_digest = Some(receipt_digest);
        record.reason = None;
        record.record_digest = record.digest();
        record.validate()?;
        Ok(record.clone())
    }

    /// 将已接受动作明确标记为 Rejected；原因和状态记录在 journal 中，不执行补偿 effect。
    pub fn reject(
        &mut self,
        idempotency_key: &str,
        reason: impl Into<String>,
    ) -> Result<UiActionRecord, String> {
        self.finish(idempotency_key, UiActionState::Rejected, reason.into())
    }

    /// 将结果无法确认的动作标记为 Unknown，防止其被当作未执行而自动重做。
    /// 调用方应通过原 idempotency key 查询，不应生成新 key 隐藏不确定结果。
    pub fn unknown(
        &mut self,
        idempotency_key: &str,
        reason: impl Into<String>,
    ) -> Result<UiActionRecord, String> {
        self.finish(idempotency_key, UiActionState::Unknown, reason.into())
    }

    /// 把 Accepted record 变为 Rejected 或 Unknown，并重算 record digest。
    ///
    /// 【调用者】公开的 `reject()` / `unknown()` 两个薄包装。
    /// 【输入/输出】原幂等 key、目标终态与原因标签；原因 trim 后不得为空且最多 512 字节。
    /// 【状态规则】Applied 返回原结果；Rejected 与 Unknown 互相转换会返回
    /// `ui_action_terminal_conflict`；Accepted 可进入其中之一。
    /// 【失败】key 未接受、旧 record 无效、reason 越界或终态冲突时返回稳定错误。
    /// 【容易误改】重复调用同一种 Rejected/Unknown 状态时，此纯 reducer 会用新 reason 重算临时 record，
    /// 即使状态未变也可能改 reason；Core facade 看到状态相同后会返回已持久 record、不追加事件，因此
    /// Domain helper 与 Core durable 重放对 reason 更新的处理略有差异。
    /// 【副作用】只修改内存记录，不执行 effect、不写持久日志。
    fn finish(
        &mut self,
        idempotency_key: &str,
        state: UiActionState,
        reason: String,
    ) -> Result<UiActionRecord, String> {
        if reason.trim().is_empty() || reason.len() > 512 {
            return Err("ui_action_reason_invalid".to_owned());
        }
        let record = self
            .records
            .get_mut(idempotency_key)
            .ok_or_else(|| "ui_action_not_accepted".to_owned())?;
        record.validate()?;
        match record.state {
            UiActionState::Applied => return Ok(record.clone()),
            UiActionState::Rejected | UiActionState::Unknown if record.state != state => {
                return Err("ui_action_terminal_conflict".to_owned())
            }
            _ => {}
        }
        record.state = state;
        record.reason = Some(reason);
        record.record_digest = record.digest();
        record.validate()?;
        Ok(record.clone())
    }

    /// 按原幂等键查找记录；该键用于 ACK 丢失后的 reconciliation。
    /// 【边界】此方法仅查询当前内存 journal，不做 caller/owner/scope 授权；Core 对外查询必须另行控制权限。
    pub fn query_original(&self, idempotency_key: &str) -> Option<&UiActionRecord> {
        self.records.get(idempotency_key)
    }

    /// 按 command ID 搜索第一条匹配记录。
    ///
    /// 【边界】唯一索引是 idempotency key，journal 不阻止不同 key 复用 command ID；出现重复时此方法按
    /// BTreeMap key 顺序返回第一条，而不是报告冲突。因此它不能单独提供全局唯一命令查询语义。
    pub fn query_command(&self, command_id: RequestId) -> Option<&UiActionRecord> {
        self.records
            .values()
            .find(|record| record.command_id == command_id)
    }

    /// 只读遍历全部当前内存记录，顺序为 idempotency key 字典序，不是时间/执行顺序。
    pub fn records(&self) -> impl Iterator<Item = &UiActionRecord> {
        self.records.values()
    }
}

//! # 文件职责
//!
//! 本文件定义 AUT-15 的领域值对象：一次 worker attempt 的分发意图（`AutomationDispatchIntent`）
//! 与事后观察结果（`AutomationObservation`），并校验两者能否安全地归并为新的 intent。它只处理
//! 传入数据的结构、摘要和身份绑定，不启动 worker、不派发 action、不保存事件，也不读取当前权限。
//!
//! # 在系统中的位置与当前范围
//!
//! ```text
//! 上游 adapter / 调度器（如已存在）构造 intent 与 observation
//!   -> validate_automation_observation()
//!   -> 成功：返回新状态和 observation digest，交给调用方决定如何记录/使用
//!   -> 失败：返回静态错误码，不修改原输入
//! ```
//!
//! Core 的 `observe_automation_dispatch()` 只是调用此处的校验器。当前 `CURRENT_STATUS.md` 将
//! AUT-15 记录为 source-level、partial 合约：worker claim、持久化、重启恢复和真实执行不由本文件
//! 实现。不要把这些值对象或校验函数的存在当成 worker 分发已经接入。
//!
//! `deny_unknown_fields` 是 serde 的反序列化约束：结构化输入带有本版本未登记的键时拒绝，避免
//! 未知数据悄悄进入已校验的契约；`rename_all = "snake_case"` 规定 enum 在线路 JSON 中的名字。

use crate::json_digest;
use serde::{Deserialize, Serialize};

/// AUT-15 intent 值对象使用的稳定 schema 名称。
///
/// 反序列化或验证时必须精确匹配；它本身不表示当前系统已有生产者写入这种对象。
pub const AUTOMATION_DISPATCH_SCHEMA: &str = "kiana.automation-dispatch-intent.v1";

/// 分发意图在 worker 处理过程中的已记录阶段。
///
/// 这些枚举值描述的是调用方保存的状态，不会由本 enum 自动执行状态转换；合法观测转换只在
/// `validate_automation_observation()` 中构造。当前没有独立 `Failed` 状态：已知成功和失败都归为
/// `Observed`，结果是否成功仍由 observation 的 `outcome` 字段保留。
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AutomationDispatchState {
    /// 意图已准备，但尚未绑定具体 worker。
    Prepared,
    /// 调度器已认领意图；后续 observation 必须来自此处绑定的 worker ID。
    Claimed,
    /// worker 已给出可判定的成功或失败结果。
    Observed,
    /// 结果无法确认；不能将未知效果推断为成功或无副作用，应交由恢复/对账流程处理。
    Unknown,
}

/// 对一次自动化 worker attempt 的不可变身份与状态摘要。
///
/// 该对象不包含待执行 action 的明文；`action_digest` 只引用 action 摘要。`intent_digest` 是对
/// 下列字段计算的规范 JSON SHA-256 指纹，用于发现对象字段变化，但不是签名，不能单独证明是谁
/// 创建了对象。实际内容是否可信仍取决于产生、持久化和读取此值的上游边界。
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AutomationDispatchIntent {
    /// 必须精确等于 `AUTOMATION_DISPATCH_SCHEMA`，用于区分契约版本。
    pub schema: String,
    /// 此次外部执行的身份字符串；本文件只检查文本形状，不解析或查找真实 execution 记录。
    pub execution_id: String,
    /// 预算/副作用预留的身份字符串；本文件只检查它与 observation 完全一致。
    pub reservation_id: String,
    /// 被认领 action 的 `sha256:` 摘要；这里只验证格式并在两份快照间比较，不重新生成 action。
    pub action_digest: String,
    /// worker 身份：Prepared 状态不能已有 worker；Claimed 状态必须绑定 worker。
    pub worker_id: Option<String>,
    /// attempt 序号；验证要求大于 0，但不在这里检查它是否与历史尝试数连续。
    pub attempt: u32,
    /// 产生此 intent 时绑定的授权 epoch；要求非零，并且观测必须带相同值。
    /// 本对象没有当前 authority provider，因此不能发现 epoch 在外部已经更新。
    pub authority_epoch: u64,
    /// 当前分发阶段。修改此字段后必须重新计算 `intent_digest`。
    pub state: AutomationDispatchState,
    /// worker 结果摘要；Prepared/Claimed 阶段可以没有，Observed/Unknown 阶段必须有。
    pub observation_digest: Option<String>,
    /// 对 intent 其他字段的 SHA-256 指纹；不是认证签名或权限凭证。
    pub intent_digest: String,
}

impl AutomationDispatchIntent {
    /// 检查 intent 的版本、文本/摘要格式、字段组合和自身指纹。
    ///
    /// 【调用者】`validate_automation_observation()` 在比较两个快照前调用；生产者也可在构造后
    /// 单独调用。此方法不读取任何其他服务或事件流。
    /// 【输入/输出】借用当前 intent；合法时返回 `Ok(())`，否则返回稳定静态错误码。
    /// 【校验点】基础字段必须有界且非空，attempt 与 epoch 非零，摘要格式正确，intent digest 必须
    /// 与当前字段重算结果相同；Prepared 不允许绑定 worker，Claimed 必须绑定 worker，Observed/Unknown
    /// 必须有 observation digest。
    /// 【副作用】无；不会归一化字段、更新摘要或修补无效对象。
    pub fn validate(&self) -> Result<(), &'static str> {
        if self.schema != AUTOMATION_DISPATCH_SCHEMA
            || !valid_text(&self.execution_id)
            || !valid_text(&self.reservation_id)
            || !valid_digest(&self.action_digest)
            || self.worker_id.as_deref().is_some_and(|v| !valid_text(v))
            || self.attempt == 0
            || self.authority_epoch == 0
            || self
                .observation_digest
                .as_deref()
                .is_some_and(|v| !valid_digest(v))
            || !valid_digest(&self.intent_digest)
            || self.intent_digest != self.digest()
        {
            return Err("automation_dispatch_intent_invalid");
        }
        match self.state {
            AutomationDispatchState::Prepared if self.worker_id.is_some() => {
                Err("automation_dispatch_prepared_worker_invalid")
            }
            AutomationDispatchState::Claimed if self.worker_id.is_none() => {
                Err("automation_dispatch_claim_worker_required")
            }
            AutomationDispatchState::Observed | AutomationDispatchState::Unknown
                if self.observation_digest.is_none() =>
            {
                Err("automation_dispatch_observation_required")
            }
            _ => Ok(()),
        }
    }

    /// 按固定字段集合重算 intent 指纹。
    ///
    /// 使用 Domain 的 `json_digest()`，它先按对象键排序生成规范 JSON，再计算 SHA-256，因此键的
    /// 插入顺序不会改变摘要。此私有方法仅供 `validate()` 与本模块状态转换更新摘要使用；摘要
    /// 不含密钥，不能代替来源认证。
    fn digest(&self) -> String {
        json_digest(&serde_json::json!({
            "schema": self.schema,
            "execution_id": self.execution_id,
            "reservation_id": self.reservation_id,
            "action_digest": self.action_digest,
            "worker_id": self.worker_id,
            "attempt": self.attempt,
            "authority_epoch": self.authority_epoch,
            "state": self.state,
            "observation_digest": self.observation_digest,
        }))
    }
}

/// 调度器或 worker 提交给领域层的单次执行观察值。
///
/// 它把观察结果与 intent 使用相同的 execution/reservation/action/worker/epoch 身份绑定。这里
/// 没有原始 action、响应正文、签名或存储游标；`result_digest` 只标识结果内容，不能证明内容来源。
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AutomationObservation {
    /// 观察结果声称对应的 execution 身份，必须与 intent 完全相同。
    pub execution_id: String,
    /// 观察结果声称消耗的 reservation 身份，必须与 intent 完全相同。
    pub reservation_id: String,
    /// 实际执行 action 的摘要，必须与 intent 中被冻结的摘要相同。
    pub action_digest: String,
    /// 提交此 observation 的 worker 标识，必须与 Claim 时记录的 worker 相同。
    pub worker_id: String,
    /// worker 执行时声称的授权 epoch，必须与 intent 的 epoch 相同；这里不查询当前 epoch。
    pub authority_epoch: u64,
    /// 外部调用方报告的 outcome；Unknown 表示无法确认结果，不可被折算成成功或失败。
    pub outcome: AutomationObservationOutcome,
    /// 结果内容的 `sha256:` 摘要；本对象不含摘要对应的明文结果。
    pub result_digest: String,
}

/// observation 对执行结果确定程度的分类。
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AutomationObservationOutcome {
    /// 外部调用方报告执行成功。
    Succeeded,
    /// 外部调用方报告执行失败；仍属于已观察到的确定结果。
    Failed,
    /// 无法确认 action 是否生效；需要上层保留未知状态并执行恢复/对账。
    Unknown,
}

impl AutomationObservation {
    /// 校验 observation 的字段形状，不验证其来源或真实性。
    ///
    /// execution/reservation/worker 文本必须非空、无空白且不超过 512 UTF-8 字节；epoch 非零；
    /// action 和 result 摘要必须是本模块接受的 `sha256:` 格式。绑定 intent 的逐字段比较由外层
    /// `validate_automation_observation()` 完成。
    pub fn validate(&self) -> Result<(), &'static str> {
        if !valid_text(&self.execution_id)
            || !valid_text(&self.reservation_id)
            || !valid_digest(&self.action_digest)
            || !valid_text(&self.worker_id)
            || self.authority_epoch == 0
            || !valid_digest(&self.result_digest)
        {
            return Err("automation_observation_invalid");
        }
        Ok(())
    }
}

/// 把与 Claimed intent 匹配的 observation 折叠成一个新的 intent 值。
///
/// 【调用者】Core 的只读 facade 会转调本函数；目前仓库没有生产代码调用这个 helper，领域契约
/// 测试直接覆盖其成功、Unknown 和身份不匹配行为。
/// 【输入】intent 与 observation 均以不可变引用传入；调用方需自行取得并信任两份快照。本函数
/// 不检查当前权限、时钟、worker 存活状态或 action 副作用。
/// 【核心流程】分别验证两个对象 → 确认 intent 状态为 Claimed → 比较 worker、execution、reservation、
/// action digest 和 authority epoch → 克隆 intent → 按 outcome 更新 state → 保存 result digest 并重算
/// intent digest → 再次验证新对象。
/// 【输出】成功返回新 intent，原输入保持不变。Succeeded 和 Failed 都变为 Observed；Unknown 保持
/// Unknown。没有单独的失败终态，所以调用方需要结合 observation/result 语义辨别已知失败。
/// 【失败】自身字段无效时返回各对象的校验错误；状态或身份绑定不匹配时返回
/// `automation_observation_binding_drift`。没有重试、持久化、事件记录或副作用补偿。
///
/// ⚠ 这是值对象转换，不是执行证明：由于没有任何 I/O 或签名验证，`Ok(next)` 只表示输入数据在
/// 此处声明的一致性约束下相互匹配，不代表 worker 真的执行了 action。
pub fn validate_automation_observation(
    intent: &AutomationDispatchIntent,
    observation: &AutomationObservation,
) -> Result<AutomationDispatchIntent, &'static str> {
    intent.validate()?;
    observation.validate()?;
    if intent.state != AutomationDispatchState::Claimed
        || intent.worker_id.as_deref() != Some(observation.worker_id.as_str())
        || intent.execution_id != observation.execution_id
        || intent.reservation_id != observation.reservation_id
        || intent.action_digest != observation.action_digest
        || intent.authority_epoch != observation.authority_epoch
    {
        return Err("automation_observation_binding_drift");
    }
    let mut next = intent.clone();
    next.state = match observation.outcome {
        AutomationObservationOutcome::Succeeded | AutomationObservationOutcome::Failed => {
            AutomationDispatchState::Observed
        }
        AutomationObservationOutcome::Unknown => AutomationDispatchState::Unknown,
    };
    next.observation_digest = Some(observation.result_digest.clone());
    next.intent_digest = next.digest();
    next.validate()?;
    Ok(next)
}

/// 验证本模块中作为 ID 使用的自由文本字段。
///
/// 规则是非空、最多 512 UTF-8 字节、不含 NUL/换行/回车和任何 Unicode 空白；它不检查 UUID、
/// 前缀、数据库存在性或调用者权限。因此不要把通过这里的字符串当作解析过的领域 ID。
fn valid_text(value: &str) -> bool {
    !value.trim().is_empty()
        && value.len() <= 512
        && !value.contains(['\0', '\n', '\r'])
        && !value.chars().any(char::is_whitespace)
}

/// 检查 SHA-256 摘要的外部字符串格式。
///
/// 要求精确的小写算法前缀 `sha256:` 加 64 个 ASCII 十六进制字符；十六进制部分接受大小写。
/// 这里只校验长度和字符集，不计算输入内容摘要，也不验证摘要对应的内容。
fn valid_digest(value: &str) -> bool {
    value
        .strip_prefix("sha256:")
        .is_some_and(|hex| hex.len() == 64 && hex.bytes().all(|byte| byte.is_ascii_hexdigit()))
}

//! # 文件职责
//!
//! 本文件定义 AUT-15 的领域值对象：一次 worker attempt 的分发意图（`AutomationDispatchIntent`）、
//! 之后观察到的结果（`AutomationObservation`/`AutomationDispatchResult`），以及把它们组织成
//! **可重取命令**的 `AutomationDispatchCommand`。它只处理传入数据的结构、摘要、版本和身份绑定，
//! 不启动 worker、不派发 action、不保存事件，也不读取当前权限。
//!
//! # 在系统中的位置与当前范围
//!
//! ```text
//! prepare_automation_dispatch_command()   // Prepared intent -> revision 1 命令
//!   -> 由 store 以 CAS 提交（幂等键重放返回当前权威副本）
//!   -> claim_automation_dispatch_command() // 绑定唯一 worker + attempt，revision +1
//!   -> 注入的 runner 恰好执行一次（由 Core 协调器负责，不在本文件）
//!   -> automation_dispatch_result()        // 用服务器身份 + runner outcome 造结构化结果
//!   -> observe_automation_dispatch_command() // 终态 Observed/Unknown，revision +1
//!   -> 任意时刻可用 command_id / idempotency_key 重取，不重新执行 effect
//! ```
//!
//! 本文件仍只是纯值转换：revision 与状态机规则在这里，事实提交与并发唯一性由 `kiana-ports`
//! 的 `AutomationDispatchStore` 适配器负责，真实执行由 `kiana-core`/`kiana-daemon` 的协调器负责。
//! 不要把这些值对象或转移函数的存在当成 worker 分发已经接入生产路径。
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

// ============================================================================
// AUT-15：可重取 dispatch command（替代 inline dispatch 的持久单位）
// ============================================================================

/// AUT-15 dispatch command 使用的固定序列化 schema 名称。
pub const AUTOMATION_DISPATCH_COMMAND_SCHEMA: &str = "kiana.automation-dispatch-command.v1";

/// worker runner 被允许声明的执行结果摘要。
///
/// 注入的 effect runner 只能通过本枚举声明“成功 / 已知失败 / 未知”和内容摘要；execution、
/// reservation、worker、attempt 与 authority epoch 都由领域层从已提交的 command 复制，runner
/// 无法自报身份。`Unknown` 表示无法确认动作是否生效，上层必须保留 Unknown 并对账，不能折算成
/// 成功或失败。本枚举只是数据，不执行动作、不检查授权。
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AutomationWorkerEffectOutcome {
    /// runner 报告动作成功完成；`result_digest` 是结果内容的 `sha256:` 摘要。
    Succeeded {
        /// 结果内容的 `sha256:` 摘要；不含明文结果。
        result_digest: String,
    },
    /// runner 报告了可判定的失败；失败码与明细摘要必须结构化给出。
    Failed {
        /// 有界的稳定失败码；用于把“已知失败”与“结果未知”分开。
        failure_code: String,
        /// 失败明细的 `sha256:` 摘要；不含明文。
        detail_digest: String,
        /// runner 声明的可重试性；本模块不据此自动重试。
        retryable: bool,
        /// 结果内容的 `sha256:` 摘要；不含明文结果。
        result_digest: String,
    },
    /// runner 无法确认结果；必须保留为 Unknown 并进入对账。
    Unknown {
        /// 最后一次尝试内容的 `sha256:` 摘要；不代表动作已生效。
        result_digest: String,
    },
}

/// 结构化失败结果：把“已知失败”与“结果未知”区分开。
///
/// 只有 `AutomationObservationOutcome::Failed` 的观察必须携带本对象；`Succeeded`/`Unknown`
/// 携带它会被拒绝（`automation_dispatch_failure_unexpected`），因此无法用一次性未知结果伪造确定失败。
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AutomationDispatchFailure {
    /// 有界的稳定失败码；本模块只检查文本形状。
    pub code: String,
    /// 失败明细的 `sha256:` 摘要；不证明明细来源或内容。
    pub detail_digest: String,
    /// runner 声明的可重试性；本模块不据此自动重试，也不判断幂等性。
    pub retryable: bool,
}

impl AutomationDispatchFailure {
    /// 构造并校验一个结构化失败结果；形状不合法时返回静态错误码。
    pub fn new(
        code: impl Into<String>,
        detail_digest: impl Into<String>,
        retryable: bool,
    ) -> Result<Self, &'static str> {
        let failure = Self {
            code: code.into(),
            detail_digest: detail_digest.into(),
            retryable,
        };
        failure.validate()?;
        Ok(failure)
    }

    /// 检查失败码非空有界、明细摘要格式正确；不解析失败码语义。
    pub fn validate(&self) -> Result<(), &'static str> {
        if !valid_text(&self.code) || !valid_digest(&self.detail_digest) {
            return Err("automation_dispatch_failure_invalid");
        }
        Ok(())
    }
}

/// 一次 worker 观察的结构化结果：确定性观察 + 可选结构化失败。
///
/// 这是“已知失败反馈结构化结果”的载体：`Failed` 必须携带 `AutomationDispatchFailure`；
/// `Succeeded`/`Unknown` 不允许携带失败对象。结果本身仍是调用方声明，不证明外部动作真的发生。
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AutomationDispatchResult {
    /// 与 intent 同身份绑定的观察值；其 `result_digest` 是终态命令记录的观察摘要。
    pub observation: AutomationObservation,
    /// 已知失败的结构化结果；仅 `Failed` 观察必须存在。
    pub failure: Option<AutomationDispatchFailure>,
}

impl AutomationDispatchResult {
    /// 校验观察形状与“失败必须结构化、成功/未知不得夹带失败”的规则。
    ///
    /// 越权与绑定比较（execution/reservation/action/worker/epoch）由
    /// `observe_automation_dispatch_command()` 在折叠时完成；这里只验证单份结果自洽。
    pub fn validate(&self) -> Result<(), &'static str> {
        self.observation.validate()?;
        match self.observation.outcome {
            AutomationObservationOutcome::Failed => {
                let failure = self
                    .failure
                    .as_ref()
                    .ok_or("automation_dispatch_failure_required")?;
                failure.validate()?;
            }
            AutomationObservationOutcome::Succeeded | AutomationObservationOutcome::Unknown => {
                if self.failure.is_some() {
                    return Err("automation_dispatch_failure_unexpected");
                }
            }
        }
        Ok(())
    }
}

/// 可重取的 worker dispatch command：替代 inline dispatch 的持久单位。
///
/// 【为什么存在】旧路径在同一个 handler 内“提交事实后立刻执行 effect”，一旦进程在提交后崩溃，
/// 调用方无法区分“尚未派发”和“已派发”，只能重新派生。本对象把派发意图变成一条带幂等键、
/// 单调 revision 和自校验摘要的事实：调用方按 `command_id`/`idempotency_key` 重取同一份命令，
/// 由状态与 worker 绑定决定是否还能执行 effect。
///
/// 【状态与字段一致性】`intent.state` 为 `Prepared`/`Claimed` 时不得携带结果；终态
/// （`Observed`/`Unknown`）必须携带结果，且结果的观察摘要必须等于 `intent.observation_digest`，
/// 观察 outcome 必须与状态对应（`Unknown` 状态只能是未知结果），否则拒绝。
///
/// 【证明边界】`command_digest` 是无密钥内容指纹，不是签名或授权凭证；本对象不执行 effect、
/// 不写 EventStore、不检查 worker 是否存活，也不判断命令是否仍被当前授权允许。
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AutomationDispatchCommand {
    /// 必须精确等于 `AUTOMATION_DISPATCH_COMMAND_SCHEMA`。
    pub schema: String,
    /// 命令身份；重取与 CAS 提交都以它为准。
    pub command_id: String,
    /// 幂等键；重复提交同一键且摘要相同视为重放，不新建事实。
    pub idempotency_key: String,
    /// 命令在 execution 流上的单调 revision；从 1 开始，每次状态转移 +1。
    pub revision: u64,
    /// 被冻结的 dispatch intent；execution/worker/attempt/epoch 身份都在这里。
    pub intent: AutomationDispatchIntent,
    /// 终态时的结构化结果；Prepared/Claimed 阶段必须为空。
    pub result: Option<AutomationDispatchResult>,
    /// 对上方全部字段的 SHA-256 指纹；不是认证签名。
    pub command_digest: String,
}

impl AutomationDispatchCommand {
    /// 检查 schema、身份文本、revision、intent/结果一致性与自身摘要。
    ///
    /// 【调用者】`prepare_automation_dispatch_command()`、claim/observe 转移函数、store 与 Core
    /// 协调器在读写事实前调用。
    /// 【校验点】schema 精确匹配；command/idempotency 文本有效；revision 非零；intent 自身有效；
    /// Prepared/Claimed 无结果，Observed/Unknown 必须有结果且与 intent 的观察摘要一致；
    /// 状态与观察 outcome 对应；`command_digest` 与重算结果相同。
    /// 【副作用】无；不修补字段，不读取外部状态。
    pub fn validate(&self) -> Result<(), &'static str> {
        if self.schema != AUTOMATION_DISPATCH_COMMAND_SCHEMA
            || !valid_text(&self.command_id)
            || !valid_text(&self.idempotency_key)
            || self.revision == 0
            || !valid_digest(&self.command_digest)
            || self.command_digest != self.digest()
        {
            return Err("automation_dispatch_command_invalid");
        }
        self.intent.validate()?;
        match self.intent.state {
            AutomationDispatchState::Prepared | AutomationDispatchState::Claimed => {
                if self.result.is_some() {
                    return Err("automation_dispatch_command_result_unexpected");
                }
            }
            AutomationDispatchState::Observed | AutomationDispatchState::Unknown => {
                let result = self
                    .result
                    .as_ref()
                    .ok_or("automation_dispatch_command_result_required")?;
                result.validate()?;
                if self.intent.observation_digest.as_deref()
                    != Some(result.observation.result_digest.as_str())
                {
                    return Err("automation_dispatch_command_result_drift");
                }
                // Observed 只能记录确定结果，Unknown 只能记录未知结果；否则终态会被伪造的成功覆盖。
                match (self.intent.state, result.observation.outcome) {
                    (AutomationDispatchState::Observed, AutomationObservationOutcome::Unknown) => {
                        return Err("automation_dispatch_command_state_outcome_drift")
                    }
                    (AutomationDispatchState::Unknown, AutomationObservationOutcome::Unknown) => {}
                    (AutomationDispatchState::Unknown, _) => {
                        return Err("automation_dispatch_command_state_outcome_drift")
                    }
                    _ => {}
                }
            }
        }
        Ok(())
    }

    /// 当前命令状态；只是 `intent.state` 的只读投影。
    pub fn state(&self) -> AutomationDispatchState {
        self.intent.state
    }

    /// 终态（`Observed`/`Unknown`）判定；Prepared/Claimed 返回 false。
    pub fn is_terminal(&self) -> bool {
        matches!(
            self.intent.state,
            AutomationDispatchState::Observed | AutomationDispatchState::Unknown
        )
    }

    /// 按固定字段集合重算命令指纹。
    ///
    /// 绑定 `intent.intent_digest` 而不是逐字段展开 intent：intent 自身的摘要已经覆盖其全部字段，
    /// 这里再绑定状态与结构化结果摘要，使任何一个可改字段变化都会改变命令摘要。摘要不是签名。
    fn digest(&self) -> String {
        let (outcome, result_digest, failure_code, failure_detail_digest, failure_retryable) =
            match &self.result {
                Some(result) => (
                    Some(result.observation.outcome),
                    Some(result.observation.result_digest.as_str()),
                    result.failure.as_ref().map(|failure| failure.code.as_str()),
                    result
                        .failure
                        .as_ref()
                        .map(|failure| failure.detail_digest.as_str()),
                    result.failure.as_ref().map(|failure| failure.retryable),
                ),
                None => (None, None, None, None, None),
            };
        json_digest(&serde_json::json!({
            "schema": self.schema,
            "command_id": self.command_id,
            "idempotency_key": self.idempotency_key,
            "revision": self.revision,
            "intent_digest": self.intent.intent_digest,
            "intent_state": self.intent.state,
            "worker_id": self.intent.worker_id,
            "attempt": self.intent.attempt,
            "observation_digest": self.intent.observation_digest,
            "outcome": outcome,
            "result_digest": result_digest,
            "failure_code": failure_code,
            "failure_detail_digest": failure_detail_digest,
            "failure_retryable": failure_retryable,
        }))
    }
}

/// 从一个尚未派发的 Prepared intent 构造可重取命令（revision = 1）。
///
/// 【输入/输出】`command_id`/`idempotency_key` 由调用方提供并校验文本形状；intent 必须是
/// `Prepared` 且自身有效，否则返回 `automation_dispatch_command_intent_not_prepared` 或
/// intent 自身的错误码。成功返回结果字段为空、命令摘要已计算的新命令。
/// 【副作用】无；不提交事实、不选择 worker、不执行 effect。
pub fn prepare_automation_dispatch_command(
    command_id: impl Into<String>,
    idempotency_key: impl Into<String>,
    intent: &AutomationDispatchIntent,
) -> Result<AutomationDispatchCommand, &'static str> {
    intent.validate()?;
    if intent.state != AutomationDispatchState::Prepared {
        return Err("automation_dispatch_command_intent_not_prepared");
    }
    let mut command = AutomationDispatchCommand {
        schema: AUTOMATION_DISPATCH_COMMAND_SCHEMA.to_owned(),
        command_id: command_id.into(),
        idempotency_key: idempotency_key.into(),
        revision: 1,
        intent: intent.clone(),
        result: None,
        command_digest: String::new(),
    };
    command.command_digest = command.digest();
    command.validate()?;
    Ok(command)
}

/// 把已提交命令绑定到唯一 attempt 的纯转移结果。
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum AutomationDispatchClaim {
    /// 本次调用取得 attempt；调用方可以执行一次 effect。
    Claimed(AutomationDispatchCommand),
    /// 同一 worker + attempt 已经占过该命令；只能重取，不得再次执行 effect。
    AlreadyClaimed(AutomationDispatchCommand),
    /// 命令已经 Observed/Unknown；只能观察/对账，不得执行 effect。
    Terminal(AutomationDispatchCommand),
}

/// 把命令绑定到一个 worker + attempt，并返回调用方可以做什么。
///
/// 【核心流程】先校验命令与 worker/attempt；再按状态判定：
/// 1. `Claimed` 且 worker 与 attempt 都相同 → `AlreadyClaimed`（崩溃重启后的安全重取路径，
///    不要求 revision 仍然匹配，因为重取方可能持有过期快照）；
/// 2. `Claimed` 但 worker 或 attempt 不同 → `automation_dispatch_worker_conflict`
///    （重复 worker / 第二次 attempt 一律拒绝）；
/// 3. `Prepared` → 要求 `expected_revision` 等于命令 revision，写回带 worker/attempt 的
///    `Claimed` 命令并把 revision +1，返回 `Claimed`；
/// 4. 终态 → `Terminal`。
/// 【失败】revision 过期返回 `automation_dispatch_revision_conflict`；worker 文本非法或 attempt=0
/// 返回 `automation_dispatch_command_invalid`。
/// 【副作用】无；不写 store、不执行 effect。并发唯一性由 store 的 CAS 负责。
pub fn claim_automation_dispatch_command(
    command: &AutomationDispatchCommand,
    worker_id: &str,
    attempt: u32,
    expected_revision: u64,
) -> Result<AutomationDispatchClaim, &'static str> {
    command.validate()?;
    if !valid_text(worker_id) || attempt == 0 {
        return Err("automation_dispatch_command_invalid");
    }
    match command.intent.state {
        AutomationDispatchState::Claimed => {
            if command.intent.worker_id.as_deref() == Some(worker_id)
                && command.intent.attempt == attempt
            {
                return Ok(AutomationDispatchClaim::AlreadyClaimed(command.clone()));
            }
            Err("automation_dispatch_worker_conflict")
        }
        AutomationDispatchState::Prepared => {
            if expected_revision != command.revision {
                return Err("automation_dispatch_revision_conflict");
            }
            let mut intent = command.intent.clone();
            intent.worker_id = Some(worker_id.to_owned());
            intent.attempt = attempt;
            intent.state = AutomationDispatchState::Claimed;
            intent.observation_digest = None;
            intent.intent_digest = intent.digest();
            let mut next = command.clone();
            next.intent = intent;
            next.revision = next
                .revision
                .checked_add(1)
                .ok_or("automation_dispatch_revision_overflow")?;
            next.command_digest = next.digest();
            next.validate()?;
            Ok(AutomationDispatchClaim::Claimed(next))
        }
        AutomationDispatchState::Observed | AutomationDispatchState::Unknown => {
            Ok(AutomationDispatchClaim::Terminal(command.clone()))
        }
    }
}

/// 把一份结构化结果折叠进已 claim 的命令；返回 `(新命令, 是否重放)`。
///
/// 【核心流程】
/// 1. 命令与结果各自校验；
/// 2. `Claimed`：要求 `expected_revision` 等于命令 revision（否则
///    `automation_dispatch_revision_conflict`），再做 execution/reservation/action/worker/epoch
///    绑定比较（Domain `validate_automation_observation()`），成功后写入结果、把 state 置为
///    `Observed`（成功或已知失败）或 `Unknown`（未知结果），revision +1，返回 `(新命令, false)`；
/// 3. 终态：只有**完全相同**的结果才是安全重放 `(原命令, true)`；不同结果的陈旧提交返回
///    `automation_dispatch_revision_conflict`，不同结果的当前 revision 提交返回
///    `automation_dispatch_observation_conflict`——终态不可能被第二个“成功”覆盖；
/// 4. `Prepared`：返回 `automation_dispatch_observation_not_claimed`。
/// 【副作用】无；不写 store、不重试、不执行 effect。
pub fn observe_automation_dispatch_command(
    command: &AutomationDispatchCommand,
    result: &AutomationDispatchResult,
    expected_revision: u64,
) -> Result<(AutomationDispatchCommand, bool), &'static str> {
    command.validate()?;
    result.validate()?;
    match command.intent.state {
        AutomationDispatchState::Prepared => Err("automation_dispatch_observation_not_claimed"),
        AutomationDispatchState::Claimed => {
            if expected_revision != command.revision {
                return Err("automation_dispatch_revision_conflict");
            }
            let mut intent = validate_automation_observation(&command.intent, &result.observation)?;
            intent.state = match result.observation.outcome {
                AutomationObservationOutcome::Succeeded | AutomationObservationOutcome::Failed => {
                    AutomationDispatchState::Observed
                }
                AutomationObservationOutcome::Unknown => AutomationDispatchState::Unknown,
            };
            intent.intent_digest = intent.digest();
            let mut next = command.clone();
            next.intent = intent;
            next.result = Some(result.clone());
            next.revision = next
                .revision
                .checked_add(1)
                .ok_or("automation_dispatch_revision_overflow")?;
            next.command_digest = next.digest();
            next.validate()?;
            Ok((next, false))
        }
        AutomationDispatchState::Observed | AutomationDispatchState::Unknown => {
            if command.result.as_ref() == Some(result) {
                return Ok((command.clone(), true));
            }
            if expected_revision != command.revision {
                return Err("automation_dispatch_revision_conflict");
            }
            Err("automation_dispatch_observation_conflict")
        }
    }
}

/// CAS 提交结果：`replayed=true` 表示没有新建事实，`command` 是当前权威副本。
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AutomationDispatchCommit {
    /// 提交后（或重放命中的）当前命令副本。
    pub command: AutomationDispatchCommand,
    /// 是否命中幂等键并重放了既有事实。
    pub replayed: bool,
}

/// 唯一 attempt 占位请求；`expected_revision` 是调用方看到的命令 revision。
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AutomationDispatchClaimRequest {
    /// 要占位的命令 ID。
    pub command_id: String,
    /// 申请执行的 worker 身份。
    pub worker_id: String,
    /// 该 worker 的 attempt 序号；必须大于 0。
    pub attempt: u32,
    /// 调用方认为当前的命令 revision；过期即拒绝。
    pub expected_revision: u64,
}

/// claim 结果：`claimed=false` 表示本次调用没有取得 attempt（已有 owner 或已终态）。
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AutomationDispatchClaimCommit {
    /// 当前命令副本；`claimed=true` 时是刚写入的 Claimed 命令。
    pub command: AutomationDispatchCommand,
    /// 本次调用是否取得 attempt。
    pub claimed: bool,
}

/// 观察提交请求。
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AutomationDispatchObservationRequest {
    /// 目标命令 ID。
    pub command_id: String,
    /// 调用方认为当前的命令 revision；终态重放可以携带过期值。
    pub expected_revision: u64,
    /// 结构化观察结果。
    pub result: AutomationDispatchResult,
}

/// 观察提交结果：`replayed=true` 表示同一结果已经记录过，没有新建事实。
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AutomationDispatchObservationCommit {
    /// 当前命令副本。
    pub command: AutomationDispatchCommand,
    /// 是否重放了既有观察。
    pub replayed: bool,
}

/// 从已 claim 的 command 与 runner 结果构造结构化观察结果。
///
/// 【作用】把 worker 身份、execution/reservation/action 与 authority epoch **从命令复制**，
/// 使 runner 只能提供 outcome、失败码与摘要；因此 runner 无法伪造身份或把它人的尝试算到自己名下。
/// 【输入/输出】命令必须处于 `Claimed`；成功返回通过 `AutomationDispatchResult::validate()`
/// 的结果。未知 outcome 不携带失败对象，已知失败必须给出结构化失败码。
/// 【失败】命令不是 Claimed、缺少 worker、摘要或失败码形状非法时返回静态错误码。
/// 【副作用】无；不执行 effect、不写事实、不判断结果真伪。
pub fn automation_dispatch_result(
    command: &AutomationDispatchCommand,
    outcome: AutomationWorkerEffectOutcome,
) -> Result<AutomationDispatchResult, &'static str> {
    command.validate()?;
    if command.intent.state != AutomationDispatchState::Claimed {
        return Err("automation_dispatch_observation_not_claimed");
    }
    let worker_id = command
        .intent
        .worker_id
        .clone()
        .ok_or("automation_dispatch_claim_worker_required")?;
    let (observation_outcome, result_digest, failure) = match outcome {
        AutomationWorkerEffectOutcome::Succeeded { result_digest } => {
            (AutomationObservationOutcome::Succeeded, result_digest, None)
        }
        AutomationWorkerEffectOutcome::Failed {
            failure_code,
            detail_digest,
            retryable,
            result_digest,
        } => (
            AutomationObservationOutcome::Failed,
            result_digest,
            Some(AutomationDispatchFailure::new(
                failure_code,
                detail_digest,
                retryable,
            )?),
        ),
        AutomationWorkerEffectOutcome::Unknown { result_digest } => {
            (AutomationObservationOutcome::Unknown, result_digest, None)
        }
    };
    let result = AutomationDispatchResult {
        observation: AutomationObservation {
            execution_id: command.intent.execution_id.clone(),
            reservation_id: command.intent.reservation_id.clone(),
            action_digest: command.intent.action_digest.clone(),
            worker_id,
            authority_epoch: command.intent.authority_epoch,
            outcome: observation_outcome,
            result_digest,
        },
        failure,
    };
    result.validate()?;
    Ok(result)
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

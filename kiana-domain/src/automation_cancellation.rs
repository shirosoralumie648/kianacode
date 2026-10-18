//! 本文件定义 AUT-17 取消代次、stop report、迟到结果 fencing 与 result_unknown 的领域契约。
//!
//! # 在系统中的位置
//!
//! ```text
//! AutomationDispatchCommand（AUT-15 已提交命令，queued/claimed/observed/unknown）
//!   + AutomationCancelRequest（一次取消代次意图：generation + authority epoch + reason）
//!   + 可选 AutomationCancellationFact（同一 execution/attempt 已记录的取消事实）
//!     -> begin_automation_cancellation()  决定“要不要真的请求 stop”
//!     -> AutomationStopReport（worker/provider 报告的停止结果 + evidence digest）
//!     -> fold_automation_stop_report()    折叠成 Stopped / NotExecuted / ResultUnknown
//!     -> fence_automation_late_result()   把迟到结果隔离成 Fenced，绝不复活执行
//! ```
//!
//! 本文件只做纯值转换与结构校验：不查当前授权、不比较真实 generation 存储、不请求 stop、不拦截
//! 迟到结果、不写事件。事实的 CAS 提交由 `kiana-ports` 的 `AutomationCancellationStore` 适配器
//! 负责，真实停止请求由 `kiana-ports` 的 `AutomationWorkerStopController` 适配器负责，编排分别由
//! `kiana-core` 协调器与 `kiana-daemon` 服务负责。
//!
//! # 状态矩阵（本切片的完整契约）
//!
//! | state | stop_requested | stop_confirmed | effect_started | late_result | stop_evidence |
//! |---|---|---|---|---|---|
//! | `Requested` | false | false | 任意 | false | None |
//! | `Stopping` | true | false | true | false | Some |
//! | `NotExecuted` | 任意 | false | false | false | 与 stop_requested 同步 |
//! | `Stopped` | true | true | true | false | Some |
//! | `ResultUnknown` | 任意 | false | true | false | 与 stop_requested 同步 |
//! | `Fenced` | 任意 | 任意 | 任意 | true | 任意 |
//!
//! 由这张表可以推出的拒绝语义：`Stopped` 必须带已确认的 stop（因此“stop 未确认却标 cancelled”
//! 在结构上不可能通过）；`NotExecuted` 必须证明 effect 尚未开始；`ResultUnknown` 必须保留“effect
//! 已开始但结果不可确认”的不确定性；`late_result` 只允许出现在 `Fenced`，所以迟到结果不能把取消
//! 事实改回任何“活着的”状态。
//!
//! ⚠ 这些字段都是**调用方声明**：`validate()` 的 `Ok` 只说明这条 fact/plan 自洽，不证明外部动作
//! 真的停止或真的没发生。不要把本文件的字段校验误当成 worker 已被 fence 或 effect 已被对账。

use crate::automation_dispatch::{AutomationDispatchCommand, AutomationDispatchState};
use crate::json_digest;
use serde::{Deserialize, Serialize};

/// 取消事实 DTO 的固定 wire schema 名称。
pub const AUTOMATION_CANCEL_SCHEMA: &str = "kiana.automation-cancel.v1";

/// 取消请求（一次取消代次意图）的固定 wire schema 名称。
pub const AUTOMATION_CANCEL_REQUEST_SCHEMA: &str = "kiana.automation-cancel-request.v1";

/// stop report（worker/provider 停止报告）的固定 wire schema 名称。
pub const AUTOMATION_STOP_REPORT_SCHEMA: &str = "kiana.automation-stop-report.v1";

/// 迟到结果隔离请求的固定 wire schema 名称。
pub const AUTOMATION_LATE_RESULT_SCHEMA: &str = "kiana.automation-late-result.v1";

/// 一条取消事实声称的阶段；枚举只限制可用字符串，不存储或执行阶段转换。
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AutomationCancelState {
    /// 已记录取消意图，尚未请求底层停止。
    Requested,
    /// 已请求底层停止，但尚无确认事实。
    Stopping,
    /// 已确认底层停止。
    Stopped,
    /// 已确认该次尝试从未开始 effect（queued 未派发，或 worker 报告从未启动）。
    NotExecuted,
    /// effect 已开始，但无法确认结果或停止情况。
    ResultUnknown,
    /// 迟到结果已被认定需要隔离，不能覆盖当前事实。
    Fenced,
}

/// 描述一次执行尝试取消进展和迟到结果隔离关系的事实 DTO。
///
/// `deny_unknown_fields` 会在 Serde 反序列化时拒绝未知字段。结构带 execution/attempt 身份、取消
/// generation、authority epoch、停止请求/确认标记、停止证据摘要和原因；`validate()` 只校验单个
/// 对象中的字段组合，不查询当前 attempt、generation、authority 或事件历史，因此不保证代次单调、
/// owner 有权取消或状态确实按 Requested→Stopping→Stopped 顺序发生。代次单调、状态转移合法性与
/// “迟到结果只能 fence”由本文件的转移函数与 store CAS 联合强制。
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AutomationCancellationFact {
    /// 必须精确匹配 `AUTOMATION_CANCEL_SCHEMA`。
    pub schema: String,
    /// 被取消的 execution 关联 ID；这里不查找真实 worker/运行记录。
    pub execution_id: String,
    /// 被取消的具体 attempt 序号；只要求非零，不验证 attempt 是否实际存在。
    pub attempt: u32,
    /// 取消 generation，用来表示哪一代取消意图；只要求非零，不与当前持久 generation 比较。
    pub cancel_generation: u64,
    /// 取消决定时声称的 authority epoch；只要求非零，不与现行控制面 epoch 比较。
    pub authority_epoch: u64,
    /// 此 fact 声称的取消阶段。
    pub state: AutomationCancelState,
    /// 声称底层外部 effect 是否已经开始；`NotExecuted` 必须为 false，`Stopped`/`Stopping` 必须为 true。
    pub effect_started: bool,
    /// 是否已把 stop 请求送给底层 worker/provider；确认停止前必须先有此标记。
    pub stop_requested: bool,
    /// 是否收到可以作为“已停止”依据的确认；true 时 stop_requested 也必须为 true。
    pub stop_confirmed: bool,
    /// 停止证据摘要；`Stopped` 必须携带，“已请求 stop”的状态必须携带报告证据，未请求时为 None。
    pub stop_evidence_digest: Option<String>,
    /// 是否有结果晚于本次取消代次到达；若为 true，状态必须是 Fenced。
    pub late_result: bool,
    /// 取消原因文本；不能为空，但本 validator 不限定原因类别或来源。
    pub reason: String,
    /// 对本 fact 其他字段的 JSON `sha256:` 摘要；无密钥，不能认证产生者。
    pub fact_digest: String,
}

impl AutomationCancellationFact {
    /// 校验单条取消事实的 schema、非空/非零身份字段、完整状态矩阵和摘要一致性。
    ///
    /// 全局约束：stop_confirmed 必须有 stop_requested；摘要字段形状与自摘要必须匹配；迟到的
    /// `late_result` 只允许出现在 `Fenced`。逐状态约束见文件头状态矩阵：`Requested` 不能已经请求
    /// stop；`Stopping`/`Stopped` 必须证明 effect 已开始；`NotExecuted` 必须证明 effect 未开始且
    /// 未被确认停止；`Stopped` 必须有确认与证据；`ResultUnknown` 不得已被确认停止。
    ///
    /// 无效 DTO 统一返回 `automation_cancellation_fact_invalid`。本函数不写事件、不发 stop、不
    /// 更新 generation，也不阻止 worker 上报 late result；其 `Ok(())` 只表示这条 fact 通过本地检查。
    pub fn validate(&self) -> Result<(), &'static str> {
        if self.schema != AUTOMATION_CANCEL_SCHEMA
            || !valid_text(&self.execution_id)
            || self.attempt == 0
            || self.cancel_generation == 0
            || self.authority_epoch == 0
            || !valid_text(&self.reason)
            || self.stop_confirmed && !self.stop_requested
            || self.late_result && self.state != AutomationCancelState::Fenced
            || self
                .stop_evidence_digest
                .as_deref()
                .is_some_and(|digest| !valid_digest(digest))
            || !valid_digest(&self.fact_digest)
            || self.fact_digest != self.digest()
        {
            return Err("automation_cancellation_fact_invalid");
        }
        let evidence_present = self.stop_evidence_digest.is_some();
        let state_invalid = match self.state {
            AutomationCancelState::Requested => {
                self.stop_requested || self.stop_confirmed || evidence_present
            }
            AutomationCancelState::Stopping => {
                !self.stop_requested
                    || self.stop_confirmed
                    || !self.effect_started
                    || !evidence_present
            }
            AutomationCancelState::NotExecuted => {
                self.effect_started
                    || self.stop_confirmed
                    || evidence_present != self.stop_requested
            }
            AutomationCancelState::Stopped => {
                !self.stop_requested
                    || !self.stop_confirmed
                    || !self.effect_started
                    || !evidence_present
            }
            AutomationCancelState::ResultUnknown => {
                !self.effect_started
                    || self.stop_confirmed
                    || evidence_present != self.stop_requested
            }
            AutomationCancelState::Fenced => !self.late_result,
        };
        if state_invalid {
            return Err("automation_cancellation_fact_invalid");
        }
        Ok(())
    }

    /// 按固定字段集合重算 `fact_digest`；调用方修改任何可改字段后必须调用它。
    ///
    /// 摘要无密钥，字段提供者可以在修改事实后重新计算，不能证明事实来源可信；这里的公开只为让
    /// 构造方与测试能复用同一条 canonical 规则，不代表任何认证语义。
    pub fn recompute_digest(&mut self) {
        self.fact_digest = self.digest();
    }

    /// 是否处于仍需对账/恢复的状态（在途 stop、结果未知或已隔离的迟到结果）。
    ///
    /// `Stopped`/`NotExecuted`/`Requested` 返回 false：前两者是可判定的关闭，`Requested` 只表示
    /// stop 尚未发出，属于在途取消而不是未知结果。
    pub fn recovery_required(&self) -> bool {
        matches!(
            self.state,
            AutomationCancelState::Stopping
                | AutomationCancelState::ResultUnknown
                | AutomationCancelState::Fenced
        )
    }

    /// 对除 fact_digest 外的字段计算内容摘要，避免摘要对自身递归。
    fn digest(&self) -> String {
        json_digest(&serde_json::json!({
            "schema": self.schema,
            "execution_id": self.execution_id,
            "attempt": self.attempt,
            "cancel_generation": self.cancel_generation,
            "authority_epoch": self.authority_epoch,
            "state": self.state,
            "effect_started": self.effect_started,
            "stop_requested": self.stop_requested,
            "stop_confirmed": self.stop_confirmed,
            "stop_evidence_digest": self.stop_evidence_digest,
            "late_result": self.late_result,
            "reason": self.reason,
        }))
    }
}

/// 对外领域入口，仅转调单条事实校验器。
pub fn validate_automation_cancellation(
    fact: &AutomationCancellationFact,
) -> Result<(), &'static str> {
    fact.validate()
}

/// 一次取消代次的请求：绑定 execution/attempt、取消 generation、authority epoch 与原因。
///
/// 请求是**意图**而不是事实：它自身自证摘要，但不代表取消已被记录、stop 已被发出或代次仍是最新。
/// 同一代次重复提交同一请求是安全重放；不同内容同代次是冲突；更低代次是陈旧取消。
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AutomationCancelRequest {
    /// 必须精确匹配 `AUTOMATION_CANCEL_REQUEST_SCHEMA`。
    pub schema: String,
    /// 目标 execution；必须与已提交命令完全一致，否则拒绝。
    pub execution_id: String,
    /// 目标 attempt；必须与已提交命令完全一致，取消不能跨 attempt 生效。
    pub attempt: u32,
    /// 本次取消代次；同一 execution 上必须严格递增，陈旧代次不能推进事实。
    pub cancel_generation: u64,
    /// 发起取消时的 authority epoch；必须与命令记录的 epoch 相同，否则视为陈旧授权。
    pub authority_epoch: u64,
    /// 取消原因文本；不能为空。
    pub reason: String,
    /// 对本请求其他字段的 `sha256:` 摘要；不是签名。
    pub request_digest: String,
}

impl AutomationCancelRequest {
    /// 构造并校验一条取消请求；形状不合法时返回稳定错误码。
    pub fn new(
        execution_id: impl Into<String>,
        attempt: u32,
        cancel_generation: u64,
        authority_epoch: u64,
        reason: impl Into<String>,
    ) -> Result<Self, &'static str> {
        let mut request = Self {
            schema: AUTOMATION_CANCEL_REQUEST_SCHEMA.to_owned(),
            execution_id: execution_id.into(),
            attempt,
            cancel_generation,
            authority_epoch,
            reason: reason.into(),
            request_digest: String::new(),
        };
        request.request_digest = request.digest();
        request.validate()?;
        Ok(request)
    }

    /// 校验 schema、身份字段、非零 generation/epoch、原因与自摘要。
    pub fn validate(&self) -> Result<(), &'static str> {
        if self.schema != AUTOMATION_CANCEL_REQUEST_SCHEMA
            || !valid_text(&self.execution_id)
            || self.attempt == 0
            || self.cancel_generation == 0
            || self.authority_epoch == 0
            || !valid_text(&self.reason)
            || !valid_digest(&self.request_digest)
            || self.request_digest != self.digest()
        {
            return Err("automation_cancel_request_invalid");
        }
        Ok(())
    }

    /// 按固定字段集合重算请求摘要；修改字段后必须调用。
    pub fn recompute_digest(&mut self) {
        self.request_digest = self.digest();
    }

    fn digest(&self) -> String {
        json_digest(&serde_json::json!({
            "schema": self.schema,
            "execution_id": self.execution_id,
            "attempt": self.attempt,
            "cancel_generation": self.cancel_generation,
            "authority_epoch": self.authority_epoch,
            "reason": self.reason,
        }))
    }
}

/// worker/provider 报告的停止结果分类。
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AutomationStopState {
    /// 报告该 attempt 从未开始 effect；必须与 `effect_started = false` 一致。
    NotStarted,
    /// 报告停止已请求但尚未完成；必须证明 effect 已开始。
    Stopping,
    /// 报告已确认停止；必须证明 effect 已开始。
    Stopped,
    /// 无法确认停止情况；保留未知，不得折算成成功或“未执行”。
    Unknown,
}

/// worker/provider 对一次取消的停止报告；必须携带 evidence digest 才能参与折叠。
///
/// 报告只是调用方声明：本文件不验证 evidence 内容、不联系 worker，也不判断报告是否真实。折叠时
/// 只有 `Stopped` 会得到 `stop_confirmed = true`，其余一律保留不确定，因此“stop 未确认却标
/// cancelled”在契约层就被拒绝。
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AutomationStopReport {
    /// 必须精确匹配 `AUTOMATION_STOP_REPORT_SCHEMA`。
    pub schema: String,
    /// 报告针对的 execution；必须与命令/请求一致。
    pub execution_id: String,
    /// 报告针对的 attempt；必须与命令/请求一致。
    pub attempt: u32,
    /// 报告针对的取消代次；必须与请求一致，避免旧报告推进新代次。
    pub cancel_generation: u64,
    /// 报告时的 authority epoch；必须与命令/请求一致。
    pub authority_epoch: u64,
    /// 停止结果分类。
    pub state: AutomationStopState,
    /// 报告方声称 effect 是否已经开始；与 state 的组合同上表，不一致即拒绝。
    pub effect_started: bool,
    /// 停止证据内容的 `sha256:` 摘要；不含证据明文，不能验证证据真伪。
    pub evidence_digest: String,
    /// 对本报告其他字段的 `sha256:` 摘要；不是签名。
    pub report_digest: String,
}

impl AutomationStopReport {
    /// 构造并校验一份 stop report；形状不合法时返回稳定错误码。
    pub fn new(
        execution_id: impl Into<String>,
        attempt: u32,
        cancel_generation: u64,
        authority_epoch: u64,
        state: AutomationStopState,
        effect_started: bool,
        evidence_digest: impl Into<String>,
    ) -> Result<Self, &'static str> {
        let mut report = Self {
            schema: AUTOMATION_STOP_REPORT_SCHEMA.to_owned(),
            execution_id: execution_id.into(),
            attempt,
            cancel_generation,
            authority_epoch,
            state,
            effect_started,
            evidence_digest: evidence_digest.into(),
            report_digest: String::new(),
        };
        report.report_digest = report.digest();
        report.validate()?;
        Ok(report)
    }

    /// 校验 schema、身份/摘要形状、非零 generation/epoch 与 state×effect_started 一致性。
    ///
    /// 无效形状返回 `automation_stop_report_invalid`；报告声称“从未开始”却又声称 effect 已开始
    /// （或声称 `Stopping`/`Stopped` 却没有开始）返回 `automation_stop_report_effect_conflict`。
    pub fn validate(&self) -> Result<(), &'static str> {
        if self.schema != AUTOMATION_STOP_REPORT_SCHEMA
            || !valid_text(&self.execution_id)
            || self.attempt == 0
            || self.cancel_generation == 0
            || self.authority_epoch == 0
            || !valid_digest(&self.evidence_digest)
            || !valid_digest(&self.report_digest)
            || self.report_digest != self.digest()
        {
            return Err("automation_stop_report_invalid");
        }
        let effect_conflict = match self.state {
            AutomationStopState::NotStarted => self.effect_started,
            AutomationStopState::Stopping | AutomationStopState::Stopped => !self.effect_started,
            AutomationStopState::Unknown => false,
        };
        if effect_conflict {
            return Err("automation_stop_report_effect_conflict");
        }
        Ok(())
    }

    /// 按固定字段集合重算报告摘要；修改字段后必须调用。
    pub fn recompute_digest(&mut self) {
        self.report_digest = self.digest();
    }

    fn digest(&self) -> String {
        json_digest(&serde_json::json!({
            "schema": self.schema,
            "execution_id": self.execution_id,
            "attempt": self.attempt,
            "cancel_generation": self.cancel_generation,
            "authority_epoch": self.authority_epoch,
            "state": self.state,
            "effect_started": self.effect_started,
            "evidence_digest": self.evidence_digest,
        }))
    }
}

/// 一份晚于取消代次到达、必须隔离而不能覆盖当前事实的结果。
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AutomationLateResult {
    /// 必须精确匹配 `AUTOMATION_LATE_RESULT_SCHEMA`。
    pub schema: String,
    /// 迟到结果声称的 execution；必须与已记录取消事实一致。
    pub execution_id: String,
    /// 迟到结果声称的 attempt；必须与已记录取消事实一致。
    pub attempt: u32,
    /// 迟到结果所属的取消代次；不得大于当前事实的代次（更大的代次不是“迟到”）。
    pub cancel_generation: u64,
    /// 迟到结果声称的 authority epoch；不得大于当前事实的 epoch。
    pub authority_epoch: u64,
    /// 迟到结果内容的 `sha256:` 摘要；不含明文。
    pub result_digest: String,
    /// 对迟到结果其他字段的 `sha256:` 摘要；不是签名。
    pub late_result_digest: String,
}

impl AutomationLateResult {
    /// 构造并校验一份迟到结果隔离请求。
    pub fn new(
        execution_id: impl Into<String>,
        attempt: u32,
        cancel_generation: u64,
        authority_epoch: u64,
        result_digest: impl Into<String>,
    ) -> Result<Self, &'static str> {
        let mut late = Self {
            schema: AUTOMATION_LATE_RESULT_SCHEMA.to_owned(),
            execution_id: execution_id.into(),
            attempt,
            cancel_generation,
            authority_epoch,
            result_digest: result_digest.into(),
            late_result_digest: String::new(),
        };
        late.late_result_digest = late.digest();
        late.validate()?;
        Ok(late)
    }

    /// 校验 schema、身份字段、非零 generation/epoch 与两个摘要的格式/自洽。
    pub fn validate(&self) -> Result<(), &'static str> {
        if self.schema != AUTOMATION_LATE_RESULT_SCHEMA
            || !valid_text(&self.execution_id)
            || self.attempt == 0
            || self.cancel_generation == 0
            || self.authority_epoch == 0
            || !valid_digest(&self.result_digest)
            || !valid_digest(&self.late_result_digest)
            || self.late_result_digest != self.digest()
        {
            return Err("automation_late_result_invalid");
        }
        Ok(())
    }

    /// 按固定字段集合重算迟到结果摘要；修改字段后必须调用。
    pub fn recompute_digest(&mut self) {
        self.late_result_digest = self.digest();
    }

    fn digest(&self) -> String {
        json_digest(&serde_json::json!({
            "schema": self.schema,
            "execution_id": self.execution_id,
            "attempt": self.attempt,
            "cancel_generation": self.cancel_generation,
            "authority_epoch": self.authority_epoch,
            "result_digest": self.result_digest,
        }))
    }
}

/// 取消第一步的判定结果：下一步要提交的事实，以及是否必须真的请求 stop。
///
/// `replayed = true` 表示同一代次同一请求已经记录过：调用方不写事实，并在 `stop_required` 为
/// true 时对同一个在途取消重新发出 stop（stop 请求按代次幂等，重复请求是安全的；effect 不会被重放）。
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AutomationCancelPlan {
    /// 下一步应提交（或已经存在）的取消事实。
    pub fact: AutomationCancellationFact,
    /// 是否必须调用 stop 控制器恰好一次。
    pub stop_required: bool,
    /// 是否命中同代次同请求的安全重放。
    pub replayed: bool,
}

/// 取消第一步：只看已提交命令与可选当前事实，决定“要不要真的请求 stop”。
///
/// 【核心流程】
/// 1. 命令、请求、当前事实各自校验；execution/attempt 必须与命令完全一致
///    （`automation_cancel_execution_drift` / `automation_cancel_attempt_drift`）；
/// 2. authority epoch 必须与命令相同，否则 `automation_cancel_authority_epoch_stale`；
/// 3. 已有事实时做代次围栏：请求代次更低 → `automation_cancel_generation_stale`；相等但原因或
///    epoch 不同 → `automation_cancel_generation_conflict`；相等且完全相同 → 安全重放
///    （`replayed = true`，只在事实仍是 `Requested` 时要求重新发 stop）；
/// 4. 新代次按命令状态判定：`Prepared`（未派发、effect 未开始）→ `NotExecuted`，不请求 stop；
///    终态 `Observed` → `automation_cancel_execution_terminal`（已结束的执行不能被取消“复活”）；
///    终态 `Unknown` → `ResultUnknown` 且不请求 stop（未知已经属于对账，不能假装能停止）；
///    `Claimed`（attempt 已被 worker 取走，effect 可能正在跑）→ `Requested` + `stop_required`。
///
/// 【输出】返回下一步事实与是否需要 stop；`Ok` 不代表 stop 已经发生，也不代表结果已确认。
/// 【副作用】无；不写 store、不调用 stop 控制器、不改变命令状态。
pub fn begin_automation_cancellation(
    command: &AutomationDispatchCommand,
    request: &AutomationCancelRequest,
    current: Option<&AutomationCancellationFact>,
) -> Result<AutomationCancelPlan, &'static str> {
    command.validate()?;
    request.validate()?;
    if request.execution_id != command.intent.execution_id {
        return Err("automation_cancel_execution_drift");
    }
    if request.attempt != command.intent.attempt {
        return Err("automation_cancel_attempt_drift");
    }
    if request.authority_epoch != command.intent.authority_epoch {
        return Err("automation_cancel_authority_epoch_stale");
    }
    if let Some(current) = current {
        current.validate()?;
        if current.execution_id != request.execution_id || current.attempt != request.attempt {
            return Err("automation_cancel_fact_drift");
        }
        if request.cancel_generation < current.cancel_generation {
            return Err("automation_cancel_generation_stale");
        }
        if request.cancel_generation == current.cancel_generation {
            if current.authority_epoch != request.authority_epoch
                || current.reason != request.reason
            {
                return Err("automation_cancel_generation_conflict");
            }
            let stop_required = current.state == AutomationCancelState::Requested;
            return Ok(AutomationCancelPlan {
                fact: current.clone(),
                stop_required,
                replayed: true,
            });
        }
    }
    let (state, effect_started, stop_required) = match command.intent.state {
        AutomationDispatchState::Prepared => (AutomationCancelState::NotExecuted, false, false),
        AutomationDispatchState::Claimed => (AutomationCancelState::Requested, true, true),
        AutomationDispatchState::Unknown => (AutomationCancelState::ResultUnknown, true, false),
        AutomationDispatchState::Observed => return Err("automation_cancel_execution_terminal"),
    };
    let mut fact = AutomationCancellationFact {
        schema: AUTOMATION_CANCEL_SCHEMA.to_owned(),
        execution_id: command.intent.execution_id.clone(),
        attempt: command.intent.attempt,
        cancel_generation: request.cancel_generation,
        authority_epoch: command.intent.authority_epoch,
        state,
        effect_started,
        stop_requested: false,
        stop_confirmed: false,
        stop_evidence_digest: None,
        late_result: false,
        reason: request.reason.clone(),
        fact_digest: String::new(),
    };
    fact.recompute_digest();
    fact.validate()?;
    Ok(AutomationCancelPlan {
        fact,
        stop_required,
        replayed: false,
    })
}

/// 把 stop report 绑定到命令与请求：身份、attempt、代次与 epoch 必须全部一致。
///
/// 绑定漂移返回 `automation_stop_report_binding_drift`；这是“旧报告不能推进新代次”的围栏。
pub fn bind_automation_stop_report(
    command: &AutomationDispatchCommand,
    request: &AutomationCancelRequest,
    report: &AutomationStopReport,
) -> Result<(), &'static str> {
    command.validate()?;
    request.validate()?;
    report.validate()?;
    if report.execution_id != command.intent.execution_id
        || report.execution_id != request.execution_id
        || report.attempt != command.intent.attempt
        || report.attempt != request.attempt
        || report.cancel_generation != request.cancel_generation
        || report.authority_epoch != command.intent.authority_epoch
        || report.authority_epoch != request.authority_epoch
    {
        return Err("automation_stop_report_binding_drift");
    }
    Ok(())
}

/// 把 stop report 折叠进在途取消事实。
///
/// 【核心流程】命令/请求/事实/报告各自校验 → 报告绑定比较 → 事实必须处于 `Requested`/`Stopping`/
/// `ResultUnknown`（在途或未知）且代次等于请求，否则 `automation_cancel_stop_not_pending` →
/// 按报告分类产生下一事实：`NotStarted` → `NotExecuted`（effect 未开始，即使请求过 stop）；
/// `Stopping`/`Unknown` → `Stopping`/`ResultUnknown`（stop 未确认，仍需对账）；`Stopped` →
/// `Stopped`（只有这一条路径会得到 `stop_confirmed = true`，且必须携带报告证据）。
///
/// 【输出】通过校验的下一事实；`stop_requested` 一律为 true，因为折叠只发生在 stop 已发出之后。
/// 【失败】绑定漂移、事实状态不在在途集合、报告与请求代次不符都返回稳定错误码。
/// 【副作用】无；不写事实、不重试、不联系 worker；调用方必须自行 CAS 提交返回的事实。
pub fn fold_automation_stop_report(
    command: &AutomationDispatchCommand,
    request: &AutomationCancelRequest,
    current: &AutomationCancellationFact,
    report: &AutomationStopReport,
) -> Result<AutomationCancellationFact, &'static str> {
    bind_automation_stop_report(command, request, report)?;
    current.validate()?;
    if current.execution_id != request.execution_id || current.attempt != request.attempt {
        return Err("automation_cancel_fact_drift");
    }
    if current.cancel_generation != request.cancel_generation {
        return Err("automation_cancel_generation_stale");
    }
    if !matches!(
        current.state,
        AutomationCancelState::Requested
            | AutomationCancelState::Stopping
            | AutomationCancelState::ResultUnknown
    ) {
        return Err("automation_cancel_stop_not_pending");
    }
    let (state, effect_started, stop_confirmed) = match report.state {
        AutomationStopState::NotStarted => (AutomationCancelState::NotExecuted, false, false),
        AutomationStopState::Stopping => (AutomationCancelState::Stopping, true, false),
        AutomationStopState::Stopped => (AutomationCancelState::Stopped, true, true),
        AutomationStopState::Unknown => (AutomationCancelState::ResultUnknown, true, false),
    };
    let mut next = current.clone();
    next.state = state;
    next.effect_started = effect_started;
    next.stop_requested = true;
    next.stop_confirmed = stop_confirmed;
    next.stop_evidence_digest = Some(report.evidence_digest.clone());
    next.late_result = false;
    next.recompute_digest();
    next.validate()?;
    Ok(next)
}

/// 同代次取消事实的状态转移白名单：一旦关闭就不能复活。
///
/// 同一状态返回 true（内容完全相同的重放由 store 的摘要 CAS 判定）；`Fenced` 是终态，除了自身
/// 重放以外没有任何出边；`Stopped`/`NotExecuted` 只允许走向 `Fenced`（隔离迟到结果），不能回到
/// `Requested`/`Stopping`/`ResultUnknown`。
pub fn automation_cancel_transition_allowed(
    from: AutomationCancelState,
    to: AutomationCancelState,
) -> bool {
    use AutomationCancelState::*;
    if from == to {
        return true;
    }
    matches!(
        (from, to),
        (
            Requested,
            Stopping | NotExecuted | Stopped | ResultUnknown | Fenced
        ) | (Stopping, NotExecuted | Stopped | ResultUnknown | Fenced)
            | (ResultUnknown, Stopped | Fenced)
            | (NotExecuted, Fenced)
            | (Stopped, Fenced)
    )
}

/// 把一份迟到结果隔离成 `Fenced`，绝不复活已取消/已隔离的执行。
///
/// 【核心流程】事实与迟到结果各自校验 → execution/attempt 必须一致，否则
/// `automation_late_result_binding_drift` → 迟到结果的代次/epoch 不得大于当前事实
/// （更大的值是“新结果”，不是迟到结果）→ 已经 `Fenced` 的事实拒绝再次隔离
/// （`automation_cancel_already_fenced`）→ 只有白名单允许的转移才能落成 `Fenced`。
///
/// 【输出】`late_result = true` 且状态为 `Fenced` 的下一事实；停止相关字段原样保留，因此迟到结果
/// 不会改写“已确认停止”的历史，也不会把状态改回任何活着的值。
/// 【副作用】无；不丢弃迟到结果本身（调用方需保留其摘要作为隔离证据），不接触 worker。
pub fn fence_automation_late_result(
    current: &AutomationCancellationFact,
    late: &AutomationLateResult,
) -> Result<AutomationCancellationFact, &'static str> {
    current.validate()?;
    late.validate()?;
    if current.execution_id != late.execution_id || current.attempt != late.attempt {
        return Err("automation_late_result_binding_drift");
    }
    if late.cancel_generation > current.cancel_generation {
        return Err("automation_late_result_generation_ahead");
    }
    if late.authority_epoch > current.authority_epoch {
        return Err("automation_late_result_authority_epoch_ahead");
    }
    if current.state == AutomationCancelState::Fenced {
        return Err("automation_cancel_already_fenced");
    }
    if !automation_cancel_transition_allowed(current.state, AutomationCancelState::Fenced) {
        return Err("automation_cancel_transition_refused");
    }
    let mut next = current.clone();
    next.state = AutomationCancelState::Fenced;
    next.late_result = true;
    next.recompute_digest();
    next.validate()?;
    Ok(next)
}

/// CAS 提交结果：`replayed=true` 表示没有新建事实，`fact` 是当前权威副本。
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AutomationCancellationCommit {
    /// 提交后（或重放命中的）当前事实副本。
    pub fact: AutomationCancellationFact,
    /// 是否命中同一内容并重放了既有事实。
    pub replayed: bool,
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

/// 检查 SHA-256 摘要文本格式；不重新计算输入内容或认证摘要来源。
fn valid_digest(value: &str) -> bool {
    value
        .strip_prefix("sha256:")
        .is_some_and(|hex| hex.len() == 64 && hex.bytes().all(|byte| byte.is_ascii_hexdigit()))
}

//! H26 人类澄清（clarification）合同。
//!
//! # 文件职责与数据流
//!
//! 澄清是模型对当前步骤提出的问题，回答后继续**原来的** Run/Turn/Step。它与审批是两种
//! 不同语义：回答没有 capability request、grant、approval ID 或 tool arguments，不能改变
//! 工具目录或发放 permit。
//!
//! ```text
//! Runner / Model step 产生 ClarificationRequest
//!        -> TTY/Web Human Inbox 展示 question/wait view
//!        -> 用户提交 ClarificationAnswer
//!        -> 本文件按 interaction/run/turn/step + request digest 校验
//!        -> ClarificationResolution（resume_original_step=true）
//!        -> Core/EventLog 持久化，Runner 继续同一 model step
//! ```
//!
//! 本文件只定义 versioned value object 和 fail-closed 状态转换；Core 负责提交结果，Runner
//! 负责等待/恢复，EventLog 负责事实。`digest` 是未加密钥的内容一致性指纹，不是用户身份、
//! 授权签名或跨进程持久化证明。

use crate::{json_digest, InteractionId, RequestId, RunId, SchemaVersion, StepId, TurnId};
use serde::{Deserialize, Serialize};
use serde_json::json;

pub const CLARIFICATION_REQUEST_SCHEMA: &str = "kiana.clarification-request.v1";
pub const CLARIFICATION_ANSWER_SCHEMA: &str = "kiana.clarification-answer.v1";
pub const CLARIFICATION_RESOLUTION_SCHEMA: &str = "kiana.clarification-resolution.v1";
pub const CLARIFICATION_WAIT_SCHEMA: &str = "kiana.clarification-wait.v1";
pub const CLARIFICATION_VERSION: SchemaVersion = SchemaVersion::new(1, 0);
pub const CLARIFICATION_WAITING_STATUS: &str = "waiting_for_input";
pub const MAX_CLARIFICATION_TEXT_BYTES: usize = 32 * 1024;
pub const MAX_CLARIFICATION_OPTIONS: usize = 32;
pub const MAX_CLARIFICATION_ROLES: usize = 16;

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
/// 澄清请求的生命周期。
///
/// `Pending` 才能接受回答、取消或过期；`Answered` 必须携带绑定到原请求的 answer；
/// `Cancelled`/`Expired` 都是没有回答的终态，不能再次 resume。重复/foreign answer 会在
/// 状态转换前拒绝，避免同一 interaction 触发两次 continuation。
pub enum ClarificationStatus {
    /// 等待合法 responder 的回答。
    Pending,
    /// 已接受一次回答并生成 resolution。
    Answered,
    /// 由允许的用户/runtime 取消，未产生回答。
    Cancelled,
    /// 到达绝对过期时间，未产生回答。
    Expired,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
/// 谁可以取消 pending clarification。
pub enum ClarificationCancelPolicy {
    /// runtime 或请求允许的 responder 都可以取消。
    UserOrRuntime,
    /// 只有 runtime 可以取消。
    RuntimeOnly,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
/// 回答来自哪个产品入口；它是来源标签，不是认证凭据。
pub enum ClarificationSource {
    /// CLI 命令入口。
    Cli,
    /// 交互式 TTY。
    Tty,
    /// 本地 Web Workbench。
    Web,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
/// 一个可选回答的稳定 ID 和显示标签。
///
/// ID 会进入 request digest，并在接收回答时与 request 的 options 精确匹配；label 只是给
/// 用户看的文本。未知 ID、重复 ID 或超过上限的文本都会 fail-closed。
pub struct ClarificationOption {
    /// 机器可比较的选项标识。
    pub id: String,
    /// UI 展示标签，不直接作为状态机输入。
    pub label: String,
}

impl ClarificationOption {
    /// 构造一个未验证的 option 值。
    ///
    /// 完整请求构造或 `validate` 会检查 ID/label 非空、长度和控制字符；这里不做 I/O 或
    /// 自动去重，因此调用方不能把构造成功当作 option 已经注册。
    pub fn new(id: impl Into<String>, label: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            label: label.into(),
        }
    }

    /// 校验 option 的文本边界。
    fn validate(&self) -> Result<(), String> {
        if !bounded(&self.id, 128) || !bounded(&self.label, 1_024) {
            return Err("clarification_option_invalid".to_owned());
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
/// 用户对一条澄清请求提交的回答。
///
/// `interaction_id/run_id/turn_id/step_id` 和 `request_digest` 共同绑定回答目标，防止把 A
/// 问题的回答送给 B 的 run 或在请求变更后继续使用旧回答。`option_id` 可为空：只有 request
/// 没有 options 时才允许为空；文字 `text` 始终保留。`answered_by`/`responder_role` 是
/// 业务审计字段，`source` 只是入口标签，不能替代外部身份认证。
pub struct ClarificationAnswer {
    /// 固定 answer schema。
    pub schema: String,
    /// 版本化 wire 形状。
    pub version: SchemaVersion,
    /// 回答事实自身的稳定请求 ID。
    pub answer_id: RequestId,
    /// 被回答的 interaction ID。
    pub interaction_id: InteractionId,
    /// 目标 Run ID。
    pub run_id: RunId,
    /// 目标 Turn ID。
    pub turn_id: TurnId,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    /// 可选目标 Step ID；缺失表示该 turn 没有细分步骤。
    pub step_id: Option<StepId>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    /// 选择的注册 option；无 options 的自由文本问题必须为空。
    pub option_id: Option<String>,
    /// 用户输入的回答文本。
    pub text: String,
    /// 提交回答的业务主体标识。
    pub answered_by: String,
    /// 提交者在 request responder_roles 中必须被允许的角色。
    pub responder_role: String,
    /// 回答入口标签。
    pub source: ClarificationSource,
    /// 回答时间（Unix 毫秒），必须位于 request 创建时间与当前检查时间之间。
    pub answered_at_unix_ms: u64,
    /// 被回答 request 的 canonical digest。
    pub request_digest: String,
    /// 对 answer 字段重算出的 canonical digest。
    pub answer_digest: String,
}

impl ClarificationAnswer {
    /// 构造并立即验证一份回答。
    ///
    /// 构造只负责 answer 自身形状、ID、时间/文本和 digest；它不能知道 request 当前是否
    /// Pending、option 是否属于该 request 或 responder role 是否获准，这些跨对象检查由
    /// `ClarificationRequest::accept_answer` 完成。失败返回 Domain 稳定错误，不写 EventLog。
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        answer_id: RequestId,
        interaction_id: InteractionId,
        run_id: RunId,
        turn_id: TurnId,
        step_id: Option<StepId>,
        option_id: Option<String>,
        text: impl Into<String>,
        answered_by: impl Into<String>,
        responder_role: impl Into<String>,
        source: ClarificationSource,
        answered_at_unix_ms: u64,
        request_digest: impl Into<String>,
    ) -> Result<Self, String> {
        let mut answer = Self {
            schema: CLARIFICATION_ANSWER_SCHEMA.to_owned(),
            version: CLARIFICATION_VERSION,
            answer_id,
            interaction_id,
            run_id,
            turn_id,
            step_id,
            option_id,
            text: text.into(),
            answered_by: answered_by.into(),
            responder_role: responder_role.into(),
            source,
            answered_at_unix_ms,
            request_digest: request_digest.into(),
            answer_digest: String::new(),
        };
        answer.answer_digest = answer.digest();
        answer.validate().map(|()| answer)
    }

    /// 验证回答自身的 schema、非空稳定 ID、文本/时间上限和 digest。
    ///
    /// 这里不验证 request 绑定关系，因为需要同时读取 request；调用方必须再经过
    /// `accept_answer`，不能只调用本方法就恢复 Runner。
    pub fn validate(&self) -> Result<(), String> {
        if self.schema != CLARIFICATION_ANSWER_SCHEMA
            || self.version != CLARIFICATION_VERSION
            || self.answer_id.as_uuid().is_nil()
            || self.interaction_id.as_uuid().is_nil()
            || self.run_id.as_uuid().is_nil()
            || self.turn_id.as_uuid().is_nil()
            || !bounded(&self.text, MAX_CLARIFICATION_TEXT_BYTES)
            || !bounded(&self.answered_by, 256)
            || !bounded(&self.responder_role, 128)
            || self.answered_at_unix_ms == 0
            || !digest(&self.request_digest)
            || self.answer_digest != self.digest()
        {
            return Err("clarification_answer_invalid".to_owned());
        }
        if let Some(step_id) = self.step_id {
            if step_id.as_uuid().is_nil() {
                return Err("clarification_answer_step_invalid".to_owned());
            }
        }
        if self
            .option_id
            .as_deref()
            .is_some_and(|id| !bounded(id, 128))
        {
            return Err("clarification_answer_option_invalid".to_owned());
        }
        Ok(())
    }

    /// 计算不包含自身 `answer_digest` 的回答 canonical digest。
    ///
    /// request digest、目标身份、回答内容、角色、来源和时间都会参与摘要；修改任意字段后
    /// 必须重新计算，否则 `validate` 会拒绝。摘要不是签名，也不证明回答者真实身份。
    pub fn digest(&self) -> String {
        json_digest(&json!({
            "schema": self.schema,
            "version": self.version,
            "answer_id": self.answer_id,
            "interaction_id": self.interaction_id,
            "run_id": self.run_id,
            "turn_id": self.turn_id,
            "step_id": self.step_id,
            "option_id": self.option_id,
            "text": self.text,
            "answered_by": self.answered_by,
            "responder_role": self.responder_role,
            "source": self.source,
            "answered_at_unix_ms": self.answered_at_unix_ms,
            "request_digest": self.request_digest,
        }))
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
/// 等待人类输入的澄清请求。
///
/// 它绑定原始 Run/Turn/可选 Step、问题内容、可选项、必答标志、创建/过期时间、取消策略和
/// responder roles。`status` 与 `answer` 的组合必须符合状态机：Pending 没有 answer，
/// Answered 恰好有一个已绑定 answer，Cancelled/Expired 不携带 answer。
pub struct ClarificationRequest {
    /// 固定 request schema。
    pub schema: String,
    /// 版本化 wire 形状。
    pub version: SchemaVersion,
    /// 本次问题的交互 ID；Human Inbox actionable_id 也由它派生。
    pub interaction_id: InteractionId,
    /// 问题所属 Run ID。
    pub run_id: RunId,
    /// 问题所属 Turn ID。
    pub turn_id: TurnId,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    /// 可选所属 Step ID。
    pub step_id: Option<StepId>,
    /// 给用户展示的问题文本。
    pub question: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    /// 允许选择的 option；为空时回答只能使用自由文本。
    pub options: Vec<ClarificationOption>,
    /// 是否必须回答；不改变过期/取消状态机，只影响上层等待策略。
    pub required: bool,
    /// 创建时间（Unix 毫秒）。
    pub created_at_unix_ms: u64,
    /// 绝对过期时间；必须晚于创建时间。
    pub expires_at_unix_ms: u64,
    /// 谁可以取消 pending request。
    pub cancel_policy: ClarificationCancelPolicy,
    /// 创建问题的业务角色。
    pub created_by_role: String,
    /// 允许回答/取消的 responder role 集合。
    pub responder_roles: Vec<String>,
    /// 当前状态。
    pub status: ClarificationStatus,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    /// Answered 状态对应的唯一回答；其他状态必须为空。
    pub answer: Option<ClarificationAnswer>,
    /// 对 request 内容（不含 answer/status）的 canonical digest，回答用它绑定原请求。
    pub request_digest: String,
}

impl ClarificationRequest {
    /// 构造一条 Pending clarification request 并计算 request digest。
    ///
    /// 构造会调用 `validate`，因此空问题、非法 ID、无 responder、重复 option/role、超过
    /// 32 KiB/32 options/16 roles、非递增时间都会立即返回错误。它不会写事件、通知 UI 或
    /// 让 Runner 停止；上层收到成功值后才把它放入 pending driver/Human Inbox。
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        interaction_id: InteractionId,
        run_id: RunId,
        turn_id: TurnId,
        step_id: Option<StepId>,
        question: impl Into<String>,
        options: Vec<ClarificationOption>,
        required: bool,
        created_at_unix_ms: u64,
        expires_at_unix_ms: u64,
        cancel_policy: ClarificationCancelPolicy,
        created_by_role: impl Into<String>,
        responder_roles: Vec<String>,
    ) -> Result<Self, String> {
        let mut request = Self {
            schema: CLARIFICATION_REQUEST_SCHEMA.to_owned(),
            version: CLARIFICATION_VERSION,
            interaction_id,
            run_id,
            turn_id,
            step_id,
            question: question.into(),
            options,
            required,
            created_at_unix_ms,
            expires_at_unix_ms,
            cancel_policy,
            created_by_role: created_by_role.into(),
            responder_roles,
            status: ClarificationStatus::Pending,
            answer: None,
            request_digest: String::new(),
        };
        request.request_digest = request.digest();
        request.validate().map(|()| request)
    }

    /// 校验 request 的身份、时间、选项/角色集合、digest 和 `status/answer` 配对。
    ///
    /// 这是“单个 request 是否自洽”的检查；它不确认当前墙上时间、调用者权限或 answer 是否
    /// 属于本 request。回答接收必须再经过 `accept_answer`，取消和过期也有各自的时间/角色
    /// 条件。失败只返回错误码，不改变 request。
    pub fn validate(&self) -> Result<(), String> {
        if self.schema != CLARIFICATION_REQUEST_SCHEMA
            || self.version != CLARIFICATION_VERSION
            || self.interaction_id.as_uuid().is_nil()
            || self.run_id.as_uuid().is_nil()
            || self.turn_id.as_uuid().is_nil()
            || !bounded(&self.question, MAX_CLARIFICATION_TEXT_BYTES)
            || self.created_at_unix_ms == 0
            || self.expires_at_unix_ms <= self.created_at_unix_ms
            || !bounded(&self.created_by_role, 128)
            || self.responder_roles.is_empty()
            || self.responder_roles.len() > MAX_CLARIFICATION_ROLES
            || self.options.len() > MAX_CLARIFICATION_OPTIONS
            || !digest(&self.request_digest)
            || self.request_digest != self.digest()
        {
            return Err("clarification_request_invalid".to_owned());
        }
        if self.step_id.is_some_and(|id| id.as_uuid().is_nil()) {
            return Err("clarification_request_step_invalid".to_owned());
        }
        let mut option_ids = std::collections::HashSet::new();
        for option in &self.options {
            option.validate()?;
            if !option_ids.insert(option.id.as_str()) {
                return Err("clarification_option_duplicate".to_owned());
            }
        }
        let mut roles = std::collections::HashSet::new();
        if self
            .responder_roles
            .iter()
            .any(|role| !bounded(role, 128) || !roles.insert(role.as_str()))
        {
            return Err("clarification_responder_roles_invalid".to_owned());
        }
        match (&self.status, &self.answer) {
            (ClarificationStatus::Pending, None)
            | (ClarificationStatus::Cancelled, None)
            | (ClarificationStatus::Expired, None) => {}
            (ClarificationStatus::Answered, Some(answer)) => {
                answer.validate()?;
                self.validate_answer_binding(answer)?;
            }
            _ => return Err("clarification_status_answer_mismatch".to_owned()),
        }
        Ok(())
    }

    /// 接受一份回答并返回更新后的 request 与 continuation resolution。
    ///
    /// 【调用者】
    /// Core `commit_clarification_answer` 在把事实写入 EventLog 前调用；Runner 仍负责在
    /// 持久化后恢复原步骤。
    ///
    /// 【检查顺序】
    /// 先校验 request/answer 自身和 interaction/run/turn/step/request digest 绑定，再要求
    /// 当前状态为 Pending、`now_unix_ms` 未到 expiry、responder role 被允许、回答时间合法，
    /// 最后检查 option 是否必需且属于 request。只有全部通过才克隆 request、置为 Answered、
    /// 保存 answer，并生成 `resume_original_step=true`、`waiting_for_input=false` 的 resolution。
    ///
    /// 【失败/副作用】
    /// foreign、duplicate、过期、未知 option 或角色未获准都会 fail-closed；方法不会修改
    /// 原 request、resume Runner、消费 approval 或写 EventLog。
    pub fn accept_answer(
        &self,
        answer: ClarificationAnswer,
        now_unix_ms: u64,
    ) -> Result<(Self, ClarificationResolution), String> {
        self.validate()?;
        answer.validate()?;
        self.validate_answer_binding(&answer)?;
        if self.status != ClarificationStatus::Pending {
            return Err("clarification_not_pending".to_owned());
        }
        if now_unix_ms >= self.expires_at_unix_ms {
            return Err("clarification_expired".to_owned());
        }
        if !self
            .responder_roles
            .iter()
            .any(|role| role == &answer.responder_role)
        {
            return Err("clarification_responder_role_denied".to_owned());
        }
        if answer.answered_at_unix_ms < self.created_at_unix_ms
            || answer.answered_at_unix_ms > now_unix_ms
        {
            return Err("clarification_answer_time_invalid".to_owned());
        }
        if self.options.is_empty() {
            if answer.option_id.is_some() {
                return Err("clarification_option_unexpected".to_owned());
            }
        } else {
            let Some(option_id) = answer.option_id.as_deref() else {
                return Err("clarification_option_required".to_owned());
            };
            if !self.options.iter().any(|option| option.id == option_id) {
                return Err("clarification_option_unknown".to_owned());
            }
        }
        let mut next = self.clone();
        next.status = ClarificationStatus::Answered;
        next.answer = Some(answer.clone());
        next.validate()?;
        let resolution = ClarificationResolution::from_answer(&next, &answer)?;
        Ok((next, resolution))
    }

    /// 按取消策略把 Pending request 转成 Cancelled。
    ///
    /// 只有 runtime，或在 `UserOrRuntime` 策略下属于 responder_roles 的 actor 才能取消；已经
    /// Answered/Cancelled/Expired 或已经过期的 request 不能再次取消。返回新值，不改变原值，
    /// 也不写事件；上层需把取消事实交给正常生命周期路径。
    pub fn cancel(&self, actor_role: &str, now_unix_ms: u64) -> Result<Self, String> {
        self.validate()?;
        if self.status != ClarificationStatus::Pending {
            return Err("clarification_not_pending".to_owned());
        }
        if now_unix_ms >= self.expires_at_unix_ms {
            return Err("clarification_expired".to_owned());
        }
        let allowed = match self.cancel_policy {
            ClarificationCancelPolicy::UserOrRuntime => {
                actor_role == "runtime"
                    || self.responder_roles.iter().any(|role| role == actor_role)
            }
            ClarificationCancelPolicy::RuntimeOnly => actor_role == "runtime",
        };
        if !allowed {
            return Err("clarification_cancel_denied".to_owned());
        }
        let mut next = self.clone();
        next.status = ClarificationStatus::Cancelled;
        next.validate()?;
        Ok(next)
    }

    /// 在当前时间已经达到 expiry 时，把 Pending request 转成 Expired。
    ///
    /// 采用 `now_unix_ms >= expires_at_unix_ms` 的边界，避免刚好到期仍接受回答。未到期、
    /// 非 Pending 或原 request 不自洽时返回错误；不会自动通知 Runner 或删除 EventLog 事实。
    pub fn expire(&self, now_unix_ms: u64) -> Result<Self, String> {
        self.validate()?;
        if self.status != ClarificationStatus::Pending {
            return Err("clarification_not_pending".to_owned());
        }
        if now_unix_ms < self.expires_at_unix_ms {
            return Err("clarification_not_expired".to_owned());
        }
        let mut next = self.clone();
        next.status = ClarificationStatus::Expired;
        next.validate()?;
        Ok(next)
    }

    /// 生成供 Human Inbox/TTY/Web 展示的等待视图。
    ///
    /// 只有 Pending request 才能生成 view；view 固定 `status=waiting_for_input`、
    /// `waiting_for_input=true`，并把 interaction ID 作为 actionable ID。它是可丢弃的 UI 投影，
    /// 不是新的请求事实，也不会把普通回答变成 approval。
    pub fn waiting_view(&self) -> Result<ClarificationWaitView, String> {
        self.validate()?;
        if self.status != ClarificationStatus::Pending {
            return Err("clarification_not_waiting".to_owned());
        }
        Ok(ClarificationWaitView {
            schema: CLARIFICATION_WAIT_SCHEMA.to_owned(),
            version: CLARIFICATION_VERSION,
            interaction_id: self.interaction_id,
            run_id: self.run_id,
            turn_id: self.turn_id,
            step_id: self.step_id,
            status: CLARIFICATION_WAITING_STATUS.to_owned(),
            waiting_for_input: true,
            required: self.required,
            expires_at_unix_ms: self.expires_at_unix_ms,
            options: self.options.clone(),
            actionable_id: self.interaction_id.to_string(),
        })
    }

    /// 计算不包含 `request_digest`、状态和 answer 的 request canonical digest。
    ///
    /// request 的问题、options、必答/时间、取消策略和 responder roles 都参与绑定；回答会
    /// 携带该摘要，防止请求内容被替换后仍接受旧回答。
    pub fn digest(&self) -> String {
        json_digest(&json!({
            "schema": self.schema,
            "version": self.version,
            "interaction_id": self.interaction_id,
            "run_id": self.run_id,
            "turn_id": self.turn_id,
            "step_id": self.step_id,
            "question": self.question,
            "options": self.options,
            "required": self.required,
            "created_at_unix_ms": self.created_at_unix_ms,
            "expires_at_unix_ms": self.expires_at_unix_ms,
            "cancel_policy": self.cancel_policy,
            "created_by_role": self.created_by_role,
            "responder_roles": self.responder_roles,
        }))
    }

    /// 检查 answer 是否仍指向当前 request 的完整身份和内容。
    ///
    /// interaction/run/turn/step 任一 ID 或 request digest 不同都拒绝；这就是 foreign answer
    /// fence 的核心。此方法只返回错误，不执行恢复或写事实。
    fn validate_answer_binding(&self, answer: &ClarificationAnswer) -> Result<(), String> {
        if answer.interaction_id != self.interaction_id {
            return Err("clarification_answer_interaction_mismatch".to_owned());
        }
        if answer.run_id != self.run_id {
            return Err("clarification_answer_run_mismatch".to_owned());
        }
        if answer.turn_id != self.turn_id {
            return Err("clarification_answer_turn_mismatch".to_owned());
        }
        if answer.step_id != self.step_id {
            return Err("clarification_answer_step_mismatch".to_owned());
        }
        if answer.request_digest != self.request_digest {
            return Err("clarification_answer_request_mismatch".to_owned());
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
/// 回答被接受后交给 Core/Runner 的 continuation 事实。
///
/// 它再次携带 interaction/run/turn/step 和 request/answer digest，供 EventLog 与恢复流程确认
/// 这次 resume 对应原问题。合同故意要求 `resume_original_step=true`、`waiting_for_input=false`；
/// 它不包含 capability、approval 或新的工具动作。
pub struct ClarificationResolution {
    /// 固定 resolution schema。
    pub schema: String,
    /// 版本化 wire 形状。
    pub version: SchemaVersion,
    /// 原始 interaction ID。
    pub interaction_id: InteractionId,
    /// 原始 Run ID。
    pub run_id: RunId,
    /// 原始 Turn ID。
    pub turn_id: TurnId,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    /// 原始可选 Step ID。
    pub step_id: Option<StepId>,
    /// 被接受的 answer ID。
    pub answer_id: RequestId,
    /// 原 request digest。
    pub request_digest: String,
    /// 被接受 answer digest。
    pub answer_digest: String,
    /// 是否恢复原步骤；当前合同必须为 true。
    pub resume_original_step: bool,
    /// 是否仍等待输入；当前合同必须为 false。
    pub waiting_for_input: bool,
}

impl ClarificationResolution {
    /// 从已经通过绑定检查的 answer 创建 resolution。
    ///
    /// 这是 request 内部的构造 helper，不接受外部自定义 resume 方向；它固定恢复原步骤并
    /// 立即调用 `validate`。失败表示输入 request/answer 无法形成安全 continuation，不会启动
    /// Runner。
    fn from_answer(
        request: &ClarificationRequest,
        answer: &ClarificationAnswer,
    ) -> Result<Self, String> {
        let resolution = Self {
            schema: CLARIFICATION_RESOLUTION_SCHEMA.to_owned(),
            version: CLARIFICATION_VERSION,
            interaction_id: request.interaction_id,
            run_id: request.run_id,
            turn_id: request.turn_id,
            step_id: request.step_id,
            answer_id: answer.answer_id,
            request_digest: request.request_digest.clone(),
            answer_digest: answer.answer_digest.clone(),
            resume_original_step: true,
            waiting_for_input: false,
        };
        resolution.validate()?;
        Ok(resolution)
    }

    /// 验证 resolution 的 schema、身份、digest 和恢复方向。
    ///
    /// 通过只表示值能被后续 EventLog/Runner 识别，不表示 EventLog 已提交或 Runner 已恢复。
    pub fn validate(&self) -> Result<(), String> {
        if self.schema != CLARIFICATION_RESOLUTION_SCHEMA
            || self.version != CLARIFICATION_VERSION
            || self.interaction_id.as_uuid().is_nil()
            || self.run_id.as_uuid().is_nil()
            || self.turn_id.as_uuid().is_nil()
            || self.answer_id.as_uuid().is_nil()
            || !digest(&self.request_digest)
            || !digest(&self.answer_digest)
            || !self.resume_original_step
            || self.waiting_for_input
        {
            return Err("clarification_resolution_invalid".to_owned());
        }
        if self.step_id.is_some_and(|id| id.as_uuid().is_nil()) {
            return Err("clarification_resolution_step_invalid".to_owned());
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
/// Pending clarification 给 Human Inbox/TTY/Web 的只读等待投影。
///
/// `status` 和 `waiting_for_input` 是稳定 wire 字段；`actionable_id` 必须等于 interaction ID，
/// 使 UI 提交动作不能改投其他 interaction。投影不保存新的 approval/capability 权限。
pub struct ClarificationWaitView {
    /// 固定 wait schema。
    pub schema: String,
    /// 版本化 wire 形状。
    pub version: SchemaVersion,
    /// 待回答 interaction ID。
    pub interaction_id: InteractionId,
    /// 目标 Run ID。
    pub run_id: RunId,
    /// 目标 Turn ID。
    pub turn_id: TurnId,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    /// 可选目标 Step ID。
    pub step_id: Option<StepId>,
    /// 固定为 `waiting_for_input` 的状态文本。
    pub status: String,
    /// 当前是否还需要用户输入；等待投影必须为 true。
    pub waiting_for_input: bool,
    /// 是否必答。
    pub required: bool,
    /// 绝对过期时间。
    pub expires_at_unix_ms: u64,
    /// 可选回答列表的副本。
    pub options: Vec<ClarificationOption>,
    /// UI 动作提交时使用的稳定交互 ID。
    pub actionable_id: String,
}

impl ClarificationWaitView {
    /// 校验等待投影的 schema、身份、状态文本、actionable ID、expiry 和 options。
    ///
    /// 这是展示 DTO 的形状检查，不确认原 request 仍在 EventLog 中，也不会改变 Runner 或
    /// Human Inbox；真正回答仍须回到 `ClarificationRequest::accept_answer`。
    pub fn validate(&self) -> Result<(), String> {
        if self.schema != CLARIFICATION_WAIT_SCHEMA
            || self.version != CLARIFICATION_VERSION
            || self.interaction_id.as_uuid().is_nil()
            || self.run_id.as_uuid().is_nil()
            || self.turn_id.as_uuid().is_nil()
            || self.status != CLARIFICATION_WAITING_STATUS
            || !self.waiting_for_input
            || self.expires_at_unix_ms == 0
            || self.actionable_id != self.interaction_id.to_string()
        {
            return Err("clarification_wait_invalid".to_owned());
        }
        if self.step_id.is_some_and(|id| id.as_uuid().is_nil()) {
            return Err("clarification_wait_step_invalid".to_owned());
        }
        if self.options.len() > MAX_CLARIFICATION_OPTIONS {
            return Err("clarification_wait_options_invalid".to_owned());
        }
        for option in &self.options {
            option.validate()?;
        }
        Ok(())
    }
}

/// 检查文本非空、长度不超过指定字节上限且不含 NUL。
///
/// 32 KiB/1 KiB/128 等上限用于限制协议和 UI 资源消耗；它们不是安全身份校验或内容审查。
fn bounded(value: &str, max: usize) -> bool {
    !value.trim().is_empty() && value.len() <= max && !value.contains('\0')
}

/// 检查未加密钥 `sha256:` 摘要的固定文本形状。
///
/// 调用方仍必须把摘要与 canonical 内容重算比较；形状通过不代表来源可信。
fn digest(value: &str) -> bool {
    value
        .strip_prefix("sha256:")
        .is_some_and(|hex| hex.len() == 64 && hex.bytes().all(|byte| byte.is_ascii_hexdigit()))
}

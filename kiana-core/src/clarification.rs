//! H26 澄清回答的 Core 准入与 Human Inbox 投影边界。
//!
//! # 文件职责与数据流
//!
//! 澄清（clarification）是模型对当前步骤提出的问题，不是权限审批。它沿以下链路流动：
//!
//! ```text
//! Runner 在某个 Run/Turn/Step 产生 ClarificationRequest
//!        -> TTY / Web 通过 human.inbox 展示 question
//!        -> 用户提交 ClarificationAnswer
//!        -> 【当前文件】校验原始 interaction/run/turn/step 与 request digest
//!        -> ClarificationResolution（继续原步骤）
//!        -> 调用方写入 EventLog，再交给同一个 Runner continuation
//! ```
//!
//! 这里不调用 approval store、不创建 CapabilityRequest、不发放 grant/permit、不修改工具目录，
//! 也不直接 resume Runner。普通回答只有在上层按正常 EventLog 路径持久化后，才可被恢复流程
//! 消费；Human Inbox 只是共同的只读/动作投影，不是第二个事实源。

use kiana_domain::{
    ClarificationAnswer, ClarificationRequest, ClarificationResolution, HumanAction,
    HumanInboxItem, HumanInboxKind,
};
use serde_json::json;

pub const CLARIFICATION_CORE_SCHEMA: &str = "kiana.core-clarification.v1";

#[derive(Clone, Debug, PartialEq)]
/// Core 在确认澄清回答后返回的待提交事实。
///
/// `request` 是绑定到原始 run/turn/step 的下一份请求，`resolution` 明确是否恢复原步骤、
/// 是否仍在等待输入。该结构没有 approval ID、grant、capability 或 tool args，因此不能被
/// 当作权限授予；调用方必须把它写入既有 EventLog 后再转交同一 Runner。
pub struct ClarificationCommit {
    /// Core facade schema 版本。
    pub schema: &'static str,
    /// 接受回答后得到的 continuation request。
    pub request: ClarificationRequest,
    /// 已验证的澄清 resolution。
    pub resolution: ClarificationResolution,
}

/// 校验一个回答并准备原模型步骤的 continuation 事实。
///
/// 【调用者】
/// 通常由 `run.clarification.answer` command handler 在写 EventLog 前调用；回答来自 TTY/Web
/// Human Inbox action，而不是 approval action card。
///
/// 【输入】
/// `request` 是当前 pending 的原始澄清请求，`answer` 携带 interaction/run/turn/可选 step、
/// responder/source、文字或 option 以及 request digest，`now_unix_ms` 用于过期判断。
/// `ClarificationRequest::accept_answer` 会检查 foreign/duplicate/expired/option 等约束。
///
/// 【输出与失败】
/// 成功返回 `ClarificationCommit`，其中 resolution 负责恢复原 step 且不再等待输入；失败返回
/// Domain 稳定字符串错误。失败不会写事件、resume Runner、调用 approval/Broker 或重试。
/// 成功也只是得到待提交事实，调用方仍需经正常 EventLog 事务持久化。
pub fn commit_clarification_answer(
    request: &ClarificationRequest,
    answer: ClarificationAnswer,
    now_unix_ms: u64,
) -> Result<ClarificationCommit, String> {
    let (next_request, resolution) = request.accept_answer(answer, now_unix_ms)?;
    resolution.validate()?;
    Ok(ClarificationCommit {
        schema: CLARIFICATION_CORE_SCHEMA,
        request: next_request,
        resolution,
    })
}

/// 构造 TTY 和 Web 共用的 Human Inbox 澄清问题投影。
///
/// 【数据流】
/// 从 request 读取 interaction/run/turn/step、问题文本、等待视图、required、expiry 和可选
/// action，输出 `HumanInboxKind::Question` 与 `run.clarification.answer` regular-input 命令。
/// action 只要求 `text` 字段，并携带服务端选定的身份参数；它没有 approval ID、grant ID、
/// capability request 或 tool arguments。
///
/// 【失败/副作用】
/// 先调用 `waiting_view()` 验证 request 当前仍可展示；失败原样返回，不写 Inbox、不改变
/// Runner 状态。成功只是 UI 投影，不能被当作已回答或已授权。
pub fn clarification_human_inbox_item(
    request: &ClarificationRequest,
) -> Result<HumanInboxItem, String> {
    let wait = request.waiting_view()?;
    Ok(HumanInboxItem {
        item_id: format!("clarification:{}", request.interaction_id),
        kind: HumanInboxKind::Question,
        title: request.question.clone(),
        source_ref: format!("clarification:{}", request.interaction_id),
        run_id: Some(request.run_id),
        detail: json!({
            "question": request.question,
            "wait": wait,
            "required": request.required,
            "expires_at_unix_ms": request.expires_at_unix_ms,
        }),
        actions: vec![HumanAction {
            id: "answer_clarification".to_owned(),
            label: "回答澄清".to_owned(),
            command: "run.clarification.answer".to_owned(),
            arguments: json!({
                "interaction_id": request.interaction_id,
                "run_id": request.run_id,
                "turn_id": request.turn_id,
                "step_id": request.step_id,
            }),
            required_fields: vec!["text".to_owned()],
        }],
    })
}

//! Owned harness 使用的 Codex 风格 token 压缩器。
//!
//! 当对话历史超过本地触发阈值时，本模块只保留最近的用户消息，并在末尾插入固定摘要
//! 标记，建立一个较小的新上下文窗口。它不调用模型生成摘要，也不修改 EventLog 或真实
//! 会话事实；摘要只是可丢弃的 Runner 上下文视图。token 估算按字节近似，不能当作供应商
//! tokenizer、计费或硬预算证明。
//!
//! 实现形状参考 OpenAI Codex 的公开压缩逻辑；`reference/` 仍然只是审计输入，当前代码
//! 不依赖其中的源码。

use crate::model::{ModelMessage, ModelRole};
use kiana_domain::{
    json_digest, CompactEvidenceKind, CompactEvidenceStatus, CompactSummary, CompactSummaryEvidence,
};
use serde::{Deserialize, Serialize};

/// 压缩摘要使用的固定前缀，用于后续识别并避免把摘要当作真实用户轮次。
pub const SUMMARY_PREFIX: &str = r#"Another language model started to solve this problem and produced a summary of its thinking process. You also have access to the state of the tools that were used by that language model. Use this to build on the work that has already been done and avoid duplicating work. Here is the summary produced by the other language model, use the information in this summary to assist with your own analysis:"#;

const APPROX_BYTES_PER_TOKEN: usize = 4;
/// 压缩后保留的用户消息 token 上限（当前为 20,000）。
pub const COMPACT_USER_MESSAGE_MAX_TOKENS: usize = 20_000;
/// 未配置模型上下文窗口时采用的自动压缩触发值（当前为 32,000）。
pub const DEFAULT_COMPACT_TRIGGER_TOKENS: usize = 32_000;

/// 以约 4 字节/token 的固定比例估算文本 token 数。
///
/// 该函数只服务于压缩的确定性上限；非 ASCII 文本、代码和真实模型 tokenizer 的结果可能
/// 差异很大，因此不能用它判断供应商请求是否一定不会超限。
pub fn approx_token_count(text: &str) -> usize {
    let len = text.len();
    len.saturating_add(APPROX_BYTES_PER_TOKEN.saturating_sub(1)) / APPROX_BYTES_PER_TOKEN
}

/// 汇总消息文本的近似 token 数。
pub fn history_tokens(messages: &[ModelMessage]) -> usize {
    messages
        .iter()
        .map(|message| {
            approx_token_count(&message.text).saturating_add(
                serde_json::to_string(&message.tool_calls)
                    .map(|calls| approx_token_count(&calls))
                    .unwrap_or(usize::MAX),
            )
        })
        .sum()
}

/// 判断一段文本是否已经带有压缩摘要前缀。
pub fn is_summary_message(message: &str) -> bool {
    message.starts_with(&format!("{SUMMARY_PREFIX}\n"))
}

#[derive(Clone, Debug, PartialEq)]
/// 一次压缩检查的结果和前后 token 估算。
pub struct CompactOutcome {
    /// 压缩后要交给模型的消息列表。
    pub messages: Vec<ModelMessage>,
    /// 是否实际替换了历史。
    pub applied: bool,
    /// 压缩前的近似 token 数。
    pub tokens_before: usize,
    /// 压缩后的近似 token 数。
    pub tokens_after: usize,
    /// 结果中是否包含摘要消息。
    pub summary_present: bool,
    /// Failure to retain a complete recent group; callers must stop before invoking a model.
    pub error: Option<String>,
}

/// 在历史超过触发阈值时构建压缩上下文。
///
/// `trigger_tokens == 0` 或历史未超限时原样返回消息；超限后调用
/// [`build_compacted_history`]，不会尝试部分修改原向量。调用方应把 `applied` 和 token
/// 统计写入展示/收据，但不要把摘要当成新的业务事实。
pub fn compact_if_needed(
    messages: Vec<ModelMessage>,
    trigger_tokens: usize,
    retain_tokens: usize,
) -> CompactOutcome {
    let tokens_before = history_tokens(&messages);
    if trigger_tokens == 0 || tokens_before <= trigger_tokens {
        return CompactOutcome {
            summary_present: messages
                .iter()
                .any(|message| is_summary_message(&message.text)),
            tokens_after: tokens_before,
            messages,
            applied: false,
            error: None,
            tokens_before,
        };
    }
    let compacted = CompactSummary::from_messages(&messages).and_then(|summary| {
        build_compacted_history_with_summary(&messages, retain_tokens, &summary)
    });
    let (compacted, error) = match compacted {
        Ok(compacted) => (compacted, None),
        Err(error) => (messages.clone(), Some(error)),
    };
    let tokens_after = history_tokens(&compacted);
    CompactOutcome {
        applied: compacted != messages,
        tokens_before,
        tokens_after,
        summary_present: compacted
            .iter()
            .any(|message| is_summary_message(&message.text)),
        messages: compacted,
        error,
    }
}

/// 保留 system 前缀、最近完整消息组并追加 evidence-only 摘要消息。
///
/// Assistant、Tool 和已有摘要消息不会被保留；从后往前选择可以优先保住最新上下文，
/// 最后再恢复时间顺序。最后一条过长用户消息会做中间截断，保留开头和结尾以减少关键
/// 目标/结果同时丢失的概率。`max_user_tokens == 0` 时只生成摘要消息。
pub fn build_compacted_history(
    messages: &[ModelMessage],
    max_user_tokens: usize,
) -> Vec<ModelMessage> {
    CompactSummary::from_messages(messages)
        .and_then(|summary| {
            build_compacted_history_with_summary(messages, max_user_tokens, &summary)
        })
        .unwrap_or_else(|_| messages.to_vec())
}

/// Compact with a validated summary while preserving the latest complete user/assistant/tool
/// group. A tool result is never retained without the assistant/tool-call group that introduced
/// it; if the latest group cannot fit, the caller must keep the original view and pause rather
/// than continue from a lossy placeholder.
pub fn build_compacted_history_with_summary(
    messages: &[ModelMessage],
    max_user_tokens: usize,
    summary: &CompactSummary,
) -> Result<Vec<ModelMessage>, String> {
    summary.validate()?;
    let mut history: Vec<ModelMessage> = messages
        .iter()
        .filter(|message| message.role == ModelRole::System)
        .cloned()
        .collect();
    history.push(ModelMessage::user(format!(
        "{SUMMARY_PREFIX}\n{}",
        summary.render()?
    )));

    let groups = message_groups(messages);
    let mut selected = Vec::new();
    let mut remaining = max_user_tokens;
    for group in groups.into_iter().rev() {
        let tokens = history_tokens(&group);
        if selected.is_empty() {
            if tokens > remaining {
                let reduced = truncate_latest_group(group, remaining);
                if history_tokens(&reduced) > remaining {
                    return Err("compact_latest_group_exceeds_budget".to_owned());
                }
                selected.extend(reduced);
            } else {
                selected.extend(group);
            }
            remaining = remaining.saturating_sub(history_tokens(&selected));
        } else if tokens <= remaining {
            let mut prefix = group;
            prefix.extend(selected);
            selected = prefix;
            remaining = remaining.saturating_sub(tokens);
        } else {
            break;
        }
    }
    history.extend(selected);
    Ok(history)
}

fn message_groups(messages: &[ModelMessage]) -> Vec<Vec<ModelMessage>> {
    let mut groups = Vec::new();
    let mut current = Vec::new();
    for message in messages {
        if message.role == ModelRole::System || is_summary_message(&message.text) {
            continue;
        }
        if message.role == ModelRole::User && !current.is_empty() {
            groups.push(std::mem::take(&mut current));
        }
        current.push(message.clone());
    }
    if !current.is_empty() {
        groups.push(current);
    }
    groups
}

fn truncate_latest_group(mut group: Vec<ModelMessage>, max_tokens: usize) -> Vec<ModelMessage> {
    let Some(index) = group
        .iter()
        .rposition(|message| message.role == ModelRole::User)
    else {
        return group;
    };
    let original = std::mem::take(&mut group[index].text);
    // Keep every assistant/tool message and the user-message framing. Only its text may shrink.
    let fixed_tokens = history_tokens(&group);
    let available = max_tokens.saturating_sub(fixed_tokens);
    group[index].text = truncate_middle_tokens(&original, available);
    group
}

fn truncate_middle_tokens(text: &str, max_tokens: usize) -> String {
    let max_bytes = max_tokens.saturating_mul(APPROX_BYTES_PER_TOKEN);
    if text.len() <= max_bytes {
        return text.to_owned();
    }
    if max_bytes <= 3 {
        return truncate_utf8(text, max_bytes);
    }
    let keep = (max_bytes - 3) / 2;
    let prefix = truncate_utf8(text, keep);
    let mut start = text.len().saturating_sub(keep);
    while start < text.len() && !text.is_char_boundary(start) {
        start += 1;
    }
    // 使用 ASCII "..."，避免再次引入一个可能改变字节预算的 Unicode 字符。
    format!("{}...{}", prefix, &text[start..])
}

fn truncate_utf8(text: &str, max_bytes: usize) -> String {
    if text.len() <= max_bytes {
        return text.to_owned();
    }
    let mut end = max_bytes.min(text.len());
    while end > 0 && !text.is_char_boundary(end) {
        end -= 1;
    }
    text[..end].to_owned()
}

// ===========================================================================
// plan / generate / validate
//
// 上面的 `compact_if_needed` 是纯本地折叠；产品路径改用下面三段式流程：
// 1. `plan_compaction` 是纯函数：先把大结果外置成引用并按 digest 去重，再选择较旧的
//    完整消息组折叠，同时保留 system/角色消息、最新目标、未决 assistant/tool 对和近期
//    完整消息组。
// 2. 摘要由已准入的 `ModelClient` 请求生成（`ModelPurpose::Compaction`、tools 为空、
//    独立且有界的预算），提示词由 `summary_request_messages` 从计划渲染。
// 3. `parse_compaction_summary` + `validate_summary_for_plan` 校验模型输出：引用必须对应
//    服务端权威证据，模型文本不能自己声明完成、审批或验证事实。
//
// 摘要失败（模型不可用、输出非法、证据不足、容量超界）一律保留原视图并让调用方暂停，
// 不用无摘要占位继续执行。
// ===========================================================================

/// 压缩计划的结构版本号。
pub const COMPACTION_PLAN_SCHEMA: &str = "kiana.compaction-plan.v1";
/// 压缩解释（可写进事件/收据的可见摘要）的结构版本号。
pub const COMPACTION_EXPLANATION_SCHEMA: &str = "kiana.compaction-explanation.v1";
/// 摘要请求允许容纳的折叠输入上限（近似 token）；超过就截断后再请求。
pub const COMPACT_SUMMARY_PROMPT_MAX_TOKENS: usize = 4_096;
/// 摘要消息自身的 token 上限；超过上限的摘要必须重来或暂停，不能塞回上下文。
pub const COMPACT_SUMMARY_MAX_TOKENS: usize = 2_048;
/// 单次压缩摘要调用的 prepared 预算上限（wire 字节估计 + 输出预留，留有余量）。
pub const COMPACT_SUMMARY_BUDGET_TOKENS: u64 = 32_768;
/// 大结果外置阈值：超过它的工具观测在摘要输入里只保留引用与 digest。
pub const EXTERNALIZE_RESULT_TOKENS: usize = 512;

/// 摘要模型调用的指令：只输出版本化 JSON，只引用历史里真实存在的证据。
pub const COMPACT_SUMMARY_INSTRUCTION: &str = r#"You are the compaction summarizer of a coding agent. Read the supplied history block and reply with exactly one JSON object, no markdown fence and no commentary. Use this schema:
{"schema":"kiana.compact-summary.v1","version":{"major":1,"minor":0},"goal":"<current task goal>","constraints":["<constraint>"],"decision_refs":["<reference>"],"completed_action_refs":["<reference>"],"verification_refs":["<reference>"],"workspace_revisions":["<revision>"],"pending_work":["<work that is not finished>"],"next_action":"<the single next action>","source_event_start":null,"source_event_end":null,"source_message_count":1}
Rules: references use the forms event:, artifact:, file: and receipt: and must be copied from the history block; never claim that a tool ran, an approval was granted or work was verified unless its reference appears in the history block, otherwise use an empty array; keep each list under 64 items and each string under 2048 characters; the server stamps source_message_count, source_event_start, source_event_end, evidence and summary_digest, so do not send an evidence array or a summary_digest."#;

fn role_label(role: ModelRole) -> &'static str {
    match role {
        ModelRole::System => "system",
        ModelRole::User => "user",
        ModelRole::Assistant => "assistant",
        ModelRole::Tool => "tool",
    }
}

/// 一个被外置的大结果：原文不再进入摘要输入，只保留稳定引用、digest 和原始大小。
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ExternalizedResult {
    /// 稳定引用，形式为 `result:<digest>`。
    pub reference: String,
    /// 原文内容摘要（`sha256:` 前缀）。
    pub digest: String,
    /// 外置前的近似 token 数。
    pub original_tokens: usize,
}

/// 被去重的重复观测：保留第一次出现的引用，后续重复只在计划里计数。
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DuplicatedResult {
    /// 第一次出现时保留的稳定引用。
    pub reference: String,
    /// 重复观测共享的内容摘要。
    pub digest: String,
    /// 被省略的重复条数。
    pub elided: usize,
}

/// 一次压缩的不可变计划：保留了什么、折叠了什么、外置/去重了什么。
///
/// 计划本身只是 runner 上下文视图的编排结果，不是 EventLog 事实，也不改变
/// Invocation/approval/任务状态。
#[derive(Clone, Debug, PartialEq)]
pub struct CompactionPlan {
    /// 触发阈值（近似 token）。
    pub trigger_tokens: usize,
    /// 保留近期完整消息组的 token 预算。
    pub retain_tokens: usize,
    /// 压缩前的近似 token 数。
    pub tokens_before: usize,
    /// 已存在的摘要消息正文；新摘要生成后由新摘要取代。
    pub previous_summary: Option<String>,
    /// system/角色消息，原样保留。
    pub system: Vec<ModelMessage>,
    /// 折叠进摘要的较旧完整消息组（大结果已外置、重复观测已省略）。
    pub summarize: Vec<ModelMessage>,
    /// 保留的近期完整消息组（原文；只有最新一组可能被有界截断）。
    pub retained: Vec<ModelMessage>,
    /// 本次外置的大结果。
    pub externalized: Vec<ExternalizedResult>,
    /// 本次去重的重复观测。
    pub deduplicated: Vec<DuplicatedResult>,
    /// 最新一条用户目标文本（原文，最多 1024 字节）。
    pub latest_goal: String,
    /// 最新用户目标是否仍在保留区里（有界截断也算保留）。
    pub latest_goal_retained: bool,
    /// 最新一组是否因为超出保留预算而被有界截断（保留首尾）。
    pub latest_goal_truncated: bool,
    /// 未决 assistant/tool 对是否完整保留在保留区里。
    pub pending_pair_preserved: bool,
}

impl CompactionPlan {
    /// 折叠区非空时必须生成新摘要。
    pub fn needs_summary(&self) -> bool {
        !self.summarize.is_empty()
    }

    /// 计划覆盖的源消息条数（折叠区 + 保留区）。
    pub fn source_message_count(&self) -> u64 {
        (self.summarize.len() + self.retained.len()) as u64
    }
}

/// 一次压缩的可见解释：只含摘要与计数，不含消息正文。
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CompactionExplanation {
    pub schema: String,
    /// `model`（模型生成）/ `retained-previous`（沿用旧摘要）/ `truncate-only`（只截断）。
    pub summary_source: String,
    pub summary_digest: Option<String>,
    /// 计划覆盖的源消息条数。
    pub source_message_count: u64,
    /// 折叠进摘要的消息条数。
    pub summarized_message_count: u64,
    /// 保留的近期消息条数。
    pub retained_message_count: u64,
    pub tokens_before: usize,
    pub tokens_after: usize,
    pub latest_goal_retained: bool,
    pub latest_goal_truncated: bool,
    pub pending_pair_preserved: bool,
    pub externalized: Vec<ExternalizedResult>,
    pub deduplicated: Vec<DuplicatedResult>,
}

/// 规划一次压缩；未触发时返回 `Ok(None)`，容量不可能满足时返回稳定错误。
///
/// 规则（先外置/去重，再选较旧完整组）：
/// 1. 最新一组完整消息永远保留；它超出保留预算时做有界截断，截断后仍放不下就返回
///    `compact_latest_group_exceeds_budget`（调用方必须在任何模型调用前暂停）。
/// 2. 从新到旧累积保留区，第一个放不下的较旧组开始，连同更旧的组一起折叠进摘要，
///    避免出现「中间被丢、两头都在」的历史空洞。
/// 3. 折叠区内超过 [`EXTERNALIZE_RESULT_TOKENS`] 的工具观测被外置成 `result:<digest>`
///    引用；重复的工具观测只保留第一次出现，其余按 digest 计数省略。
/// 保留区始终是原文（只有最新一组可能被截断），因此未决 assistant/tool 对和最新目标
/// 不会在压缩中丢失。
pub fn plan_compaction(
    messages: &[ModelMessage],
    trigger_tokens: usize,
    retain_tokens: usize,
) -> Result<Option<CompactionPlan>, String> {
    let tokens_before = history_tokens(messages);
    if trigger_tokens == 0 || tokens_before <= trigger_tokens {
        return Ok(None);
    }
    let previous_summary = messages
        .iter()
        .find(|message| is_summary_message(&message.text))
        .map(|message| message.text.clone());
    let system: Vec<ModelMessage> = messages
        .iter()
        .filter(|message| message.role == ModelRole::System)
        .cloned()
        .collect();
    let mut groups = message_groups(messages);
    let newest = groups.pop();
    let mut remaining = retain_tokens;
    let mut latest_goal_truncated = false;
    let mut retained_groups: Vec<Vec<ModelMessage>> = Vec::new();
    if let Some(group) = newest {
        let tokens = history_tokens(&group);
        if tokens > remaining {
            let reduced = truncate_latest_group(group, remaining);
            if history_tokens(&reduced) > remaining {
                return Err("compact_latest_group_exceeds_budget".to_owned());
            }
            latest_goal_truncated = true;
            remaining = 0;
            retained_groups.push(reduced);
        } else {
            remaining -= tokens;
            retained_groups.push(group);
        }
    }
    let mut folded: Vec<Vec<ModelMessage>> = Vec::new();
    let mut folding = false;
    while let Some(group) = groups.pop() {
        if !folding {
            let tokens = history_tokens(&group);
            if tokens <= remaining {
                remaining -= tokens;
                retained_groups.insert(0, group);
                continue;
            }
            folding = true;
        }
        folded.insert(0, group);
    }
    let summarize_source: Vec<ModelMessage> = folded.into_iter().flatten().collect();
    let (summarize, externalized, deduplicated) = externalize_and_dedupe(summarize_source);
    let latest_goal = messages
        .iter()
        .rev()
        .find(|message| message.role == ModelRole::User && !is_summary_message(&message.text))
        .map(|message| message.text.clone())
        .unwrap_or_default();
    let latest_goal_retained = retained_groups
        .iter()
        .flatten()
        .any(|message| message.role == ModelRole::User && !is_summary_message(&message.text));
    let pending_pair_preserved = messages.last().is_some_and(|message| {
        message.role == ModelRole::Tool
            || (message.role == ModelRole::Assistant && !message.tool_calls.is_empty())
    });
    Ok(Some(CompactionPlan {
        trigger_tokens,
        retain_tokens,
        tokens_before,
        previous_summary,
        system,
        summarize,
        retained: retained_groups.into_iter().flatten().collect(),
        externalized,
        deduplicated,
        latest_goal: truncate_utf8(&latest_goal, 1_024),
        latest_goal_retained,
        latest_goal_truncated,
        pending_pair_preserved,
    }))
}

fn externalize_and_dedupe(
    messages: Vec<ModelMessage>,
) -> (
    Vec<ModelMessage>,
    Vec<ExternalizedResult>,
    Vec<DuplicatedResult>,
) {
    let mut summarize = Vec::new();
    let mut externalized = Vec::new();
    let mut deduplicated: Vec<(String, DuplicatedResult)> = Vec::new();
    let mut seen: Vec<(String, String)> = Vec::new();
    for message in messages {
        if message.role != ModelRole::Tool {
            summarize.push(message);
            continue;
        }
        let digest = json_digest(&serde_json::Value::String(message.text.clone()));
        let tokens = history_tokens(std::slice::from_ref(&message));
        let reference = format!("result:{digest}");
        if let Some((_, first_reference)) = seen.iter().find(|(seen, _)| *seen == digest) {
            if let Some((_, entry)) = deduplicated.iter_mut().find(|(seen, _)| *seen == digest) {
                entry.elided += 1;
            } else {
                deduplicated.push((
                    digest.clone(),
                    DuplicatedResult {
                        reference: first_reference.clone(),
                        digest: digest.clone(),
                        elided: 1,
                    },
                ));
            }
            let mut stub = message;
            stub.text = format!("[duplicate observation {first_reference} elided by compaction]");
            stub.content = Vec::new();
            stub.continuation = None;
            summarize.push(stub);
            continue;
        }
        seen.push((digest.clone(), reference.clone()));
        if tokens > EXTERNALIZE_RESULT_TOKENS {
            externalized.push(ExternalizedResult {
                reference: reference.clone(),
                digest,
                original_tokens: tokens,
            });
            let mut stub = message;
            stub.text = format!("[externalized result {reference}] original_tokens={tokens}");
            stub.content = Vec::new();
            stub.continuation = None;
            summarize.push(stub);
        } else {
            summarize.push(message);
        }
    }
    (
        summarize,
        externalized,
        deduplicated.into_iter().map(|(_, entry)| entry).collect(),
    )
}

fn render_for_summary(message: &ModelMessage, max_tokens: usize) -> String {
    let tools = if message.tool_calls.is_empty() {
        String::new()
    } else {
        let rendered = serde_json::to_string(&message.tool_calls).unwrap_or_default();
        format!(
            " tool_calls={}",
            truncate_middle_tokens(&rendered, max_tokens.min(256))
        )
    };
    let text = truncate_middle_tokens(&message.text, max_tokens);
    let id = message
        .tool_call_id
        .as_ref()
        .map(|id| format!(" tool_call_id={id}"))
        .unwrap_or_default();
    format!("[{}]{id}{tools} {text}", role_label(message.role))
}

/// 渲染摘要模型请求：一条系统指令加折叠区/旧摘要正文，整体有界。
///
/// 调用方必须把 `tools` 传空（domain 路由准入对 `purpose=compaction` 强制执行
/// `compaction_tools_denied`）；模型只能看到折叠区、旧摘要和指令，看不到保留区正文。
pub fn summary_request_messages(plan: &CompactionPlan) -> Vec<ModelMessage> {
    let mut messages = vec![ModelMessage::system(COMPACT_SUMMARY_INSTRUCTION)];
    let mut body = String::new();
    if let Some(previous) = plan.previous_summary.as_ref() {
        body.push_str("[previous summary to supersede]\n");
        body.push_str(&truncate_middle_tokens(
            previous,
            COMPACT_SUMMARY_PROMPT_MAX_TOKENS / 4,
        ));
        body.push('\n');
    }
    body.push_str("[history to summarize]\n");
    for message in &plan.summarize {
        body.push_str(&render_for_summary(message, 512));
        body.push('\n');
    }
    if plan.summarize.is_empty() {
        body.push_str("(no older complete groups)\n");
    }
    let body = truncate_middle_tokens(&body, COMPACT_SUMMARY_PROMPT_MAX_TOKENS);
    messages.push(ModelMessage::user(body));
    messages
}

/// 解析模型摘要，绑定服务端权威证据并盖上 digest。
///
/// 模型只能提供内容字段：`evidence` 一律被丢弃并替换成权威证据投影里已确认的条目，
/// `source_message_count` 由计划覆盖。任何引用都必须在权威证据里存在且状态为
/// `confirmed`，否则整份摘要被拒绝（不部分采纳）。
pub fn parse_compaction_summary(
    text: &str,
    plan: &CompactionPlan,
    authoritative: &[CompactSummaryEvidence],
) -> Result<CompactSummary, String> {
    let mut summary = CompactSummary::parse_model_output(text)?;
    summary.source_message_count = plan.source_message_count().max(1);
    summary.source_event_start = None;
    summary.source_event_end = None;
    summary.evidence = Vec::new();
    let mut evidence: Vec<CompactSummaryEvidence> = Vec::new();
    let groups: [(&[String], &[CompactEvidenceKind]); 3] = [
        (
            summary.decision_refs.as_slice(),
            &[CompactEvidenceKind::Decision, CompactEvidenceKind::Approval],
        ),
        (
            summary.completed_action_refs.as_slice(),
            &[CompactEvidenceKind::CompletedAction],
        ),
        (
            summary.verification_refs.as_slice(),
            &[CompactEvidenceKind::Verification],
        ),
    ];
    for (references, kinds) in groups {
        for reference in references {
            let Some(fact) = authoritative.iter().find(|candidate| {
                candidate.reference == *reference && kinds.contains(&candidate.kind)
            }) else {
                return Err("compact_summary_evidence_not_authoritative".to_owned());
            };
            if fact.status != CompactEvidenceStatus::Confirmed {
                return Err("compact_summary_evidence_not_confirmed".to_owned());
            }
            if !evidence.contains(fact) {
                evidence.push(fact.clone());
            }
        }
    }
    summary.evidence = evidence;
    summary.stamp_digest()?;
    Ok(summary)
}

/// 校验摘要与计划的边界绑定：来源条数、最近指令、摘要引用。
pub fn validate_summary_for_plan(
    plan: &CompactionPlan,
    summary: &CompactSummary,
) -> Result<(), String> {
    summary.validate()?;
    if summary.source_message_count != plan.source_message_count().max(1) {
        return Err("compact_summary_source_range_invalid".to_owned());
    }
    if !plan.latest_goal_retained || plan.latest_goal.trim().is_empty() {
        return Err("compact_summary_latest_goal_missing".to_owned());
    }
    Ok(())
}

/// 用计划与（可选）新摘要构造压缩后的视图。
///
/// 视图顺序固定为：system/角色消息、摘要（新摘要优先，其次旧摘要）、保留区。没有新摘要
/// 也没有旧摘要时视图只有截断后的保留区；摘要超过 [`COMPACT_SUMMARY_MAX_TOKENS`] 时
/// 返回 `compact_summary_capacity_exceeded`。
pub fn build_compacted_view(
    plan: &CompactionPlan,
    summary: Option<&CompactSummary>,
) -> Result<Vec<ModelMessage>, String> {
    let mut view = plan.system.clone();
    let summary_message = match summary {
        Some(summary) => {
            summary.validate()?;
            let message = ModelMessage::user(format!("{SUMMARY_PREFIX}\n{}", summary.render()?));
            if history_tokens(std::slice::from_ref(&message)) > COMPACT_SUMMARY_MAX_TOKENS {
                return Err("compact_summary_capacity_exceeded".to_owned());
            }
            Some(message)
        }
        None => plan
            .previous_summary
            .as_ref()
            .map(|text| ModelMessage::user(text.clone())),
    };
    if let Some(message) = summary_message {
        view.push(message);
    }
    view.extend(plan.retained.iter().cloned());
    Ok(view)
}

/// 从计划与结果构造可写进事件/收据的解释对象。
pub fn explain_compaction(
    plan: &CompactionPlan,
    summary: Option<&CompactSummary>,
    summary_source: &str,
    tokens_after: usize,
) -> CompactionExplanation {
    CompactionExplanation {
        schema: COMPACTION_EXPLANATION_SCHEMA.to_owned(),
        summary_source: summary_source.to_owned(),
        summary_digest: summary.map(|summary| summary.summary_digest.clone()),
        source_message_count: plan.source_message_count(),
        summarized_message_count: plan.summarize.len() as u64,
        retained_message_count: plan.retained.len() as u64,
        tokens_before: plan.tokens_before,
        tokens_after,
        latest_goal_retained: plan.latest_goal_retained,
        latest_goal_truncated: plan.latest_goal_truncated,
        pending_pair_preserved: plan.pending_pair_preserved,
        externalized: plan.externalized.clone(),
        deduplicated: plan.deduplicated.clone(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recent_group_that_cannot_fit_is_rejected_without_dropping_tool_pairs() {
        let messages = vec![
            ModelMessage::user("goal"),
            ModelMessage::assistant_with_tools(
                "call",
                vec![crate::model::ModelToolCall {
                    id: "pending".to_owned(),
                    name: "shell".to_owned(),
                    arguments: serde_json::json!({"command":"true"}),
                }],
            ),
            ModelMessage::tool("pending", "result".repeat(100)),
        ];
        let out = compact_if_needed(messages.clone(), 1, 4);
        assert_eq!(
            out.error.as_deref(),
            Some("compact_latest_group_exceeds_budget")
        );
        assert!(!out.applied);
        assert_eq!(out.messages, messages);
    }

    #[test]
    fn truncation_accounts_for_message_framing_and_other_group_messages() {
        let messages = vec![
            ModelMessage::user("x".repeat(100)),
            ModelMessage::assistant("done"),
        ];
        let summary = CompactSummary::from_messages(&messages).unwrap();
        let compacted = build_compacted_history_with_summary(&messages, 5, &summary).unwrap();
        let recent: Vec<_> = compacted
            .into_iter()
            .filter(|m| !is_summary_message(&m.text))
            .collect();
        assert!(history_tokens(&recent) <= 5);
        assert_eq!(recent.len(), 2);
        assert_eq!(recent[1], messages[1]);
        assert!(!recent[0].text.is_empty());
    }

    #[test]
    fn compacted_history_keeps_recent_group_after_validated_summary() {
        let messages = vec![
            ModelMessage::user("old-1"),
            ModelMessage::assistant("ignored assistant"),
            ModelMessage::user("keep-me-recent"),
        ];
        let compacted = build_compacted_history(&messages, 5);
        assert!(compacted
            .iter()
            .all(|message| message.role == ModelRole::User));
        assert!(!compacted
            .iter()
            .any(|message| message.text == "ignored assistant"));
        assert_eq!(
            compacted
                .first()
                .map(|message| is_summary_message(&message.text)),
            Some(true)
        );
        assert!(compacted
            .iter()
            .any(|message| message.text == "keep-me-recent"));
    }

    #[test]
    fn compact_if_needed_is_noop_under_trigger() {
        let messages = vec![ModelMessage::user("short")];
        let out = compact_if_needed(messages.clone(), 1_000, 20);
        assert!(!out.applied);
        assert_eq!(out.messages, messages);
        assert_eq!(out.tokens_after, out.tokens_before);
    }

    #[test]
    fn compact_if_needed_replaces_over_budget_history() {
        let messages = vec![
            ModelMessage::user("a".repeat(80)),
            ModelMessage::assistant("tooling"),
            ModelMessage::user("b".repeat(80)),
        ];
        let out = compact_if_needed(messages, 8, 4);
        assert!(out.applied);
        assert!(out.summary_present);
        assert!(is_summary_message(&out.messages.first().unwrap().text));
        let rendered = out
            .messages
            .first()
            .unwrap()
            .text
            .strip_prefix(&format!("{SUMMARY_PREFIX}\n"))
            .unwrap();
        serde_json::from_str::<CompactSummary>(rendered)
            .unwrap()
            .validate()
            .unwrap();
        assert!(out.error.is_none());
        assert!(history_tokens(&out.messages[1..]) <= 4);
        assert!(!out
            .messages
            .iter()
            .any(|message| message.role == ModelRole::Assistant));
    }

    #[test]
    fn compact_outcome_shrinks_large_discarded_history() {
        let messages = vec![ModelMessage::user("a".repeat(8000))];
        let out = compact_if_needed(messages, 200, 40);
        assert!(out.applied);
        assert!(out.summary_present);
        assert!(
            out.tokens_after < out.tokens_before,
            "before={} after={}",
            out.tokens_before,
            out.tokens_after
        );
    }

    #[test]
    fn cm19_compaction_keeps_latest_goal_and_pending_pairs() {
        let messages = vec![
            ModelMessage::system("product rules"),
            ModelMessage::user("old goal"),
            ModelMessage::assistant_with_tools(
                "old call",
                vec![crate::model::ModelToolCall {
                    id: "old-call".to_owned(),
                    name: "shell".to_owned(),
                    arguments: serde_json::json!({"command":"true"}),
                }],
            ),
            ModelMessage::tool("old-call", "old result"),
            ModelMessage::user("latest goal"),
            ModelMessage::assistant_with_tools(
                "pending call",
                vec![crate::model::ModelToolCall {
                    id: "pending-call".to_owned(),
                    name: "shell".to_owned(),
                    arguments: serde_json::json!({"command":"pending"}),
                }],
            ),
            ModelMessage::tool("pending-call", "pending result"),
        ];
        let summary = CompactSummary::from_messages(&messages).unwrap();
        let compacted = build_compacted_history_with_summary(&messages, 64, &summary).unwrap();
        assert!(compacted
            .iter()
            .any(|message| message.text.contains("latest goal")));
        assert!(compacted.iter().any(|message| {
            message.role == ModelRole::Assistant
                && message
                    .tool_calls
                    .iter()
                    .any(|call| call.id == "pending-call")
        }));
        assert!(compacted.iter().any(|message| {
            message.role == ModelRole::Tool
                && message.tool_call_id.as_deref() == Some("pending-call")
        }));
    }
}

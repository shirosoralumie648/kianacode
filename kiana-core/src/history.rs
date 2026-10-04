//! 从事件账本重建「模型能看见的那部分对话历史」。
//!
//! # 关键性质：历史不是存出来的，是**重建**出来的
//!
//! 这里没有「读一段 transcript 然后交给模型」这种做法。
//! 模型可见历史是**从 EventLog 里重新折叠**出来的——而这正是它值得单独一个文件的原因：
//! transcript 是可丢弃的展示视图，事件账本才是事实。
//! 折叠出来的东西，天然继承了账本的一切约束：撤销、隔离、拒绝，都在账本里。
//!
//! # 为什么要「只给模型看一部分」
//!
//! 一次会话里发生过的所有事，不等于模型该看到的所有事。
//! 被撤销的数据、被恢复覆盖的工��、被拒绝的动作——它们在账本里存在，
//! 但它们**不该**作为上下文回到模型眼前。
//! 可见性因此是一个需要判断的事，而不是一个「全量传过去」的默认值。
use super::*;
use kiana_domain::{ConversationMessage, ConversationRole};
use std::collections::{HashMap, HashSet};

/// 能力事件里**允许**进入模型上下文的元数据键。
///
/// 【这是一份允许清单】
/// 不是「要排除哪些」，而是「哪些可以」——默认是不给。
/// 换句话说，将来有人往能力事件里加一个新字段，它**不会**自动出现在模型眼前；
/// 要出现必须先被加进这个列表，那是一次有意的、被 review 看见的动作。
///
/// 【为什么这些键是安全的】
/// 它们都是**标识**（run_id / session_id / cell_id / grant_id / lease_id）
/// 和**操作名**（capability / operation）——没有 payload，没有参数，没有内容。
/// 模型看到「这个 run 做过一次 file_write」，看不到「它写了什么」。
const CAPABILITY_EVENT_METADATA_KEYS: &[&str] = &[
    "run_id",
    "session_id",
    "capability",
    "operation",
    "cell_id",
    "capability_grant_id",
    "budget_lease_id",
    "capability_request_id",
];

impl ControlPlane {
    /// 从事件账本只读重建某个 session（可选限定 run）的模型可见历史。
    ///
    /// 存储明确不支持全量读取或账本中没有该 session 的事件时返回空历史；真实读取
    /// 失败仍原样返回错误。未知事件 kind 会被跳过，不会被猜测成对话消息。
    /// 重建某个会话（可选限定 run）的模型可见历史。
    ///
    /// 【三种「空」要分清，函数把它们区分开了】
    /// - 存储不支持全量读取            → 空历史
    /// - 账本里没有这个 session 的事件  → 空历史
    /// - **真的读失败了**               → 原样返回错误
    ///
    /// 前两种是「确实没有」，第三种是「不知道」。把它们混成一种，
    /// 就会在读失败时把「读不到」显示成「没有历史」——界面看起来是空的，
    /// 而实际上系统并不知道答案。
    ///
    /// 【撤销的处理是这里最要紧的一段】
    /// 函数会先扫一遍账本，找出那些后来被
    /// `data.revocation_requested` / `workspace.restore_requested` / `workspace.restored`
    /// 波及的 run，把它们标记为 revoked，然后在重建时**跳过**。
    ///
    /// 也就是说：数据被撤销之后，它不会因为「历史里曾经有过」而重新回到模型眼前。
    /// 这是 `redaction.rs` 那条纪律在对话层的落点——不可逆的东西不能被读回来。
    ///
    /// 【未知事件 kind 被跳过，不被猜成对话消息】
    /// 账本里可能有许多与对话无关的事件。遇到不认识的一跳过去，
    /// 而不是「反正大概是条消息吧」——猜出来的东西会以对话的形式进入上下文，
    /// 而它可能根本不是。
    pub async fn model_visible_history(
        &self,
        session_id: &str,
        run_id: Option<RunId>,
    ) -> Result<Vec<ConversationMessage>, CoreError> {
        let Some(mut events) = self.read_all_events().await? else {
            return Ok(Vec::new());
        };
        let mut revoked_runs = HashSet::new();
        for (index, event) in events.iter().enumerate().filter(|(_, event)| {
            event.kind == "run.authorized" && event.data["session_id"] == session_id
        }) {
            if events[index + 1..].iter().any(|later| {
                matches!(
                    later.kind.as_str(),
                    "data.revocation_requested"
                        | "workspace.restore_requested"
                        | "workspace.restored"
                ) && later.data["project_root"] == event.data["project_root"]
            }) {
                if let Some(id) = event.data["run_id"].as_str() {
                    revoked_runs.insert(id.to_owned());
                }
            }
        }
        let revoked_requests = events
            .iter()
            .filter(|event| {
                event.data["run_id"]
                    .as_str()
                    .is_some_and(|id| revoked_runs.contains(id))
            })
            .map(|event| event.request_id)
            .collect::<HashSet<_>>();
        events.retain(|event| !revoked_requests.contains(&event.request_id));
        Ok(fold_model_visible_history(&events, session_id, run_id))
    }
}

impl ControlPlane {
    /// Reconstruct complete assistant/tool pairs from the authoritative ledger.
    /// Streaming text is a projection only; an incomplete attempt is never a completed message.
    pub(crate) async fn model_protocol_history(
        &self,
        session_id: &str,
    ) -> Result<Vec<kiana_domain::ModelMessage>, CoreError> {
        use kiana_domain::{ModelMessage, ModelOutput};
        let Some(events) = self.read_all_events().await? else {
            return Ok(Vec::new());
        };
        let mut selected = session_run_ids(&events, session_id);
        for (index, event) in events.iter().enumerate().filter(|(_, event)| {
            event.kind == "run.authorized" && event.data["session_id"] == session_id
        }) {
            if events[index + 1..].iter().any(|later| {
                matches!(
                    later.kind.as_str(),
                    "data.revocation_requested"
                        | "workspace.restore_requested"
                        | "workspace.restored"
                ) && later.data["project_root"] == event.data["project_root"]
            }) {
                if let Some(run) = event_run_id(event) {
                    selected.remove(&run);
                }
            }
        }
        let exact_runs = events
            .iter()
            .filter(|event| {
                event.kind == "run.model_turn" && event.data["schema"] == "kiana.model-turn.v2"
            })
            .filter_map(event_run_id)
            .collect::<HashSet<_>>();
        let calls = capability_call_ids(&events, &selected);
        let mut history = Vec::new();
        let mut seen = HashSet::new();
        let mut closed = HashSet::new();
        for event in &events {
            if !event_is_in_scope(event, session_id, &selected) || !seen.insert(event.event_id) {
                continue;
            }
            let run = event_run_id(event).expect("scoped event has a run");
            match event.kind.as_str() {
                "run.prompt" => {
                    if let Some(text) = event.data["text"].as_str() {
                        history.push(ModelMessage::user(text));
                    }
                }
                "run.model_turn" if event.data["purpose"] == "task" => {
                    if let Some(value) =
                        event.data.get("assistant").filter(|value| !value.is_null())
                    {
                        let output: ModelOutput =
                            serde_json::from_value(value.clone()).map_err(|_| {
                                CoreError::from(PortError::Failed(
                                    "model_history_assistant_invalid".to_owned(),
                                ))
                            })?;
                        history.push(ModelMessage::assistant_with_tools(
                            output.text,
                            output.tool_calls,
                        ));
                    }
                }
                "run.delta" if !exact_runs.contains(&run) => {
                    if let Some(text) = event.data["text"].as_str() {
                        history.push(ModelMessage::assistant(text));
                    }
                }
                "capability.completed" | "capability.failed" => {
                    let call_id = event.data["capability_request_id"]
                        .as_str()
                        .and_then(|id| calls.get(id))
                        .cloned()
                        .flatten();
                    if let Some(id) = call_id.filter(|id| closed.insert((run, id.clone()))) {
                        let text = capability_result_text(&event.data);
                        if exact_runs.contains(&run) {
                            history.push(ModelMessage::tool(id, text));
                        } else {
                            history.push(ModelMessage::user(format!(
                                "[Legacy tool observation; no replay authority]\n{text}"
                            )));
                        }
                    }
                }
                "run.tool_result" if exact_runs.contains(&run) => {
                    if let Some(id) = event.data["call_id"]
                        .as_str()
                        .filter(|id| closed.insert((run, (*id).to_owned())))
                    {
                        history.push(ModelMessage::tool(id, event.data["result"].to_string()));
                    }
                }
                _ => {}
            }
        }
        kiana_domain::validate_model_history(&history).map_err(|error| {
            CoreError::from(PortError::Failed(format!(
                "model_history_unrecoverable:{error}"
            )))
        })?;
        Ok(history)
    }
}

pub(crate) fn fold_model_visible_history(
    events: &[RuntimeEvent],
    session_id: &str,
    requested_run_id: Option<RunId>,
) -> Vec<ConversationMessage> {
    let session_runs = session_run_ids(events, session_id);
    if session_runs.is_empty() {
        return Vec::new();
    }
    let selected_runs = match requested_run_id {
        Some(run_id) if session_runs.contains(&run_id) => HashSet::from([run_id]),
        Some(_) => return Vec::new(),
        None => session_runs,
    };
    let run_order = selected_run_order(events, &selected_runs);
    let capability_call_ids = capability_call_ids(events, &selected_runs);

    let mut seen = HashSet::new();
    let mut ordered = Vec::new();
    for (index, event) in events.iter().enumerate() {
        if !is_history_event(event) || !event_is_in_scope(event, session_id, &selected_runs) {
            continue;
        }
        // `sequence` is request-local. Bind the fold to the request even for legacy events
        // without stream metadata, otherwise two requests in one run with the same sequence
        // would be mistaken for a duplicate. Modern events use the aggregate version when it
        // is available, while the request id remains part of the identity boundary.
        let event_order = event.stream_version.unwrap_or(event.sequence);
        let dedupe_key = (event_run_id(event), event.request_id, event_order);
        if !seen.insert(dedupe_key) {
            continue;
        }
        ordered.push((index, event));
    }

    ordered.sort_by_key(|(index, event)| {
        let run_rank = event_run_id(event)
            .and_then(|run_id| run_order.get(&run_id).copied())
            .unwrap_or(usize::MAX);
        (run_rank, event_order, *index)
    });

    let mut history = Vec::new();
    for (_, event) in ordered {
        match event.kind.as_str() {
            "run.prompt" => {
                if let Some(text) = event.data.get("text").and_then(Value::as_str) {
                    history.push(ConversationMessage {
                        role: ConversationRole::User,
                        text: text.to_owned(),
                        tool_call_id: None,
                    });
                }
            }
            "run.delta" => {
                if let Some(text) = event.data.get("text").and_then(Value::as_str) {
                    history.push(ConversationMessage {
                        role: ConversationRole::Assistant,
                        text: text.to_owned(),
                        tool_call_id: None,
                    });
                }
            }
            "capability.completed" | "capability.failed" => {
                let tool_call_id = event
                    .data
                    .get("capability_request_id")
                    .and_then(Value::as_str)
                    .and_then(|request_id| capability_call_ids.get(request_id))
                    .cloned()
                    .flatten();
                history.push(ConversationMessage {
                    role: ConversationRole::Tool,
                    text: capability_result_text(&event.data),
                    tool_call_id,
                });
            }
            _ => {}
        }
    }
    history
}

fn is_history_event(event: &RuntimeEvent) -> bool {
    matches!(
        event.kind.as_str(),
        "run.prompt" | "run.delta" | "capability.completed" | "capability.failed"
    )
}

fn event_run_id(event: &RuntimeEvent) -> Option<RunId> {
    event
        .data
        .get("run_id")
        .and_then(Value::as_str)
        .and_then(RunId::parse_str)
}

fn event_is_in_scope(
    event: &RuntimeEvent,
    session_id: &str,
    selected_runs: &HashSet<RunId>,
) -> bool {
    let Some(run_id) = event_run_id(event) else {
        return false;
    };
    if !selected_runs.contains(&run_id) {
        return false;
    }
    event
        .data
        .get("session_id")
        .and_then(Value::as_str)
        .is_none_or(|event_session_id| event_session_id == session_id)
}

fn session_run_ids(events: &[RuntimeEvent], session_id: &str) -> HashSet<RunId> {
    events
        .iter()
        .filter_map(|event| {
            let run_id = event_run_id(event)?;
            let event_session_id = event.data.get("session_id").and_then(Value::as_str)?;
            (event_session_id == session_id).then_some(run_id)
        })
        .collect()
}

fn selected_run_order(
    events: &[RuntimeEvent],
    selected_runs: &HashSet<RunId>,
) -> HashMap<RunId, usize> {
    let mut order = HashMap::new();
    for (index, event) in events.iter().enumerate() {
        if let Some(run_id) = event_run_id(event) {
            if selected_runs.contains(&run_id) {
                order.entry(run_id).or_insert(index);
            }
        }
    }
    order
}

fn capability_call_ids(
    events: &[RuntimeEvent],
    selected_runs: &HashSet<RunId>,
) -> HashMap<String, Option<String>> {
    let mut call_ids = HashMap::new();
    for event in events {
        if event.kind != "run.capability_requested" {
            continue;
        }
        let Some(run_id) = event_run_id(event) else {
            continue;
        };
        if !selected_runs.contains(&run_id) {
            continue;
        }
        let Some(request_id) = event.data.get("request_id").and_then(Value::as_str) else {
            continue;
        };
        let call_id = event
            .data
            .get("arguments")
            .and_then(|arguments| arguments.get("call_id"))
            .and_then(Value::as_str)
            .map(str::to_owned);
        call_ids.entry(request_id.to_owned()).or_insert(call_id);
    }
    call_ids
}

fn capability_result_text(data: &Value) -> String {
    let output = match data.as_object() {
        Some(object) => {
            let mut output = object.clone();
            for key in CAPABILITY_EVENT_METADATA_KEYS {
                output.remove(*key);
            }
            if output.len() == 1 {
                match output.remove("output") {
                    Some(value) => value,
                    None => Value::Object(output),
                }
            } else {
                Value::Object(output)
            }
        }
        None => data.clone(),
    };
    match output {
        Value::String(text) => text,
        value => serde_json::to_string(&value).unwrap_or_else(|_| "null".to_owned()),
    }
}

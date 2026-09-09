use super::*;
use kiana_domain::{ConversationMessage, ConversationRole};
use std::collections::{HashMap, HashSet};

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
    pub async fn model_visible_history(
        &self,
        session_id: &str,
        run_id: Option<RunId>,
    ) -> Result<Vec<ConversationMessage>, CoreError> {
        let Some(events) = self.read_all_events().await? else {
            return Ok(Vec::new());
        };
        Ok(fold_model_visible_history(&events, session_id, run_id))
    }
}

fn fold_model_visible_history(
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
        // `sequence` is request-local. Real ledger events also carry a run-level stream
        // version; only fall back to sequence-wide first-wins deduplication for legacy
        // events that lack that metadata.
        let request_scope = event.stream_version.map(|_| event.request_id);
        let dedupe_key = (event_run_id(event), request_scope, event.sequence);
        if !seen.insert(dedupe_key) {
            continue;
        }
        ordered.push((index, event));
    }

    ordered.sort_by_key(|(index, event)| {
        let run_rank = event_run_id(event)
            .and_then(|run_id| run_order.get(&run_id).copied())
            .unwrap_or(usize::MAX);
        let event_order = event.stream_version.unwrap_or(event.sequence);
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

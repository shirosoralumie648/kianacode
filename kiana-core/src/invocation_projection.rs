use kiana_domain::{CapabilityExecutionState, RequestId, RunId, RuntimeEvent};
use serde::Serialize;
use serde_json::Value;
use std::collections::BTreeMap;

#[derive(Clone, Debug, Serialize)]
pub struct InvocationProjection {
    pub request_id: RequestId,
    pub call_id: Option<String>,
    pub operation: Option<String>,
    pub state: CapabilityExecutionState,
    pub args_fingerprint: Option<String>,
    pub result: Option<Value>,
    pub event_ids: Vec<String>,
}

/// Pure event projection. A dispatch without a terminal result is unknown after restart.
pub fn project_invocations(
    run_id: RunId,
    events: &[RuntimeEvent],
) -> Result<Vec<InvocationProjection>, String> {
    let mut projections: BTreeMap<String, InvocationProjection> = BTreeMap::new();
    let mut terminal: BTreeMap<String, CapabilityExecutionState> = BTreeMap::new();
    for event in events {
        if event.data.get("run_id").and_then(Value::as_str) != Some(run_id.to_string().as_str()) {
            continue;
        }
        let Some(id) = event
            .data
            .get("capability_request_id")
            .or_else(|| {
                (event.kind == "run.capability_requested")
                    .then(|| event.data.get("request_id"))
                    .flatten()
            })
            .and_then(|value| serde_json::from_value::<RequestId>(value.clone()).ok())
        else {
            continue;
        };
        let key = id.to_string();
        let entry = projections
            .entry(key.clone())
            .or_insert_with(|| InvocationProjection {
                request_id: id,
                call_id: None,
                operation: None,
                state: CapabilityExecutionState::Requested,
                args_fingerprint: None,
                result: None,
                event_ids: Vec::new(),
            });
        entry.event_ids.push(event.event_id.to_string());
        for (name, target) in [
            ("call_id", &mut entry.call_id),
            ("operation", &mut entry.operation),
            ("args_fingerprint", &mut entry.args_fingerprint),
        ] {
            if let Some(value) = event.data.get(name).and_then(Value::as_str) {
                *target = Some(value.to_owned());
            }
        }
        let next = match event.kind.as_str() {
            "invocation.dispatching" => Some(CapabilityExecutionState::Unknown),
            "execution.result_committed" => Some(if event.data["effect_known"] != true {
                CapabilityExecutionState::Unknown
            } else if event.data["result"]["output"]["cancelled"] == true {
                CapabilityExecutionState::Cancelled
            } else if event.data["result"]["success"] == true {
                CapabilityExecutionState::Succeeded
            } else {
                CapabilityExecutionState::Failed
            }),
            "capability.completed" => Some(CapabilityExecutionState::Succeeded),
            "capability.failed" => Some(CapabilityExecutionState::Failed),
            "capability.cancelled" => Some(CapabilityExecutionState::Cancelled),
            "capability.result_unknown" => Some(CapabilityExecutionState::Unknown),
            "run.tool_result" if event.data["not_executed"] == true => {
                Some(CapabilityExecutionState::Cancelled)
            }
            _ => None,
        };
        if let Some(next) = next {
            if event.kind != "invocation.dispatching" {
                if terminal
                    .insert(key, next)
                    .is_some_and(|previous| previous != next)
                {
                    return Err("invocation_terminal_conflict".to_owned());
                }
                entry.result = Some(event.data.get("result").unwrap_or(&event.data).clone());
            }
            entry.state = next;
        }
    }
    Ok(projections.into_values().collect())
}

use kiana_domain::{
    ApprovalId, CapabilityExecutionState, CapabilityKind, CapabilityRequest, RequestId, RiskLevel,
    RunId, RuntimeEvent,
};
use serde::Serialize;
use serde_json::Value;
use std::collections::{BTreeMap, HashMap, HashSet};

#[derive(Clone, Debug, Serialize)]
pub struct InvocationProjection {
    pub request_id: RequestId,
    pub call_id: Option<String>,
    pub operation: Option<String>,
    pub state: CapabilityExecutionState,
    pub args_fingerprint: Option<String>,
    pub result: Option<Value>,
    pub event_ids: Vec<String>,
    // These are recovery-only material. Receipts expose the stable projection fields above,
    // while request arguments and approval records remain in their redacted event streams.
    #[serde(skip_serializing)]
    pub request: Option<CapabilityRequest>,
    #[serde(skip_serializing)]
    pub approval_id: Option<ApprovalId>,
}

fn event_run_id(event: &RuntimeEvent) -> Result<Option<RunId>, String> {
    let aggregate_run_id = match (
        event.aggregate_type.as_deref(),
        event.aggregate_id.as_deref(),
    ) {
        (Some("run"), Some(value)) => Some(
            RunId::parse_str(value).ok_or_else(|| "invocation_event_run_id_invalid".to_owned())?,
        ),
        (Some("run"), None) => return Err("invocation_event_run_id_invalid".to_owned()),
        _ => None,
    };
    let payload_run_id = match event.data.get("run_id") {
        None | Some(Value::Null) => None,
        Some(Value::String(value)) => Some(
            RunId::parse_str(value).ok_or_else(|| "invocation_event_run_id_invalid".to_owned())?,
        ),
        Some(_) => return Err("invocation_event_run_id_invalid".to_owned()),
    };
    if let (Some(payload), Some(aggregate)) = (payload_run_id, aggregate_run_id) {
        if payload != aggregate {
            return Err("invocation_event_run_id_conflict".to_owned());
        }
    }
    Ok(payload_run_id.or(aggregate_run_id))
}

fn event_matches_run(event: &RuntimeEvent, run_id: RunId) -> Result<bool, String> {
    Ok(event_run_id(event)?.is_some_and(|event_run_id| event_run_id == run_id))
}

fn request_id_for(event: &RuntimeEvent) -> Option<RequestId> {
    event
        .data
        .get("capability_request_id")
        .or_else(|| {
            (event.kind == "run.capability_requested")
                .then(|| event.data.get("request_id"))
                .flatten()
        })
        .or_else(|| {
            matches!(event.kind.as_str(), "approval.approved" | "approval.denied")
                .then(|| event.data.get("subject_request_id"))
                .flatten()
        })
        .and_then(|value| serde_json::from_value::<RequestId>(value.clone()).ok())
}

fn string_field(event: &RuntimeEvent, name: &str) -> Option<String> {
    event
        .data
        .get(name)
        .and_then(Value::as_str)
        .map(str::to_owned)
}

fn terminal_state(event: &RuntimeEvent) -> Result<Option<CapabilityExecutionState>, String> {
    let state = match event.kind.as_str() {
        "execution.result_committed" => {
            let effect_known = event
                .data
                .get("effect_known")
                .and_then(Value::as_bool)
                .ok_or_else(|| "invocation_result_malformed".to_owned())?;
            let result = event
                .data
                .get("result")
                .ok_or_else(|| "invocation_result_malformed".to_owned())?;
            if !result.is_object() {
                return Err("invocation_result_malformed".to_owned());
            }
            if !effect_known {
                CapabilityExecutionState::Unknown
            } else if result["output"]["cancelled"] == true {
                CapabilityExecutionState::Cancelled
            } else if result
                .get("success")
                .and_then(Value::as_bool)
                .is_some_and(|success| success)
            {
                CapabilityExecutionState::Succeeded
            } else if result.get("success").and_then(Value::as_bool).is_some() {
                CapabilityExecutionState::Failed
            } else {
                return Err("invocation_result_malformed".to_owned());
            }
        }
        "capability.completed" => CapabilityExecutionState::Succeeded,
        "capability.failed" => CapabilityExecutionState::Failed,
        "capability.cancelled" => CapabilityExecutionState::Cancelled,
        "capability.result_unknown" => CapabilityExecutionState::Unknown,
        // `run.tool_result` is a runner bookkeeping fact. It is terminal only when the
        // cancellation is explicit; approval denial and gate rejection also emit a
        // not-executed result, but their authoritative terminal facts are approval.denied or
        // run.capability_blocked. Treating every not-executed result as cancellation creates a
        // false Denied -> Cancelled conflict during recovery.
        "run.tool_result"
            if event.data["cancelled"] == true
                || event
                    .data
                    .get("result")
                    .and_then(|result| result.get("error"))
                    .and_then(Value::as_str)
                    .is_some_and(|error| error.starts_with("cancelled:")) =>
        {
            CapabilityExecutionState::Cancelled
        }
        "approval.denied" | "run.capability_blocked" => CapabilityExecutionState::Denied,
        _ => return Ok(None),
    };
    Ok(Some(state))
}

fn intermediate_state(event: &RuntimeEvent) -> Option<CapabilityExecutionState> {
    match event.kind.as_str() {
        "run.capability_requested" => Some(CapabilityExecutionState::Requested),
        "capability.decision" => match event.data["gate"]["decision"].as_str() {
            Some("awaiting_approval") => Some(CapabilityExecutionState::AwaitingApproval),
            Some("allowed") => Some(CapabilityExecutionState::Authorized),
            Some("denied") => Some(CapabilityExecutionState::Denied),
            _ => Some(CapabilityExecutionState::PolicyChecked),
        },
        "approval.requested" | "run.awaiting_approval" => {
            Some(CapabilityExecutionState::AwaitingApproval)
        }
        "approval.approved" => Some(CapabilityExecutionState::Authorized),
        "invocation.dispatching" => Some(CapabilityExecutionState::Dispatching),
        "invocation.executing" => Some(CapabilityExecutionState::Executing),
        _ => None,
    }
}

fn terminal_signature(event: &RuntimeEvent) -> String {
    // Execution evidence and runner delivery facts use different envelopes. Their durable
    // result payload is still the same capability output, so compare that normalized value
    // instead of letting metadata differences hide a conflicting terminal result.
    let mut result = match event.kind.as_str() {
        "execution.result_committed" => event
            .data
            .get("result")
            .and_then(|result| result.get("output"))
            .or_else(|| event.data.get("result"))
            .cloned()
            .unwrap_or(Value::Null),
        "run.tool_result" => event.data.get("result").cloned().unwrap_or(Value::Null),
        // `capability.*` events use a flat payload: handler output is merged into the event
        // object before correlation metadata is added. Do not unwrap a user/MCP field named
        // `result`; it is part of the durable output, not an envelope.
        _ => event.data.clone(),
    };
    if let Value::Object(object) = &mut result {
        for key in [
            "run_id",
            "session_id",
            "capability",
            "operation",
            "cell_id",
            "capability_grant_id",
            "budget_lease_id",
            "capability_request_id",
            "approval_id",
            "request_hash",
            "decision",
            "scope",
            "expires_at_unix_ms",
        ] {
            object.remove(key);
        }
    }
    kiana_domain::json_digest(&result)
}

fn is_invocation_event(kind: &str) -> bool {
    matches!(
        kind,
        "run.tool_call"
            | "run.capability_requested"
            | "capability.decision"
            | "approval.requested"
            | "run.awaiting_approval"
            | "approval.approved"
            | "approval.denied"
            | "run.capability_blocked"
            | "invocation.dispatching"
            | "invocation.executing"
            | "execution.result_committed"
            | "capability.completed"
            | "capability.failed"
            | "capability.cancelled"
            | "capability.result_unknown"
            | "run.tool_result"
    )
}

fn merge_optional_field(
    current: &mut Option<String>,
    next: Option<String>,
    conflict: &'static str,
) -> Result<(), String> {
    let Some(next) = next else {
        return Ok(());
    };
    if let Some(previous) = current.as_ref() {
        if previous != &next {
            return Err(conflict.to_owned());
        }
    } else {
        *current = Some(next);
    }
    Ok(())
}

fn transition_allowed(
    previous: CapabilityExecutionState,
    next: CapabilityExecutionState,
    event: &RuntimeEvent,
) -> bool {
    if previous == next || previous.can_transition_to(next) {
        return true;
    }
    match (previous, next) {
        // A capability.decision records policy and gate evaluation together. It is therefore a
        // valid compressed Requested -> Authorized/AwaitingApproval/Denied transition.
        (CapabilityExecutionState::Requested, CapabilityExecutionState::Authorized)
        | (CapabilityExecutionState::Requested, CapabilityExecutionState::AwaitingApproval)
        | (CapabilityExecutionState::Requested, CapabilityExecutionState::Denied)
            if event.kind == "capability.decision" =>
        {
            true
        }
        // A preparation failure is durably recorded as a blocked fact after the redacted
        // request snapshot, so it is terminal without a policy decision or broker grant.
        (CapabilityExecutionState::Requested, CapabilityExecutionState::Denied)
            if event.kind == "run.capability_blocked" =>
        {
            true
        }
        // The broker records dispatch admission and execution result in separate aggregates;
        // the result may be the first fact after dispatching when no executing heartbeat exists.
        (
            CapabilityExecutionState::Dispatching,
            CapabilityExecutionState::Succeeded
            | CapabilityExecutionState::Failed
            | CapabilityExecutionState::Cancelled
            | CapabilityExecutionState::Unknown,
        ) if matches!(
            event.kind.as_str(),
            "execution.result_committed"
                | "capability.completed"
                | "capability.failed"
                | "capability.cancelled"
                | "capability.result_unknown"
        ) =>
        {
            true
        }
        // Direct (non-run) capability execution can emit a capability terminal fact without a
        // broker dispatching heartbeat, but only after the authorization fact is durable.
        (
            CapabilityExecutionState::Authorized,
            CapabilityExecutionState::Succeeded
            | CapabilityExecutionState::Failed
            | CapabilityExecutionState::Cancelled
            | CapabilityExecutionState::Unknown,
        ) if matches!(
            event.kind.as_str(),
            "capability.completed"
                | "capability.failed"
                | "capability.cancelled"
                | "capability.result_unknown"
        ) =>
        {
            true
        }
        _ => false,
    }
}

fn request_from_event(event: &RuntimeEvent) -> Option<CapabilityRequest> {
    if event.kind != "run.capability_requested" {
        return None;
    }
    Some(CapabilityRequest {
        request_id: request_id_for(event)?,
        capability: serde_json::from_value::<CapabilityKind>(event.data.get("capability")?.clone())
            .ok()?,
        operation: event.data.get("operation")?.as_str()?.to_owned(),
        arguments: event.data.get("arguments")?.clone(),
        risk: serde_json::from_value::<RiskLevel>(event.data.get("risk")?.clone()).ok()?,
        execution_scope: event
            .data
            .get("execution_scope")
            .and_then(|value| serde_json::from_value(value.clone()).ok()),
        cell_id: event
            .data
            .get("cell_id")
            .and_then(|value| serde_json::from_value(value.clone()).ok())
            .flatten(),
        capability_grant_id: event
            .data
            .get("capability_grant_id")
            .and_then(|value| serde_json::from_value(value.clone()).ok())
            .flatten(),
        budget_lease_id: event
            .data
            .get("budget_lease_id")
            .and_then(|value| serde_json::from_value(value.clone()).ok())
            .flatten(),
    })
}

/// Pure event projection. A dispatch without a terminal result remains an explicit
/// intermediate state; callers that require a terminal answer must treat it as unresolved.
pub fn project_invocations(
    run_id: RunId,
    events: &[RuntimeEvent],
) -> Result<Vec<InvocationProjection>, String> {
    let mut ordered_events = events
        .iter()
        .enumerate()
        .filter_map(|(index, event)| match event_matches_run(event, run_id) {
            Ok(true) => Some(Ok((index, event))),
            Ok(false) => None,
            Err(error) => Some(Err(error)),
        })
        .collect::<Result<Vec<_>, _>>()?;
    let same_stream = ordered_events.first().is_none_or(|(_, first)| {
        ordered_events.iter().all(|(_, event)| {
            event.aggregate_type == first.aggregate_type && event.aggregate_id == first.aggregate_id
        })
    });
    if same_stream {
        ordered_events
            .sort_by_key(|(index, event)| (event.stream_version.unwrap_or(event.sequence), *index));
    }

    let mut projections: BTreeMap<String, InvocationProjection> = BTreeMap::new();
    let mut terminals: HashMap<String, (CapabilityExecutionState, String, String)> = HashMap::new();
    let mut seen_event_ids = HashSet::new();
    for (_, event) in ordered_events {
        if !seen_event_ids.insert(event.event_id.to_string()) {
            return Err("invocation_duplicate_event_id".to_owned());
        }
        let Some(id) = request_id_for(event) else {
            // A tool call without a durable capability request ID cannot be safely associated
            // with an invocation. Keep it visible in the event ledger, but do not guess by
            // operation name because concurrent calls may share the same operation.
            if is_invocation_event(&event.kind) {
                return Err("invocation_request_id_missing".to_owned());
            }
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
                request: None,
                approval_id: None,
            });
        entry.event_ids.push(event.event_id.to_string());
        if event.kind == "run.capability_requested" && request_from_event(event).is_none() {
            return Err("invocation_request_malformed".to_owned());
        }
        if let Some(request) = request_from_event(event) {
            if let Some(previous) = entry.request.as_ref() {
                if previous != &request {
                    return Err("invocation_request_conflict".to_owned());
                }
            }
            entry.request = Some(request);
        }
        if let Some(value) = event.data.get("approval_id") {
            let approval_id = serde_json::from_value::<ApprovalId>(value.clone())
                .map_err(|_| "invocation_approval_id_invalid".to_owned())?;
            if entry
                .approval_id
                .is_some_and(|previous| previous != approval_id)
            {
                return Err("invocation_approval_conflict".to_owned());
            }
            entry.approval_id = Some(approval_id);
        }
        merge_optional_field(
            &mut entry.call_id,
            string_field(event, "call_id"),
            "invocation_call_id_conflict",
        )?;
        merge_optional_field(
            &mut entry.operation,
            string_field(event, "operation"),
            "invocation_operation_conflict",
        )?;
        if let Some(args_fingerprint) =
            string_field(event, "args_fingerprint").or_else(|| string_field(event, "action_digest"))
        {
            merge_optional_field(
                &mut entry.args_fingerprint,
                Some(args_fingerprint),
                "invocation_action_digest_conflict",
            )?;
        }
        let next = terminal_state(event)?.or_else(|| intermediate_state(event));
        let Some(next) = next else {
            continue;
        };
        if let Some((previous, previous_kind, previous_signature)) = terminals.get(&key) {
            if !next.is_terminal() || *previous != next {
                return Err("invocation_state_transition_invalid".to_owned());
            }
            if previous_signature != &terminal_signature(event) {
                return Err("invocation_terminal_conflict".to_owned());
            }
            let _ = previous_kind;
            continue;
        }
        if next.is_terminal() {
            terminals.insert(key, (next, event.kind.clone(), terminal_signature(event)));
            entry.result = Some(event.data.get("result").unwrap_or(&event.data).clone());
        }
        if entry.request.is_none() && event.kind != "run.tool_call" {
            return Err("invocation_request_missing".to_owned());
        }
        if !transition_allowed(entry.state, next, event) {
            return Err("invocation_state_transition_invalid".to_owned());
        }
        entry.state = next;
    }
    // A durable dispatch without a terminal result cannot be treated as success after a
    // restart. Keep the projection fail-closed until a reconciliation/terminal fact exists.
    for entry in projections.values_mut() {
        if matches!(
            entry.state,
            CapabilityExecutionState::Dispatching | CapabilityExecutionState::Executing
        ) {
            entry.state = CapabilityExecutionState::Unknown;
        }
    }
    Ok(projections.into_values().collect())
}

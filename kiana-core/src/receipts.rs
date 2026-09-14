use super::events::*;
use super::redaction::*;
use super::*;

impl ControlPlane {
    pub async fn read_receipt(
        &self,
        context: RequestContext,
        run_id: Option<RunId>,
    ) -> Result<CoreResponse, CoreError> {
        let request_id = context.request_id;
        let run_id = match run_id {
            Some(run_id) => {
                if let Some(binding) = self.session_binding(context.session_id.as_str()) {
                    if !Self::same_session_principal(&binding, &context) || binding.run_id != run_id
                    {
                        return Ok(CoreResponse::blocked(request_id, "run_owner_mismatch"));
                    }
                }
                run_id
            }
            None => match self.resolve_run_id(&context, None).await? {
                Ok(run_id) => run_id,
                Err(reason) => return Ok(CoreResponse::blocked(request_id, reason)),
            },
        };
        let events = self.events_for_persisted_run(run_id).await?;
        if events.is_empty() {
            return Ok(CoreResponse::blocked(request_id, "receipt_not_found"));
        }
        if receipt_owner_mismatch(&events, &context) {
            return Ok(CoreResponse::blocked(request_id, "run_owner_mismatch"));
        }
        if self.run_data_revoked(run_id).await? {
            return Ok(CoreResponse {
                request_id,
                status: ExecutionStatus::Blocked,
                output: json!({"run_id":run_id,"data_revoked":true,"retained_event_ids":events.iter().map(|event|event.event_id).collect::<Vec<_>>()}),
                error: Some("receipt_data_revoked".to_owned()),
            });
        }
        let sandbox = events
            .iter()
            .rev()
            .find_map(|event| event.data.get("sandbox").and_then(Value::as_str))
            .unwrap_or("read-only");
        let terminal_error = |kind: &str, fallback: &str| {
            events
                .iter()
                .rev()
                .find(|event| event.kind == kind)
                .and_then(|event| event.data.get("error").and_then(Value::as_str))
                .map(redact_event_text)
                .unwrap_or_else(|| fallback.to_owned())
        };
        let turn_start = events
            .iter()
            .rposition(|event| event.kind == "run.prompt")
            .unwrap_or(0);
        let turn_events = &events[turn_start..];
        if let Err(reason) = crate::project_invocations(run_id, &events) {
            return Ok(CoreResponse {
                request_id,
                status: ExecutionStatus::ResultUnknown,
                output: receipt_from_events(&context, run_id, sandbox, Value::Null, &events),
                error: Some(reason),
            });
        }
        let has_completed = turn_events
            .iter()
            .any(|event| event.kind == "run.completed");
        let has_failed = turn_events.iter().any(|event| event.kind == "run.failed");
        let has_cancelled = turn_events
            .iter()
            .any(|event| event.kind == "run.cancelled");
        let has_result_unknown = turn_events
            .iter()
            .any(|event| event.kind == "run.result_unknown");
        let terminal_count = has_completed as usize
            + has_failed as usize
            + has_cancelled as usize
            + has_result_unknown as usize;

        // A replay may not invent success from an incomplete or contradictory event stream.
        // There is no reconciliation authority here, so either condition remains unknown.
        if has_result_unknown || terminal_count > 1 {
            let error = if has_result_unknown {
                terminal_error("run.result_unknown", "result_unknown")
            } else {
                "run_terminal_conflict".to_owned()
            };
            return Ok(CoreResponse {
                request_id,
                status: ExecutionStatus::ResultUnknown,
                output: receipt_from_events(&context, run_id, sandbox, Value::Null, &events),
                error: Some(error),
            });
        }
        if has_cancelled {
            return Ok(CoreResponse {
                request_id,
                status: ExecutionStatus::Cancelled,
                output: receipt_from_events(&context, run_id, sandbox, Value::Null, &events),
                error: Some(terminal_error("run.cancelled", "run_cancelled")),
            });
        }
        if has_failed {
            return Ok(CoreResponse {
                request_id,
                status: ExecutionStatus::Failed,
                output: receipt_from_events(&context, run_id, sandbox, Value::Null, &events),
                error: Some(terminal_error("run.failed", "run_failed")),
            });
        }
        if !has_completed {
            return Ok(CoreResponse {
                request_id,
                status: ExecutionStatus::ResultUnknown,
                output: receipt_from_events(&context, run_id, sandbox, Value::Null, &events),
                error: Some("run_result_missing".to_owned()),
            });
        }
        let output = events
            .iter()
            .rev()
            .find(|event| event.kind == "run.completed")
            .map(|event| event.data.clone())
            .unwrap_or(Value::Null);
        Ok(CoreResponse::completed(
            request_id,
            receipt_from_events(&context, run_id, sandbox, output, &events),
        ))
    }

    pub(crate) async fn run_receipt_from_store(
        &self,
        context: &RequestContext,
        run_id: RunId,
        sandbox: &str,
        output: Value,
    ) -> Result<Value, CoreError> {
        let events = self.events_for_current_run(context, run_id).await?;
        crate::project_invocations(run_id, &events)
            .map_err(|reason| PortError::Conflict(reason))?;
        Ok(receipt_from_events(
            context, run_id, sandbox, output, &events,
        ))
    }
    pub(crate) async fn events_for_persisted_run(
        &self,
        run_id: RunId,
    ) -> Result<Vec<RuntimeEvent>, CoreError> {
        match self.read_all_events().await? {
            Some(all) => Ok(filter_run_events(&all, run_id)),
            None => {
                let events = self.events.read_stream("run", &run_id.to_string()).await?;
                Ok(filter_run_events(&events, run_id))
            }
        }
    }

    /// Build an immediate receipt for the command that just produced this run.
    /// This is the sole request-scoped compatibility path: a later public receipt
    /// has no trustworthy request-to-run association to use as a fallback.
    pub(crate) async fn events_for_current_run(
        &self,
        context: &RequestContext,
        run_id: RunId,
    ) -> Result<Vec<RuntimeEvent>, CoreError> {
        match self.read_all_events().await? {
            Some(all) => Ok(filter_run_events(&all, run_id)),
            None => {
                let events = self.events.read_request(&context.request_id).await?;
                Ok(filter_run_events(&events, run_id))
            }
        }
    }

    pub(crate) async fn events_for_author(
        &self,
        reviewer: &RequestContext,
        author_session_id: &str,
        author_run_id: Option<RunId>,
    ) -> Result<Vec<RuntimeEvent>, CoreError> {
        let run_id = author_run_id
            .or_else(|| RunId::parse_str(author_session_id))
            .or_else(|| self.session_run_id(author_session_id));
        let events = match self.read_all_events().await? {
            Some(all) => match run_id {
                Some(run_id) => filter_run_events(&all, run_id),
                None => unique_authorized_run_id(&all, reviewer, author_session_id)
                    .map(|run_id| filter_run_events(&all, run_id))
                    .unwrap_or_default(),
            },
            None => {
                let run_id = run_id.ok_or_else(|| {
                    CoreError::Port(PortError::Failed(
                        "event_store_read_all_unsupported".to_owned(),
                    ))
                })?;
                let events = self.events.read_stream("run", &run_id.to_string()).await?;
                filter_run_events(&events, run_id)
            }
        };
        let identity = events.iter().find(|event| event.kind == "run.authorized");
        if let Some(identity) = identity {
            let session_matches = identity
                .data
                .get("session_id")
                .and_then(Value::as_str)
                .is_none_or(|session| session == author_session_id);
            let project_matches = identity
                .data
                .get("project_root")
                .and_then(Value::as_str)
                .is_none_or(|project| {
                    Self::canonical_project_root(project)
                        == Self::canonical_project_root(&reviewer.project_root)
                });
            let actor_matches = match (
                identity.data.get("actor_id").and_then(Value::as_str),
                reviewer.actor_id.as_deref(),
            ) {
                (Some(actor), Some(expected)) => actor == expected,
                _ => true,
            };
            if !session_matches || !project_matches || !actor_matches {
                return Ok(Vec::new());
            }
        }
        Ok(events)
    }

    /// `None` is reserved for the explicit legacy capability limit documented on
    /// `EventStorePort::read_all`; every actual read failure must reach the caller.
    pub(crate) async fn read_all_events(&self) -> Result<Option<Vec<RuntimeEvent>>, CoreError> {
        match self.events.read_all().await {
            Ok(events) => Ok(Some(events)),
            Err(PortError::Failed(reason)) if reason == "event_store_read_all_unsupported" => {
                Ok(None)
            }
            Err(error) => Err(error.into()),
        }
    }

    /// 只读地把账本全量事件交给宿主，供展示层做只读投影。
    ///
    /// 展示层不得自己解析账本文件：路径推导、torn-tail 容忍和 symlink 拒绝都由
    /// `EventStorePort` 的实现负责，这里只转发同一次读取。`None` 表示该存储不支持
    /// 全量读取（见 [`Self::read_all_events`]）。
    pub async fn persisted_events(&self) -> Result<Option<Vec<RuntimeEvent>>, CoreError> {
        self.read_all_events().await
    }
}

pub(crate) fn receipt_from_events(
    context: &RequestContext,
    run_id: RunId,
    sandbox: &str,
    output: Value,
    events: &[RuntimeEvent],
) -> Value {
    let worker = RoleSpec::lookup(&context.role_id).unwrap_or_else(RoleSpec::builder);
    let invocation_projection = crate::project_invocations(run_id, events);
    let invocation_error = invocation_projection.as_ref().err().cloned();
    let invocations = invocation_projection.ok();
    let receipt = with_work_packet(
        json!({
            "schema": RUN_RESULT_SCHEMA,
            "run_id": run_id,
            "session_id": context.session_id,
            "harness": HARNESS_ID,
            "sandbox": sandbox,
            "actor_id": context.actor_id,
            "role_id": worker.role_id,
            "department_id": worker.department_id,
            "max_steps_per_turn": events.iter().find(|event| event.kind == "run.authorized")
                .and_then(|event| event.data.get("max_steps_per_turn"))
                .cloned().unwrap_or_else(|| json!(worker.max_steps)),
            "prompt_hash": events.iter().rev().find(|event| event.kind == "run.model_turn")
                .and_then(|event| event.data.get("prompt_hash")).cloned().unwrap_or_else(|| json!(worker.prompt_hash)),
            "model_turns": model_turns_from_events(events),
            "cost_ledger": cost_ledger_from_events(events, run_id),
            "files_changed": files_changed_from_events(events),
            "memory_hits": memory_hits_from_events(events),
            "memory_proposals": events.iter().filter(|event| event.kind == "memory.proposed").map(|event| event.data.clone()).collect::<Vec<_>>(),
            "invocations":invocations,
            "invocation_projection_error":invocation_error,
            "compact": compact_from_events(events),
            "capabilities": capabilities_from_events(events),
            "output": output,
        }),
        context,
    );
    // Re-apply the boundary while projecting so legacy events written before centralized
    // redaction cannot reintroduce a credential into a restart receipt.
    redact_event_value(&receipt)
}
pub(crate) fn receipt_owner_mismatch(events: &[RuntimeEvent], context: &RequestContext) -> bool {
    let Some(identity) = events.iter().find(|event| event.kind == "run.authorized") else {
        return false;
    };
    let session_matches = identity
        .data
        .get("session_id")
        .and_then(Value::as_str)
        .is_none_or(|session| session == context.session_id.as_str());
    let project_matches = identity
        .data
        .get("project_root")
        .and_then(Value::as_str)
        .is_none_or(|project| {
            ControlPlane::canonical_project_root(project)
                == ControlPlane::canonical_project_root(&context.project_root)
        });
    let actor_matches = match (
        identity.data.get("actor_id").and_then(Value::as_str),
        context.actor_id.as_deref(),
    ) {
        (Some(actor), Some(expected)) => actor == expected,
        _ => true,
    };
    let role_matches = identity
        .data
        .get("role_id")
        .and_then(Value::as_str)
        .is_none_or(|role| role == context.role_id);
    let department_matches = identity
        .data
        .get("department_id")
        .and_then(Value::as_str)
        .is_none_or(|department| department == context.department_id);
    !(session_matches && project_matches && actor_matches && role_matches && department_matches)
}

pub(crate) fn filter_run_events(events: &[RuntimeEvent], run_id: RunId) -> Vec<RuntimeEvent> {
    let run_id_str = run_id.to_string();
    events
        .iter()
        .filter(|event| {
            let stream = (
                event.aggregate_type.as_deref(),
                event.aggregate_id.as_deref(),
            );
            let exact_run_stream = stream == (Some("run"), Some(run_id_str.as_str()));
            let conflicting_run_stream =
                matches!(stream, (Some("run"), Some(_))) && !exact_run_stream;
            match event.data.get("run_id") {
                Some(Value::String(value)) => value == &run_id_str && !conflicting_run_stream,
                Some(_) => false,
                // Legacy events without a payload run ID are only usable when their
                // durable aggregate metadata identifies this exact run.
                None => exact_run_stream,
            }
        })
        .cloned()
        .collect()
}

pub(crate) fn unique_authorized_run_id(
    events: &[RuntimeEvent],
    reviewer: &RequestContext,
    author_session_id: &str,
) -> Option<RunId> {
    let reviewer_actor_id = reviewer.actor_id.as_deref()?;
    let reviewer_project_root = ControlPlane::canonical_project_root(&reviewer.project_root);
    let candidates: HashSet<_> = events
        .iter()
        .filter_map(|event| {
            if event.kind != "run.authorized"
                || event.data.get("session_id").and_then(Value::as_str) != Some(author_session_id)
                || event.data.get("actor_id").and_then(Value::as_str) != Some(reviewer_actor_id)
                || event.data.get("role_id").and_then(Value::as_str) != Some(ROLE_BUILDER)
                || event.data.get("department_id").and_then(Value::as_str)
                    != Some(DEPARTMENT_EXECUTING)
            {
                return None;
            }
            let project_root = event.data.get("project_root").and_then(Value::as_str)?;
            if ControlPlane::canonical_project_root(project_root) != reviewer_project_root {
                return None;
            }
            event
                .data
                .get("run_id")
                .and_then(Value::as_str)
                .and_then(RunId::parse_str)
        })
        .collect();
    (candidates.len() == 1)
        .then(|| candidates.into_iter().next())
        .flatten()
}
pub(crate) fn files_changed_from_events(events: &[RuntimeEvent]) -> Vec<String> {
    let mut files = Vec::new();
    for event in events {
        if event.kind != "capability.completed" {
            continue;
        }
        let Some(changed) = event.data.get("changed").and_then(Value::as_array) else {
            continue;
        };
        for item in changed {
            let Some(path) = item.get("path").and_then(Value::as_str) else {
                continue;
            };
            if !path.is_empty() && !files.iter().any(|existing| existing == path) {
                files.push(path.to_owned());
            }
        }
    }
    files
}

pub(crate) fn compact_from_events(events: &[RuntimeEvent]) -> Value {
    let mut count = 0u64;
    let mut last = None;
    for event in events {
        if event.kind != "run.compacted" {
            continue;
        }
        count += 1;
        last = Some(event.data.clone());
    }
    match last {
        Some(data) => json!({
            "applied": true,
            "count": count,
            "tokens_before": data.get("tokens_before"),
            "tokens_after": data.get("tokens_after"),
            "summary_present": data.get("summary_present"),
        }),
        None => json!({
            "applied": false,
            "count": 0,
        }),
    }
}

pub(crate) fn memory_hits_from_events(events: &[RuntimeEvent]) -> Vec<Value> {
    let mut hits = Vec::new();
    for event in events {
        if event.kind != "capability.completed" {
            continue;
        }
        if event.data.get("schema").and_then(Value::as_str) != Some(MEMORY_SEARCH_SCHEMA) {
            continue;
        }
        let Some(items) = event.data.get("hits").and_then(Value::as_array) else {
            continue;
        };
        for item in items {
            let mut hit = item.clone();
            if let Some(fields) = hit.as_object_mut() {
                fields.insert("retrieval_event_id".to_owned(), json!(event.event_id));
                fields.insert(
                    "retrieval_request_id".to_owned(),
                    event
                        .data
                        .get("capability_request_id")
                        .cloned()
                        .unwrap_or_else(|| json!(event.request_id)),
                );
                fields.insert(
                    "query".to_owned(),
                    event.data.get("query").cloned().unwrap_or(Value::Null),
                );
                fields.insert(
                    "retrieved_by".to_owned(),
                    event.data.get("role_id").cloned().unwrap_or(Value::Null),
                );
            }
            hits.push(hit);
        }
    }
    hits
}

pub(crate) fn capabilities_from_events(events: &[RuntimeEvent]) -> Vec<Value> {
    events
        .iter()
        .filter(|event| event.kind == "run.capability_requested")
        .map(|event| {
            json!({
                "capability": event.data.get("capability"),
                "operation": event.data.get("operation"),
            })
        })
        .collect()
}

/// Model calls, including calls that produce tools, are projected from the authoritative ledger.
pub(crate) fn model_turns_from_events(events: &[RuntimeEvent]) -> Vec<Value> {
    events
        .iter()
        .filter(|event| event.kind == "run.model_turn")
        .map(|event| {
            let mut data = event.data.clone();
            if let Some(fields) = data.as_object_mut() {
                fields.insert("event_id".to_owned(), json!(event.event_id));
                fields.insert("request_id".to_owned(), json!(event.request_id));
            }
            data
        })
        .collect()
}
pub(crate) fn cost_ledger_from_events(
    events: &[RuntimeEvent],
    run_id: RunId,
) -> kiana_domain::CostLedger {
    let records = events
        .iter()
        .filter(|event| {
            event.kind == "run.model_turn"
                && event.data.get("attempted").and_then(Value::as_bool) == Some(true)
        })
        .map(|event| {
            let data = &event.data;
            kiana_domain::UsageRecord {
                event_id: event.event_id,
                request_id: event.request_id,
                run_id,
                step: data
                    .get("step")
                    .and_then(Value::as_u64)
                    .unwrap_or(0)
                    .min(u64::from(u32::MAX)) as u32,
                provider_id: data
                    .get("provider_id")
                    .and_then(Value::as_str)
                    .map(str::to_owned),
                model_id: data
                    .get("model_id")
                    .and_then(Value::as_str)
                    .map(str::to_owned),
                input_tokens: data.pointer("/usage/input_tokens").and_then(Value::as_u64),
                output_tokens: data.pointer("/usage/output_tokens").and_then(Value::as_u64),
                elapsed_ms: data.get("elapsed_ms").and_then(Value::as_u64).unwrap_or(0),
            }
        })
        .collect();
    kiana_domain::CostLedger::from_records(records)
}

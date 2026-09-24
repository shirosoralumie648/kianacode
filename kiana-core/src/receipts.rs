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
                output: json!({"run_id":run_id,"data_revoked":true,"retained_event_ids":events.iter().map(|event|event.event_id).collect::<Vec<_>>(),
                    "historical_receipt":{"state":"preserved_invalid","deleted_at":null,"reinjection_allowed":false}}),
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
        if let Err(reason) = self.cache_invocation_projection(run_id, &events) {
            let reason = reason.to_string();
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
        self.cache_invocation_projection(run_id, &events)?;
        Ok(receipt_from_events(
            context, run_id, sandbox, output, &events,
        ))
    }
    pub(crate) async fn events_for_persisted_run(
        &self,
        run_id: RunId,
    ) -> Result<Vec<RuntimeEvent>, CoreError> {
        match self.read_all_events().await? {
            Some(all) => try_filter_run_events(&all, run_id)
                .map_err(|reason| CoreError::Port(PortError::Failed(reason))),
            None => {
                let events = self.events.read_stream("run", &run_id.to_string()).await?;
                try_filter_run_events(&events, run_id)
                    .map_err(|reason| CoreError::Port(PortError::Failed(reason)))
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

    /// Flush the EventStore's durable boundary without changing authorization or run state.
    pub async fn flush_event_store(&self) -> Result<kiana_domain::EventStoreHealth, CoreError> {
        self.events.flush().await.map_err(Into::into)
    }

    /// Read a bounded EventStore health snapshot for daemon readiness/shutdown reporting.
    pub async fn event_store_health(&self) -> Result<kiana_domain::EventStoreHealth, CoreError> {
        self.events.health().await.map_err(Into::into)
    }

    pub async fn last_durable_cursor(&self) -> Result<kiana_domain::EventCursor, CoreError> {
        self.events.last_durable_cursor().await.map_err(Into::into)
    }

    /// Close the EventStore only after its own durable flush acknowledgement.
    pub async fn close_event_store(&self) -> Result<kiana_domain::EventStoreHealth, CoreError> {
        self.events.close().await.map_err(Into::into)
    }
}

pub(crate) fn receipt_from_events(
    context: &RequestContext,
    run_id: RunId,
    sandbox: &str,
    output: Value,
    events: &[RuntimeEvent],
) -> Value {
    let worker = RoleSpec::lookup(&context.role_id);
    let role_id = worker
        .as_ref()
        .map(|role| role.role_id.clone())
        .unwrap_or_else(|| context.role_id.clone());
    let department_id = worker
        .as_ref()
        .map(|role| role.department_id.clone())
        .unwrap_or_else(|| context.department_id.clone());
    let default_max_steps = worker
        .as_ref()
        .map(|role| json!(role.max_steps))
        .unwrap_or(Value::Null);
    let default_prompt_hash = worker
        .as_ref()
        .map(|role| json!(role.prompt_hash))
        .unwrap_or(Value::Null);
    let invocation_projection = crate::project_invocations(run_id, events);
    let invocation_error = invocation_projection.as_ref().err().cloned();
    let invocations = invocation_projection.ok();
    let typed_receipt = typed_run_receipt(context, run_id, output.clone(), events)
        .unwrap_or_else(|error| json!({"error": error, "status": "result_unknown"}));
    let receipt_data_binding = typed_receipt
        .get("receipt_digest")
        .and_then(Value::as_str)
        .and_then(|receipt_digest| {
            crate::data_governance::receipt_data_binding_from_events(
                receipt_digest,
                &ControlPlane::canonical_project_root(&context.project_root),
                &kiana_domain::DataPolicy::default(),
                events,
            )
            .ok()
            .and_then(|binding| serde_json::to_value(binding).ok())
        })
        .unwrap_or_else(|| json!({"state":"unknown","reason":"receipt_data_binding_unavailable"}));
    let typed_execution_receipts = typed_execution_receipts(run_id, events);
    let aggregation = aggregate_receipt_facts(run_id, events)
        .and_then(|aggregation| aggregation.to_json())
        .unwrap_or_else(|error| json!({"error": error, "verification": "unknown"}));
    let cost_breakdown = aggregation
        .get("cost_breakdown")
        .cloned()
        .unwrap_or(Value::Null);
    let effect_usage = aggregation
        .get("effect_usage")
        .cloned()
        .unwrap_or(Value::Null);
    let receipt = with_work_packet(
        json!({
            "schema": RUN_RESULT_SCHEMA,
            "run_id": run_id,
            "session_id": context.session_id,
            "harness": HARNESS_ID,
            "sandbox": sandbox,
            "actor_id": context.actor_id,
            "role_id": role_id,
            "department_id": department_id,
            "role_resolution": worker.is_some(),
            "role_spec_schema": worker.as_ref().map(|role| role.schema.clone()),
            "role_version": worker.as_ref().map(|role| role.version),
            "role_catalog_schema": kiana_domain::ROLE_CATALOG_SCHEMA,
            "role_catalog_version": kiana_domain::SchemaVersion::new(1, 0),
            "input_schema": worker.as_ref().map(|role| role.input_schema.clone()),
            "output_schema": worker.as_ref().map(|role| role.output_schema.clone()),
            "model_profile": worker.as_ref().map(|role| role.model_profile.clone()),
            "max_steps_per_turn": events.iter().find(|event| event.kind == "run.authorized")
                .and_then(|event| event.data.get("max_steps_per_turn"))
                .cloned().unwrap_or(default_max_steps),
            "prompt_hash": events.iter().rev().find(|event| event.kind == "run.model_turn")
                .and_then(|event| event.data.get("prompt_hash")).cloned().unwrap_or(default_prompt_hash),
            "model_turns": model_turns_from_events(events),
            "cost_ledger": cost_ledger_from_events(events, run_id),
            "files_changed": files_changed_from_events(events),
            "memory_hits": memory_hits_from_events(events),
            "retrieval_receipts": retrieval_receipts_from_events(events),
            "memory_proposals": events.iter().filter(|event| event.kind == "memory.proposed").map(|event| event.data.clone()).collect::<Vec<_>>(),
            "invocations":invocations,
            "invocation_projection_error":invocation_error,
            "run_receipt": typed_receipt,
            "receipt_data_binding": receipt_data_binding,
            "projection": projection_lag_from_events(events),
            "execution_receipts": typed_execution_receipts,
            "aggregation": aggregation,
            "cost_breakdown": cost_breakdown,
            "effect_usage": effect_usage,
            "compact": compact_from_events(events),
            "capabilities": capabilities_from_events(events),
            "observability": observability_from_events(run_id, events),
            "output": output,
        }),
        context,
    );
    // Re-apply the boundary while projecting so legacy events written before centralized
    // redaction cannot reintroduce a credential into a restart receipt.
    redact_event_value(&receipt)
}

fn typed_run_receipt(
    context: &RequestContext,
    run_id: RunId,
    output: Value,
    events: &[RuntimeEvent],
) -> Result<Value, String> {
    let state = crate::project_run_state(run_id, events).map_err(|error| error.to_string())?;
    let status = match state.outcome {
        Some(crate::RunOutcome::Completed) => ExecutionStatus::Completed,
        Some(crate::RunOutcome::Failed) => ExecutionStatus::Failed,
        Some(crate::RunOutcome::Cancelled) => ExecutionStatus::Cancelled,
        Some(crate::RunOutcome::ResultUnknown) => ExecutionStatus::ResultUnknown,
        None => match state.phase {
            crate::RunPhase::Queued => ExecutionStatus::Queued,
            crate::RunPhase::AwaitingApproval => ExecutionStatus::AwaitingApproval,
            crate::RunPhase::Cancelling => ExecutionStatus::Cancelling,
            crate::RunPhase::Running | crate::RunPhase::Authorized | crate::RunPhase::Terminal => {
                ExecutionStatus::Running
            }
        },
    };
    let terminal_reason = events
        .iter()
        .rev()
        .find(|event| {
            matches!(
                event.kind.as_str(),
                "run.failed" | "run.cancelled" | "run.result_unknown"
            )
        })
        .and_then(|event| event.data.get("error"))
        .and_then(Value::as_str)
        .map(redact_event_text);
    let mut seen_event_ids = HashSet::new();
    let source_event_ids = events
        .iter()
        .rev()
        .map(|event| event.event_id)
        .filter(|event_id| seen_event_ids.insert(*event_id))
        .take(kiana_domain::MAX_SOURCE_EVENT_IDS)
        .collect::<Vec<_>>();
    if source_event_ids.is_empty() {
        return Err("run_receipt_source_empty".to_owned());
    }
    let profile = kiana_domain::RedactionProfile::for_signal(kiana_domain::RedactionSignal::Audit);
    let receipt = kiana_domain::RunReceipt::new(
        run_id,
        context.session_id.clone(),
        context.actor_id.clone(),
        kiana_domain::json_digest(&json!({
            "project_root": ControlPlane::canonical_project_root(&context.project_root)
        })),
        status,
        terminal_reason,
        events.len().min(u64::MAX as usize) as u64,
        source_event_ids,
        profile.profile_digest,
        "implemented",
        "source",
        kiana_domain::json_digest(&output),
        typed_execution_receipts(run_id, events)
            .iter()
            .filter_map(|value| value.get("receipt_digest").and_then(Value::as_str))
            .map(str::to_owned)
            .collect(),
    )?;
    receipt.to_json()
}

fn typed_execution_receipts(run_id: RunId, events: &[RuntimeEvent]) -> Vec<Value> {
    let profile = kiana_domain::RedactionProfile::for_signal(kiana_domain::RedactionSignal::Audit);
    let Ok(records) = crate::project_capability_attempts(run_id, events) else {
        return Vec::new();
    };
    records
        .into_iter()
        .filter_map(|record| {
            let status = match record.effect {
                kiana_domain::CapabilityEffectState::Succeeded => {
                    kiana_domain::CapabilityExecutionState::Succeeded
                }
                kiana_domain::CapabilityEffectState::Failed => {
                    if record.admission == kiana_domain::CapabilityAdmissionState::Denied {
                        kiana_domain::CapabilityExecutionState::Denied
                    } else {
                        kiana_domain::CapabilityExecutionState::Failed
                    }
                }
                kiana_domain::CapabilityEffectState::Unknown => {
                    kiana_domain::CapabilityExecutionState::Unknown
                }
                kiana_domain::CapabilityEffectState::Started => {
                    kiana_domain::CapabilityExecutionState::Executing
                }
                kiana_domain::CapabilityEffectState::NotStarted => {
                    if record.approval == kiana_domain::CapabilityApprovalState::Pending {
                        kiana_domain::CapabilityExecutionState::AwaitingApproval
                    } else {
                        kiana_domain::CapabilityExecutionState::Requested
                    }
                }
            };
            kiana_domain::ExecutionReceipt::new(
                record.request_id,
                record.execution_id,
                record.invocation_id,
                record.attempt,
                record.action_digest,
                status,
                record.effect_known,
                record.stop_confirmed,
                record.fenced,
                None,
                record.source_cursor,
                record.source_event_ids,
                profile.profile_digest.clone(),
                "implemented",
                "source",
            )
            .ok()
            .and_then(|receipt| receipt.to_json().ok())
        })
        .collect()
}

pub fn aggregate_receipt_facts(
    run_id: RunId,
    events: &[RuntimeEvent],
) -> Result<kiana_domain::ReceiptAggregation, String> {
    let events = try_filter_run_events(events, run_id)?;
    if events.is_empty() {
        return Err("receipt_aggregation_source_empty".to_owned());
    }
    let source_cursor = events.len().min(u64::MAX as usize) as u64;
    let mut seen = HashSet::new();
    let mut source_event_ids = Vec::new();
    let mut model_turns = 0u64;
    let mut committed_executions = 0u64;
    let mut input_tokens = Some(0u64);
    let mut output_tokens = Some(0u64);
    let mut usage_unknown = false;
    let mut files_changed = Vec::new();
    let mut memory_hits = 0u64;
    let mut evidence_ref_digests = Vec::new();
    let mut provider_receipt_refs = Vec::new();
    let mut verification = kiana_domain::AggregationVerification::Complete;
    for event in &events {
        if !seen.insert(event.event_id) {
            continue;
        }
        source_event_ids.push(event.event_id);
        if event.payload_recoverable == Some(false) {
            verification = kiana_domain::AggregationVerification::Partial;
        }
        if event.data.get("committed") == Some(&Value::Bool(false)) {
            verification = kiana_domain::AggregationVerification::Partial;
        }
        if event.kind == "run.model_turn" && event.data["attempted"] == true {
            model_turns = model_turns.saturating_add(1);
            let input = event
                .data
                .pointer("/usage/input_tokens")
                .and_then(Value::as_u64);
            let output = event
                .data
                .pointer("/usage/output_tokens")
                .and_then(Value::as_u64);
            match (input, output) {
                (Some(input), Some(output)) => {
                    input_tokens = input_tokens.and_then(|current| current.checked_add(input));
                    output_tokens = output_tokens.and_then(|current| current.checked_add(output));
                    if input_tokens.is_none() || output_tokens.is_none() {
                        usage_unknown = true;
                    }
                }
                _ => {
                    usage_unknown = true;
                    input_tokens = None;
                    output_tokens = None;
                    verification = kiana_domain::AggregationVerification::Partial;
                }
            }
        }
        if event.kind == "execution.result_committed" {
            committed_executions = committed_executions.saturating_add(1);
            if event.data.get("effect_known") == Some(&Value::Bool(false)) {
                verification = kiana_domain::AggregationVerification::Unknown;
            }
        }
        if event.kind == "capability.completed" {
            if let Some(changed) = event.data.get("changed").and_then(Value::as_array) {
                for item in changed {
                    let Some(path) = item.get("path").and_then(Value::as_str) else {
                        verification = kiana_domain::AggregationVerification::Partial;
                        continue;
                    };
                    let Some(path) = kiana_domain::normalize_role_path(path) else {
                        verification = kiana_domain::AggregationVerification::Partial;
                        continue;
                    };
                    files_changed.push(path);
                }
            }
            if event.data.get("schema").and_then(Value::as_str)
                == Some(kiana_domain::MEMORY_SEARCH_SCHEMA)
            {
                memory_hits = memory_hits.saturating_add(
                    event
                        .data
                        .get("hits")
                        .and_then(Value::as_array)
                        .map_or(0, |hits| hits.len() as u64),
                );
            }
        }
        if let Some(refs) = event.data.get("evidence_refs").and_then(Value::as_array) {
            evidence_ref_digests.extend(refs.iter().map(kiana_domain::json_digest));
        }
        for field in ["provider_receipt_ref", "provider_receipt_id"] {
            if let Some(reference) = event.data.get(field).and_then(Value::as_str) {
                provider_receipt_refs.push(kiana_domain::json_digest(
                    &json!({"field":field,"reference":reference}),
                ));
            }
        }
    }
    if usage_unknown && verification == kiana_domain::AggregationVerification::Complete {
        verification = kiana_domain::AggregationVerification::Partial;
    }
    source_event_ids = source_event_ids
        .into_iter()
        .rev()
        .take(kiana_domain::MAX_SOURCE_EVENT_IDS)
        .collect();
    let mut aggregation = kiana_domain::ReceiptAggregation::new(
        source_cursor,
        source_event_ids,
        model_turns,
        committed_executions,
        input_tokens,
        output_tokens,
        usage_unknown,
        None,
        false,
        files_changed,
        memory_hits,
        evidence_ref_digests,
        provider_receipt_refs,
        verification,
    )?;
    if let Some(cost_breakdown) = kiana_domain::project_receipt_cost_breakdown(run_id, &events)
        .map_err(|reason| format!("receipt_cost_projection:{reason}"))?
    {
        aggregation = aggregation.with_cost_breakdown(cost_breakdown)?;
    }
    if let Some(effect_usage) = crate::project_effect_usage(run_id, &events)
        .map_err(|reason| format!("receipt_effect_usage_projection:{reason}"))?
    {
        aggregation = aggregation.with_effect_usage(effect_usage)?;
    }
    Ok(aggregation)
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
    try_filter_run_events(events, run_id).unwrap_or_default()
}

pub(crate) fn try_filter_run_events(
    events: &[RuntimeEvent],
    run_id: RunId,
) -> Result<Vec<RuntimeEvent>, String> {
    let run_id_str = run_id.to_string();
    let mut filtered = Vec::new();
    for event in events {
        let stream = (
            event.aggregate_type.as_deref(),
            event.aggregate_id.as_deref(),
        );
        let exact_run_stream = stream == (Some("run"), Some(run_id_str.as_str()));
        let conflicting_run_stream = matches!(stream, (Some("run"), Some(_))) && !exact_run_stream;
        let belongs = match event.data.get("run_id") {
            Some(Value::String(value)) => {
                if (conflicting_run_stream || exact_run_stream) && value != &run_id_str {
                    return Err("invocation_event_run_id_conflict".to_owned());
                }
                value == &run_id_str && !conflicting_run_stream
            }
            Some(Value::Null) => {
                if exact_run_stream {
                    true
                } else {
                    false
                }
            }
            Some(_) => {
                if exact_run_stream {
                    return Err("invocation_event_run_id_invalid".to_owned());
                }
                false
            }
            // Legacy events without a payload run ID are only usable when their
            // durable aggregate metadata identifies this exact run.
            None => exact_run_stream,
        };
        if belongs {
            filtered.push(event.clone());
        }
    }
    Ok(filtered)
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
            let source_revision = hit.get("revision").cloned().unwrap_or(Value::Null);
            let degraded = hit.get("degraded").cloned().unwrap_or(Value::Bool(false));
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
                fields.insert("stage".to_owned(), json!("retrieved"));
                fields.insert("source_revision".to_owned(), source_revision);
                fields.insert("degraded".to_owned(), degraded);
            }
            hits.push(hit);
        }
    }
    hits
}

fn projection_lag_from_events(events: &[RuntimeEvent]) -> Value {
    let source_cursor = events.len().min(u64::MAX as usize) as u64;
    let projection_cursor = events
        .iter()
        .filter_map(|event| event.data.get("projector_cursor").and_then(Value::as_u64))
        .max();
    let projection_generation = events
        .iter()
        .rev()
        .find_map(|event| {
            event
                .data
                .get("projection_generation")
                .and_then(Value::as_u64)
        })
        .unwrap_or(1);
    let data_epoch = events
        .iter()
        .rev()
        .find_map(|event| event.data.get("data_epoch").and_then(Value::as_u64))
        .unwrap_or(1);
    let reason = if projection_cursor.is_none() {
        Some("projection_cursor_unobserved".to_owned())
    } else if projection_cursor.is_some_and(|cursor| cursor < source_cursor) {
        Some("projection_cursor_lagging".to_owned())
    } else {
        None
    };
    match kiana_domain::ProjectionLagView::new(
        source_cursor,
        projection_cursor,
        projection_generation,
        data_epoch,
        reason,
    ) {
        Ok(view) => serde_json::to_value(view).unwrap_or_else(|_| {
            json!({
                "schema": kiana_domain::PROJECTION_LAG_VIEW_SCHEMA,
                "status": "unknown",
                "projection_pending": true,
                "reason": "projection_view_encode_failed",
            })
        }),
        Err(reason) => json!({
            "schema": kiana_domain::PROJECTION_LAG_VIEW_SCHEMA,
            "status": "unknown",
            "projection_pending": true,
            "reason": reason,
        }),
    }
}

pub(crate) fn retrieval_receipts_from_events(events: &[RuntimeEvent]) -> Vec<Value> {
    events
        .iter()
        .filter(|event| {
            event.kind == "capability.completed"
                && event.data.get("schema").and_then(Value::as_str) == Some(MEMORY_SEARCH_SCHEMA)
        })
        .map(|event| {
            let query = event.data["query"].as_str().unwrap_or("unknown");
            let query_digest = kiana_domain::json_digest(&json!({"query": query}));
            let permission_scope_digest = kiana_domain::json_digest(&json!({
                "role_id": event.data.get("role_id"),
                "department_id": event.data.get("department_id"),
                "session_id": event.data.get("session_id"),
            }));
            let hits = event
                .data
                .get("hits")
                .and_then(Value::as_array)
                .cloned()
                .unwrap_or_default();
            let algorithm_version = hits
                .iter()
                .find_map(|hit| hit.get("retrieval_algorithm").and_then(Value::as_str))
                .unwrap_or("memory-search-legacy.v1");
            let source_generation = hits
                .iter()
                .find_map(|hit| hit.get("generation").and_then(Value::as_u64))
                .filter(|generation| *generation > 0)
                .unwrap_or(1);
            let mut degraded_reasons = Vec::new();
            let mut entries = Vec::new();
            for hit in hits {
                let Some(candidate_id) = hit.get("id").and_then(Value::as_str) else {
                    continue;
                };
                let collection = hit
                    .get("collection")
                    .and_then(Value::as_str)
                    .unwrap_or("memory");
                let revision = hit
                    .get("revision")
                    .and_then(Value::as_u64)
                    .map(|revision| format!("revision:{revision}"))
                    .or_else(|| {
                        hit.get("revision")
                            .and_then(Value::as_str)
                            .map(str::to_owned)
                    })
                    .unwrap_or_else(|| "revision:unknown".to_owned());
                let content_digest = hit
                    .get("content_hash")
                    .and_then(Value::as_str)
                    .filter(|digest| {
                        digest.strip_prefix("sha256:").is_some_and(|hex| {
                            hex.len() == 64 && hex.bytes().all(|byte| byte.is_ascii_hexdigit())
                        })
                    })
                    .map(str::to_owned)
                    .unwrap_or_else(|| {
                        kiana_domain::json_digest(&json!({
                            "candidate_id": candidate_id,
                            "text": hit.get("text"),
                        }))
                    });
                let evidence = match hit.get("provenance").and_then(Value::as_str) {
                    Some("attributed") => kiana_domain::EvidenceStatus::Attributed,
                    Some("verified") => kiana_domain::EvidenceStatus::Verified,
                    _ => kiana_domain::EvidenceStatus::Unverifiable,
                };
                let degraded = hit
                    .get("degraded")
                    .and_then(Value::as_bool)
                    .unwrap_or(false);
                if let Some(reason) = hit.get("degraded_reason").and_then(Value::as_str) {
                    if !reason.trim().is_empty() && !degraded_reasons.contains(&reason.to_owned()) {
                        degraded_reasons.push(reason.to_owned());
                    }
                }
                let Ok(snapshot) = kiana_domain::memory_source_snapshot(
                    candidate_id,
                    collection,
                    &revision,
                    &content_digest,
                    evidence,
                ) else {
                    continue;
                };
                let Ok(entry) = kiana_domain::RetrievalReceiptEntry::new(
                    candidate_id,
                    kiana_domain::RetrievalReceiptStage::Retrieved,
                    snapshot,
                    kiana_domain::json_digest(&hit),
                    degraded,
                    None,
                ) else {
                    continue;
                };
                entries.push(entry);
            }
            match kiana_domain::RetrievalReceipt::new(
                format!("retrieval:{}", event.event_id),
                query,
                query_digest,
                permission_scope_digest,
                algorithm_version,
                source_generation,
                !degraded_reasons.is_empty(),
                degraded_reasons,
                entries,
                Vec::new(),
            ) {
                Ok(receipt) => serde_json::to_value(receipt).unwrap_or_else(|_| {
                    json!({"schema":kiana_domain::RETRIEVAL_RECEIPT_SCHEMA,"status":"unavailable",
                        "reason":"retrieval_receipt_encode_failed"})
                }),
                Err(reason) => json!({
                    "schema": kiana_domain::RETRIEVAL_RECEIPT_SCHEMA,
                    "status": "unavailable",
                    "reason": reason,
                }),
            }
        })
        .collect()
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

fn bounded_observation_text(value: Option<&Value>) -> Option<String> {
    let text = value.and_then(Value::as_str).map(redact_event_text)?;
    let mut bounded = text.chars().take(256).collect::<String>();
    if text.chars().count() > 256 {
        while bounded.len() > 253 {
            bounded.pop();
        }
        bounded.push('…');
    }
    (!bounded.is_empty()).then_some(bounded)
}

fn digest_observation(data: &Value, names: &[&str]) -> Option<String> {
    names.iter().find_map(|name| {
        let value = data.get(*name).and_then(Value::as_str)?;
        let hex = value.strip_prefix("sha256:")?;
        (hex.len() == 64 && hex.bytes().all(|byte| byte.is_ascii_hexdigit()))
            .then(|| value.to_owned())
    })
}

fn policy_observations_from_events(events: &[RuntimeEvent]) -> Vec<Value> {
    events
        .iter()
        .enumerate()
        .filter(|(_, event)| event.kind == "capability.decision")
        .map(|(index, event)| {
            let policy = event.data.get("policy");
            let gate = event.data.get("gate");
            json!({
                "event_id": event.event_id,
                "source_cursor": index.saturating_add(1),
                "request_id": event.data.get("capability_request_id").cloned().unwrap_or_else(|| json!(event.request_id)),
                "policy_verdict": policy.and_then(|value| value.get("decision")).and_then(Value::as_str),
                "policy_reason": bounded_observation_text(policy.and_then(|value| value.get("reason"))),
                "gate_verdict": gate.and_then(|value| value.get("decision")).and_then(Value::as_str),
                "gate_reason": bounded_observation_text(gate.and_then(|value| value.get("reason"))),
                "policy_revision": policy.and_then(|value| value.get("policy_revision")).and_then(Value::as_u64),
                "authority_epoch": event.data.get("authority_epoch").and_then(Value::as_u64),
                "action_digest": digest_observation(&event.data, &["action_digest", "args_fingerprint"]),
                "tool_args_hash": digest_observation(&event.data, &["args_fingerprint", "action_digest"]),
            })
        })
        .collect()
}

fn cancellation_observations_from_events(events: &[RuntimeEvent]) -> Vec<Value> {
    events
        .iter()
        .enumerate()
        .filter(|(_, event)| {
            event.kind.contains("cancel")
                || event.data.get("cancellation_reason").is_some()
                || event.data.get("cancel_reason").is_some()
        })
        .map(|(index, event)| {
            json!({
                "event_id": event.event_id,
                "source_cursor": index.saturating_add(1),
                "kind": event.kind,
                "reason": bounded_observation_text(event.data.get("cancellation_reason").or_else(|| event.data.get("cancel_reason")).or_else(|| event.data.get("reason")).or_else(|| event.data.get("error"))),
                "stop_confirmed": event.data.get("stop_confirmed").and_then(Value::as_bool),
                "action_digest": digest_observation(&event.data, &["action_digest", "args_fingerprint"]),
            })
        })
        .collect()
}

fn observability_from_events(run_id: RunId, events: &[RuntimeEvent]) -> Value {
    let source_cursor = events.len().min(u64::MAX as usize) as u64;
    let source_event_ids = events
        .iter()
        .rev()
        .map(|event| event.event_id)
        .take(kiana_domain::MAX_SOURCE_EVENT_IDS)
        .collect::<Vec<_>>();
    let persistence_revision = kiana_domain::json_digest(&json!({
        "source_cursor": source_cursor,
        "source_event_ids": source_event_ids,
    }));
    let model_attempts = crate::project_model_attempts(run_id, events)
        .ok()
        .and_then(|records| serde_json::to_value(records).ok())
        .unwrap_or_else(|| json!([]));
    let capability_attempts = crate::project_capability_attempts(run_id, events)
        .ok()
        .and_then(|records| serde_json::to_value(records).ok())
        .unwrap_or_else(|| json!([]));
    let span_lifecycle = crate::project_spans(run_id, events)
        .ok()
        .and_then(|records| serde_json::to_value(records).ok())
        .unwrap_or_else(|| json!([]));
    // Receipt metadata may point at operator evidence, but it cannot manufacture a health probe
    // or claim an external effect succeeded. The full bounded projection is exposed through the
    // read-only ControlPlane/DaemonHost operator-evidence query.
    let operator_evidence = json!({
        "schema": kiana_domain::OPERATOR_EVIDENCE_SCHEMA,
        "source_cursor": source_cursor,
        "source_event_ids": source_event_ids,
        "effect_success_claim": false,
        "limitations": ["receipt_projection_requires_health_probe"],
    });
    json!({
        "schema": kiana_domain::OBSERVABILITY_SCHEMA,
        "source_cursor": source_cursor,
        "source_event_ids": source_event_ids,
        "persistence_revision": persistence_revision,
        "model_attempts": model_attempts,
        "capability_attempts": capability_attempts,
        "span_lifecycle": span_lifecycle,
        "policy_decisions": policy_observations_from_events(events),
        "cancellations": cancellation_observations_from_events(events),
        "operator_evidence": operator_evidence,
    })
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

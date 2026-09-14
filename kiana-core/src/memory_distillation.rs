//! Durable, at-most-once model distillation through the existing ControlPlane run path.
//! Queuing does not call a model. A separate operator command claims one bounded job.
use super::*;
use kiana_domain::{
    DistillationEvidence, MemoryDistillationArguments, MemoryDistillationJob, SessionId,
    MEMORY_DISTILL_COMMAND, MEMORY_DISTILL_SESSION_PREFIX, MEMORY_DISTILL_STREAM,
};

const QUEUED: &str = "memory.distillation_queued";
const CLAIMED: &str = "memory.distillation_claimed";
const STARTED: &str = "memory.distillation_started";
const COMPLETED: &str = "memory.distillation_completed";
const FAILED: &str = "memory.distillation_failed";

impl ControlPlane {
    /// Queue every recorded terminal, including denial/cancellation and unknown outcomes.
    /// Failure to derive a candidate must never change the source execution's result.
    pub(crate) async fn queue_terminal_distillation(&self, run_id: RunId, kind: &str) {
        if let Err(error) = self.try_queue_terminal_distillation(run_id, kind).await {
            let _ = self
                .append_event(
                    RequestId::new(),
                    1,
                    "memory.extraction_incident",
                    json!({
                        "source_run_id":run_id,"source_kind":kind,
                        "reason":super::redaction::redact_event_text(&error.to_string()),
                    }),
                )
                .await;
        }
    }

    async fn try_queue_terminal_distillation(
        &self,
        run_id: RunId,
        kind: &str,
    ) -> Result<(), CoreError> {
        let events = self.events.read_stream("run", &run_id.to_string()).await?;
        let Some(authorized) = events.iter().find(|e| e.kind == "run.authorized") else {
            return Ok(());
        };
        let Some(source) = events.iter().rev().find(|e| e.kind == kind) else {
            return Ok(());
        };
        let data = &authorized.data;
        let session = data["session_id"].as_str().unwrap_or_default();
        if session.starts_with(MEMORY_DISTILL_SESSION_PREFIX) {
            return Ok(());
        }
        let Some(actor) = data["actor_id"].as_str() else {
            return Ok(());
        };
        let context = RequestContext {
            request_id: source.request_id,
            session_id: SessionId::new(session),
            project_root: data["project_root"].as_str().unwrap_or_default().to_owned(),
            actor_id: Some(actor.to_owned()),
            project_trusted: false,
            permission_profile: PermissionProfile::Safe,
            role_id: data["role_id"].as_str().unwrap_or_default().to_owned(),
            department_id: data["department_id"]
                .as_str()
                .unwrap_or_default()
                .to_owned(),
            work_packet_id: None,
            cell_id: None,
            path_allow: Vec::new(),
        };
        self.queue_memory_distillation(&context, Some(run_id), source, "lesson", &events)
            .await
    }

    pub(crate) async fn queue_memory_distillation(
        &self,
        context: &RequestContext,
        run_id: Option<RunId>,
        source: &RuntimeEvent,
        kind: &str,
        events: &[RuntimeEvent],
    ) -> Result<(), CoreError> {
        if context
            .session_id
            .as_str()
            .starts_with(MEMORY_DISTILL_SESSION_PREFIX)
        {
            return Ok(());
        }
        let Some(actor) = context.actor_id.as_deref().filter(|a| !a.trim().is_empty()) else {
            return Ok(());
        };
        let job_id = source.event_id.to_string();
        // The source event itself is the queue key; duplicate terminal hooks cannot add work.
        if !self
            .events
            .read_stream(MEMORY_DISTILL_STREAM, &job_id)
            .await?
            .is_empty()
        {
            return Ok(());
        }
        let text = evidence_text(source, kind == "decision");
        if text.trim().is_empty() {
            return Ok(());
        }
        let mut evidence = vec![DistillationEvidence {
            event_id: source.event_id,
            request_id: source.request_id,
            run_id,
            kind: source.kind.clone(),
            text: bounded(&text, 6000),
        }];
        let mut remaining = 16 * 1024 - evidence[0].text.len();
        // A meeting contributes only its published decision, never private debate.
        if kind == "lesson" {
            let turn = events
                .iter()
                .rposition(|e| e.kind == "run.prompt")
                .unwrap_or(0);
            for event in events[turn..].iter().rev().filter(|e| {
                e.event_id != source.event_id
                    && matches!(
                        e.kind.as_str(),
                        "run.prompt"
                            | "capability.completed"
                            | "capability.failed"
                            | "capability.result_unknown"
                    )
            }) {
                if evidence.len() >= 12 || remaining < 64 {
                    break;
                }
                let text = bounded(&evidence_text(event, false), remaining.min(4000));
                if text.trim().is_empty() {
                    continue;
                }
                remaining -= text.len();
                evidence.push(DistillationEvidence {
                    event_id: event.event_id,
                    request_id: event.request_id,
                    run_id,
                    kind: event.kind.clone(),
                    text,
                });
            }
        }
        let role = RoleSpec::lookup(&context.role_id)
            .ok_or_else(|| PortError::Failed("role_unknown".to_owned()))?;
        // Reuse only references already returned by the source run's authorized memory search.
        let mut similar_records = Vec::new();
        if kind == "lesson" {
            for hit in events
                .iter()
                .rev()
                .filter(|e| e.kind == "capability.completed")
                .filter_map(|e| e.data.get("hits").and_then(Value::as_array))
                .flatten()
            {
                if similar_records.len() == 3 {
                    break;
                }
                if hit["collection"]
                    .as_str()
                    .is_some_and(|c| role.allows_knowledge(c))
                    && hit["id"].as_str().is_some()
                    && hit["text"].as_str().is_some()
                    && !similar_records
                        .iter()
                        .any(|old: &Value| old["id"] == hit["id"])
                {
                    similar_records.push(json!({"id":hit["id"],"collection":hit["collection"],
                        "text":bounded(hit["text"].as_str().unwrap_or_default(),2048)}));
                }
            }
        }
        let job = MemoryDistillationJob {
            schema: "kiana.memory-distillation-job.v1".to_owned(),
            job_id: job_id.clone(),
            actor_id: actor.to_owned(),
            project_root: context.project_root.clone(),
            role_id: context.role_id.clone(),
            department_id: context.department_id.clone(),
            source_session_id: context.session_id.clone(),
            source_run_id: run_id,
            source_event_id: source.event_id,
            source_request_id: source.request_id,
            kind: kind.to_owned(),
            evidence,
            similar_records,
        };
        job.validate().map_err(distill_error)?;
        let event = RuntimeEvent::new(RequestId::new(),1,QUEUED,super::redaction::redact_event_value(&json!({
            "job":job,"source_run_id":run_id,"source_event_id":source.event_id,"project_root":context.project_root,
            "automatic_execution":false,
        })))?.with_stream_metadata(MEMORY_DISTILL_STREAM,&job_id,1)
            .with_idempotency_key(format!("memory-distill:{job_id}:queued"));
        // Validate the redacted representation that the consumer will actually read.
        parse_job(&event)?;
        match self.events.append_expected(event, Some(0)).await {
            Ok(()) => Ok(()),
            Err(PortError::Conflict(reason)) if reason == "event_stream_version_mismatch" => {
                if self
                    .events
                    .read_stream(MEMORY_DISTILL_STREAM, &job_id)
                    .await?
                    .iter()
                    .any(|event| {
                        event.kind == QUEUED
                            && event.data["job"]["source_event_id"] == json!(source.event_id)
                    })
                {
                    Ok(())
                } else {
                    Err(PortError::Conflict(reason).into())
                }
            }
            Err(error) => Err(error.into()),
        }
    }

    pub async fn handle_memory_distillation(
        &self,
        context: RequestContext,
        arguments: Value,
    ) -> Result<CoreResponse, CoreError> {
        let request_id = context.request_id;
        let accepted = super::redaction::redact_event_value(&json!({
            "command":MEMORY_DISTILL_COMMAND,"arguments":arguments,"actor_id":context.actor_id,
            "project_root":Self::canonical_project_root(&context.project_root),"role_id":context.role_id,
            "department_id":context.department_id,"session_id":context.session_id,
            "project_trusted":context.project_trusted,"cell_id":context.cell_id,"work_packet_id":context.work_packet_id,
        }));
        let prior = self.events.read_request(&request_id).await?;
        if let Some(previous) = prior.iter().find(|e| e.kind == "request.accepted") {
            if previous.data != accepted {
                return Ok(CoreResponse::blocked(
                    request_id,
                    "memory_distillation_request_conflict",
                ));
            }
            // An envelope replay cannot claim a second queued job, even after a restart.
            if let Some(result) = prior.iter().rev().find(|e| {
                matches!(
                    e.kind.as_str(),
                    "command.completed" | "command.failed" | "command.rejected"
                )
            }) {
                let status = match result.kind.as_str() {
                    "command.completed" => ExecutionStatus::Completed,
                    "command.failed" => ExecutionStatus::Failed,
                    _ => ExecutionStatus::Blocked,
                };
                return Ok(CoreResponse {
                    request_id,
                    status,
                    output: result.data.clone(),
                    error: if status == ExecutionStatus::Completed {
                        None
                    } else {
                        result.data["reason"].as_str().map(str::to_owned)
                    },
                });
            }
            if prior.iter().any(|event| event.kind == CLAIMED) {
                return Ok(CoreResponse::blocked(
                    request_id,
                    "memory_distillation_already_claimed",
                ));
            }
        }
        self.append_event(request_id, 1, "request.accepted", accepted)
            .await?;
        let args = match serde_json::from_value::<MemoryDistillationArguments>(arguments) {
            Ok(args) if matches!(args.action.as_str(), "list" | "consume" | "finalize") => args,
            _ => {
                return self
                    .reject_distillation_command(
                        request_id,
                        "memory_distillation_arguments_invalid",
                    )
                    .await
            }
        };
        if !context.project_trusted {
            return self
                .reject_distillation_command(request_id, "project_untrusted")
                .await;
        }
        if context
            .actor_id
            .as_deref()
            .is_none_or(|a| a.trim().is_empty())
            || context.cell_id.is_some()
            || context
                .session_id
                .as_str()
                .starts_with(MEMORY_DISTILL_SESSION_PREFIX)
        {
            return self
                .reject_distillation_command(request_id, "memory_distillation_operator_required")
                .await;
        }
        let events = self.events.read_all().await?;
        let mut jobs = Vec::new();
        for event in events.iter().filter(|e| {
            e.kind == QUEUED && e.aggregate_type.as_deref() == Some(MEMORY_DISTILL_STREAM)
        }) {
            let job = parse_job(event)?;
            if !job_matches_context(&job, &context)
                || args.job_id.as_deref().is_some_and(|id| id != job.job_id)
            {
                continue;
            }
            let stream = events
                .iter()
                .filter(|e| {
                    e.aggregate_type.as_deref() == Some(MEMORY_DISTILL_STREAM)
                        && e.aggregate_id.as_deref() == Some(job.job_id.as_str())
                })
                .collect::<Vec<_>>();
            let latest = stream
                .iter()
                .max_by_key(|e| e.stream_version.unwrap_or(e.sequence))
                .copied()
                .ok_or_else(|| distill_error("memory_distillation_state_missing"))?;
            jobs.push((job, latest.clone()));
        }
        if args.action == "list" {
            let output = json!({"schema":"kiana.memory-distillation-status.v1","jobs":jobs.iter().map(|(job,last)|json!({
                "job_id":job.job_id,"source_run_id":job.source_run_id,"source_event_id":job.source_event_id,
                "kind":job.kind,"role_id":job.role_id,"department_id":job.department_id,"state":last.kind,
                "internal_run_id":last.data.get("internal_run_id"),"result_unknown":matches!(last.kind.as_str(),CLAIMED|STARTED),
            })).collect::<Vec<_>>()});
            self.append_event(request_id, 2, "command.completed", output.clone())
                .await?;
            return Ok(CoreResponse::completed(request_id, output));
        }
        if args.action == "finalize" {
            let Some((job, last)) = jobs.into_iter().next().filter(|_| args.job_id.is_some())
            else {
                return self
                    .reject_distillation_command(request_id, "memory_distillation_job_required")
                    .await;
            };
            if !matches!(last.kind.as_str(), CLAIMED | STARTED) {
                return self
                    .reject_distillation_command(request_id, "memory_distillation_not_unsettled")
                    .await;
            }
            // Reconcile an interrupted consumer from the recorded model result; never rerun it.
            return self
                .finish_memory_distillation(request_id, &job, &last)
                .await;
        }
        let Some((job, last)) = jobs.into_iter().find(|(_, last)| last.kind == QUEUED) else {
            let output = json!({"schema":"kiana.memory-distillation-status.v1","consumed":false,"reason":"no_queued_job"});
            self.append_event(request_id, 2, "command.completed", output.clone())
                .await?;
            return Ok(CoreResponse::completed(request_id, output));
        };
        let internal_run_id = RunId::new();
        let internal_request_id = RequestId::new();
        let internal_session_id =
            SessionId::new(format!("{MEMORY_DISTILL_SESSION_PREFIX}{}", job.job_id));
        let claim=self.append_distillation_state(request_id,&job,&last,CLAIMED,json!({
            "internal_run_id":internal_run_id,"internal_request_id":internal_request_id,"internal_session_id":internal_session_id,
            "actor_id":job.actor_id,"role_id":job.role_id,"department_id":job.department_id,"project_root":job.project_root,
            "max_model_calls":1,"max_steps_per_turn":1,"sandbox":"read-only","tool_policy":"deny_all",
        })).await?;
        let mut internal = context.clone();
        internal.request_id = internal_request_id;
        internal.session_id = internal_session_id;
        internal.role_id = job.role_id.clone();
        internal.department_id = job.department_id.clone();
        internal.permission_profile = PermissionProfile::Safe;
        internal.work_packet_id = None;
        internal.cell_id = None;
        internal.path_allow.clear();
        let result = self
            .start_run_with_id(
                internal,
                distillation_prompt(&job)?,
                Vec::new(),
                Some("read-only".to_owned()),
                Some(internal_run_id),
            )
            .await;
        let stream = self
            .events
            .read_stream(MEMORY_DISTILL_STREAM, &job.job_id)
            .await?;
        let latest = stream
            .iter()
            .max_by_key(|e| e.stream_version.unwrap_or(e.sequence))
            .unwrap_or(&claim);
        if let Err(error) = result {
            return self
                .fail_memory_distillation(
                    request_id,
                    &job,
                    latest,
                    &format!("distillation_run_error:{error}"),
                )
                .await;
        }
        self.finish_memory_distillation(request_id, &job, latest)
            .await
    }

    /// Called only by start_run_with_id. Prefixes may restrict but never authorize a run.
    /// The single-use claim binds every identity and the exact evidence prompt before the model.
    pub(crate) async fn claim_distillation_start(
        &self,
        context: &RequestContext,
        run_id: RunId,
        prompt: &str,
        history_empty: bool,
        sandbox: Option<&str>,
    ) -> Result<bool, CoreError> {
        let Some(job_id) = context
            .session_id
            .as_str()
            .strip_prefix(MEMORY_DISTILL_SESSION_PREFIX)
        else {
            return Ok(false);
        };
        let events = self
            .events
            .read_stream(MEMORY_DISTILL_STREAM, job_id)
            .await?;
        let queued = events
            .iter()
            .find(|e| e.kind == QUEUED)
            .ok_or_else(|| distill_error("memory_distillation_claim_required"))?;
        let job = parse_job(queued)?;
        let claim = events
            .iter()
            .max_by_key(|e| e.stream_version.unwrap_or(e.sequence))
            .ok_or_else(|| distill_error("memory_distillation_claim_required"))?;
        if claim.kind != CLAIMED
            || !job_matches_context(&job, context)
            || !context.project_trusted
            || claim.data["internal_request_id"] != json!(context.request_id)
            || claim.data["internal_run_id"] != json!(run_id)
            || claim.data["internal_session_id"] != json!(context.session_id)
            || !history_empty
            || context.permission_profile != PermissionProfile::Safe
            || context.cell_id.is_some()
            || context.work_packet_id.is_some()
            || !context.path_allow.is_empty()
            || sandbox != Some("read-only")
            || prompt != distillation_prompt(&job)?
        {
            return Err(distill_error("memory_distillation_claim_mismatch"));
        }
        self.append_distillation_state(
            context.request_id,
            &job,
            claim,
            STARTED,
            claim.data.clone(),
        )
        .await?;
        Ok(true)
    }

    async fn finish_memory_distillation(
        &self,
        request_id: RequestId,
        job: &MemoryDistillationJob,
        last: &RuntimeEvent,
    ) -> Result<CoreResponse, CoreError> {
        if matches!(last.kind.as_str(), "memory.proposed" | COMPLETED) {
            return Ok(CoreResponse::completed(request_id, last.data.clone()));
        }
        if !matches!(last.kind.as_str(), CLAIMED | STARTED) {
            return Ok(CoreResponse::blocked(
                request_id,
                "memory_distillation_already_settled",
            ));
        }
        let Some(run_id) = last.data["internal_run_id"]
            .as_str()
            .and_then(RunId::parse_str)
        else {
            return self
                .fail_memory_distillation(request_id, job, last, "memory_distillation_run_missing")
                .await;
        };
        let events = self.events.read_stream("run", &run_id.to_string()).await?;
        let terminal = events.iter().rev().find(|e| {
            matches!(
                e.kind.as_str(),
                "run.completed" | "run.failed" | "run.cancelled" | "run.result_unknown"
            )
        });
        let Some(terminal) = terminal else {
            // A crash or concurrent invocation may still be in flight. Do not settle or retry.
            return self
                .reject_distillation_command(request_id, "memory_distillation_result_unknown")
                .await;
        };
        if terminal.kind != "run.completed" {
            return self
                .fail_memory_distillation(
                    request_id,
                    job,
                    last,
                    &format!("memory_distillation_run_terminal:{}", terminal.kind),
                )
                .await;
        }
        let model_turns = events
            .iter()
            .filter(|e| e.kind == "run.model_turn")
            .collect::<Vec<_>>();
        if model_turns.len() != 1
            || model_turns[0].data["attempted"] != true
            || events.iter().any(|e| e.kind == "capability.requested")
        {
            return self
                .fail_memory_distillation(
                    request_id,
                    job,
                    last,
                    "memory_distillation_model_call_bound",
                )
                .await;
        }
        let text = terminal.data["text"].as_str().unwrap_or_default();
        let (verdict, reason, proposal) = match job.validate_output(text) {
            Ok(output) => output,
            Err(reason) => {
                return self
                    .fail_memory_distillation(request_id, job, last, reason)
                    .await
            }
        };
        let event_kind = if proposal.is_some() {
            "memory.proposed"
        } else {
            COMPLETED
        };
        let output = json!({"schema":"kiana.memory-distillation-result.v1","job_id":job.job_id,"consumed":true,
            "verdict":verdict,"reason":reason,"proposal":proposal,"project_root":job.project_root,
            "session_id":job.source_session_id,"source_run_id":job.source_run_id,"internal_run_id":run_id,
            "distillation":{"schema":kiana_domain::MEMORY_DISTILLATION_SCHEMA,"model_distillation":true,
                "source_event_id":job.source_event_id,"model_turn_event_id":model_turns[0].event_id,
                "provider_id":model_turns[0].data["provider_id"],"model_id":model_turns[0].data["model_id"],
                "prompt_hash":model_turns[0].data["prompt_hash"],"usage":model_turns[0].data["usage"],
                "method":"llm.memory-distillation.v1","status":"candidate","max_model_calls":1}});
        // Proposal and settled status share one atomic event, so a crash cannot create a second proposal.
        self.append_distillation_state(request_id, job, last, event_kind, output.clone())
            .await?;
        self.append_event(request_id, 3, "command.completed", output.clone())
            .await?;
        Ok(CoreResponse::completed(request_id, output))
    }

    async fn fail_memory_distillation(
        &self,
        request_id: RequestId,
        job: &MemoryDistillationJob,
        last: &RuntimeEvent,
        reason: &str,
    ) -> Result<CoreResponse, CoreError> {
        let reason = super::redaction::redact_event_text(reason);
        let output = json!({"schema":"kiana.memory-distillation-result.v1","job_id":job.job_id,"consumed":true,
            "state":FAILED,"reason":reason,"internal_run_id":last.data["internal_run_id"],"candidate_created":false});
        self.append_distillation_state(request_id, job, last, FAILED, output.clone())
            .await?;
        self.append_event(request_id, 3, "command.failed", output.clone())
            .await?;
        Ok(CoreResponse {
            request_id,
            status: ExecutionStatus::Failed,
            output,
            error: Some(reason),
        })
    }

    async fn reject_distillation_command(
        &self,
        request_id: RequestId,
        reason: &str,
    ) -> Result<CoreResponse, CoreError> {
        self.append_event(
            request_id,
            2,
            "command.rejected",
            json!({"command":MEMORY_DISTILL_COMMAND,"reason":reason}),
        )
        .await?;
        Ok(CoreResponse::blocked(request_id, reason))
    }

    async fn append_distillation_state(
        &self,
        request_id: RequestId,
        job: &MemoryDistillationJob,
        previous: &RuntimeEvent,
        kind: &str,
        mut data: Value,
    ) -> Result<RuntimeEvent, CoreError> {
        if !matches!(
            (previous.kind.as_str(), kind),
            (QUEUED, CLAIMED)
                | (CLAIMED, STARTED)
                | (CLAIMED | STARTED, "memory.proposed" | COMPLETED | FAILED)
        ) {
            return Err(distill_error("memory_distillation_transition_invalid"));
        }
        let version = previous
            .stream_version
            .ok_or_else(|| distill_error("memory_distillation_version_required"))?;
        data["job_id"] = json!(job.job_id);
        data["project_root"] = json!(job.project_root);
        data["source_run_id"] = json!(job.source_run_id);
        let event = RuntimeEvent::new(
            request_id,
            version + 1,
            kind,
            super::redaction::redact_event_value(&data),
        )?
        .with_stream_metadata(MEMORY_DISTILL_STREAM, &job.job_id, version + 1)
        .with_idempotency_key(format!("memory-distill:{}:{kind}", job.job_id));
        // CAS, not idempotent replay: only the winner may proceed to the paid model call.
        self.events
            .append_expected(event.clone(), Some(version))
            .await?;
        Ok(event)
    }
}

fn distill_error(reason: &str) -> CoreError {
    PortError::Failed(reason.to_owned()).into()
}
fn parse_job(event: &RuntimeEvent) -> Result<MemoryDistillationJob, CoreError> {
    let job: MemoryDistillationJob = serde_json::from_value(event.data["job"].clone())
        .map_err(|_| distill_error("memory_distillation_job_invalid"))?;
    job.validate().map_err(distill_error)?;
    if event.aggregate_id.as_deref() != Some(job.job_id.as_str()) {
        return Err(distill_error("memory_distillation_job_scope_mismatch"));
    }
    Ok(job)
}
fn job_matches_context(job: &MemoryDistillationJob, context: &RequestContext) -> bool {
    context.actor_id.as_deref() == Some(job.actor_id.as_str())
        && job.role_id == context.role_id
        && job.department_id == context.department_id
        && ControlPlane::canonical_project_root(&job.project_root)
            == ControlPlane::canonical_project_root(&context.project_root)
}
fn distillation_prompt(job: &MemoryDistillationJob) -> Result<String, CoreError> {
    serde_json::to_string(
        &json!({"schema":"kiana.memory-distillation-input.v1","job_id":job.job_id,
        "kind":job.kind,"evidence":job.evidence,"similar_records":job.similar_records,
        "output_schema":kiana_domain::MEMORY_DISTILLATION_SCHEMA}),
    )
    .map_err(|_| distill_error("memory_distillation_prompt_invalid"))
}
fn bounded(text: &str, bytes: usize) -> String {
    let mut end = text.len().min(bytes);
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    text[..end].to_owned()
}
fn evidence_text(event: &RuntimeEvent, decision_only: bool) -> String {
    if decision_only {
        return event
            .data
            .pointer("/decision/summary")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_owned();
    }
    event
        .data
        .get("text")
        .or_else(|| event.data.get("error"))
        .or_else(|| event.data.get("reason"))
        .and_then(Value::as_str)
        .map(str::to_owned)
        .unwrap_or_else(|| serde_json::to_string(&event.data).unwrap_or_default())
}

use super::events::*;
use super::redaction::*;
use super::*;
use kiana_domain::CapabilityErrorCode;

fn lifecycle_error_code(error: &str) -> CapabilityErrorCode {
    CapabilityErrorCode::from_reason(error)
}

impl ControlPlane {
    pub async fn start_run(
        &self,
        context: RequestContext,
        prompt: String,
        sandbox: Option<String>,
    ) -> Result<CoreResponse, CoreError> {
        self.start_run_with_id(context, prompt, Vec::new(), sandbox, None, None)
            .await
    }

    pub async fn start_run_with_history(
        &self,
        context: RequestContext,
        prompt: String,
        history: Vec<kiana_domain::ConversationMessage>,
        sandbox: Option<String>,
    ) -> Result<CoreResponse, CoreError> {
        self.start_run_with_id(context, prompt, history, sandbox, None, None)
            .await
    }

    pub(crate) async fn start_run_with_id(
        &self,
        mut context: RequestContext,
        prompt: String,
        history: Vec<kiana_domain::ConversationMessage>,
        sandbox: Option<String>,
        requested_run_id: Option<RunId>,
        predecessor: Option<RunId>,
    ) -> Result<CoreResponse, CoreError> {
        let request_id = context.request_id;
        let run_id = requested_run_id.unwrap_or_else(|| {
            RunId::parse_str(context.session_id.as_str()).unwrap_or_else(RunId::new)
        });
        let turn_id = kiana_domain::TurnId::from_uuid(request_id.as_uuid());
        let turn = kiana_domain::TurnIdentity::new(
            context.session_id.as_str(),
            run_id,
            turn_id,
            predecessor,
            if predecessor.is_some() {
                kiana_domain::TurnSemantics::NewTurn
            } else {
                kiana_domain::TurnSemantics::Start
            },
            1,
        )
        .map_err(|error| PortError::Failed(format!("turn_identity_invalid:{error}")))?;
        let mut sequence = 1u64;
        if let Some(previous_run_id) = predecessor {
            self.record_event(
                request_id,
                &mut sequence,
                "run.predecessor",
                json!({
                    "run_id": run_id,
                    "previous_run_id": previous_run_id,
                    "turn_id": turn_id,
                    "turn": turn,
                    "session_id": context.session_id,
                    "semantics": "new_turn_v2",
                }),
            )
            .await?;
        }
        let command = if predecessor.is_some() {
            "run.turn.v2"
        } else {
            "run.start"
        };
        self.record_event(
            request_id,
            &mut sequence,
            "request.accepted",
            json!({
                "command": command,
                "run_id": run_id,
                "session_id": context.session_id,
                "actor_id": context.actor_id,
                "project_root": context.project_root,
                "role_id": context.role_id,
                "harness": HARNESS_ID,
            }),
        )
        .await?;

        if prompt.trim().is_empty() {
            self.record_event(
                request_id,
                &mut sequence,
                "run.rejected",
                json!({ "reason": "prompt_required" }),
            )
            .await?;
            return Ok(CoreResponse::blocked(request_id, "prompt_required"));
        }

        let Some(role) = RoleSpec::lookup(&context.role_id) else {
            self.record_event(
                request_id,
                &mut sequence,
                "run.rejected",
                json!({ "reason": "role_unknown" }),
            )
            .await?;
            return Ok(CoreResponse::blocked(request_id, "role_unknown"));
        };
        if !context.department_id.trim().is_empty() && context.department_id != role.department_id {
            self.record_event(
                request_id,
                &mut sequence,
                "run.rejected",
                json!({ "reason": "role_department_mismatch" }),
            )
            .await?;
            return Ok(CoreResponse::blocked(
                request_id,
                "role_department_mismatch",
            ));
        }

        let distillation = match self
            .claim_distillation_start(
                &context,
                run_id,
                &prompt,
                history.is_empty(),
                sandbox.as_deref(),
            )
            .await
        {
            Ok(value) => value,
            Err(error) => {
                self.record_event(
                    request_id,
                    &mut sequence,
                    "run.rejected",
                    json!({"run_id":run_id,"reason":error.to_string()}),
                )
                .await?;
                return Ok(CoreResponse::blocked(request_id, error.to_string()));
            }
        };
        let max_steps_per_turn = self
            .company_runtime_step_limit(&context, self.max_steps_per_turn.unwrap_or(role.max_steps))
            .await?;
        let max_steps_per_turn = if distillation {
            max_steps_per_turn.min(1)
        } else {
            max_steps_per_turn
        };
        if max_steps_per_turn == 0 {
            self.record_event(
                request_id,
                &mut sequence,
                "run.rejected",
                json!({ "reason": "runtime_config_invalid:max_steps_per_turn" }),
            )
            .await?;
            return Ok(CoreResponse::blocked(
                request_id,
                "runtime_config_invalid:max_steps_per_turn",
            ));
        }

        let sandbox = match authorized_harness_sandbox(&context, sandbox.as_deref()) {
            Ok(sandbox) => sandbox,
            Err(reason) => {
                self.record_event(
                    request_id,
                    &mut sequence,
                    "run.rejected",
                    json!({ "reason": reason }),
                )
                .await?;
                return Ok(CoreResponse::blocked(request_id, reason));
            }
        };

        if let Err(error) = self
            .bind_company_run_scope(&mut context, sandbox, true)
            .await
        {
            self.record_event(
                request_id,
                &mut sequence,
                "run.rejected",
                json!({"reason":error.to_string(),"run_id":run_id}),
            )
            .await?;
            return Ok(CoreResponse::blocked(request_id, error.to_string()));
        }

        let runtime_budget = self
            .runtime_budget_for_run(&context, max_steps_per_turn)
            .await?;
        let authority_revision = self.authority_revision(&context.project_root).await?;
        self.record_event(
            request_id,
            &mut sequence,
            "run.authorized",
            json!({
                "run_id": run_id,
                "session_id": context.session_id,
                "actor_id": context.actor_id,
                "project_root": context.project_root,
                "role_id": context.role_id,
                "department_id": context.department_id,
                "harness": HARNESS_ID,
                "sandbox": sandbox,
                "capability_mode": "brokered",
                "max_steps_per_turn": max_steps_per_turn,
                "runtime_budget":runtime_budget,
                "authority_revision":authority_revision,
                "turn_id":kiana_domain::TurnId::from_uuid(request_id.as_uuid()),
                "turn":turn,
                "role_prompt_hash": role.prompt_hash,
                "model_profile": role.model_profile,
                "role_spec_schema": role.schema,
                "role_version": role.version,
                "role_catalog_schema": kiana_domain::ROLE_CATALOG_SCHEMA,
                "role_catalog_version": kiana_domain::SchemaVersion::new(1, 0),
                "role_input_schema": role.input_schema,
                "role_output_schema": role.output_schema,
            }),
        )
        .await?;

        if let Some(response) = self
            .checkpoint_before_input(&context, run_id, &mut sequence)
            .await?
        {
            return Ok(response);
        }

        self.record_event(
            request_id,
            &mut sequence,
            "run.prompt",
            json!({
                "run_id": run_id,
                "session_id": context.session_id,
                "turn_id": turn_id,
                "turn": turn.clone(),
                "text": &prompt,
            }),
        )
        .await?;

        // Session index and cancel watch must exist before the first model step
        // so an in-flight cancel can resolve session-1 and abort shell.exec.
        self.remember_session(&context, run_id);
        let _cancel_rx = self.watch_cancel(run_id);
        let _terminal_scope = self.begin_terminal_scope(run_id);

        let mut prompt_bundle = kiana_domain::PromptBundle::for_role(&role);
        if distillation {
            prompt_bundle.sections.push(kiana_domain::PromptSection {
                name: "memory_distillation".to_owned(),
                order: 180,
                text: kiana_domain::MEMORY_DISTILL_SYSTEM.to_owned(),
                source: kiana_domain::MEMORY_DISTILL_PROMPT_SOURCE.to_owned(),
                authority: kiana_domain::PromptAuthority::Product,
            });
        }

        self.runner.bind_model_assignment(
            run_id,
            kiana_domain::ModelAssignment {
                schema: "kiana.model-assignment.v1".to_owned(),
                run_id,
                turn_id: kiana_domain::TurnId::from_uuid(request_id.as_uuid()),
                role_id: role.role_id.clone(),
                role_version: Some(role.version),
                catalog_version: Some(kiana_domain::SchemaVersion::new(1, 0)),
                prompt_hash: Some(role.prompt_hash.clone()),
                input_schema: Some(role.input_schema.clone()),
                output_schema: Some(role.output_schema.clone()),
                profile: role.model_profile.clone(),
                project_root: context.project_root.clone(),
                project_trusted: context.project_trusted,
                authority_revision: authority_revision.clone(),
                max_wall_time_ms: runtime_budget.max_wall_time_ms,
            },
        )?;

        let pending_events = match self
            .runner
            .send(RunnerCommand::start_in_with_history_and_turn(
                run_id,
                prompt,
                history,
                context.project_root.clone(),
                sandbox.to_owned(),
                prompt_bundle.encode().map_err(|error| {
                    CoreError::from(PortError::Failed(format!("prompt_bundle_invalid:{error}")))
                })?,
                context.project_trusted,
                max_steps_per_turn,
                Some(turn_id),
            ))
            .await
        {
            Ok(events) => events,
            Err(error) => {
                self.forget_session(context.session_id.as_str(), run_id);
                self.clear_cancel(run_id);
                let reason = redact_event_text(&error.to_string());
                self.record_terminal_event(
                    request_id,
                    &mut sequence,
                    run_id,
                    "run.failed",
                    json!({ "run_id": run_id, "error": redact_event_text(&reason) }),
                )
                .await?;
                return Ok(CoreResponse {
                    request_id,
                    status: ExecutionStatus::Failed,
                    output: run_identity(&context, run_id, sandbox),
                    error: Some(reason),
                });
            }
        };

        self.drive_run(&context, run_id, sandbox, pending_events, &mut sequence)
            .await
    }

    /// New protocol semantics: a terminal Run is immutable; a new turn links a fresh Run.
    pub async fn continue_new_turn(
        &self,
        context: RequestContext,
        prompt: String,
        sandbox: Option<String>,
        previous: Option<RunId>,
    ) -> Result<CoreResponse, CoreError> {
        if context
            .session_id
            .as_str()
            .starts_with(kiana_domain::MEMORY_DISTILL_SESSION_PREFIX)
        {
            return Ok(CoreResponse::blocked(
                context.request_id,
                "memory_distillation_continue_denied",
            ));
        }
        let previous = match self.resolve_run_id(&context, previous).await? {
            Ok(id) => id,
            Err(reason) => return Ok(CoreResponse::blocked(context.request_id, reason)),
        };
        let state = self.run_state(previous).await?;
        if state.outcome.is_none() {
            return Ok(CoreResponse::blocked(
                context.request_id,
                "run_not_terminal_use_resume",
            ));
        }
        if state.outcome == Some(RunOutcome::ResultUnknown) {
            return Ok(CoreResponse::blocked(
                context.request_id,
                "run_unknown_requires_reconciliation",
            ));
        }
        let history = self
            .model_protocol_history(context.session_id.as_str())
            .await?;
        let run_id = RunId::new();
        self.runner.bind_model_history(run_id, history)?;
        self.start_run_with_id(
            context,
            prompt,
            Vec::new(),
            sandbox,
            Some(run_id),
            Some(previous),
        )
        .await
    }

    /// Legacy v1 continuation keeps its historical per-turn interpretation.
    pub async fn continue_run(
        &self,
        mut context: RequestContext,
        prompt: String,
        sandbox: Option<String>,
        run_id: Option<RunId>,
    ) -> Result<CoreResponse, CoreError> {
        let request_id = context.request_id;
        if context
            .session_id
            .as_str()
            .starts_with(kiana_domain::MEMORY_DISTILL_SESSION_PREFIX)
        {
            self.append_event(
                request_id,
                1,
                "run.rejected",
                json!({"reason":"memory_distillation_continue_denied"}),
            )
            .await?;
            return Ok(CoreResponse::blocked(
                request_id,
                "memory_distillation_continue_denied",
            ));
        }
        let mut sequence = 1u64;
        let run_id = match self.resolve_run_id(&context, run_id).await? {
            Ok(run_id) => run_id,
            Err(reason) => {
                self.record_event(
                    request_id,
                    &mut sequence,
                    "request.accepted",
                    json!({ "command": "run.continue" }),
                )
                .await?;
                self.record_event(
                    request_id,
                    &mut sequence,
                    "run.rejected",
                    json!({ "reason": reason }),
                )
                .await?;
                return Ok(CoreResponse::blocked(request_id, reason));
            }
        };
        let legacy_turn_number = self
            .events
            .read_stream("run", &run_id.to_string())
            .await?
            .iter()
            .filter(|event| event.kind == "run.prompt")
            .count()
            .saturating_add(1) as u64;
        let legacy_turn = kiana_domain::TurnIdentity::new(
            context.session_id.as_str(),
            run_id,
            kiana_domain::TurnId::from_uuid(request_id.as_uuid()),
            None,
            kiana_domain::TurnSemantics::LegacyContinue,
            legacy_turn_number,
        )
        .map_err(|error| PortError::Failed(format!("turn_identity_invalid:{error}")))?;

        let has_pending = self
            .pending_invocations
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .values()
            .any(|pending| pending.run_id == run_id);
        if has_pending
            || self
                .run_state(run_id)
                .await
                .is_ok_and(|state| state.phase == RunPhase::AwaitingApproval)
        {
            return Ok(CoreResponse::blocked(request_id, "run_awaiting_approval"));
        }
        self.record_event(
            request_id,
            &mut sequence,
            "request.accepted",
            json!({
                "command": "run.continue",
                "run_id": run_id,
                "turn_id": legacy_turn.turn_id,
                "turn": legacy_turn,
                "harness": HARNESS_ID,
            }),
        )
        .await?;

        if prompt.trim().is_empty() {
            self.record_event(
                request_id,
                &mut sequence,
                "run.rejected",
                json!({ "reason": "prompt_required" }),
            )
            .await?;
            return Ok(CoreResponse::blocked(request_id, "prompt_required"));
        }

        let sandbox = match authorized_harness_sandbox(&context, sandbox.as_deref()) {
            Ok(sandbox) => sandbox,
            Err(reason) => {
                self.record_event(
                    request_id,
                    &mut sequence,
                    "run.rejected",
                    json!({ "reason": reason }),
                )
                .await?;
                return Ok(CoreResponse::blocked(request_id, reason));
            }
        };

        if let Err(error) = self
            .bind_company_run_scope(&mut context, sandbox, false)
            .await
        {
            self.record_event(
                request_id,
                &mut sequence,
                "run.rejected",
                json!({"reason":error.to_string(),"run_id":run_id}),
            )
            .await?;
            return Ok(CoreResponse::blocked(request_id, error.to_string()));
        }

        if let Some(response) = self
            .checkpoint_before_input(&context, run_id, &mut sequence)
            .await?
        {
            return Ok(response);
        }

        self.record_event(
            request_id,
            &mut sequence,
            "run.prompt",
            json!({
                "run_id": run_id,
                "session_id": context.session_id,
                "turn_id": legacy_turn.turn_id,
                "turn": legacy_turn.clone(),
                "text": &prompt,
            }),
        )
        .await?;

        let _terminal_scope = self.begin_terminal_scope(run_id);
        let pending_events = match self
            .runner
            .send(RunnerCommand::continue_run(run_id, prompt))
            .await
        {
            Ok(events) => events,
            Err(error) => {
                let reason = redact_event_text(&error.to_string());
                self.record_terminal_event(
                    request_id,
                    &mut sequence,
                    run_id,
                    "run.failed",
                    json!({ "run_id": run_id, "error": redact_event_text(&reason) }),
                )
                .await?;
                return Ok(CoreResponse {
                    request_id,
                    status: ExecutionStatus::Failed,
                    output: run_identity(&context, run_id, sandbox),
                    error: Some(reason),
                });
            }
        };

        self.drive_run(&context, run_id, sandbox, pending_events, &mut sequence)
            .await
    }

    pub async fn cancel_run(
        &self,
        context: RequestContext,
        run_id: Option<RunId>,
        reason: String,
    ) -> Result<CoreResponse, CoreError> {
        let request_id = context.request_id;
        let mut sequence = 1u64;
        let reason = if reason.trim().is_empty() {
            "user".to_owned()
        } else {
            reason
        };
        // Cancellation reasons cross the runner and direct-response boundaries, so sanitize
        // them once at ingress instead of relying only on the event-log redaction boundary.
        let reason = redact_event_text(&reason);
        let run_id = match self.resolve_run_id(&context, run_id).await? {
            Ok(run_id) => run_id,
            Err(code) => {
                self.record_event(
                    request_id,
                    &mut sequence,
                    "request.accepted",
                    json!({ "command": "run.cancel" }),
                )
                .await?;
                self.record_event(
                    request_id,
                    &mut sequence,
                    "run.rejected",
                    json!({ "reason": code }),
                )
                .await?;
                return Ok(CoreResponse::blocked(request_id, code));
            }
        };

        let context = self.approval_scope(&context).await?;
        if let Ok(state) = self.run_state(run_id).await {
            if let Some(outcome) = state.outcome {
                let status = match outcome {
                    RunOutcome::Completed => ExecutionStatus::Completed,
                    RunOutcome::Failed => ExecutionStatus::Failed,
                    RunOutcome::Cancelled => ExecutionStatus::Cancelled,
                    RunOutcome::ResultUnknown => ExecutionStatus::ResultUnknown,
                };
                return Ok(CoreResponse {
                    request_id,
                    status,
                    output: json!({"run_id":run_id,"session_id":context.session_id,"already_terminal":true}),
                    error: state.error,
                });
            }
        }

        self.record_event(
            request_id,
            &mut sequence,
            "request.accepted",
            json!({
                "command": "run.cancel",
                "run_id": run_id,
                "reason": &reason,
            }),
        )
        .await?;

        let pending_approvals: Vec<(ApprovalId, PendingInvocation)> = self
            .pending_invocations
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .iter()
            .filter_map(|(approval_id, pending)| {
                (pending.run_id == run_id).then_some((*approval_id, pending.clone()))
            })
            .collect();
        for (approval_id, pending) in pending_approvals {
            if let Err(error) = self
                .approvals
                .invalidate(&context, approval_id, &reason)
                .await
            {
                let error = redact_event_text(&error.to_string());
                self.record_terminal_event(
                    request_id,
                    &mut sequence,
                    run_id,
                    "run.failed",
                    json!({
                        "run_id": run_id,
                        "error": &error,
                        "reason": "approval_invalidation_failed",
                        "approval_id": approval_id,
                    }),
                )
                .await?;
                return Ok(CoreResponse {
                    request_id,
                    status: ExecutionStatus::Failed,
                    output: run_identity(&context, run_id, "read-only"),
                    error: Some(format!("approval_invalidation_failed:{error}")),
                });
            }
            self.record_event(request_id,&mut sequence,"run.tool_result",json!({"run_id":run_id,
                "capability_request_id":pending.request_id,"call_id":pending.request.arguments["call_id"],
                "result":{"error":"cancelled:approval_pending","not_executed":true,"replay_safe":true},"not_executed":true})).await?;
            self.pending_invocations
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .remove(&approval_id);
        }

        self.record_event(
            request_id,
            &mut sequence,
            "run.cancelling",
            json!({
                "run_id":run_id,"reason":reason,"cancellation_state":"stopping",
            }),
        )
        .await?;
        self.signal_cancel(run_id);
        let events = match self
            .runner
            .send(RunnerCommand::Cancel {
                run_id,
                reason: reason.clone(),
            })
            .await
        {
            Ok(events) => events,
            Err(error) => {
                let error = redact_event_text(&error.to_string());
                let error = format!("result_unknown:{error}");
                self.record_terminal_event(
                    request_id,
                    &mut sequence,
                    run_id,
                    "run.result_unknown",
                    json!({ "run_id": run_id, "error": &error }),
                )
                .await?;
                return Ok(CoreResponse {
                    request_id,
                    status: ExecutionStatus::ResultUnknown,
                    output: run_identity(&context, run_id, "read-only"),
                    error: Some(error),
                });
            }
        };

        let stop_confirmed = self.await_capability_stop(run_id).await;
        for event in &events {
            if let RunnerEvent::ToolCancelled {
                run_id: event_run_id,
                request_id: tool_request_id,
                call_id,
                result,
            } = event
            {
                if *event_run_id == run_id {
                    self.record_event(request_id,&mut sequence,"run.tool_result",json!({
                        "run_id":run_id,"capability_request_id":tool_request_id,"call_id":call_id,
                        "result":result,"cancelled":true,"not_executed":true,
                    })).await?;
                }
            }
        }
        if !stop_confirmed {
            let error = "result_unknown:cancel_stop_unconfirmed";
            self.record_terminal_event(
                request_id,
                &mut sequence,
                run_id,
                "run.result_unknown",
                json!({"run_id":run_id,"error":error}),
            )
            .await?;
            return Ok(CoreResponse {
                request_id,
                status: ExecutionStatus::ResultUnknown,
                output: run_identity(&context, run_id, "read-only"),
                error: Some(error.to_owned()),
            });
        }
        let response_run_ids_match = events.iter().all(|event| event.run_id() == run_id);
        let matching_failure_errors = events
            .iter()
            .filter_map(|event| match event {
                RunnerEvent::Failed {
                    run_id: event_run_id,
                    error,
                } if *event_run_id == run_id => Some(error.as_str()),
                _ => None,
            })
            .collect::<Vec<_>>();
        let matching_non_cancel_error = matching_failure_errors
            .iter()
            .copied()
            .find(|error| lifecycle_error_code(error) != CapabilityErrorCode::Cancelled);
        let has_matching_completion = events.iter().any(|event| {
            matches!(
                event,
                RunnerEvent::Completed {
                    run_id: event_run_id,
                    ..
                } if *event_run_id == run_id
            )
        });
        let cancelled = response_run_ids_match
            && matching_failure_errors
                .iter()
                .any(|error| lifecycle_error_code(error) == CapabilityErrorCode::Cancelled)
            && matching_non_cancel_error.is_none()
            && !has_matching_completion;
        if cancelled {
            let cancelled = matching_failure_errors
                .iter()
                .copied()
                .find(|error| lifecycle_error_code(error) == CapabilityErrorCode::Cancelled)
                .map(redact_event_text)
                .expect("cancelled response was checked above");
            self.forget_session(context.session_id.as_str(), run_id);
            self.record_terminal_event(
                request_id,
                &mut sequence,
                run_id,
                "run.cancelled",
                json!({ "run_id": run_id, "error": &cancelled }),
            )
            .await?;
            self.settle_resumed_cell(
                &context,
                run_id,
                "read-only",
                ExecutionStatus::Cancelled,
                &mut sequence,
            )
            .await?;
            return Ok(CoreResponse {
                request_id,
                status: ExecutionStatus::Cancelled,
                output: run_identity(&context, run_id, "read-only"),
                error: Some(cancelled),
            });
        }

        let runner_error = if !response_run_ids_match {
            "cancel_response_run_id_mismatch".to_owned()
        } else if has_matching_completion {
            "cancel_confirmation_inconsistent".to_owned()
        } else if let Some(error) = matching_non_cancel_error {
            redact_event_text(error)
        } else {
            "cancel_confirmation_missing".to_owned()
        };
        let error = format!("result_unknown:{runner_error}");
        self.record_terminal_event(
            request_id,
            &mut sequence,
            run_id,
            "run.result_unknown",
            json!({ "run_id": run_id, "error": &error }),
        )
        .await?;
        Ok(CoreResponse {
            request_id,
            status: ExecutionStatus::ResultUnknown,
            output: run_identity(&context, run_id, "read-only"),
            error: Some(error),
        })
    }

    /// 把一轮内连续到达的模型增量合并成一条 `run.delta` 落账。
    ///
    /// 事件账本是事实源：按块落账会让每次 append 都付出全量 `read_stream` 算版本加 CAS 的
    /// 代价（见 `docs/streaming-unfreeze-plan.md` §5）。细粒度增量只走易失的展示通道。
    async fn flush_run_delta(
        &self,
        request_id: RequestId,
        sequence: &mut u64,
        run_id: RunId,
        buffer: &mut String,
    ) -> Result<(), CoreError> {
        if buffer.is_empty() {
            return Ok(());
        }
        let text = std::mem::take(buffer);
        self.record_event(
            request_id,
            sequence,
            "run.delta",
            json!({ "run_id": run_id, "text": text }),
        )
        .await
    }

    pub(crate) async fn drive_run(
        &self,
        context: &RequestContext,
        run_id: RunId,
        sandbox: &str,
        mut pending_events: Vec<RunnerEvent>,
        sequence: &mut u64,
    ) -> Result<CoreResponse, CoreError> {
        let request_id = context.request_id;
        let cancel_rx = self.watch_cancel(run_id);
        let mut output = Value::Null;
        let mut failed = None;
        let mut completed = false;
        // 同一轮模型调用会连出多条增量；先攒起来，遇到非增量事件时再合并落账。
        let mut delta_text = String::new();
        while !pending_events.is_empty() {
            let event = pending_events.remove(0);
            if event.run_id() != run_id {
                failed = Some("result_unknown:runner_event_run_id_mismatch".to_owned());
                break;
            }
            if !matches!(event, RunnerEvent::Delta { .. }) {
                self.flush_run_delta(request_id, sequence, run_id, &mut delta_text)
                    .await?;
            }
            match event {
                RunnerEvent::ToolCancelled {
                    run_id,
                    request_id: tool_request_id,
                    call_id,
                    result,
                } => {
                    self.record_event(request_id,sequence,"run.tool_result",json!({
                        "run_id":run_id,"capability_request_id":tool_request_id,"call_id":call_id,
                        "result":result,"cancelled":true,"not_executed":true,
                    })).await?;
                }
                RunnerEvent::ModelTurn {
                    run_id,
                    step,
                    mut metadata,
                } => {
                    if let Some(fields) = metadata.as_object_mut() {
                        fields.insert("run_id".to_owned(), json!(run_id));
                        fields.insert("step".to_owned(), json!(step));
                        fields.insert("session_id".to_owned(), json!(context.session_id));
                    }
                    if metadata["attempted"] == true {
                        if let Some(cell_id) = context.cell_id {
                            let measured = metadata["usage"]["input_tokens"]
                                .as_u64()
                                .zip(metadata["usage"]["output_tokens"].as_u64())
                                .and_then(|(input, output)| input.checked_add(output));
                            let charge = measured
                                .or_else(|| metadata["budget"]["total"].as_u64())
                                .unwrap_or(u64::MAX);
                            let turn_key = metadata["model_request_id"]
                                .as_str()
                                .map(str::to_owned)
                                .unwrap_or_else(|| format!("{request_id}:{step}"));
                            match self
                                .cell_registry
                                .account_model_usage(cell_id, &turn_key, charge)
                                .await
                            {
                                Ok(budget) => {
                                    metadata["cell_budget"] = json!(budget);
                                    metadata["usage_accounting"] = json!(if measured.is_some() {
                                        "provider_reported"
                                    } else {
                                        "conservative_request_reserve"
                                    });
                                }
                                Err(error) => {
                                    failed = Some(format!("run_budget_exceeded:{}", error));
                                    let _ = self
                                        .runner
                                        .send(RunnerCommand::Cancel {
                                            run_id,
                                            reason: "run_budget_exceeded:tokens".to_owned(),
                                        })
                                        .await;
                                    metadata["budget_error"] = json!(error.to_string());
                                }
                            }
                        }
                    }
                    self.record_event(
                        request_id,
                        sequence,
                        "run.model_turn",
                        redact_event_value(&metadata),
                    )
                    .await?;
                }
                RunnerEvent::Started { run_id } => {
                    self.record_event(
                        request_id,
                        sequence,
                        "run.started",
                        json!({ "run_id": run_id }),
                    )
                    .await?;
                }
                RunnerEvent::Delta { text, .. } => {
                    delta_text.push_str(&text);
                }
                RunnerEvent::CapabilityRequested { run_id, request } => {
                    match self
                        .broker_harness_capability(
                            context, request_id, run_id, sandbox, sequence, request, &cancel_rx,
                        )
                        .await?
                    {
                        Ok(Some(events)) => pending_events.extend(events),
                        Ok(None) => {
                            self.checkpoint_run(context, run_id, sandbox, sequence)
                                .await?;
                            let pending = self
                                .pending_invocations
                                .lock()
                                .unwrap_or_else(PoisonError::into_inner)
                                .values()
                                .find(|pending| pending.run_id == run_id)
                                .cloned();
                            let Some(pending) = pending else {
                                failed = Some("pending_invocation_missing".to_owned());
                                break;
                            };
                            self.clear_cancel(run_id);
                            return Ok(CoreResponse {
                                request_id,
                                status: ExecutionStatus::AwaitingApproval,
                                output: json!({
                                    "approval": pending.challenge,
                                    "capability": pending.request,
                                    "run_id": run_id
                                }),
                                error: Some("approval_required".to_owned()),
                            });
                        }
                        Err(reason) => {
                            failed = Some(reason);
                            break;
                        }
                    }
                }
                RunnerEvent::Completed {
                    run_id,
                    output: mut harness_output,
                } => {
                    if let Some(object) = harness_output.as_object_mut() {
                        object.insert("run_id".to_owned(), json!(run_id));
                    }
                    output = redact_event_value(&harness_output);
                    completed = true;
                    self.record_terminal_event(
                        request_id,
                        sequence,
                        run_id,
                        "run.completed",
                        output.clone(),
                    )
                    .await?;
                    self.derive_memory_proposal(
                        context,
                        Some(run_id),
                        "run.completed",
                        "lesson",
                        sequence,
                    )
                    .await;
                }
                RunnerEvent::Failed { run_id, error } => {
                    let error = redact_event_text(&error);
                    failed = Some(error.clone());
                    let terminal_kind = match lifecycle_error_code(&error) {
                        CapabilityErrorCode::Cancelled => "run.cancelled",
                        CapabilityErrorCode::ResultUnknown
                        | CapabilityErrorCode::CompensationRequired => "run.result_unknown",
                        _ => "run.failed",
                    };
                    self.record_terminal_event(
                        request_id,
                        sequence,
                        run_id,
                        terminal_kind,
                        json!({ "run_id": run_id, "error": error }),
                    )
                    .await?;
                    self.derive_memory_proposal(
                        context,
                        Some(run_id),
                        terminal_kind,
                        "lesson",
                        sequence,
                    )
                    .await;
                }
                RunnerEvent::Compacted {
                    run_id,
                    tokens_before,
                    tokens_after,
                    summary_present,
                } => {
                    self.record_event(
                        request_id,
                        sequence,
                        "run.compacted",
                        json!({
                            "schema": COMPACT_SCHEMA,
                            "run_id": run_id,
                            "tokens_before": tokens_before,
                            "tokens_after": tokens_after,
                            "summary_present": summary_present,
                        }),
                    )
                    .await?;
                }
            }
            if completed || failed.is_some() {
                break;
            }
        }
        // 末尾可能还有没被非增量事件触发的增量（例如事件列表直接结束）。
        self.flush_run_delta(request_id, sequence, run_id, &mut delta_text)
            .await?;
        self.clear_cancel(run_id);

        if let Some(error) = failed {
            if error == "run_not_found" {
                self.forget_session(context.session_id.as_str(), run_id);
            }
            let error_code = lifecycle_error_code(&error);
            let result_unknown = matches!(
                error_code,
                CapabilityErrorCode::ResultUnknown | CapabilityErrorCode::CompensationRequired
            );
            self.record_terminal_event(
                request_id,
                sequence,
                run_id,
                if result_unknown {
                    "run.result_unknown"
                } else if error_code == CapabilityErrorCode::Cancelled {
                    "run.cancelled"
                } else {
                    "run.failed"
                },
                json!({"run_id":run_id,"error":error}),
            )
            .await?;
            if result_unknown {
                let _ = self
                    .record_terminal_event(
                        request_id,
                        sequence,
                        run_id,
                        "run.result_unknown",
                        json!({ "run_id": run_id, "error": redact_event_text(&error) }),
                    )
                    .await;
            }
            return Ok(CoreResponse {
                request_id,
                status: if error == "run_not_found" {
                    ExecutionStatus::Blocked
                } else if result_unknown {
                    ExecutionStatus::ResultUnknown
                } else if error_code == CapabilityErrorCode::Cancelled {
                    ExecutionStatus::Cancelled
                } else {
                    ExecutionStatus::Failed
                },
                output: run_identity(context, run_id, sandbox),
                error: Some(error),
            });
        }
        if !completed {
            let reason = "run_result_missing";
            self.record_terminal_event(
                request_id,
                sequence,
                run_id,
                "run.result_unknown",
                json!({ "run_id": run_id, "error": redact_event_text(&reason) }),
            )
            .await?;
            return Ok(CoreResponse {
                request_id,
                status: ExecutionStatus::ResultUnknown,
                output: run_identity(context, run_id, sandbox),
                error: Some(reason.to_owned()),
            });
        }

        self.remember_session(&context, run_id);
        let receipt = self
            .run_receipt_from_store(context, run_id, sandbox, output)
            .await?;
        self.record_event(request_id, sequence, "run.receipt", receipt.clone())
            .await?;
        self.checkpoint_run(context, run_id, sandbox, sequence)
            .await?;
        Ok(CoreResponse::completed(request_id, receipt))
    }
}

pub(crate) fn authorized_harness_sandbox(
    context: &RequestContext,
    requested: Option<&str>,
) -> Result<&'static str, &'static str> {
    let requested = requested
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or("read-only");
    match requested {
        "read-only" => Ok("read-only"),
        "workspace-write" => {
            if context.project_trusted
                && !matches!(context.permission_profile, PermissionProfile::Safe)
            {
                let role = RoleSpec::lookup(&context.role_id).ok_or("role_unknown")?;
                if role.workspace_write_allowed() {
                    Ok("workspace-write")
                } else {
                    Err("role_sandbox_read_only")
                }
            } else {
                Err("workspace_write_requires_trusted_non_safe_profile")
            }
        }
        _ => Err("sandbox_unsupported"),
    }
}

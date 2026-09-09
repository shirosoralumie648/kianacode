use super::events::*;
use super::redaction::*;
use super::*;

impl ControlPlane {
    pub async fn start_run(
        &self,
        context: RequestContext,
        prompt: String,
        sandbox: Option<String>,
    ) -> Result<CoreResponse, CoreError> {
        self.start_run_with_id(context, prompt, Vec::new(), sandbox, None)
            .await
    }

    pub async fn start_run_with_history(
        &self,
        context: RequestContext,
        prompt: String,
        history: Vec<kiana_domain::ConversationMessage>,
        sandbox: Option<String>,
    ) -> Result<CoreResponse, CoreError> {
        self.start_run_with_id(context, prompt, history, sandbox, None)
            .await
    }

    pub(crate) async fn start_run_with_id(
        &self,
        context: RequestContext,
        prompt: String,
        history: Vec<kiana_domain::ConversationMessage>,
        sandbox: Option<String>,
        requested_run_id: Option<RunId>,
    ) -> Result<CoreResponse, CoreError> {
        let request_id = context.request_id;
        let run_id = requested_run_id.unwrap_or_else(|| {
            RunId::parse_str(context.session_id.as_str()).unwrap_or_else(RunId::new)
        });
        let mut sequence = 1u64;
        self.record_event(
            request_id,
            &mut sequence,
            "request.accepted",
            json!({
                "command": "run.start",
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
            }),
        )
        .await?;

        // Session index and cancel watch must exist before the first model step
        // so an in-flight cancel can resolve session-1 and abort shell.exec.
        self.remember_session(&context, run_id);
        let _cancel_rx = self.watch_cancel(run_id);
        let _terminal_scope = self.begin_terminal_scope(run_id);

        let pending_events = match self
            .runner
            .send(RunnerCommand::start_in_with_history(
                run_id,
                prompt,
                history,
                context.project_root.clone(),
                sandbox.to_owned(),
                String::new(),
                context.project_trusted,
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

    pub async fn continue_run(
        &self,
        context: RequestContext,
        prompt: String,
        sandbox: Option<String>,
        run_id: Option<RunId>,
    ) -> Result<CoreResponse, CoreError> {
        let request_id = context.request_id;
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

        self.record_event(
            request_id,
            &mut sequence,
            "request.accepted",
            json!({
                "command": "run.continue",
                "run_id": run_id,
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

        let pending_approvals: Vec<ApprovalId> = self
            .pending_invocations
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .iter()
            .filter_map(|(approval_id, pending)| (pending.run_id == run_id).then_some(*approval_id))
            .collect();
        for approval_id in pending_approvals {
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
            self.pending_invocations
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .remove(&approval_id);
        }

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
            .find(|error| !error.starts_with("cancelled:"));
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
                .any(|error| error.starts_with("cancelled:"))
            && matching_non_cancel_error.is_none()
            && !has_matching_completion;
        if cancelled {
            let cancelled = matching_failure_errors
                .iter()
                .copied()
                .find(|error| error.starts_with("cancelled:"))
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
                }
                RunnerEvent::Failed { run_id, error } => {
                    let error = redact_event_text(&error);
                    failed = Some(error.clone());
                    let terminal_kind = if error.starts_with("cancelled:") {
                        "run.cancelled"
                    } else {
                        "run.failed"
                    };
                    self.record_terminal_event(
                        request_id,
                        sequence,
                        run_id,
                        terminal_kind,
                        json!({ "run_id": run_id, "error": error }),
                    )
                    .await?;
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
            let result_unknown = error.strip_prefix("result_unknown:").is_some();
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
                } else if error.starts_with("cancelled:") {
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
                let role = RoleSpec::lookup(&context.role_id).unwrap_or_else(RoleSpec::builder);
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

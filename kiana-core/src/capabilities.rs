use super::events::*;
use super::redaction::*;
use super::*;

impl ControlPlane {
    pub(crate) async fn bind_cell_scope(
        &self,
        context: &RequestContext,
        request: &mut CapabilityRequest,
    ) -> Result<(), CoreError> {
        let Some(cell_id) = context.cell_id else {
            if request.capability_grant_id.is_some() || request.budget_lease_id.is_some() {
                return Err(CoreError::Port(PortError::Failed(
                    "cell_capability_scope_incomplete".to_owned(),
                )));
            }
            return Ok(());
        };
        let reservation = self
            .cell_registry
            .reservation_for_cell(cell_id)
            .await?
            .ok_or_else(|| CoreError::Port(PortError::Failed("cell_not_found".to_owned())))?;
        request.cell_id = Some(cell_id);
        request.capability_grant_id = Some(reservation.grant.grant_id);
        request.budget_lease_id = Some(reservation.budget.lease_id);
        Ok(())
    }

    pub(crate) async fn begin_cell_capability_from_request(
        &self,
        request: &CapabilityRequest,
    ) -> Result<Option<CapabilityLease>, CoreError> {
        let Some(cell_id) = request.cell_id else {
            if request.capability_grant_id.is_some() || request.budget_lease_id.is_some() {
                return Err(CoreError::Port(PortError::Failed(
                    "cell_capability_scope_incomplete".to_owned(),
                )));
            }
            return Ok(None);
        };
        let grant_id = request.capability_grant_id.ok_or_else(|| {
            CoreError::Port(PortError::Failed(
                "cell_capability_scope_incomplete".to_owned(),
            ))
        })?;
        let budget_id = request.budget_lease_id.ok_or_else(|| {
            CoreError::Port(PortError::Failed(
                "cell_capability_scope_incomplete".to_owned(),
            ))
        })?;
        Ok(Some(
            self.cell_registry
                .begin_capability(cell_id, grant_id, budget_id, request)
                .await?,
        ))
    }

    pub(crate) async fn finish_cell_capability(
        &self,
        lease: CapabilityLease,
        outcome: CapabilityOutcome,
    ) -> Result<(), CoreError> {
        self.cell_registry
            .finish_capability(lease, outcome)
            .await
            .map_err(CoreError::from)
    }

    pub(crate) async fn broker_harness_capability(
        &self,
        context: &RequestContext,
        request_id: kiana_domain::RequestId,
        run_id: RunId,
        sandbox: &str,
        sequence: &mut u64,
        mut request: CapabilityRequest,
        cancel_rx: &watch::Receiver<bool>,
    ) -> Result<Result<Option<Vec<RunnerEvent>>, String>, CoreError> {
        stamp_request_identity(&mut request, context);
        // RunnerEvent::CapabilityRequested carries no model-visible tool name; retain the
        // stable capability class and exact broker operation available to the control plane.
        self.record_event(
            request_id,
            sequence,
            "run.tool_call",
            json!({
                "run_id": run_id,
                "call_id": request.arguments.get("call_id").cloned().unwrap_or(Value::Null),
                "tool": request.capability.clone(),
                "operation": request.operation.clone(),
            }),
        )
        .await?;
        if let Err(error) = self.bind_cell_scope(context, &mut request).await {
            let reason = error.to_string();
            self.record_event(
                request_id,
                sequence,
                "run.capability_blocked",
                json!({ "run_id": run_id, "error": redact_event_text(&reason) }),
            )
            .await?;
            return Ok(Err(reason));
        }
        self.record_event(
            request_id,
            sequence,
            "run.capability_requested",
            json!({
                "run_id": run_id,
                "request_id": request.request_id,
                "capability": request.capability,
                "operation": request.operation,
                "risk": request.risk,
                "cell_id": request.cell_id,
                "capability_grant_id": request.capability_grant_id,
                "budget_lease_id": request.budget_lease_id,
                "arguments": redact_event_value(&request.arguments),
            }),
        )
        .await?;

        if *cancel_rx.borrow() {
            self.record_terminal_event(
                request_id,
                sequence,
                run_id,
                "run.cancelled",
                json!({ "run_id": run_id, "error": "cancelled:user" }),
            )
            .await?;
            return Ok(Err("cancelled:user".to_owned()));
        }

        let policy = self.evaluate_policy(context, &request);
        let gate = self.evaluate_gate(&request, &policy);
        self.record_event(
            request_id,
            sequence,
            "capability.decision",
            json!({ "run_id": run_id, "policy": &policy, "gate": &gate }),
        )
        .await?;

        let result = match gate {
            GateDecision::Allowed { authorization_id } => {
                let hook_decision = self.pre_tool_hooks.decide(context, &request).await;
                let hook_error = match hook_decision {
                    Ok(PreToolHookDecision::Allow) => None,
                    Ok(PreToolHookDecision::Block(reason)) => {
                        Some(format!("hook_blocked:{reason}"))
                    }
                    Ok(PreToolHookDecision::Ask { reason }) => {
                        Some(format!("hook_ask_unattended:{reason}"))
                    }
                    Err(error) => Some(format!("hook_blocked:{error}")),
                };
                if let Some(reason) = hook_error {
                    self.record_event(
                        request_id,
                        sequence,
                        "capability.failed",
                        json!({ "run_id": run_id, "error": redact_event_text(&reason) }),
                    )
                    .await?;
                    CapabilityResult::failure(request.request_id, reason)
                } else {
                    let cell_lease = match self.begin_cell_capability_from_request(&request).await {
                        Ok(lease) => lease,
                        Err(error) => {
                            let reason = error.to_string();
                            self.record_event(
                                request_id,
                                sequence,
                                "capability.failed",
                                json!({ "run_id": run_id, "error": redact_event_text(&reason) }),
                            )
                            .await?;
                            return Ok(Err(reason));
                        }
                    };
                    let authorized =
                        AuthorizedCapabilityRequest::new(authorization_id, request.clone())?;
                    tokio::select! {
                        biased;
                        _ = wait_until_cancelled(cancel_rx) => {
                            if let Some(lease) = cell_lease {
                                self.finish_cell_capability(lease, CapabilityOutcome::Unknown)
                                    .await?;
                            }
                            self.record_terminal_event(
                                request_id,
                                sequence,
                                run_id,
                                "run.cancelled",
                                json!({ "run_id": run_id, "error": "cancelled:user" }),
                            )
                            .await?;
                            return Ok(Err("cancelled:user".to_owned()));
                        }
                        executed = self.capabilities.execute(authorized) => {
                            match executed {
                                Ok(result) => {
                                    let result = redact_capability_result(result);
                                    let outcome = if result.request_id != request.request_id {
                                        CapabilityOutcome::Unknown
                                    } else if result.success {
                                        CapabilityOutcome::Succeeded
                                    } else {
                                        CapabilityOutcome::Failed
                                    };
                                    if result.request_id != request.request_id {
                                        if let Some(lease) = cell_lease {
                                            self.finish_cell_capability(lease, outcome).await?;
                                        }
                                        return Ok(Err(
                                            "result_unknown:capability_result_mismatch".to_owned(),
                                        ));
                                    }
                                    let result_event = self.record_event(
                                        request_id,
                                        sequence,
                                        if result.success {
                                            "capability.completed"
                                        } else {
                                            "capability.failed"
                                        },
                                        capability_event_payload(
                                            &result.output,
                                            &request,
                                            context,
                                            run_id,
                                        ),
                                    )
                                    .await;
                                    if let Err(error) = result_event {
                                        if let Some(lease) = cell_lease {
                                            self.finish_cell_capability(lease, CapabilityOutcome::Unknown)
                                                .await?;
                                        }
                                        return Ok(Err(format!(
                                            "result_unknown:{}",
                                            redact_event_text(&error.to_string())
                                        )));
                                    }
                                    if let Some(lease) = cell_lease {
                                        self.finish_cell_capability(lease, outcome).await?;
                                    }
                                    result
                                }
                                Err(error) => {
                                    if let Some(lease) = cell_lease {
                                        self.finish_cell_capability(lease, CapabilityOutcome::Unknown)
                                            .await?;
                                    }
                                    let reason = redact_event_text(&error.to_string());
                                    if reason.starts_with("shell_result_unknown:") {
                                        self.record_event(
                                            request_id,
                                            sequence,
                                            "capability.result_unknown",
                                            json!({
                                                "run_id": run_id,
                                                "capability_request_id": request.request_id,
                                                "error": redact_event_text(&reason),
                                            }),
                                        )
                                        .await?;
                                        return Ok(Err(format!("result_unknown:{reason}")));
                                    }
                                    self.record_event(
                                        request_id,
                                        sequence,
                                        "capability.failed",
                                        json!({ "run_id": run_id, "error": redact_event_text(&reason) }),
                                    )
                                    .await?;
                                    CapabilityResult::failure(request.request_id, reason)
                                }
                            }
                        }
                    }
                }
            }
            GateDecision::Denied { reason }
                if reason.starts_with("role_") || reason.starts_with("packet_") =>
            {
                self.record_event(
                    request_id,
                    sequence,
                    "run.capability_blocked",
                    json!({ "run_id": run_id, "reason": &reason }),
                )
                .await?;
                return Ok(Err(reason));
            }
            GateDecision::AwaitingApproval { reason } => {
                let mut approval_context = context.clone();
                approval_context.request_id = request.request_id;
                let challenge = self
                    .approvals
                    .stage(&approval_context, request.clone(), &reason)
                    .await?;
                self.record_event(
                    request_id,
                    sequence,
                    "approval.requested",
                    json!({
                        "run_id": run_id,
                        "approval_id": challenge.approval_id,
                        "request_hash": &challenge.request_hash,
                        "session_id": context.session_id,
                        "actor_id": context.actor_id,
                        "expires_at_unix_ms": challenge.expires_at_unix_ms,
                    }),
                )
                .await?;
                self.approvals.activate(challenge.approval_id).await?;
                self.pending_invocations
                    .lock()
                    .unwrap_or_else(PoisonError::into_inner)
                    .insert(
                        challenge.approval_id,
                        PendingInvocation {
                            approval_id: challenge.approval_id,
                            challenge: challenge.clone(),
                            request_id: request.request_id,
                            event_request_id: request_id,
                            event_sequence: *sequence + 1,
                            run_id,
                            request,
                            context: context.clone(),
                            sandbox: sandbox.to_owned(),
                        },
                    );
                self.record_event(
                    request_id,
                    sequence,
                    "run.awaiting_approval",
                    json!({ "run_id": run_id, "approval_id": challenge.approval_id }),
                )
                .await?;
                return Ok(Ok(None));
            }
            GateDecision::Denied { reason } => {
                self.record_event(
                    request_id,
                    sequence,
                    "run.capability_blocked",
                    json!({ "run_id": run_id, "reason": &reason }),
                )
                .await?;
                CapabilityResult::failure(
                    request.request_id,
                    format!("capability_blocked:{reason}"),
                )
            }
        };

        let result = redact_capability_result(result);
        match self
            .runner
            .send(RunnerCommand::CapabilityResult { run_id, result })
            .await
        {
            Ok(events) => Ok(Ok(Some(events))),
            Err(error) => {
                let reason = redact_event_text(&error.to_string());
                self.record_terminal_event(
                    request_id,
                    sequence,
                    run_id,
                    "run.failed",
                    json!({ "run_id": run_id, "error": redact_event_text(&reason) }),
                )
                .await?;
                Ok(Err(reason))
            }
        }
    }
}

async fn wait_until_cancelled(rx: &watch::Receiver<bool>) {
    let mut rx = rx.clone();
    loop {
        if *rx.borrow() {
            return;
        }
        if rx.changed().await.is_err() {
            return;
        }
    }
}

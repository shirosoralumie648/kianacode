use super::events::*;
use super::redaction::*;
use super::*;

impl ControlPlane {
    pub async fn authorize_and_execute(
        &self,
        context: &RequestContext,
        request: CapabilityRequest,
    ) -> Result<CoreResponse, CoreError> {
        let request_id = request.request_id;
        if request_id != context.request_id {
            return Ok(CoreResponse::blocked(
                request_id,
                "request_context_mismatch",
            ));
        }
        self.append_event(
            request_id,
            1,
            "request.accepted",
            json!({
                "capability": &request.capability,
                "operation": &request.operation,
                "risk": request.risk,
            }),
        )
        .await?;

        let policy = self.evaluate_policy(context, &request);
        let gate = self.evaluate_gate(&request, &policy);
        self.append_event(
            request_id,
            2,
            "capability.decision",
            json!({ "policy": &policy, "gate": &gate }),
        )
        .await?;

        let authorization_id = match gate {
            GateDecision::Allowed { authorization_id } => authorization_id,
            GateDecision::AwaitingApproval { reason } => {
                let challenge = self.approvals.stage(context, request, &reason).await?;
                self.append_event(
                    request_id,
                    3,
                    "approval.requested",
                    json!({
                        "approval_id": challenge.approval_id,
                        "request_hash": &challenge.request_hash,
                        "session_id": context.session_id,
                        "actor_id": context.actor_id,
                        "expires_at_unix_ms": challenge.expires_at_unix_ms,
                    }),
                )
                .await?;
                self.approvals.activate(challenge.approval_id).await?;
                return Ok(CoreResponse {
                    request_id,
                    status: ExecutionStatus::AwaitingApproval,
                    output: json!({ "approval": challenge }),
                    error: Some(reason),
                });
            }
            GateDecision::Denied { reason } => {
                return Ok(CoreResponse {
                    request_id,
                    status: ExecutionStatus::Denied,
                    output: Value::Null,
                    error: Some(reason),
                });
            }
        };

        self.execute_authorized_request(request, authorization_id, 3)
            .await
    }

    pub async fn decide_approval(
        &self,
        context: &RequestContext,
        approval_id: ApprovalId,
        decision: ApprovalDecision,
    ) -> Result<CoreResponse, CoreError> {
        self.decide_approval_with_proof(context, approval_id, decision, None, None)
            .await
    }

    pub async fn decide_approval_with_proof(
        &self,
        context: &RequestContext,
        approval_id: ApprovalId,
        decision: ApprovalDecision,
        request_hash: Option<&str>,
        nonce: Option<&str>,
    ) -> Result<CoreResponse, CoreError> {
        let persisted_approval = self.approval_cursor(approval_id).await?;
        let has_live_invocation = self
            .pending_invocations
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .contains_key(&approval_id);
        if decision == ApprovalDecision::Approve
            && persisted_approval.is_some()
            && !has_live_invocation
        {
            // A restarted host can authenticate and preserve the approval while
            // it lacks the Runner continuation needed to execute it. Do not
            // consume the durable approval before that continuation exists.
            if let Err(error) = self
                .approvals
                .validate_with_proof(context, approval_id, request_hash, nonce)
                .await
            {
                return Ok(CoreResponse::blocked(context.request_id, error.to_string()));
            }
            let persisted_approval = persisted_approval.expect("checked above");
            if !persisted_approval.continuation_recorded {
                self.append_event(
                    persisted_approval.event_request_id,
                    persisted_approval.event_sequence.saturating_add(1),
                    "approval.continuation_unavailable",
                    json!({
                        "approval_id": approval_id,
                        "run_id": persisted_approval.run_id,
                        "error": "approval_continuation_unavailable",
                    }),
                )
                .await?;
            }
            return Ok(CoreResponse::blocked(
                context.request_id,
                "approval_continuation_unavailable",
            ));
        }
        let pending = match self
            .approvals
            .consume_with_proof(context, approval_id, request_hash, nonce)
            .await
        {
            Ok(pending) => pending,
            Err(error) => {
                return Ok(CoreResponse::blocked(context.request_id, error.to_string()));
            }
        };
        let invocation = self
            .pending_invocations
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .remove(&approval_id);
        let request_id = pending.request.request_id;
        let event_request_id = invocation
            .as_ref()
            .map(|pending| pending.event_request_id)
            .unwrap_or(request_id);
        let event_sequence = invocation
            .as_ref()
            .map(|pending| pending.event_sequence)
            .unwrap_or(4);
        let event_kind = match decision {
            ApprovalDecision::Approve => "approval.approved",
            ApprovalDecision::Deny => "approval.denied",
        };
        let mut approval_event = json!({
            "approval_id": approval_id,
            "request_hash": pending.challenge.request_hash,
            "session_id": context.session_id,
            "actor_id": context.actor_id,
        });
        if let Some(invocation) = &invocation {
            approval_event["run_id"] = json!(invocation.run_id);
        }
        self.append_event(event_request_id, event_sequence, event_kind, approval_event)
            .await?;

        if let Some(invocation) = invocation {
            return self
                .resume_approved_invocation(context, request_id, approval_id, decision, invocation)
                .await;
        }

        if decision == ApprovalDecision::Deny {
            return Ok(CoreResponse {
                request_id,
                status: ExecutionStatus::Denied,
                output: json!({ "approval_id": approval_id }),
                error: Some("approval_denied".to_owned()),
            });
        }
        self.execute_authorized_request(pending.request, format!("approval:{approval_id}"), 5)
            .await
    }

    pub(crate) async fn resume_approved_invocation(
        &self,
        _decision_context: &RequestContext,
        request_id: RequestId,
        approval_id: ApprovalId,
        decision: ApprovalDecision,
        invocation: PendingInvocation,
    ) -> Result<CoreResponse, CoreError> {
        if decision == ApprovalDecision::Deny {
            let _ = self
                .runner
                .send(RunnerCommand::CapabilityResult {
                    run_id: invocation.run_id,
                    result: CapabilityResult::failure(invocation.request_id, "approval_denied"),
                })
                .await;
            return Ok(CoreResponse {
                request_id,
                status: ExecutionStatus::Denied,
                output: json!({ "approval_id": approval_id, "run_id": invocation.run_id }),
                error: Some("approval_denied".to_owned()),
            });
        }

        if let Some(reason) = capability_risk_violation(&invocation.request) {
            self.append_event(
                invocation.event_request_id,
                invocation.event_sequence + 1,
                "run.capability_blocked",
                json!({
                    "run_id": invocation.run_id,
                    "reason": reason,
                }),
            )
            .await?;
            let _ = self
                .runner
                .send(RunnerCommand::CapabilityResult {
                    run_id: invocation.run_id,
                    result: CapabilityResult::failure(
                        invocation.request_id,
                        format!("capability_blocked:{reason}"),
                    ),
                })
                .await;
            return Ok(CoreResponse {
                request_id,
                status: ExecutionStatus::Blocked,
                output: run_identity(&invocation.context, invocation.run_id, &invocation.sandbox),
                error: Some(reason.to_owned()),
            });
        }

        let mut request = invocation.request.clone();
        if let Some(object) = request.arguments.as_object_mut() {
            object.insert("sandbox".to_owned(), json!(invocation.sandbox));
        }
        if let Err(error) = self
            .bind_cell_scope(&invocation.context, &mut request)
            .await
        {
            let reason = error.to_string();
            let _ = self
                .runner
                .send(RunnerCommand::CapabilityResult {
                    run_id: invocation.run_id,
                    result: CapabilityResult::failure(invocation.request_id, reason.clone()),
                })
                .await;
            return Ok(CoreResponse {
                request_id,
                status: ExecutionStatus::Blocked,
                output: run_identity(&invocation.context, invocation.run_id, &invocation.sandbox),
                error: Some(reason),
            });
        }
        let cell_lease = self.begin_cell_capability_from_request(&request).await?;
        let result = match self
            .capabilities
            .execute(AuthorizedCapabilityRequest::new(
                format!("approval:{approval_id}"),
                request.clone(),
            )?)
            .await
        {
            Ok(result) => result,
            Err(error) => CapabilityResult::failure(
                invocation.request_id,
                redact_event_text(&error.to_string()),
            ),
        };
        let result = redact_capability_result(result);
        let outcome = if result.success {
            CapabilityOutcome::Succeeded
        } else {
            CapabilityOutcome::Failed
        };
        if let Some(lease) = cell_lease {
            self.finish_cell_capability(lease, outcome).await?;
        }
        if result.request_id != invocation.request_id {
            let reason = "capability_result_mismatch";
            self.append_event(
                invocation.event_request_id,
                invocation.event_sequence + 1,
                "capability.result_unknown",
                json!({
                    "run_id": invocation.run_id,
                    "capability_request_id": invocation.request_id,
                    "result_request_id": result.request_id,
                    "error": reason,
                }),
            )
            .await?;
            return Ok(CoreResponse {
                request_id,
                status: ExecutionStatus::ResultUnknown,
                output: run_identity(&invocation.context, invocation.run_id, &invocation.sandbox),
                error: Some(reason.to_owned()),
            });
        }
        self.append_event(
            invocation.event_request_id,
            invocation.event_sequence + 1,
            if result.success {
                "capability.completed"
            } else {
                "capability.failed"
            },
            capability_event_payload(
                &result.output,
                &request,
                &invocation.context,
                invocation.run_id,
            ),
        )
        .await?;
        let events = match self
            .runner
            .send(RunnerCommand::CapabilityResult {
                run_id: invocation.run_id,
                result,
            })
            .await
        {
            Ok(events) => events,
            Err(error) => {
                let error = redact_event_text(&error.to_string());
                return Ok(CoreResponse {
                    request_id,
                    status: ExecutionStatus::Failed,
                    output: run_identity(
                        &invocation.context,
                        invocation.run_id,
                        &invocation.sandbox,
                    ),
                    error: Some(error),
                });
            }
        };
        let mut sequence = invocation.event_sequence + 2;
        self.drive_run(
            &invocation.context,
            invocation.run_id,
            &invocation.sandbox,
            events,
            &mut sequence,
        )
        .await
    }

    pub(crate) async fn execute_authorized_request(
        &self,
        request: CapabilityRequest,
        authorization_id: String,
        result_sequence: u64,
    ) -> Result<CoreResponse, CoreError> {
        let request_id = request.request_id;
        if let Some(reason) = capability_risk_violation(&request) {
            self.append_event(
                request_id,
                result_sequence,
                "capability.blocked",
                json!({ "error": reason }),
            )
            .await?;
            return Ok(CoreResponse::blocked(request_id, reason));
        }
        let cell_lease = self.begin_cell_capability_from_request(&request).await?;
        let authorized = AuthorizedCapabilityRequest::new(authorization_id, request.clone())?;
        match self.capabilities.execute(authorized).await {
            Ok(result) => {
                let result = redact_capability_result(result);
                let outcome = if result.request_id != request_id {
                    CapabilityOutcome::Unknown
                } else if result.success {
                    CapabilityOutcome::Succeeded
                } else {
                    CapabilityOutcome::Failed
                };
                if let Some(lease) = cell_lease {
                    self.finish_cell_capability(lease, outcome).await?;
                }
                if result.request_id != request_id {
                    self.append_event(
                        request_id,
                        result_sequence,
                        "capability.result_unknown",
                        json!({
                            "capability_request_id": request_id,
                            "result_request_id": result.request_id,
                            "error": "capability_result_mismatch",
                        }),
                    )
                    .await?;
                    return Ok(CoreResponse {
                        request_id,
                        status: ExecutionStatus::ResultUnknown,
                        output: Value::Null,
                        error: Some("capability_result_mismatch".to_owned()),
                    });
                }
                let success = result.success;
                let status = if success {
                    ExecutionStatus::Completed
                } else {
                    ExecutionStatus::Failed
                };
                let output = result.output;
                let error = (!success).then(|| {
                    output
                        .get("error")
                        .and_then(Value::as_str)
                        .unwrap_or("capability_failed")
                        .to_owned()
                });
                if self
                    .append_event(
                        request_id,
                        result_sequence,
                        if success {
                            "capability.completed"
                        } else {
                            "capability.failed"
                        },
                        direct_capability_event_payload(&output, &request),
                    )
                    .await
                    .is_err()
                {
                    return Ok(CoreResponse {
                        request_id,
                        status: ExecutionStatus::ResultUnknown,
                        output: Value::Null,
                        error: Some("result_event_persistence_failed".to_owned()),
                    });
                }
                Ok(CoreResponse {
                    request_id,
                    status,
                    output,
                    error,
                })
            }
            Err(error) => {
                if let Some(lease) = cell_lease {
                    self.finish_cell_capability(lease, CapabilityOutcome::Unknown)
                        .await?;
                }
                let reason = redact_event_text(&error.to_string());
                self.append_event(
                    request_id,
                    result_sequence,
                    "capability.failed",
                    direct_capability_event_payload(
                        &json!({ "error": redact_event_text(&reason) }),
                        &request,
                    ),
                )
                .await?;
                Ok(CoreResponse {
                    request_id,
                    status: ExecutionStatus::Failed,
                    output: Value::Null,
                    error: Some(reason),
                })
            }
        }
    }

    /// Apply server-owned operation invariants before consulting the replaceable policy engine.
    ///
    /// Policy implementations may vary by deployment, but no policy is allowed to turn a
    /// malformed MCP request into an executable low-risk capability.
    pub(crate) fn evaluate_policy(
        &self,
        context: &RequestContext,
        request: &CapabilityRequest,
    ) -> PolicyDecision {
        if let Some(reason) = capability_risk_violation(request) {
            return PolicyDecision::Deny {
                reason: reason.to_owned(),
            };
        }
        self.policy.evaluate(context, request)
    }

    /// Keep server-owned operation invariants authoritative even if a deployment supplies a
    /// custom Gate implementation that would otherwise widen a policy decision.
    pub(crate) fn evaluate_gate(
        &self,
        request: &CapabilityRequest,
        policy: &PolicyDecision,
    ) -> GateDecision {
        if let Some(reason) = capability_risk_violation(request) {
            return GateDecision::Denied {
                reason: reason.to_owned(),
            };
        }
        self.gates.evaluate(policy)
    }
    pub(crate) async fn approval_cursor(
        &self,
        approval_id: ApprovalId,
    ) -> Result<Option<PersistedApprovalCursor>, CoreError> {
        let Some(events) = self.read_all_events().await? else {
            // A legacy adapter may intentionally expose only read_request. That is a
            // capability limitation, not evidence that a persisted Run association is absent.
            return Ok(None);
        };
        let approval_id = approval_id.to_string();
        Ok(events.iter().rev().find_map(|event| {
            if event.kind != "approval.requested"
                || event.data.get("approval_id").and_then(Value::as_str)
                    != Some(approval_id.as_str())
            {
                return None;
            }
            let run_id = event
                .data
                .get("run_id")
                .and_then(Value::as_str)
                .and_then(RunId::parse_str)?;
            Some(PersistedApprovalCursor {
                run_id,
                event_request_id: event.request_id,
                event_sequence: event.sequence,
                continuation_recorded: events.iter().any(|candidate| {
                    candidate.kind == "approval.continuation_unavailable"
                        && candidate.data.get("approval_id").and_then(Value::as_str)
                            == Some(approval_id.as_str())
                }),
            })
        }))
    }
}

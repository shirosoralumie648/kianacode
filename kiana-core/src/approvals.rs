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
        self.append_event(request_id,1,"request.accepted",json!({"capability":request.capability,"operation":request.operation,"risk":request.risk})).await?;
        let request = match self
            .prepare_capability_action(context, request, None, false)
            .await
        {
            Ok(request) => request,
            Err(error) => {
                let reason = redact_event_text(&error.to_string());
                self.append_event(request_id, 2, "capability.blocked", json!({"error":reason}))
                    .await?;
                return Ok(CoreResponse::blocked(request_id, reason));
            }
        };
        let (policy, gate) = self
            .authorize_capability_action(context, &request, None)
            .await?;
        self.append_event(
            request_id,
            2,
            "capability.decision",
            json!({"policy":policy,"gate":gate,
            "action_digest":kiana_domain::capability_action_digest(&request)}),
        )
        .await?;
        let authorization_id = match gate {
            GateDecision::Allowed { authorization_id } => authorization_id,
            GateDecision::Denied { reason } => {
                return Ok(CoreResponse {
                    request_id,
                    status: ExecutionStatus::Denied,
                    output: Value::Null,
                    error: Some(reason),
                })
            }
            GateDecision::AwaitingApproval { reason } => {
                let mut sequence = 3;
                let challenge = self
                    .stage_capability_action(
                        context,
                        &request,
                        &reason,
                        None,
                        request_id,
                        &mut sequence,
                    )
                    .await?;
                return Ok(CoreResponse {
                    request_id,
                    status: ExecutionStatus::AwaitingApproval,
                    output: json!({"approval":challenge}),
                    error: Some(reason),
                });
            }
        };
        self.execute_authorized_request(context, request, authorization_id, 3)
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
        let record = self.approvals.read_decision(context, approval_id).await?;
        if request_hash.is_some_and(|hash| hash != record.challenge.request_hash)
            || nonce.is_some_and(|nonce| nonce != record.challenge.nonce)
        {
            return Ok(CoreResponse::blocked(
                context.request_id,
                "approval_proof_mismatch",
            ));
        }
        if record.decision.is_some() {
            return self
                .replay_approval_decision(context, &record, decision)
                .await;
        }
        let scoped_context = self
            .approvals
            .context_for_pending(context, approval_id)
            .await?;
        if decision == ApprovalDecision::Approve
            && context.permission_profile != scoped_context.permission_profile
        {
            return Ok(CoreResponse::blocked(
                context.request_id,
                "approval_permission_profile_changed",
            ));
        }
        let context = &scoped_context;
        let persisted_approval = self.approval_cursor(approval_id).await?;
        let live_run = self
            .pending_invocations
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .get(&approval_id)
            .map(|pending| pending.run_id);
        if decision == ApprovalDecision::Approve {
            if let Some(run_id) = live_run {
                if self.run_state(run_id).await?.outcome.is_some() {
                    return Ok(CoreResponse::blocked(
                        context.request_id,
                        "approval_run_already_terminal",
                    ));
                }
            }
        }
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
        // Verify the exact execution material before recording Approved. The approval journal
        // owns the decision CAS; its later dispatch transaction consumes the single-use authority.
        if decision == ApprovalDecision::Approve {
            let validated = self
                .approvals
                .pending_with_proof(context, approval_id, request_hash, nonce)
                .await?;
            let prepared = self
                .prepare_capability_action(
                    context,
                    validated.request.clone(),
                    None,
                    has_live_invocation || persisted_approval.is_some(),
                )
                .await?;
            if prepared != validated.request {
                return Ok(CoreResponse::blocked(
                    context.request_id,
                    "approval_action_changed",
                ));
            }
            let (_, gate) = self
                .authorize_capability_action(
                    context,
                    &prepared,
                    Some((approval_id, &validated.challenge.reason)),
                )
                .await?;
            match gate {
                GateDecision::Allowed { .. } => {}
                GateDecision::Denied { reason } => {
                    return Ok(CoreResponse::blocked(context.request_id, reason))
                }
                GateDecision::AwaitingApproval { .. } => {
                    return Ok(CoreResponse::blocked(
                        context.request_id,
                        "approval_requirements_changed",
                    ))
                }
            }
            if let Some(run_id) = live_run {
                if *self.watch_cancel(run_id).borrow() {
                    return Ok(CoreResponse::blocked(
                        context.request_id,
                        "cancelled:before_approval",
                    ));
                }
            }
        }
        let pending = match self
            .approvals
            .decide_with_proof(context, approval_id, decision, request_hash, nonce)
            .await
        {
            Ok(pending) => pending,
            Err(error) => {
                return Ok(CoreResponse::blocked(context.request_id, error.to_string()));
            }
        };
        let committed = self.approvals.read_decision(context, approval_id).await?;
        if committed.decision_command_id != Some(context.request_id) {
            return self
                .replay_approval_decision(context, &committed, decision)
                .await;
        }
        let invocation = self
            .pending_invocations
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .remove(&approval_id);
        if decision == ApprovalDecision::Approve
            && invocation.is_none()
            && (has_live_invocation || persisted_approval.is_some())
        {
            // A concurrent replay cannot turn a Run-bound action into a direct command.
            return self
                .replay_approval_decision(context, &committed, decision)
                .await;
        }
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
            "decision":decision,
            "subject_request_id": pending.request.request_id,
            "operation": pending.request.operation,
            "expires_at_unix_ms":pending.challenge.expires_at_unix_ms,
            "scope": {"project_root":context.project_root,"role_id":context.role_id,"department_id":context.department_id,"paths":context.path_allow},
        });
        if let Some(invocation) = &invocation {
            approval_event["run_id"] = json!(invocation.run_id);
        } else if let Some(cursor) = persisted_approval {
            approval_event["run_id"] = json!(cursor.run_id);
        }
        self.append_event(event_request_id, event_sequence, event_kind, approval_event)
            .await?;

        if let Some(invocation) = invocation {
            return self
                .resume_approved_invocation(context, request_id, approval_id, decision, invocation)
                .await;
        }

        if decision == ApprovalDecision::Deny {
            if let Some(cursor) = persisted_approval {
                let mut sequence = event_sequence + 1;
                self.record_terminal_event(
                    event_request_id,
                    &mut sequence,
                    cursor.run_id,
                    "run.failed",
                    json!({"run_id":cursor.run_id,"error":"approval_denied"}),
                )
                .await?;
                return Ok(CoreResponse {
                    request_id,
                    status: ExecutionStatus::Failed,
                    output: json!({"run_id":cursor.run_id,"approval_id":approval_id,"decision":"deny"}),
                    error: Some("approval_denied".to_owned()),
                });
            }
            return Ok(CoreResponse {
                request_id,
                status: ExecutionStatus::Denied,
                output: json!({ "approval_id": approval_id }),
                error: Some("approval_denied".to_owned()),
            });
        }
        if let Err(error) = self
            .guard_company_capability(context, &pending.request)
            .await
        {
            self.append_event(
                event_request_id,
                event_sequence + 1,
                "capability.blocked",
                json!({"error":error.to_string()}),
            )
            .await?;
            return Ok(CoreResponse::blocked(request_id, error.to_string()));
        }
        self.execute_authorized_request(
            context,
            pending.request,
            format!("approval:{approval_id}"),
            5,
        )
        .await
    }

    async fn replay_approval_decision(
        &self,
        context: &RequestContext,
        record: &kiana_domain::ApprovalDecisionRecord,
        decision: ApprovalDecision,
    ) -> Result<CoreResponse, CoreError> {
        let approval_id = record.approval_id;
        if record.decision != Some(decision) {
            return Ok(CoreResponse::blocked(
                context.request_id,
                "approval_decision_conflict",
            ));
        }
        if decision == ApprovalDecision::Deny {
            return Ok(CoreResponse {
                request_id: context.request_id,
                status: ExecutionStatus::Denied,
                output: json!({"approval_id":approval_id,"replayed":true,"decision":"deny"}),
                error: Some("approval_denied".to_owned()),
            });
        }
        let events = self.read_all_events().await?.unwrap_or_default();
        let run_id = events
            .iter()
            .find(|event| {
                event.kind == "approval.requested"
                    && event.data["approval_id"] == json!(approval_id)
            })
            .and_then(|event| event.data["run_id"].as_str())
            .and_then(RunId::parse_str);
        if let Some(run_id) = run_id {
            // A tool completion cannot stand in for completion of the resumed Run.
            let owned = crate::receipts::filter_run_events(&events, run_id);
            if crate::receipts::receipt_owner_mismatch(&owned, context) {
                return Ok(CoreResponse::blocked(
                    context.request_id,
                    "run_owner_mismatch",
                ));
            }
            let state = self.run_state(run_id).await?;
            let status = match state.outcome {
                Some(RunOutcome::Completed) => ExecutionStatus::Completed,
                Some(RunOutcome::Failed) => ExecutionStatus::Failed,
                Some(RunOutcome::Cancelled) => ExecutionStatus::Cancelled,
                Some(RunOutcome::ResultUnknown) => ExecutionStatus::ResultUnknown,
                None if state.phase == RunPhase::AwaitingApproval => {
                    ExecutionStatus::AwaitingApproval
                }
                None => ExecutionStatus::ResultUnknown,
            };
            return Ok(CoreResponse {
                request_id: context.request_id,
                status,
                output: json!({"approval_id":approval_id,"run_id":run_id,"replayed":true,"state":record.state}),
                error: state.error.or_else(|| {
                    (status == ExecutionStatus::ResultUnknown)
                        .then(|| "result_unknown:approval_run_not_terminal".to_owned())
                }),
            });
        }
        if let Some(result) = events
            .iter()
            .rev()
            .filter(|event| {
                event.kind == "execution.result_committed"
                    && event.data["capability_request_id"] == json!(record.challenge.request_id)
            })
            .find_map(|event| {
                serde_json::from_value::<CapabilityResult>(event.data["result"].clone()).ok()
            })
        {
            let result =
                kiana_domain::normalize_capability_result(record.challenge.request_id, result);
            let status = match result.failure_code() {
                None => ExecutionStatus::Completed,
                Some(kiana_domain::CapabilityErrorCode::Cancelled) => ExecutionStatus::Cancelled,
                Some(
                    kiana_domain::CapabilityErrorCode::ResultUnknown
                    | kiana_domain::CapabilityErrorCode::CompensationRequired,
                ) => ExecutionStatus::ResultUnknown,
                _ => ExecutionStatus::Failed,
            };
            return Ok(CoreResponse {
                request_id: context.request_id,
                status,
                error: (!result.success).then(|| {
                    result.output["error"]
                        .as_str()
                        .unwrap_or("capability_failed")
                        .to_owned()
                }),
                output: json!({"approval_id":approval_id,"replayed":true,"result":redact_capability_result(result)}),
            });
        }
        return Ok(CoreResponse {
            request_id: context.request_id,
            status: ExecutionStatus::ResultUnknown,
            output: json!({"approval_id":approval_id,"replayed":true,"state":record.state,"dispatch_command_id":record.dispatch_command_id}),
            error: Some("result_unknown:approval_dispatch_reconciliation_required".to_owned()),
        });
    }

    pub(crate) async fn resume_approved_invocation(
        &self,
        decision_context: &RequestContext,
        request_id: RequestId,
        approval_id: ApprovalId,
        decision: ApprovalDecision,
        invocation: PendingInvocation,
    ) -> Result<CoreResponse, CoreError> {
        let mut continuation_context = decision_context.clone();
        continuation_context.request_id = invocation.event_request_id;
        let mut sequence = invocation.event_sequence + 1;
        if decision == ApprovalDecision::Deny {
            return self
                .finish_rejected_approval(
                    &continuation_context,
                    request_id,
                    approval_id,
                    &invocation,
                    "approval_denied",
                    ExecutionStatus::Failed,
                    &mut sequence,
                )
                .await;
        }
        let request = match self
            .prepare_capability_action(
                decision_context,
                invocation.request.clone(),
                Some(&invocation.sandbox),
                true,
            )
            .await
        {
            Ok(request) if request == invocation.request => request,
            Ok(_) => {
                return self
                    .finish_rejected_approval(
                        &continuation_context,
                        request_id,
                        approval_id,
                        &invocation,
                        "approval_action_changed",
                        ExecutionStatus::Failed,
                        &mut sequence,
                    )
                    .await
            }
            Err(error) => {
                return self
                    .finish_rejected_approval(
                        &continuation_context,
                        request_id,
                        approval_id,
                        &invocation,
                        &redact_event_text(&error.to_string()),
                        ExecutionStatus::Failed,
                        &mut sequence,
                    )
                    .await;
            }
        };
        let cancellation = self.watch_cancel(invocation.run_id);
        if *cancellation.borrow() {
            return self
                .finish_rejected_approval(
                    &continuation_context,
                    request_id,
                    approval_id,
                    &invocation,
                    "cancelled:before_dispatch",
                    ExecutionStatus::Cancelled,
                    &mut sequence,
                )
                .await;
        }
        let (policy, gate) = self
            .authorize_capability_action(
                decision_context,
                &request,
                Some((approval_id, &invocation.challenge.reason)),
            )
            .await?;
        self.record_event(
            invocation.event_request_id,
            &mut sequence,
            "capability.decision",
            json!({"run_id":invocation.run_id,"capability_request_id":request.request_id,"policy":policy,"gate":gate}),
        )
        .await?;
        let authorization_id = match gate {
            GateDecision::Allowed { authorization_id } => authorization_id,
            GateDecision::Denied { reason } => {
                return self
                    .finish_rejected_approval(
                        &continuation_context,
                        request_id,
                        approval_id,
                        &invocation,
                        &reason,
                        ExecutionStatus::Failed,
                        &mut sequence,
                    )
                    .await;
            }
            GateDecision::AwaitingApproval { .. } => {
                return self
                    .finish_rejected_approval(
                        &continuation_context,
                        request_id,
                        approval_id,
                        &invocation,
                        "approval_requirements_changed",
                        ExecutionStatus::Failed,
                        &mut sequence,
                    )
                    .await;
            }
        };
        if let Some(cell_id) = request.cell_id {
            let current = self
                .cell_registry
                .reservation_for_cell(cell_id)
                .await?
                .ok_or_else(|| PortError::Failed("cell_not_found".to_owned()))?;
            if current.cell.lifecycle == CellLifecycle::WaitingInput {
                self.cell_registry
                    .transition_cell(cell_id, CellLifecycle::WaitingInput, CellLifecycle::Running)
                    .await?;
            }
        }
        let finalized = self
            .dispatch_capability_action(
                decision_context,
                Some(invocation.run_id),
                &request,
                authorization_id,
                cancellation,
                invocation.event_request_id,
                &mut sequence,
            )
            .await?;
        if matches!(
            finalized.status,
            ExecutionStatus::Cancelled | ExecutionStatus::ResultUnknown
        ) || finalized.result.output["dispatch_rejected"] == true
        {
            let reason = finalized
                .error
                .unwrap_or_else(|| "result_unknown:approved_invocation_unconfirmed".to_owned());
            let _ = self
                .runner
                .send(RunnerCommand::Cancel {
                    run_id: invocation.run_id,
                    reason: reason.clone(),
                })
                .await;
            self.record_terminal_event(
                invocation.event_request_id,
                &mut sequence,
                invocation.run_id,
                if finalized.status == ExecutionStatus::Cancelled {
                    "run.cancelled"
                } else if finalized.status == ExecutionStatus::ResultUnknown {
                    "run.result_unknown"
                } else {
                    "run.failed"
                },
                json!({"run_id":invocation.run_id,"error":reason}),
            )
            .await?;
            self.settle_resumed_cell(
                &continuation_context,
                invocation.run_id,
                &invocation.sandbox,
                finalized.status,
                &mut sequence,
            )
            .await?;
            return Ok(CoreResponse {
                request_id,
                status: finalized.status,
                output: run_identity(decision_context, invocation.run_id, &invocation.sandbox),
                error: Some(reason),
            });
        }
        let _terminal_scope = self.begin_terminal_scope(invocation.run_id);
        let events = match self
            .deliver_capability_result(invocation.run_id, finalized.result)
            .await
        {
            Ok(events) => events,
            Err(error) => {
                let error = redact_event_text(&error.to_string());
                let status = match kiana_domain::CapabilityErrorCode::from_reason(&error) {
                    kiana_domain::CapabilityErrorCode::Cancelled => ExecutionStatus::Cancelled,
                    _ => ExecutionStatus::ResultUnknown,
                };
                self.record_terminal_event(
                    invocation.event_request_id,
                    &mut sequence,
                    invocation.run_id,
                    if status == ExecutionStatus::Cancelled {
                        "run.cancelled"
                    } else {
                        "run.result_unknown"
                    },
                    json!({"run_id":invocation.run_id,"error":error}),
                )
                .await?;
                self.settle_resumed_cell(
                    &continuation_context,
                    invocation.run_id,
                    &invocation.sandbox,
                    status,
                    &mut sequence,
                )
                .await?;
                return Ok(CoreResponse {
                    request_id,
                    status,
                    output: run_identity(decision_context, invocation.run_id, &invocation.sandbox),
                    error: Some(error),
                });
            }
        };
        let response = self
            .drive_run(
                &continuation_context,
                invocation.run_id,
                &invocation.sandbox,
                events,
                &mut sequence,
            )
            .await?;
        self.settle_resumed_cell(
            &continuation_context,
            invocation.run_id,
            &invocation.sandbox,
            response.status,
            &mut sequence,
        )
        .await?;
        Ok(response)
    }

    /// A recorded human decision cannot leave its consumed in-memory continuation waiting.
    /// No capability has started at these rejection points; stop the Runner and retire its scope.
    async fn finish_rejected_approval(
        &self,
        context: &RequestContext,
        request_id: RequestId,
        approval_id: ApprovalId,
        invocation: &PendingInvocation,
        reason: &str,
        status: ExecutionStatus,
        sequence: &mut u64,
    ) -> Result<CoreResponse, CoreError> {
        let reason = redact_event_text(reason);
        let tools_recorded = self
            .cancel_pending_tools(
                invocation.run_id,
                context.request_id,
                sequence,
                Some(&invocation.request),
                &reason,
            )
            .await;
        let recorded = self.record_terminal_event(context.request_id, sequence, invocation.run_id,
            if status == ExecutionStatus::Cancelled { "run.cancelled" } else { "run.failed" },
            json!({"run_id":invocation.run_id,"approval_id":approval_id,"error":reason,"not_executed":true})).await;
        let settled = self
            .settle_resumed_cell(
                context,
                invocation.run_id,
                &invocation.sandbox,
                status,
                sequence,
            )
            .await;
        if tools_recorded.is_err() || recorded.is_err() || settled.is_err() {
            return Ok(CoreResponse {
                request_id,
                status: ExecutionStatus::ResultUnknown,
                output: json!({"approval_id":approval_id,"run_id":invocation.run_id,"not_executed":true}),
                error: Some("result_unknown:approval_rejection_cleanup_failed".to_owned()),
            });
        }
        Ok(CoreResponse {
            request_id,
            status,
            output: json!({"approval_id":approval_id,"run_id":invocation.run_id,"not_executed":true}),
            error: Some(reason),
        })
    }

    pub(crate) async fn execute_authorized_request(
        &self,
        context: &RequestContext,
        request: CapabilityRequest,
        authorization_id: String,
        result_sequence: u64,
    ) -> Result<CoreResponse, CoreError> {
        let (_cancellation_sender, cancellation) = watch::channel(false);
        let mut sequence = result_sequence;
        let finalized = self
            .dispatch_capability_action(
                context,
                None,
                &request,
                authorization_id,
                cancellation,
                request.request_id,
                &mut sequence,
            )
            .await?;
        Ok(CoreResponse {
            request_id: request.request_id,
            status: finalized.status,
            output: finalized.result.output,
            error: finalized.error,
        })
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
        let fixed = kiana_policy::DefaultPolicyEngine.evaluate(context, request);
        if matches!(fixed, PolicyDecision::Deny { .. }) {
            return fixed;
        }
        let configured = self.policy.evaluate(context, request);
        if matches!(&configured, PolicyDecision::Allow { authorization_id } if authorization_id.trim().is_empty())
        {
            return PolicyDecision::Deny {
                reason: "authorization_id_required".to_owned(),
            };
        }
        match (fixed, configured) {
            (_, PolicyDecision::Deny { reason }) => PolicyDecision::Deny { reason },
            (PolicyDecision::Ask { reason: left }, PolicyDecision::Ask { reason: right }) => {
                PolicyDecision::Ask {
                    reason: super::capabilities::merge_requirements(&left, &right),
                }
            }
            (PolicyDecision::Ask { reason }, _) | (_, PolicyDecision::Ask { reason }) => {
                PolicyDecision::Ask { reason }
            }
            _ => PolicyDecision::Allow {
                authorization_id: format!("policy:{}", request.request_id),
            },
        }
    }

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
        if let PolicyDecision::Deny { reason } = policy {
            return GateDecision::Denied {
                reason: reason.clone(),
            };
        }
        let contribution = self.gates.evaluate(policy);
        match (policy, contribution) {
            (_, GateDecision::Denied { reason }) => GateDecision::Denied { reason },
            (
                PolicyDecision::Ask { reason: left },
                GateDecision::AwaitingApproval { reason: right },
            ) => GateDecision::AwaitingApproval {
                reason: super::capabilities::merge_requirements(left, &right),
            },
            (PolicyDecision::Ask { reason }, _) => GateDecision::AwaitingApproval {
                reason: reason.clone(),
            },
            (_, GateDecision::AwaitingApproval { reason }) => {
                GateDecision::AwaitingApproval { reason }
            }
            (
                PolicyDecision::Allow {
                    authorization_id: expected,
                },
                GateDecision::Allowed { authorization_id },
            ) if !expected.trim().is_empty() && expected == &authorization_id => {
                GateDecision::Allowed {
                    authorization_id: expected.clone(),
                }
            }
            _ => GateDecision::Denied {
                reason: "gate_authorization_changed".to_owned(),
            },
        }
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

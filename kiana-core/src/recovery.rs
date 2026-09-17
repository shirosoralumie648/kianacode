use super::redaction::*;
use super::*;
use kiana_domain::{json_digest, ApprovalView, RunSnapshot};

impl ControlPlane {
    async fn rebuild_pending_invocation(
        &self,
        context: &RequestContext,
        run_id: RunId,
        sandbox: &str,
        events: &[RuntimeEvent],
        projections: &[InvocationProjection],
        snapshot_pending: Option<PendingInvocation>,
    ) -> Result<Option<PendingInvocation>, CoreError> {
        let mut pending = match snapshot_pending {
            Some(pending) => pending,
            None => {
                let pending_projections = projections
                    .iter()
                    .filter(|projection| {
                        projection.state == kiana_domain::CapabilityExecutionState::AwaitingApproval
                            && projection.approval_id.is_some()
                            && projection.request.is_some()
                    })
                    .collect::<Vec<_>>();
                let projection = match pending_projections.as_slice() {
                    [] => return Ok(None),
                    [projection] => *projection,
                    _ => {
                        return Err(
                            PortError::Failed("approval_continuation_ambiguous".to_owned()).into(),
                        )
                    }
                };
                let approval_id = projection.approval_id.expect("checked above");
                let listed = self.approvals.list_pending(context).await?;
                let Some(preview) = listed
                    .into_iter()
                    .find(|pending| pending.challenge.approval_id == approval_id)
                else {
                    return Err(
                        PortError::Failed("approval_continuation_unavailable".to_owned()).into(),
                    );
                };
                let approval_context = RequestContext {
                    request_id: projection.request_id,
                    ..context.clone()
                };
                let material = self
                    .approvals
                    .pending_with_proof(
                        &approval_context,
                        approval_id,
                        Some(&preview.challenge.request_hash),
                        Some(&preview.challenge.nonce),
                    )
                    .await?;
                let approval_event = events.iter().rev().find(|event| {
                    event.kind == "approval.requested"
                        && event.data.get("approval_id") == Some(&json!(approval_id))
                        && event.data.get("capability_request_id")
                            == Some(&json!(material.request.request_id))
                });
                let approval_event = approval_event.ok_or_else(|| {
                    CoreError::Port(PortError::Failed(
                        "approval_continuation_unavailable".to_owned(),
                    ))
                })?;
                let resume_binding = match approval_event.data.get("resume_binding") {
                    Some(value) => Some(serde_json::from_value(value.clone()).map_err(|_| {
                        CoreError::Port(PortError::Failed(
                            "approval_resume_binding_invalid".to_owned(),
                        ))
                    })?),
                    None => None,
                };
                PendingInvocation {
                    approval_id,
                    challenge: material.challenge,
                    request_id: material.request.request_id,
                    event_request_id: approval_event.request_id,
                    event_sequence: events
                        .iter()
                        .filter(|candidate| candidate.request_id == approval_event.request_id)
                        .map(|candidate| candidate.sequence)
                        .max()
                        .unwrap_or(approval_event.sequence),
                    run_id,
                    request: material.request,
                    context: context.clone(),
                    sandbox: sandbox.to_owned(),
                    resume_binding,
                }
            }
        };
        let Some(projection) = projections
            .iter()
            .find(|projection| projection.request_id == pending.request.request_id)
        else {
            return Err(
                PortError::Failed("run_snapshot_pending_invocation_missing".to_owned()).into(),
            );
        };
        if pending.run_id != run_id {
            return Err(PortError::Failed(
                "run_snapshot_pending_invocation_run_mismatch".to_owned(),
            )
            .into());
        }
        if pending.approval_id != pending.challenge.approval_id {
            return Err(PortError::Failed(
                "run_snapshot_pending_invocation_approval_mismatch".to_owned(),
            )
            .into());
        }
        if pending.challenge.request_id != pending.request.request_id {
            return Err(PortError::Failed(
                "run_snapshot_pending_invocation_request_mismatch".to_owned(),
            )
            .into());
        }
        if projection.state != kiana_domain::CapabilityExecutionState::AwaitingApproval {
            return Err(PortError::Failed(
                "run_snapshot_pending_invocation_state_mismatch".to_owned(),
            )
            .into());
        }
        if projection.approval_id != Some(pending.approval_id) {
            return Err(PortError::Failed(
                "run_snapshot_pending_invocation_approval_mismatch".to_owned(),
            )
            .into());
        }
        let approval_context = RequestContext {
            request_id: pending.request.request_id,
            ..context.clone()
        };
        let material = self
            .approvals
            .pending_with_proof(
                &approval_context,
                pending.approval_id,
                Some(&pending.challenge.request_hash),
                Some(&pending.challenge.nonce),
            )
            .await?;
        if material.request.request_id != pending.request.request_id
            || material.challenge.approval_id != pending.approval_id
            || material.challenge.request_id != material.request.request_id
        {
            return Err(PortError::Failed("approval_binding_changed".to_owned()).into());
        }
        let material_digest = kiana_domain::capability_action_digest(&material.request);
        if projection.request.as_ref() != Some(&material.request)
            || projection.args_fingerprint.as_deref() != Some(material_digest.as_str())
        {
            return Err(PortError::Failed("approval_action_changed".to_owned()).into());
        }
        let resume_binding = match pending.resume_binding.as_ref() {
            Some(binding) => {
                binding
                    .validate_against(
                        run_id,
                        pending.event_request_id,
                        &material.request,
                        context,
                        sandbox,
                    )
                    .map_err(PortError::Failed)?;
                binding.clone()
            }
            None => {
                let pending_batch_digest = material
                    .request
                    .arguments
                    .get("pending_batch_digest")
                    .and_then(Value::as_str)
                    .map(str::to_owned)
                    .unwrap_or_else(|| {
                        json_digest(&json!({
                            "request_id": material.request.request_id,
                            "call_id": material.request.arguments.get("call_id"),
                        }))
                    });
                kiana_domain::InvocationResumeBinding::from_request(
                    run_id,
                    pending.event_request_id,
                    &material.request,
                    context,
                    sandbox,
                    pending_batch_digest,
                )
                .map_err(PortError::Failed)?
            }
        };
        pending.challenge = material.challenge;
        pending.request = material.request;
        pending.context = context.clone();
        pending.resume_binding = Some(resume_binding);
        // The cursor is local to the continuation request stream. A global event sequence can
        // belong to an unrelated aggregate and would allow a stale approval to overwrite facts.
        let latest_sequence = events
            .iter()
            .filter(|event| event.request_id == pending.event_request_id)
            .map(|event| event.sequence)
            .max()
            .unwrap_or(pending.event_sequence);
        pending.event_sequence = latest_sequence;
        Ok(Some(pending))
    }

    pub async fn list_pending_approvals(
        &self,
        context: &RequestContext,
        requested_run: Option<RunId>,
    ) -> Result<CoreResponse, CoreError> {
        if let Some(run_id) = requested_run {
            if let Err(reason) = self.resolve_run_id(context, Some(run_id)).await? {
                return Ok(CoreResponse::blocked(context.request_id, reason));
            }
        }
        let mut approvals = Vec::new();
        for pending in self.approvals.list_pending(context).await? {
            let (expected_version, payload_available) = match self
                .approvals
                .read_decision(context, pending.challenge.approval_id)
                .await
            {
                Ok(record) => (Some(record.version), record.payload_available),
                Err(PortError::Unavailable(reason))
                    if reason == "approval_decision_read_unsupported" =>
                {
                    (None, false)
                }
                Err(PortError::Failed(reason)) if reason == "approval_payload_unrecoverable" => {
                    (None, false)
                }
                Err(error) => return Err(error.into()),
            };
            let scope = self
                .approvals
                .context_for_pending(context, pending.challenge.approval_id)
                .await?;
            let plan_preview = kiana_domain::ApprovalPlanPreview::from_pending(
                &pending,
                &scope,
                payload_available,
            )
            .map_err(PortError::Failed)?;
            approvals.push(ApprovalView {
                permission_profile: Some(scope.permission_profile),
                challenge: pending.challenge,
                operation: pending.request.operation,
                arguments: redact_event_value(&pending.request.arguments),
                available_decisions: vec![ApprovalDecision::Approve, ApprovalDecision::Deny],
                expected_version,
                plan_preview: Some(plan_preview),
            });
        }
        Ok(CoreResponse::completed(
            context.request_id,
            json!({"approvals": approvals}),
        ))
    }

    pub(crate) async fn checkpoint_run(
        &self,
        context: &RequestContext,
        run_id: RunId,
        sandbox: &str,
        sequence: &mut u64,
    ) -> Result<(), CoreError> {
        let runner_state = match self.runner.checkpoint(run_id).await {
            Ok(state) => state,
            Err(PortError::Unavailable(_)) => return Ok(()),
            Err(error) => return Err(error.into()),
        };
        let pending_invocation = self
            .pending_invocations
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .values()
            .find(|pending| pending.run_id == run_id)
            .cloned();
        let snapshot = RunSnapshot {
            cell_state: if context.cell_id.is_some() {
                Some(self.cell_registry.checkpoint_run(run_id).await?)
            } else {
                None
            },
            data_epoch: self.data_epoch(&context.project_root).await?,
            authority_revision: self.authority_revision(&context.project_root).await?,
            schema: "kiana.run-snapshot.v1".to_owned(),
            run_id,
            context: context.clone(),
            sandbox: sandbox.to_owned(),
            role_prompt_hash: RoleSpec::lookup(&context.role_id)
                .map(|role| role.prompt_hash)
                .unwrap_or_default(),
            runner_state_digest: json_digest(&runner_state),
            runner_state,
            pending_invocation,
            resumable: true,
        };
        let raw =
            serde_json::to_value(snapshot).map_err(|error| PortError::Failed(error.to_string()))?;
        let mut safe = redact_event_value(&raw);
        // Redacted arguments/history cannot be used as if they were the original request.
        if safe != raw {
            safe["resumable"] = json!(false);
        }
        self.record_event(
            context.request_id,
            sequence,
            "run.snapshot",
            json!({"run_id":run_id,"snapshot":safe}),
        )
        .await?;
        for pending in self
            .pending_invocations
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .values_mut()
        {
            if pending.run_id == run_id {
                pending.event_sequence = *sequence;
            }
        }
        Ok(())
    }

    pub async fn resume_run(
        &self,
        mut context: RequestContext,
        requested_run: Option<RunId>,
    ) -> Result<CoreResponse, CoreError> {
        if context
            .session_id
            .as_str()
            .starts_with(kiana_domain::MEMORY_DISTILL_SESSION_PREFIX)
        {
            return Ok(CoreResponse::blocked(
                context.request_id,
                "memory_distillation_internal_resume_denied",
            ));
        }
        let run_id = match self.resolve_run_id(&context, requested_run).await? {
            Ok(id) => id,
            Err(reason) => return Ok(CoreResponse::blocked(context.request_id, reason)),
        };
        // Pin the CAS version before inspecting related facts. A concurrent append
        // can only make this claim fail; it cannot validate an uninspected version.
        let observed_run_stream = self.events.read_stream("run", &run_id.to_string()).await?;
        let last_version = observed_run_stream
            .iter()
            .filter_map(|event| event.stream_version)
            .max()
            .unwrap_or(0);
        let events = self
            .read_all_events()
            .await?
            .ok_or_else(|| PortError::Unavailable("run_resume_unsupported".to_owned()))?;
        let events = crate::receipts::try_filter_run_events(&events, run_id)
            .map_err(|reason| CoreError::Port(PortError::Failed(reason)))?;
        let projections = self.cache_invocation_projection(run_id, &events)?;
        let Some((index, event)) = events
            .iter()
            .enumerate()
            .rev()
            .find(|(_, event)| event.kind == "run.snapshot")
        else {
            return Ok(CoreResponse::blocked(
                context.request_id,
                "approval_continuation_unavailable",
            ));
        };
        let mut snapshot: RunSnapshot =
            serde_json::from_value(event.data.get("snapshot").cloned().unwrap_or(Value::Null))
                .map_err(|_| PortError::Failed("run_snapshot_invalid".to_owned()))?;
        let role = RoleSpec::lookup(&context.role_id)
            .ok_or_else(|| PortError::Failed("role_unknown".to_owned()))?;
        if snapshot.authority_revision != self.authority_revision(&context.project_root).await? {
            return Ok(CoreResponse::blocked(
                context.request_id,
                "run_resume_authority_changed",
            ));
        }
        if snapshot.data_epoch != self.data_epoch(&context.project_root).await? {
            return Ok(CoreResponse::blocked(
                context.request_id,
                "run_resume_data_revoked",
            ));
        }
        if snapshot.schema != "kiana.run-snapshot.v1"
            || !snapshot.resumable
            || snapshot.run_id != run_id
            || json_digest(&snapshot.runner_state) != snapshot.runner_state_digest
        {
            return Ok(CoreResponse::blocked(
                context.request_id,
                "run_snapshot_invalid",
            ));
        }
        context = self.approval_scope(&context).await?;
        if snapshot.context.work_packet_id.is_some() {
            self.bind_company_run_scope(&mut context, &snapshot.sandbox, false)
                .await?;
        }
        let original = &snapshot.context;
        if original.session_id != context.session_id
            || original.actor_id != context.actor_id
            || original.project_root != context.project_root
            || original.project_trusted != context.project_trusted
            || original.permission_profile != context.permission_profile
            || original.role_id != context.role_id
            || original.department_id != context.department_id
            || original.path_allow != context.path_allow
            || snapshot.role_prompt_hash != role.prompt_hash
        {
            return Ok(CoreResponse::blocked(
                context.request_id,
                "run_resume_scope_changed",
            ));
        }
        if crate::lifecycle::authorized_harness_sandbox(&context, Some(&snapshot.sandbox)).is_err()
        {
            return Ok(CoreResponse::blocked(
                context.request_id,
                "run_resume_sandbox_denied",
            ));
        }
        if events[index + 1..].iter().any(|event| {
            !matches!(
                event.kind.as_str(),
                "approval.continuation_unavailable"
                    | "run.resume_prepared"
                    | "request.accepted"
                    | "cell.waiting_input"
                    | "packet.awaiting_approval"
            )
        }) {
            return Ok(CoreResponse::blocked(
                context.request_id,
                "run_snapshot_stale",
            ));
        }
        let mut pending_invocation = match self
            .rebuild_pending_invocation(
                &context,
                run_id,
                &snapshot.sandbox,
                &events,
                &projections,
                snapshot.pending_invocation.clone(),
            )
            .await
        {
            Ok(pending) => pending,
            Err(error) => return Ok(CoreResponse::blocked(context.request_id, error.to_string())),
        };
        if let Some(pending) = &pending_invocation {
            let prepared = match self
                .prepare_capability_action(
                    &context,
                    pending.request.clone(),
                    Some(&snapshot.sandbox),
                    true,
                )
                .await
            {
                Ok(prepared) if prepared == pending.request => prepared,
                Ok(_) => {
                    return Ok(CoreResponse::blocked(
                        context.request_id,
                        "approval_action_changed",
                    ))
                }
                Err(error) => {
                    return Ok(CoreResponse::blocked(
                        context.request_id,
                        redact_event_text(&error.to_string()),
                    ))
                }
            };
            let (_, gate) = self
                .authorize_capability_action(
                    &context,
                    &prepared,
                    Some((pending.approval_id, &pending.challenge.reason)),
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
        }
        let resume_turn = kiana_domain::TurnIdentity::new(
            context.session_id.as_str(),
            run_id,
            kiana_domain::TurnId::from_uuid(context.request_id.as_uuid()),
            None,
            kiana_domain::TurnSemantics::Resume,
            events
                .iter()
                .filter(|event| event.kind == "run.prompt")
                .count()
                .saturating_add(1) as u64,
        )
        .map_err(|error| PortError::Failed(format!("turn_identity_invalid:{error}")))?;
        // Claim against the exact observed stream version before installing a runner.
        let claim = RuntimeEvent::new(
            context.request_id,
            1,
            "run.resume_prepared",
            json!({
                "run_id":run_id,"session_id":context.session_id,"actor_id":context.actor_id,
                "snapshot_event_id":event.event_id,
                "turn_id":resume_turn.turn_id,"turn":resume_turn,
            }),
        )?
        .with_stream_metadata("run", run_id.to_string(), last_version + 1);
        self.events
            .append_expected(claim, Some(last_version))
            .await?;
        if snapshot.authority_revision != self.authority_revision(&context.project_root).await? {
            return Ok(CoreResponse::blocked(
                context.request_id,
                "run_resume_authority_changed",
            ));
        }
        if snapshot.data_epoch != self.data_epoch(&context.project_root).await? {
            return Ok(CoreResponse::blocked(
                context.request_id,
                "run_resume_data_revoked",
            ));
        }
        if snapshot.context.cell_id.is_some() {
            let state = snapshot
                .cell_state
                .take()
                .ok_or_else(|| PortError::Failed("run_resume_cell_snapshot_missing".to_owned()))?;
            self.cell_registry.restore_run(run_id, state).await?;
        }
        self.runner.restore(run_id, snapshot.runner_state).await?;
        self.remember_session(&context, run_id);
        if let Some(mut pending) = pending_invocation.take() {
            // Keep original capability IDs, while placing subsequent facts in this explicit resume request.
            pending.context.request_id = context.request_id;
            pending.event_request_id = context.request_id;
            if let Some(binding) = pending.resume_binding.as_mut() {
                binding
                    .rebind_event_request(context.request_id)
                    .map_err(PortError::Failed)?;
            }
            // `run.resume_prepared` is sequence 1 in the fresh request-local continuation;
            // pending approval facts appended by this resume therefore start at sequence 2.
            pending.event_sequence = 2;
            let challenge = pending.challenge.clone();
            let resume_binding = pending.resume_binding.clone();
            self.pending_invocations
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .insert(pending.approval_id, pending);
            return Ok(CoreResponse {
                request_id: context.request_id,
                status: ExecutionStatus::AwaitingApproval,
                output: json!({"run_id":run_id,"session_id":context.session_id,"restored":true,
                    "approval":challenge,"resume_binding":resume_binding}),
                error: Some("approval_required".to_owned()),
            });
        }
        Ok(CoreResponse::completed(
            context.request_id,
            json!({"run_id":run_id,"session_id":context.session_id,"restored":true}),
        ))
    }

    pub(crate) async fn settle_resumed_cell(
        &self,
        context: &RequestContext,
        run_id: RunId,
        sandbox: &str,
        status: ExecutionStatus,
        sequence: &mut u64,
    ) -> Result<(), CoreError> {
        let Some(cell_id) = context.cell_id else {
            return Ok(());
        };
        let Some(reservation) = self.cell_registry.reservation_for_cell(cell_id).await? else {
            return Ok(());
        };
        let current = reservation.cell.lifecycle;
        if current.is_terminal() {
            return Ok(());
        }
        let next = match status {
            ExecutionStatus::Completed => CellLifecycle::ReadyToMerge,
            ExecutionStatus::AwaitingApproval => CellLifecycle::WaitingInput,
            ExecutionStatus::ResultUnknown => CellLifecycle::Quarantined,
            ExecutionStatus::Cancelled => CellLifecycle::CancelRequested,
            ExecutionStatus::Failed | ExecutionStatus::Blocked | ExecutionStatus::Denied => {
                CellLifecycle::Failed
            }
            _ => return Ok(()),
        };
        if current != next {
            let cell = self
                .cell_registry
                .transition_cell(cell_id, current, next)
                .await?;
            self.record_event(
                context.request_id,
                sequence,
                &format!("cell.{}", next.as_str()),
                json!({"run_id":run_id,"cell_id":cell_id,"cell":cell}),
            )
            .await?;
        }
        if status == ExecutionStatus::AwaitingApproval {
            self.checkpoint_run(context, run_id, sandbox, sequence)
                .await?;
        } else if status != ExecutionStatus::ResultUnknown {
            // Unknown effects retain their resource reservation until reconciliation.
            if next == CellLifecycle::CancelRequested {
                self.cell_registry
                    .transition_cell(cell_id, next, CellLifecycle::Cancelled)
                    .await?;
            }
            let retirement = self
                .cell_registry
                .retire_cell(cell_id, "run_continuation_terminal")
                .await?;
            self.record_event(
                context.request_id,
                sequence,
                "cell.retired",
                json!({"run_id":run_id,"retirement":retirement}),
            )
            .await?;
        }
        Ok(())
    }

    /// Derive packet scope from an owned ledger snapshot, never from UI arguments.
    pub(crate) async fn approval_scope(
        &self,
        context: &RequestContext,
    ) -> Result<RequestContext, CoreError> {
        let live = self
            .pending_invocations
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .values()
            .find(|pending| pending.context.session_id == context.session_id)
            .map(|pending| pending.context.clone());
        let original = match live {
            Some(original) => Some(original),
            None => self
                .events
                .read_all()
                .await?
                .iter()
                .rev()
                .filter(|event| event.kind == "run.snapshot")
                .filter_map(|event| {
                    serde_json::from_value::<RunSnapshot>(event.data["snapshot"].clone()).ok()
                })
                .find(|snapshot| snapshot.context.session_id == context.session_id)
                .map(|snapshot| snapshot.context),
        };
        let mut derived = context.clone();
        if let Some(original) = original {
            if original.actor_id != context.actor_id
                || original.role_id != context.role_id
                || original.department_id != context.department_id
                || original.project_root != context.project_root
                || original.project_trusted != context.project_trusted
                || original.permission_profile != context.permission_profile
                || (!context.path_allow.is_empty() && context.path_allow != original.path_allow)
            {
                return Err(PortError::Failed("approval_context_mismatch".to_owned()).into());
            }
            derived.path_allow = original.path_allow;
            derived.work_packet_id = original.work_packet_id;
            derived.cell_id = original.cell_id;
        }
        Ok(derived)
    }
}

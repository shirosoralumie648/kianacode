use super::redaction::*;
use super::*;
use kiana_domain::{json_digest, ApprovalView, RunSnapshot};

impl ControlPlane {
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
            let scope = self
                .approvals
                .context_for_pending(context, pending.challenge.approval_id)
                .await?;
            approvals.push(ApprovalView {
                permission_profile: Some(scope.permission_profile),
                challenge: pending.challenge,
                operation: pending.request.operation,
                arguments: redact_event_value(&pending.request.arguments),
                available_decisions: vec![ApprovalDecision::Approve, ApprovalDecision::Deny],
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
        let events = crate::receipts::filter_run_events(&events, run_id);
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
        if let Some(pending) = &snapshot.pending_invocation {
            let mut approval_context = context.clone();
            approval_context.request_id = pending.request.request_id;
            self.approvals
                .validate_with_proof(
                    &approval_context,
                    pending.approval_id,
                    Some(&pending.challenge.request_hash),
                    Some(&pending.challenge.nonce),
                )
                .await?;
            if let GateDecision::Denied { reason } = self.evaluate_gate(
                &pending.request,
                &self.evaluate_policy(&context, &pending.request),
            ) {
                return Ok(CoreResponse::blocked(context.request_id, reason));
            }
        }
        // Claim against the exact observed stream version before installing a runner.
        let claim = RuntimeEvent::new(
            context.request_id,
            1,
            "run.resume_prepared",
            json!({
                "run_id":run_id,"session_id":context.session_id,"actor_id":context.actor_id,
                "snapshot_event_id":event.event_id,
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
        if let Some(mut pending) = snapshot.pending_invocation.take() {
            // Keep original capability IDs, while placing subsequent facts in this explicit resume request.
            pending.context.request_id = context.request_id;
            pending.event_request_id = context.request_id;
            pending.event_sequence = 2;
            let challenge = pending.challenge.clone();
            self.pending_invocations
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .insert(pending.approval_id, pending);
            return Ok(CoreResponse {
                request_id: context.request_id,
                status: ExecutionStatus::AwaitingApproval,
                output: json!({"run_id":run_id,"session_id":context.session_id,"restored":true,"approval":challenge}),
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

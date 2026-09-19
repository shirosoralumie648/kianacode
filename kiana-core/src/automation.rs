//! Durable deterministic workflows and triggers on the existing EventStore and execution spine.
use super::*;
use kiana_domain::*;
use kiana_workflow::{definition_key, plan_command, plan_command_intent};

const AGGREGATE: &str = "workflow";
fn automation_error(reason: &str) -> CoreError {
    CoreError::Port(PortError::Conflict(reason.into()))
}
fn root(context: &RequestContext) -> String {
    ControlPlane::canonical_project_root(&context.project_root)
        .to_string_lossy()
        .into_owned()
}
fn aggregate_id(context: &RequestContext) -> String {
    format!(
        "{}\n{}",
        context.actor_id.as_deref().unwrap_or_default(),
        root(context)
    )
}
fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|v| v.as_millis().min(u128::from(u64::MAX)) as u64)
        .unwrap_or(0)
}
fn authority(context: &RequestContext) -> AutomationAuthority {
    AutomationAuthority {
        context: context.clone(),
        now_ms: now_ms(),
        execution_id: RequestId::new(),
        session_id: SessionId::new(format!("workflow-{}", RequestId::new())),
    }
}

impl ControlPlane {
    pub(crate) async fn guard_workflow_capability(
        &self,
        context: &RequestContext,
        request: &CapabilityRequest,
    ) -> Result<(), CoreError> {
        let (state, _) = self.load_workflows(context).await?;
        for instance in state.instances.values() {
            if let Some(node) = instance
                .nodes
                .values()
                .find(|node| node.execution_id == request.request_id)
            {
                if !kiana_workflow::workflow_ancestors_active(&state, &instance.instance_id)
                    || instance.status.terminal()
                    || matches!(
                        instance.status,
                        WorkflowInstanceStatus::Paused | WorkflowInstanceStatus::CancelRequested
                    )
                    || node.status.terminal()
                    || node.session_id != context.session_id
                    || instance.role_id != context.role_id
                {
                    return Err(automation_error("workflow_execution_not_authorized"));
                }
            }
        }
        Ok(())
    }

    pub(crate) async fn workflow_snapshot(
        &self,
        context: RequestContext,
    ) -> Result<CoreResponse, CoreError> {
        if context.actor_id.as_deref().is_none_or(|id| id.is_empty()) {
            return Ok(CoreResponse::blocked(
                context.request_id,
                "workflow_actor_required",
            ));
        }
        let (state, _) = self.load_workflows(&context).await?;
        Ok(CoreResponse::completed(
            context.request_id,
            json!({"schema":"kiana.workflow-state.v1","state":state}),
        ))
    }
    pub(crate) async fn handle_workflow_command(
        &self,
        context: RequestContext,
        arguments: Value,
    ) -> Result<CoreResponse, CoreError> {
        if !context.project_trusted || context.permission_profile == PermissionProfile::Safe {
            return self
                .reject_workflow(&context, "workflow_project_trust_and_authority_required")
                .await;
        }
        let valid_role = RoleSpec::lookup(&context.role_id)
            .is_some_and(|r| r.department_id == context.department_id);
        if !valid_role {
            return self
                .reject_workflow(&context, "workflow_role_unknown")
                .await;
        }
        if serde_json::to_vec(&arguments).map_or(true, |v| v.len() > 512 * 1024) {
            return self
                .reject_workflow(&context, "workflow_command_too_large")
                .await;
        }
        let request: AutomationCommandRequest = match serde_json::from_value(arguments) {
            Ok(r) => r,
            Err(_) => {
                return self
                    .reject_workflow(&context, "workflow_command_invalid")
                    .await
            }
        };
        if request.schema != AUTOMATION_SCHEMA
            || request.idempotency_key.trim().is_empty()
            || request.idempotency_key.len() > 256
            || matches!(request.command, AutomationCommand::RecordObservation { .. })
        {
            return self
                .reject_workflow(&context, "workflow_schema_or_internal_command_denied")
                .await;
        }
        let (state, history) = self.load_workflows(&context).await?;
        if let Some(previous) = history
            .iter()
            .find(|e| e.request.idempotency_key == request.idempotency_key)
        {
            if previous.request != request
                || previous.authority.context.actor_id != context.actor_id
                || previous.authority.context.role_id != context.role_id
                || previous.authority.context.session_id != context.session_id
            {
                return self
                    .reject_workflow(&context, "workflow_idempotency_conflict")
                    .await;
            }
            return Ok(CoreResponse::completed(
                context.request_id,
                json!({"schema":"kiana.workflow-state.v1","replayed":true,"state":state}),
            ));
        }
        if state.revision != request.expected_revision {
            return self
                .reject_workflow(&context, "workflow_revision_conflict")
                .await;
        }
        let proof = match self
            .workflow_proof(&context, &state, &request.command)
            .await
        {
            Ok(p) => p,
            Err(CoreError::Port(PortError::Conflict(reason))) => {
                return self.reject_workflow(&context, &reason).await
            }
            Err(e) => return Err(e),
        };
        let a = authority(&context);
        let (next, _planner_intent, effect) =
            match plan_command_intent(&state, &request.command, &a, &proof) {
                Ok(n) => n,
                Err(reason) => return self.reject_workflow(&context, reason).await,
            };
        let event = AutomationEvent {
            schema: AUTOMATION_EVENT_SCHEMA.into(),
            request: request.clone(),
            authority: a,
            proof,
            envelope: None,
        };
        let committed = match self.commit_workflow(&context, event).await {
            Ok(e) => e,
            Err(CoreError::Port(PortError::Conflict(reason))) => {
                return self.reject_workflow(&context, &reason).await
            }
            Err(e) => return Err(e),
        };
        if let Some(WorkflowEffect::Dispatch {
            instance_id,
            node_id,
            kind,
            execution_id,
            session_id,
        }) = effect
        {
            // The reservation is durable before any execution. Replaying it never dispatches again.
            let mut execution_context = context.clone();
            execution_context.request_id = execution_id;
            execution_context.session_id = session_id;
            execution_context.cell_id = None;
            execution_context.work_packet_id = None;
            execution_context.path_allow.clear();
            let runtime = match kind {
                WorkflowNodeKind::AgentTask {
                    project_id,
                    packet_id,
                    sandbox,
                } => {
                    let (company, _) = self.load_company(&execution_context).await?;
                    self.handle_company_command(
                        execution_context.clone(),
                        serde_json::to_value(CompanyCommandRequest {
                            schema: COMPANY_COMMAND_SCHEMA.into(),
                            expected_revision: company.revision,
                            idempotency_key: format!("workflow:{execution_id}"),
                            command: CompanyCommand::StartRun {
                                project_id,
                                packet_id,
                                sandbox,
                            },
                        })
                        .map_err(|_| automation_error("workflow_dispatch_encode_failed"))?,
                    )
                    .await
                }
                WorkflowNodeKind::Capability { mut request } => {
                    request.request_id = execution_id;
                    self.authorize_and_execute(&execution_context, request)
                        .await
                }
                _ => return Err(automation_error("workflow_effect_invalid")),
            };
            let response = runtime.unwrap_or_else(|error| CoreResponse {
                request_id: execution_id,
                status: ExecutionStatus::ResultUnknown,
                output: Value::Null,
                error: Some(error.to_string()),
            });
            let events = self.events.read_all().await?;
            let proof = AutomationProof {
                response: Some(response),
                evidence_refs: events
                    .iter()
                    .filter(|e| e.request_id == execution_id)
                    .map(|e| format!("event:{}", e.event_id))
                    .collect(),
                event_kind: None,
            };
            match self
                .record_workflow_observation(&context, &instance_id, &node_id, proof)
                .await
            {
                Ok(state) => {
                    return Ok(CoreResponse::completed(
                        context.request_id,
                        json!({"schema":"kiana.workflow-state.v1","event_id":committed.event_id,"state":state}),
                    ))
                }
                Err(error) => {
                    return Ok(CoreResponse {
                        request_id: context.request_id,
                        status: ExecutionStatus::ResultUnknown,
                        output: json!({"instance_id":instance_id,"node_id":node_id}),
                        error: Some(format!("workflow_reconciliation_required:{error}")),
                    })
                }
            }
        }
        if let Some(WorkflowEffect::Cancel { instance_id }) = effect {
            self.cancel_workflow_effects(&context, &next, &instance_id)
                .await?;
        }
        if let AutomationCommand::Tick { trigger_id } | AutomationCommand::Fire { trigger_id, .. } =
            &request.command
        {
            for instance in next.instances.values().filter(|i| {
                i.trigger_id.as_deref() == Some(trigger_id)
                    && i.status == WorkflowInstanceStatus::CancelRequested
            }) {
                self.cancel_workflow_effects(&context, &next, &instance.instance_id)
                    .await?;
            }
        }
        Ok(CoreResponse::completed(
            context.request_id,
            json!({"schema":"kiana.workflow-state.v1","event_id":committed.event_id,"replayed":false,"state":next}),
        ))
    }
    async fn reject_workflow(
        &self,
        context: &RequestContext,
        reason: &str,
    ) -> Result<CoreResponse, CoreError> {
        self.append_event(
            context.request_id,
            1,
            "workflow.rejected",
            json!({"reason":reason,"actor_id":context.actor_id,"project_root":root(context)}),
        )
        .await?;
        Ok(CoreResponse::blocked(context.request_id, reason))
    }
    pub(crate) async fn load_workflows(
        &self,
        context: &RequestContext,
    ) -> Result<(AutomationState, Vec<AutomationEvent>), CoreError> {
        let mut events = self
            .events
            .read_stream(AGGREGATE, &aggregate_id(context))
            .await?;
        events.sort_by_key(|e| e.stream_version);
        let mut state = AutomationState::default();
        let mut history = Vec::new();
        let mut keys = HashSet::new();
        for event in events {
            let e: AutomationEvent = serde_json::from_value(event.data)
                .map_err(|_| automation_error("workflow_event_invalid"))?;
            if e.schema != AUTOMATION_EVENT_SCHEMA
                || e.request.schema != AUTOMATION_SCHEMA
                || e.request.expected_revision != state.revision
                || event.stream_version != Some(state.revision + 1)
                || event.kind != "workflow.command_applied"
                || e.authority.context.actor_id != context.actor_id
                || root(&e.authority.context) != root(context)
                || !keys.insert(e.request.idempotency_key.clone())
            {
                return Err(automation_error("workflow_replay_conflict"));
            }
            if let Some(envelope) = &e.envelope {
                let command_digest = json_digest(
                    &serde_json::to_value(&e.request)
                        .map_err(|_| automation_error("workflow_event_invalid"))?,
                );
                let payload_digest = json_digest(&json!({
                    "authority": &e.authority,
                    "proof": &e.proof,
                }));
                envelope
                    .validate_against(
                        &aggregate_id(context),
                        event.stream_version.unwrap_or_default(),
                        e.authority.context.request_id,
                        &e.request.idempotency_key,
                        &command_digest,
                        &payload_digest,
                    )
                    .map_err(|_| automation_error("workflow_event_envelope_mismatch"))?;
            }
            state = plan_command(&state, &e.request.command, &e.authority, &e.proof)
                .map_err(automation_error)?
                .0;
            history.push(e);
        }
        Ok((state, history))
    }
    async fn commit_workflow(
        &self,
        context: &RequestContext,
        mut event: AutomationEvent,
    ) -> Result<RuntimeEvent, CoreError> {
        let expected = event.request.expected_revision;
        let command_digest = json_digest(
            &serde_json::to_value(&event.request)
                .map_err(|_| automation_error("workflow_event_encode_failed"))?,
        );
        let payload_digest = json_digest(&json!({
            "authority": &event.authority,
            "proof": &event.proof,
        }));
        event.envelope = Some(
            AutomationEventEnvelope::new(
                aggregate_id(context),
                expected
                    .checked_add(1)
                    .ok_or_else(|| automation_error("workflow_revision_exhausted"))?,
                event.authority.context.request_id,
                event.request.idempotency_key.clone(),
                command_digest,
                payload_digest,
            )
            .map_err(|_| automation_error("workflow_event_envelope_invalid"))?,
        );
        let data = serde_json::to_value(&event)
            .map_err(|_| automation_error("workflow_event_encode_failed"))?;
        if super::redaction::redact_event_value(&data) != data {
            return Err(automation_error("workflow_sensitive_payload_denied"));
        }
        let record = RuntimeEvent::new(context.request_id, 1, "workflow.command_applied", data)?
            .with_stream_metadata(
                AGGREGATE,
                aggregate_id(context),
                expected
                    .checked_add(1)
                    .ok_or_else(|| automation_error("workflow_revision_exhausted"))?,
            )
            .with_idempotency_key(format!(
                "workflow:{}:{}",
                aggregate_id(context),
                event.request.idempotency_key
            ));
        self.commit_protected_event(context, record, expected).await
    }
    async fn record_workflow_observation(
        &self,
        context: &RequestContext,
        instance_id: &str,
        node_id: &str,
        proof: AutomationProof,
    ) -> Result<AutomationState, CoreError> {
        for _ in 0..3 {
            let (state, _) = self.load_workflows(context).await?;
            let command = AutomationCommand::RecordObservation {
                instance_id: instance_id.into(),
                node_id: node_id.into(),
            };
            let a = authority(context);
            let (next, _) = plan_command(&state, &command, &a, &proof).map_err(automation_error)?;
            let event = AutomationEvent {
                schema: AUTOMATION_EVENT_SCHEMA.into(),
                request: AutomationCommandRequest {
                    schema: AUTOMATION_SCHEMA.into(),
                    expected_revision: state.revision,
                    idempotency_key: format!("observe:{instance_id}:{node_id}:{}", state.revision),
                    command,
                },
                authority: a,
                proof: proof.clone(),
                envelope: None,
            };
            let mut observation = context.clone();
            observation.request_id = RequestId::new();
            match self.commit_workflow(&observation, event).await {
                Ok(_) => return Ok(next),
                Err(CoreError::Port(PortError::Conflict(_))) => continue,
                Err(e) => return Err(e),
            }
        }
        Err(automation_error("workflow_observation_contention"))
    }
    async fn workflow_proof(
        &self,
        context: &RequestContext,
        state: &AutomationState,
        command: &AutomationCommand,
    ) -> Result<AutomationProof, CoreError> {
        let all = self.events.read_all().await?;
        let mut proof = AutomationProof::default();
        let reference = match command {
            AutomationCommand::RegisterTrigger { trigger } => Some(&trigger.approval_ref),
            AutomationCommand::Decide { evidence_ref, .. }
            | AutomationCommand::Signal { evidence_ref, .. } => Some(evidence_ref),
            AutomationCommand::Fire { event_ref, .. } => event_ref.as_ref(),
            _ => None,
        };
        if let Some(reference) = reference {
            let id = reference
                .strip_prefix("event:")
                .ok_or_else(|| automation_error("workflow_event_reference_required"))?;
            let event = all
                .iter()
                .find(|e| e.event_id.to_string() == id)
                .ok_or_else(|| automation_error("workflow_evidence_not_found"))?;
            let owned = (event.aggregate_type.as_deref() == Some(AGGREGATE)
                && event.aggregate_id.as_deref() == Some(aggregate_id(context).as_str()))
                || super::company::company_event_owned(event, context, &all);
            if !owned {
                return Err(automation_error("workflow_evidence_owner_mismatch"));
            }
            if matches!(command, AutomationCommand::RegisterTrigger { .. }) {
                let approved = event.kind == "company.ProjectApproved"
                    || (event.kind == "workflow.command_applied"
                        && event
                            .data
                            .pointer("/request/command/type")
                            .and_then(Value::as_str)
                            == Some("decide")
                        && event
                            .data
                            .pointer("/request/command/approve")
                            .and_then(Value::as_bool)
                            == Some(true));
                if !approved {
                    return Err(automation_error("trigger_approval_event_required"));
                }
            }
            proof.evidence_refs.push(reference.clone());
            proof.event_kind = Some(event.kind.clone());
        }
        if let AutomationCommand::Reconcile {
            instance_id,
            node_id,
        } = command
        {
            let instance = state
                .instances
                .get(instance_id)
                .ok_or_else(|| automation_error("workflow_instance_not_found"))?;
            let node = instance
                .nodes
                .get(node_id)
                .ok_or_else(|| automation_error("workflow_node_not_started"))?;
            let definition = &state.definitions
                [&definition_key(&instance.definition_id, instance.definition_version)];
            let mut response = CoreResponse {
                request_id: node.execution_id,
                status: ExecutionStatus::ResultUnknown,
                output: Value::Null,
                error: Some("workflow_execution_unobserved".into()),
            };
            match &definition.nodes[node_id].kind {
                WorkflowNodeKind::AgentTask {
                    project_id,
                    packet_id,
                    ..
                } => {
                    let (company, _) = self.load_company(context).await?;
                    if let Some(run) = company.runs.get(packet_id) {
                        if run.author_session_id != node.session_id || run.project_id != *project_id
                        {
                            return Err(automation_error("workflow_run_identity_mismatch"));
                        }
                        let observed = super::company::observe_company_run(run, context, &all)
                            .map_err(CoreError::from)?;
                        response.status = observed.status;
                        response.output = serde_json::to_value(&observed)
                            .map_err(|_| automation_error("workflow_run_encode_failed"))?;
                        response.error = None;
                        proof.evidence_refs = observed.evidence_refs;
                    }
                }
                WorkflowNodeKind::Capability { .. } => {
                    let events: Vec<_> = all
                        .iter()
                        .filter(|e| e.request_id == node.execution_id)
                        .collect();
                    proof.evidence_refs = events
                        .iter()
                        .map(|e| format!("event:{}", e.event_id))
                        .collect();
                    if let Some(event) = events.iter().rev().find(|e| {
                        matches!(
                            e.kind.as_str(),
                            "capability.completed"
                                | "capability.failed"
                                | "capability.result_unknown"
                                | "capability.blocked"
                        )
                    }) {
                        response.status = match event.kind.as_str() {
                            "capability.completed" => ExecutionStatus::Completed,
                            "capability.failed" => ExecutionStatus::Failed,
                            "capability.blocked" => ExecutionStatus::Blocked,
                            _ => ExecutionStatus::ResultUnknown,
                        };
                        response.output = event.data.clone();
                        response.error = event
                            .data
                            .get("error")
                            .and_then(Value::as_str)
                            .map(str::to_owned);
                    } else if events.iter().any(|e| e.kind == "approval.denied") {
                        response.status = ExecutionStatus::Cancelled;
                        response.error = Some("workflow_approval_denied".into());
                    } else if events.iter().any(|e| e.kind == "approval.requested")
                        && now_ms() < node.lease_expires_at
                    {
                        response.status = ExecutionStatus::AwaitingApproval;
                    }
                }
                WorkflowNodeKind::SubWorkflow { .. } => {
                    let child = node
                        .child_instance_id
                        .as_ref()
                        .and_then(|id| state.instances.get(id))
                        .ok_or_else(|| automation_error("workflow_child_instance_missing"))?;
                    response.status = match child.status {
                        WorkflowInstanceStatus::Succeeded => ExecutionStatus::Completed,
                        WorkflowInstanceStatus::Failed => ExecutionStatus::Failed,
                        WorkflowInstanceStatus::Cancelled => ExecutionStatus::Cancelled,
                        WorkflowInstanceStatus::ResultUnknown => ExecutionStatus::ResultUnknown,
                        _ => ExecutionStatus::Running,
                    };
                    response.output =
                        json!({"child_instance_id":child.instance_id,"outputs":child.outputs});
                    response.error = None;
                    proof.evidence_refs = all
                        .iter()
                        .filter(|e| {
                            e.aggregate_type.as_deref() == Some(AGGREGATE)
                                && e.aggregate_id.as_deref() == Some(aggregate_id(context).as_str())
                        })
                        .map(|e| format!("event:{}", e.event_id))
                        .collect();
                }
                _ => return Err(automation_error("workflow_node_has_no_runtime")),
            }
            if matches!(
                response.status,
                ExecutionStatus::Running | ExecutionStatus::Accepted
            ) && now_ms() >= node.lease_expires_at
            {
                response.status = ExecutionStatus::ResultUnknown;
                response.error = Some("workflow_execution_lease_expired".into());
            }
            proof.response = Some(response);
        }
        Ok(proof)
    }
    async fn cancel_workflow_effects(
        &self,
        context: &RequestContext,
        state: &AutomationState,
        instance_id: &str,
    ) -> Result<(), CoreError> {
        if !state.instances.contains_key(instance_id) {
            return Err(automation_error("workflow_instance_not_found"));
        }
        for instance in state.instances.values().filter(|i| {
            i.instance_id == instance_id
                || kiana_workflow::workflow_descends_from(state, &i.instance_id, instance_id)
        }) {
            let definition = &state.definitions
                [&definition_key(&instance.definition_id, instance.definition_version)];
            let (company, _) = self.load_company(context).await?;
            for (node_id, node) in &instance.nodes {
                if node.status.terminal() {
                    continue;
                }
                if let WorkflowNodeKind::Capability { .. } = &definition.nodes[node_id].kind {
                    let events = self.events.read_all().await?;
                    if let Some(approval_id) = events
                        .iter()
                        .filter(|e| {
                            e.request_id == node.execution_id && e.kind == "approval.requested"
                        })
                        .find_map(|e| e.data.get("approval_id").cloned())
                        .and_then(|id| serde_json::from_value::<ApprovalId>(id).ok())
                    {
                        let mut cancel = context.clone();
                        cancel.request_id = RequestId::new();
                        cancel.session_id = node.session_id.clone();
                        let _ = self
                            .decide_approval(&cancel, approval_id, ApprovalDecision::Deny)
                            .await?;
                    }
                }
                if let WorkflowNodeKind::AgentTask { packet_id, .. } =
                    &definition.nodes[node_id].kind
                {
                    if let Some(run) = company.runs.get(packet_id) {
                        if run.author_session_id != node.session_id {
                            return Err(automation_error("workflow_run_identity_mismatch"));
                        }
                        let mut cancel = context.clone();
                        cancel.request_id = RequestId::new();
                        cancel.session_id = node.session_id.clone();
                        let _ = self
                            .cancel_run(cancel, run.run_id, "workflow_cancelled".into())
                            .await?;
                    }
                }
                // Even after a cancellation request the node remains unresolved until Reconcile reads
                // terminal facts. There is no claim that an external capability was rolled back.
            }
        }
        Ok(())
    }
}

//! Bounded Cell fan-out using the existing Company packet and harness route.
use super::*;
use kiana_domain::*;
const AGGREGATE: &str = "swarm";
fn error(reason: &str) -> CoreError {
    CoreError::Port(PortError::Conflict(reason.into()))
}
fn now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|v| v.as_millis().min(u128::from(u64::MAX)) as u64)
        .unwrap_or(0)
}
fn key(c: &RequestContext) -> String {
    format!(
        "{}\n{}",
        c.actor_id.as_deref().unwrap_or_default(),
        ControlPlane::canonical_project_root(&c.project_root).to_string_lossy()
    )
}
fn authority(c: &RequestContext) -> AutomationAuthority {
    AutomationAuthority {
        context: c.clone(),
        now_ms: now(),
        execution_id: RequestId::new(),
        session_id: SessionId::new(format!("swarm-{}", RequestId::new())),
    }
}
impl ControlPlane {
    pub(crate) async fn swarm_snapshot(
        &self,
        context: RequestContext,
    ) -> Result<CoreResponse, CoreError> {
        let (state, _) = self.load_swarms(&context).await?;
        Ok(CoreResponse::completed(
            context.request_id,
            json!({"schema":"kiana.swarm-state.v1","state":state}),
        ))
    }
    pub(crate) async fn handle_swarm_command(
        &self,
        context: RequestContext,
        arguments: Value,
    ) -> Result<CoreResponse, CoreError> {
        if !context.project_trusted
            || context.permission_profile == PermissionProfile::Safe
            || !RoleSpec::lookup(&context.role_id)
                .is_some_and(|r| r.department_id == context.department_id)
        {
            return self
                .reject_swarm(&context, "swarm_authority_required")
                .await;
        }
        if serde_json::to_vec(&arguments).map_or(true, |v| v.len() > 256 * 1024) {
            return self.reject_swarm(&context, "swarm_command_too_large").await;
        }
        let request: SwarmCommandRequest = match serde_json::from_value(arguments) {
            Ok(r) => r,
            Err(_) => return self.reject_swarm(&context, "swarm_command_invalid").await,
        };
        if request.schema != SWARM_SCHEMA
            || request.idempotency_key.is_empty()
            || request.idempotency_key.len() > 256
            || matches!(request.command, SwarmCommand::ControllerReady { .. })
        {
            return self
                .reject_swarm(&context, "swarm_schema_or_internal_command_denied")
                .await;
        }
        let (state, history) = self.load_swarms(&context).await?;
        if let Some(previous) = history
            .iter()
            .find(|e| e.request.idempotency_key == request.idempotency_key)
        {
            if previous.request != request
                || previous.authority.context.actor_id != context.actor_id
                || previous.authority.context.session_id != context.session_id
                || previous.authority.context.role_id != context.role_id
            {
                return self
                    .reject_swarm(&context, "swarm_idempotency_conflict")
                    .await;
            }
            return Ok(CoreResponse::completed(
                context.request_id,
                json!({"schema":"kiana.swarm-state.v1","replayed":true,"state":state}),
            ));
        }
        if state.revision != request.expected_revision {
            return self.reject_swarm(&context, "swarm_revision_conflict").await;
        }
        let a = authority(&context);
        let proof = match self.swarm_proof(&context, &state, &request.command).await {
            Ok(p) => p,
            Err(CoreError::Port(PortError::Conflict(reason))) => {
                return self.reject_swarm(&context, &reason).await
            }
            Err(e) => return Err(e),
        };
        let next = match state.transition(&request.command, &a, &proof) {
            Ok(s) => s,
            Err(r) => return self.reject_swarm(&context, r).await,
        };
        let event = SwarmEvent {
            schema: "kiana.swarm-event.v1".into(),
            request: request.clone(),
            authority: a.clone(),
            proof,
            transitions: Vec::new(),
        };
        let committed = self.commit_swarm(&context, event).await?;
        if let SwarmCommand::Create { plan } = &request.command {
            let swarm = &next.swarms[&plan.swarm_id];
            self.ensure_swarm_controller(&swarm.controller).await?;
            let observed = self.observe_swarm(&context, &plan.swarm_id, true).await?;
            return Ok(CoreResponse::completed(
                context.request_id,
                json!({"event_id":committed.event_id,"state":observed}),
            ));
        }
        if let SwarmCommand::StartChild {
            swarm_id,
            packet_id,
            sandbox,
        } = &request.command
        {
            let swarm = &next.swarms[swarm_id];
            let mut child = context.clone();
            child.request_id = a.execution_id;
            child.session_id = a.session_id;
            let (company, _) = self.load_company(&child).await?;
            let response = self
                .handle_company_command(
                    child,
                    serde_json::to_value(CompanyCommandRequest {
                        schema: COMPANY_COMMAND_SCHEMA.into(),
                        expected_revision: company.revision,
                        idempotency_key: format!("swarm:{}:{}", swarm_id, packet_id),
                        command: CompanyCommand::StartRun {
                            project_id: swarm.plan.project_id.clone(),
                            packet_id: packet_id.clone(),
                            sandbox: sandbox.clone(),
                        },
                    })
                    .map_err(|_| error("swarm_dispatch_encode_failed"))?,
                )
                .await;
            match self.observe_swarm(&context, swarm_id, false).await {
                Ok(state) => {
                    if matches!(
                        state.swarms[swarm_id].status,
                        SwarmStatus::Failed | SwarmStatus::ResultUnknown
                    ) {
                        self.cancel_swarm_children(
                            &context,
                            &state.swarms[swarm_id],
                            "swarm_sibling_failed",
                        )
                        .await?;
                    }
                    return Ok(CoreResponse::completed(
                        context.request_id,
                        json!({"event_id":committed.event_id,"state":state,"runtime":response.map_err(|e|e.to_string())}),
                    ));
                }
                Err(e) => {
                    return Ok(CoreResponse {
                        request_id: context.request_id,
                        status: ExecutionStatus::ResultUnknown,
                        output: json!({"swarm_id":swarm_id,"packet_id":packet_id}),
                        error: Some(format!("swarm_reconciliation_required:{e}")),
                    })
                }
            }
        }
        if let SwarmCommand::Reconcile { swarm_id } = &request.command {
            let swarm = &next.swarms[swarm_id];
            if matches!(
                swarm.status,
                SwarmStatus::Failed | SwarmStatus::ResultUnknown
            ) {
                self.cancel_swarm_children(&context, swarm, "swarm_sibling_failed")
                    .await?;
            }
            if swarm.status == SwarmStatus::Cancelled {
                let _ = self
                    .cell_registry
                    .retire_cell(swarm.controller.cell.cell_id, "swarm_cancelled")
                    .await?;
            }
        }
        if let SwarmCommand::Cancel { swarm_id, reason } = &request.command {
            self.cancel_swarm_children(&context, &next.swarms[swarm_id], reason)
                .await?;
        }
        if let SwarmCommand::Merge { swarm_id, .. } = &request.command {
            let swarm = &next.swarms[swarm_id];
            // Retirement releases the parent's budget reservation after every child is known stopped.
            if swarm
                .children
                .values()
                .all(|c| c.status.is_terminal() && c.status != ExecutionStatus::ResultUnknown)
            {
                let _ = self
                    .cell_registry
                    .retire_cell(swarm.controller.cell.cell_id, "swarm_merged")
                    .await?;
            }
        }
        Ok(CoreResponse::completed(
            context.request_id,
            json!({"schema":"kiana.swarm-state.v1","event_id":committed.event_id,"state":next}),
        ))
    }
    async fn reject_swarm(
        &self,
        c: &RequestContext,
        reason: &str,
    ) -> Result<CoreResponse, CoreError> {
        self.append_event(
            c.request_id,
            1,
            "swarm.rejected",
            json!({"reason":reason,"actor_id":c.actor_id}),
        )
        .await?;
        Ok(CoreResponse::blocked(c.request_id, reason))
    }
    pub(crate) async fn load_swarms(
        &self,
        c: &RequestContext,
    ) -> Result<(SwarmState, Vec<SwarmEvent>), CoreError> {
        let mut events = self.events.read_stream(AGGREGATE, &key(c)).await?;
        events.sort_by_key(|e| e.stream_version);
        let mut state = SwarmState::default();
        let mut history = Vec::new();
        let mut ids = HashSet::new();
        for event in events {
            let e: SwarmEvent =
                serde_json::from_value(event.data).map_err(|_| error("swarm_event_invalid"))?;
            e.validate_transitions()
                .map_err(|_| error("swarm_transition_replay_invalid"))?;
            if e.schema != "kiana.swarm-event.v1"
                || e.request.schema != SWARM_SCHEMA
                || e.request.expected_revision != state.revision
                || event.stream_version != Some(state.revision + 1)
                || event.kind != "swarm.command_applied"
                || key(&e.authority.context) != key(c)
                || !ids.insert(e.request.idempotency_key.clone())
            {
                return Err(error("swarm_replay_conflict"));
            }
            state = state
                .transition(&e.request.command, &e.authority, &e.proof)
                .map_err(error)?;
            history.push(e);
        }
        Ok((state, history))
    }
    async fn commit_swarm(
        &self,
        c: &RequestContext,
        event: SwarmEvent,
    ) -> Result<RuntimeEvent, CoreError> {
        let expected = event.request.expected_revision;
        let data = serde_json::to_value(&event).map_err(|_| error("swarm_event_encode_failed"))?;
        if super::redaction::redact_event_value(&data) != data {
            return Err(error("swarm_sensitive_payload_denied"));
        }
        let record = RuntimeEvent::new(c.request_id, 1, "swarm.command_applied", data)?
            .with_stream_metadata(
                AGGREGATE,
                key(c),
                expected
                    .checked_add(1)
                    .ok_or_else(|| error("swarm_revision_exhausted"))?,
            )
            .with_idempotency_key(format!(
                "swarm:{}:{}",
                key(c),
                event.request.idempotency_key
            ));
        self.commit_protected_event(c, record, expected).await
    }
    async fn observe_swarm(
        &self,
        c: &RequestContext,
        id: &str,
        controller_ready: bool,
    ) -> Result<SwarmState, CoreError> {
        for _ in 0..3 {
            let (state, _) = self.load_swarms(c).await?;
            let command = if controller_ready {
                SwarmCommand::ControllerReady {
                    swarm_id: id.into(),
                }
            } else {
                SwarmCommand::Reconcile {
                    swarm_id: id.into(),
                }
            };
            let proof = self.swarm_proof(c, &state, &command).await?;
            let a = authority(c);
            let next = state.transition(&command, &a, &proof).map_err(error)?;
            let event = SwarmEvent {
                schema: "kiana.swarm-event.v1".into(),
                request: SwarmCommandRequest {
                    schema: SWARM_SCHEMA.into(),
                    expected_revision: state.revision,
                    idempotency_key: format!("observe:{id}:{}", state.revision),
                    command,
                },
                authority: a,
                proof,
                transitions: Vec::new(),
            };
            let mut observation = c.clone();
            observation.request_id = RequestId::new();
            match self.commit_swarm(&observation, event).await {
                Ok(_) => return Ok(next),
                Err(CoreError::Port(PortError::Conflict(_))) => continue,
                Err(e) => return Err(e),
            }
        }
        Err(error("swarm_observation_contention"))
    }
    async fn swarm_proof(
        &self,
        c: &RequestContext,
        state: &SwarmState,
        command: &SwarmCommand,
    ) -> Result<SwarmProof, CoreError> {
        let (company, _) = self.load_company(c).await?;
        let all = self.events.read_all().await?;
        let mut proof = SwarmProof::default();
        if let SwarmCommand::Create { plan } = command {
            let project = company
                .projects
                .get(&plan.project_id)
                .ok_or_else(|| error("swarm_project_not_found"))?;
            if !matches!(
                project.status,
                ProjectStatus::Planned | ProjectStatus::Active
            ) {
                return Err(error("swarm_project_not_active"));
            }
            let budget = company
                .budgets
                .get(&plan.project_id)
                .ok_or_else(|| error("swarm_project_budget_required"))?;
            proof.budget = Some(budget.clone());
            let reference = plan
                .approval_ref
                .strip_prefix("event:")
                .ok_or_else(|| error("swarm_approval_event_required"))?;
            let event = all
                .iter()
                .find(|e| e.event_id.to_string() == reference)
                .ok_or_else(|| error("swarm_approval_not_found"))?;
            if event.kind != "company.ProjectBudgetConfigured"
                || !super::company::company_event_owned(event, c, &all)
                || event
                    .data
                    .pointer("/request/command/project_id")
                    .and_then(Value::as_str)
                    != Some(plan.project_id.as_str())
            {
                return Err(error("swarm_budget_approval_mismatch"));
            }
            proof.evidence_refs.push(plan.approval_ref.clone());
            for id in &plan.packet_ids {
                let packet = company
                    .packets
                    .get(id)
                    .ok_or_else(|| error("swarm_packet_not_found"))?;
                if packet.project_id != plan.project_id || company.runs.contains_key(id) {
                    return Err(error("swarm_packet_not_available"));
                }
                proof.packets.insert(id.clone(), packet.packet.clone());
            }
            proof.controller = Some(
                self.prepare_swarm_controller(c, plan, &proof.packets)
                    .await?,
            );
        } else {
            let id = match command {
                SwarmCommand::StartChild { swarm_id, .. }
                | SwarmCommand::Reconcile { swarm_id }
                | SwarmCommand::Merge { swarm_id, .. }
                | SwarmCommand::Cancel { swarm_id, .. }
                | SwarmCommand::ControllerReady { swarm_id } => swarm_id,
                _ => unreachable!(),
            };
            let swarm = state
                .swarms
                .get(id)
                .ok_or_else(|| error("swarm_not_found"))?;
            if matches!(command, SwarmCommand::Reconcile { .. })
                && swarm.status == SwarmStatus::Reserved
                && now() < swarm.plan.expires_at
            {
                self.ensure_swarm_controller(&swarm.controller).await?;
            }
            proof.controller_ready = self
                .cell_registry
                .reservation_for_cell(swarm.controller.cell.cell_id)
                .await?
                .is_some_and(|r| r.cell.lifecycle == CellLifecycle::Running);
            if matches!(
                command,
                SwarmCommand::Reconcile { .. } | SwarmCommand::Merge { .. }
            ) {
                for (id, child) in &swarm.children {
                    let observed = if let Some(run) = company.runs.get(id) {
                        super::company::observe_company_run(run, c, &all)?
                    } else {
                        CompanyRun {
                            packet_id: id.clone(),
                            project_id: swarm.plan.project_id.clone(),
                            execution_request_id: child.dispatch_request_id,
                            author_session_id: child.session_id.clone(),
                            run_id: None,
                            status: if all.iter().any(|e| {
                                e.request_id == child.dispatch_request_id
                                    && e.kind == "company.rejected"
                            }) {
                                ExecutionStatus::Blocked
                            } else {
                                ExecutionStatus::ResultUnknown
                            },
                            evidence_refs: all
                                .iter()
                                .filter(|e| e.request_id == child.dispatch_request_id)
                                .map(|e| format!("event:{}", e.event_id))
                                .collect(),
                            incident_id: None,
                        }
                    };
                    proof.evidence_refs.extend(observed.evidence_refs.clone());
                    proof.runs.insert(id.clone(), observed);
                }
                proof.reviews = company.packet_reviews.clone();
            }
        }
        Ok(proof)
    }
    async fn prepare_swarm_controller(
        &self,
        c: &RequestContext,
        plan: &SwarmPlan,
        packets: &std::collections::BTreeMap<String, WorkPacket>,
    ) -> Result<SwarmController, CoreError> {
        let template = self
            .cell_registry
            .resolve_template(ROLE_BUILDER, SWARM_CONTROLLER_TEMPLATE)
            .await?;
        let mut budget = BudgetLease::new(
            plan.max_model_calls,
            plan.max_tokens,
            plan.expires_at.saturating_sub(now()),
            plan.max_concurrency,
            plan.max_concurrency,
        );
        budget.max_reserved_budget = plan.packet_ids.len() as u64 + 1;
        let grant = CapabilityGrant {
            schema: CAPABILITY_GRANT_SCHEMA.into(),
            grant_id: CapabilityGrantId::new(),
            capability: CapabilityKind::Other("coding".into()),
            operation: "builder.packet".into(),
            resources: vec!["workspace".into()],
            paths: packets
                .values()
                .flat_map(|p| p.path_allow.clone())
                .collect(),
            expires_at_unix_ms: plan.expires_at,
            approval_id: None,
            delegation_allowed: true,
        };
        let supervision = SupervisionLease {
            schema: SUPERVISION_LEASE_SCHEMA.into(),
            lease_id: SupervisionLeaseId::new(),
            heartbeat_interval_seconds: 15,
            stall_threshold_seconds: 300,
            retry_limit: 0,
            retries_used: 0,
        };
        let spawn = SpawnPlan {
            schema: SPAWN_PLAN_SCHEMA.into(),
            plan_id: SpawnPlanId::new(),
            parent_cell_id: None,
            reason_code: "bounded_swarm_controller".into(),
            candidate_templates: vec![template.template_id],
            count: 1,
            partition: plan.swarm_id.clone(),
            input_refs: plan.packet_ids.clone(),
            output_contract: "kiana.swarm-result.v1".into(),
            requested_capabilities: Vec::new(),
            budget_reservation: 1,
            deadline_unix_ms: plan.expires_at,
            rollback_policy: "abort".into(),
            idempotency_key: format!("swarm-controller:{}:{}", key(c), plan.swarm_id),
            expected_utility: 0,
            status: SpawnPlanStatus::Proposed,
        };
        let cell = CellSpec {
            schema: CELL_SCHEMA.into(),
            cell_id: CellId::new(),
            parent_cell_id: None,
            root_run_id: RunId::new(),
            template_id: template.template_id,
            template_version: template.version.clone(),
            role_id: ROLE_BUILDER.into(),
            objective: plan.reason_code.clone(),
            input_refs: spawn.input_refs.clone(),
            output_contract: spawn.output_contract.clone(),
            partition_key: spawn.partition.clone(),
            owned_paths: Vec::new(),
            work_packet_id: None,
            owner_actor_id: c.actor_id.clone(),
            capability_grant_id: grant.grant_id,
            budget_lease_id: budget.lease_id,
            supervision_lease_id: supervision.lease_id,
            depth: 0,
            spawn_quota: plan.max_concurrency,
            lifecycle: CellLifecycle::Proposed,
        };
        let fingerprint = WorkFingerprint::from_parts(
            &plan.reason_code,
            &plan.packet_ids,
            &plan.swarm_id,
            "kiana.swarm-result.v1",
            "kiana.policy.v1",
        )
        .map_err(error)?;
        Ok(SwarmController {
            plan: spawn,
            cell,
            template,
            budget,
            grant,
            supervision,
            fingerprint,
        })
    }
    async fn ensure_swarm_controller(&self, s: &SwarmController) -> Result<(), CoreError> {
        if let Some(r) = self
            .cell_registry
            .reservation_for_cell(s.cell.cell_id)
            .await?
        {
            if r.cell.lifecycle == CellLifecycle::Running {
                return Ok(());
            }
            return Err(error("swarm_controller_not_running"));
        }
        // Recreate only resource admission; this controller has no model session or side effects.
        self.cell_registry
            .resolve_template(ROLE_BUILDER, SWARM_CONTROLLER_TEMPLATE)
            .await?;
        self.cell_registry
            .reserve_spawn(SpawnReservationRequest {
                plan: s.plan.clone(),
                cell: s.cell.clone(),
                template: s.template.clone(),
                budget: s.budget.clone(),
                grant: s.grant.clone(),
                supervision: s.supervision.clone(),
                fingerprint: s.fingerprint.clone(),
                owned_paths: Vec::new(),
            })
            .await?;
        self.cell_registry.commit_spawn(s.plan.plan_id).await?;
        self.cell_registry
            .transition_cell(s.cell.cell_id, CellLifecycle::Ready, CellLifecycle::Running)
            .await?;
        Ok(())
    }
    pub(crate) async fn swarm_parent_for_packet(
        &self,
        c: &RequestContext,
        packet_id: &str,
    ) -> Result<Option<SwarmController>, CoreError> {
        let (state, _) = self.load_swarms(c).await?;
        // A packet is reserved by exactly one swarm, so the first (and only)
        // matching reservation owns the parent authority for this packet.
        let Some(swarm) = state
            .swarms
            .values()
            .find(|s| s.plan.packet_ids.iter().any(|id| id == packet_id))
        else {
            return Ok(None);
        };
        if matches!(
            swarm.status,
            SwarmStatus::Completed
                | SwarmStatus::Failed
                | SwarmStatus::Cancelled
                | SwarmStatus::ResultUnknown
                | SwarmStatus::CancelRequested
        ) {
            return Err(error("swarm_packet_inactive"));
        }
        let child = swarm
            .children
            .get(packet_id)
            .ok_or_else(|| error("swarm_partition_not_reserved"))?;
        if child.session_id != c.session_id
            || child.status.is_terminal()
            || now() >= swarm.plan.expires_at
        {
            return Err(error("swarm_child_authority_mismatch"));
        }
        self.ensure_swarm_controller(&swarm.controller).await?;
        Ok(Some(swarm.controller.clone()))
    }
    async fn cancel_swarm_children(
        &self,
        c: &RequestContext,
        swarm: &BoundedSwarm,
        reason: &str,
    ) -> Result<(), CoreError> {
        let (company, _) = self.load_company(c).await?;
        for (id, child) in &swarm.children {
            if child.status.is_terminal() {
                continue;
            }
            if let Some(run) = company.runs.get(id) {
                if run.author_session_id != child.session_id {
                    return Err(error("swarm_child_identity_mismatch"));
                }
                let mut cancel = c.clone();
                cancel.request_id = RequestId::new();
                cancel.session_id = child.session_id.clone();
                cancel.assign_role(&RoleSpec::builder());
                let _ = self.cancel_run(cancel, run.run_id, reason.into()).await?;
            }
        }
        Ok(())
    }
}

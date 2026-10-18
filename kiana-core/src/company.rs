//! Company commands share the existing ControlPlane. The Company aggregate is an event stream,
//! not a second database or a model loop. A workspace stream serializes cross-object decisions.
use super::*;
use kiana_domain::{
    CompanyAuthority, CompanyBusinessAction, CompanyCommand, CompanyCommandReceipt,
    CompanyCommandRequest, CompanyEvent, CompanyProof, CompanyRun, CompanyState, DecisionActorKind,
    DispatchIntent, RuntimeReceiptRef, COMPANY_COMMAND_SCHEMA, COMPANY_EVENT_SCHEMA,
    COMPANY_STATE_SCHEMA,
};

const COMPANY_AGGREGATE: &str = "company";
const MAX_COMPANY_COMMAND_BYTES: usize = 256 * 1024;

impl ControlPlane {
    async fn company_revoked_paths(
        &self,
        context: &RequestContext,
    ) -> Result<std::collections::HashSet<String>, PortError> {
        let mut revoked = std::collections::HashSet::new();
        let mut pending = false;
        for event in self.events.read_all().await? {
            if event.kind == "data.revocation_requested"
                && event.data["project_root"].as_str().is_some_and(|root| {
                    Self::canonical_project_root(root)
                        == Self::canonical_project_root(&context.project_root)
                })
            {
                pending = true;
            }
            let output = if event.kind == "execution.result_committed" {
                &event.data["result"]["output"]
            } else {
                &event.data
            };
            if output["schema"] == "kiana.data-governance-result.v1"
                && output["project_root"].as_str().is_some_and(|root| {
                    Self::canonical_project_root(root)
                        == Self::canonical_project_root(&context.project_root)
                })
            {
                if let Some(paths) = output["policy"]["revoked_sources"].as_array() {
                    revoked.extend(paths.iter().filter_map(Value::as_str).map(str::to_owned));
                }
                pending = false;
            }
        }
        if pending {
            revoked.insert(".".to_owned());
        }
        Ok(revoked)
    }
    async fn company_view(
        &self,
        context: &RequestContext,
        state: &CompanyState,
    ) -> Result<Value, CoreError> {
        let revoked = self.company_revoked_paths(context).await?;
        let mut view = json!(state);
        if let Some(artifacts) = view["artifacts"].as_object_mut() {
            for artifact in artifacts.values_mut() {
                if revoked.contains(".")
                    || artifact["relative_path"]
                        .as_str()
                        .is_some_and(|path| revoked.contains(path))
                {
                    artifact["text"] = Value::Null;
                    artifact["unavailable_reason"] = json!("source_data_revoked");
                }
            }
        }
        Ok(view)
    }

    pub(crate) async fn company_runtime_step_limit(
        &self,
        context: &RequestContext,
        configured: u32,
    ) -> Result<u32, CoreError> {
        let (state, _) = self.load_company(context).await?;
        let policy = context
            .work_packet_id
            .as_ref()
            .and_then(|id| state.packets.get(id))
            .and_then(|packet| state.budgets.get(&packet.project_id))
            .or_else(|| {
                state
                    .business
                    .review_tasks
                    .values()
                    .find(|task| task.session_id == context.session_id)
                    .and_then(|task| state.business.acceptances.get(&task.acceptance_id))
                    .and_then(|acceptance| state.budgets.get(&acceptance.project_id))
            });
        Ok(policy.map_or(configured, |policy| {
            configured.min(policy.runtime.max_model_calls.min(u64::from(u32::MAX)) as u32)
        }))
    }

    pub(crate) async fn runtime_budget_for_run(
        &self,
        context: &RequestContext,
        steps: u32,
    ) -> Result<kiana_domain::RuntimeBudget, CoreError> {
        let (state, _) = self.load_company(context).await?;
        if let Some(policy) = context
            .work_packet_id
            .as_ref()
            .and_then(|id| state.packets.get(id))
            .and_then(|packet| state.budgets.get(&packet.project_id))
            .or_else(|| {
                state
                    .business
                    .review_tasks
                    .values()
                    .find(|task| task.session_id == context.session_id)
                    .and_then(|task| state.business.acceptances.get(&task.acceptance_id))
                    .and_then(|acceptance| state.budgets.get(&acceptance.project_id))
            })
        {
            let mut budget = policy.runtime.clone();
            budget.max_model_calls = budget.max_model_calls.min(u64::from(steps));
            return Ok(budget);
        }
        Ok(kiana_domain::RuntimeBudget {
            max_model_calls: u64::from(steps),
            max_tokens: 128_000u64.saturating_mul(u64::from(steps)),
            max_wall_time_ms: 300_000,
        })
    }

    pub async fn company_snapshot(
        &self,
        context: RequestContext,
    ) -> Result<CoreResponse, CoreError> {
        if let Err(reason) = company_context(&context, false) {
            return Ok(CoreResponse::blocked(context.request_id, reason));
        }
        let (state, _) = self.load_company(&context).await?;
        let scheduling = state
            .projects
            .keys()
            .map(|id| {
                // CompanyReadiness delegates the DAG portion to the canonical
                // kiana_domain::ready_packets predicate before applying typed Company gates.
                let projection = match state.project_company_readiness(id, company_now()) {
                    Ok(readiness) => serde_json::to_value(readiness).unwrap_or(Value::Null),
                    Err(error) => json!({"error":error}),
                };
                (id.clone(), projection)
            })
            .collect::<std::collections::BTreeMap<_, _>>();
        Ok(CoreResponse::completed(
            context.request_id,
            json!({"schema":COMPANY_STATE_SCHEMA,"state":self.company_view(&context,&state).await?,"scheduling":scheduling}),
        ))
    }

    pub(crate) async fn reclaim_packet_leases(
        &self,
        context: RequestContext,
        arguments: Value,
    ) -> Result<CoreResponse, CoreError> {
        if let Err(reason) = company_context(&context, true) {
            return self.reject_company(&context, reason).await;
        }
        let project_id = arguments.get("project_id").and_then(Value::as_str);
        let (state, _) = self.load_company(&context).await?;
        let expired = state
            .packets
            .values()
            .filter(|p| project_id.is_none_or(|id| p.project_id == id))
            .filter_map(|packet| {
                packet
                    .packet
                    .claim
                    .as_ref()
                    .filter(|claim| !claim.active(company_now()))
                    .map(|claim| (packet.packet.id.clone(), claim.clone()))
            })
            .take(128)
            .collect::<Vec<_>>();
        let mut results = Vec::new();
        for (packet_id, claim) in expired {
            let (current, _) = self.load_company(&context).await?;
            let request = CompanyCommandRequest {
                schema: COMPANY_COMMAND_SCHEMA.into(),
                expected_revision: current.revision,
                idempotency_key: format!(
                    "reclaim:{}:{}:{}",
                    packet_id, claim.owner, claim.lease_expires_at
                ),
                command: CompanyCommand::ReclaimPacketClaim {
                    packet_id: packet_id.clone(),
                },
            };
            let mut reclaim = context.clone();
            reclaim.request_id = RequestId::new();
            let result = self
                .handle_company_command(
                    reclaim,
                    serde_json::to_value(request)
                        .map_err(|_| company_error("company_command_encode_failed"))?,
                )
                .await?;
            results
                .push(json!({"packet_id":packet_id,"status":result.status,"error":result.error}));
        }
        Ok(CoreResponse::completed(
            context.request_id,
            json!({"schema":"kiana.packet-lease-scan.v1","results":results}),
        ))
    }

    pub(crate) async fn handle_company_command(
        &self,
        context: RequestContext,
        arguments: Value,
    ) -> Result<CoreResponse, CoreError> {
        if let Err(reason) = company_context(&context, true) {
            return self.reject_company(&context, reason).await;
        }
        if serde_json::to_vec(&arguments).map_or(true, |v| v.len() > MAX_COMPANY_COMMAND_BYTES) {
            return self
                .reject_company(&context, "company_command_too_large")
                .await;
        }
        let request: CompanyCommandRequest = match serde_json::from_value(arguments) {
            Ok(request) => request,
            Err(_) => {
                return self
                    .reject_company(&context, "company_command_invalid")
                    .await
            }
        };
        if request.schema != COMPANY_COMMAND_SCHEMA
            || request.idempotency_key.trim().is_empty()
            || request.idempotency_key.len() > 256
        {
            return self
                .reject_company(&context, "company_command_schema_or_idempotency_invalid")
                .await;
        }
        let command_policy = request.command.policy();
        if let Err(reason) = command_policy.authorize_context(&context) {
            return self.reject_company(&context, reason).await;
        }
        // CP-23: every Company mutation is registered in the protected mutation table. A command
        // whose class entry is missing, malformed, or drifted from the per-command policy fails
        // closed before the Company state is read, so no related object can change on drift.
        let mutation_permission = request.command.mutation_permission();
        if let Err(reason) = mutation_permission.authorize_policy(&command_policy) {
            return self.reject_company(&context, reason).await;
        }
        let (state, history) = self.load_company(&context).await?;
        if let Some(previous) = history
            .iter()
            .find(|e| e.request.idempotency_key == request.idempotency_key)
        {
            if previous.request != request
                || previous.authority.actor_id != context.actor_id.clone().unwrap_or_default()
                || previous.authority.role_id != context.role_id
                || (!matches!(request.command, CompanyCommand::Business { .. })
                    && previous.authority.session_id != context.session_id)
            {
                return self
                    .reject_company(&context, "company_idempotency_conflict")
                    .await;
            }
            // A durable StartRun reservation is never executed twice. A missing result requires
            // reconciliation, even when the first process died before it could dispatch.
            let original = self
                .events
                .read_stream(COMPANY_AGGREGATE, &company_aggregate_id(&context))
                .await?
                .into_iter()
                .find(|event| {
                    event.idempotency_key.as_deref()
                        == Some(
                            format!(
                                "company:{}:{}",
                                company_aggregate_id(&context),
                                request.idempotency_key
                            )
                            .as_str(),
                        )
                })
                .ok_or_else(|| company_error("company_command_receipt_missing"))?;
            let mut original_context = context.clone();
            original_context.actor_id = Some(previous.authority.actor_id.clone());
            original_context.session_id = previous.authority.session_id.clone();
            original_context.role_id = previous.authority.role_id.clone();
            if let Some(role) = RoleSpec::lookup(&original_context.role_id) {
                original_context.department_id = role.department_id;
            }
            let receipt = CompanyCommandReceipt::new(
                &company_aggregate_id(&context),
                &request.idempotency_key,
                &request.command,
                &original_context,
                previous.request.expected_revision,
                Some(previous.request.expected_revision.saturating_add(1)),
                Some(original.event_id),
                kiana_domain::CompanyReceiptStatus::Replayed,
                previous.proof.dispatch_intent.clone(),
            )
            .map_err(|error| company_error(&error))?;
            return Ok(CoreResponse::completed(
                context.request_id,
                json!({"schema":COMPANY_STATE_SCHEMA,"replayed":true,"receipt":receipt,"legacy_receipt":{"event_id":original.event_id,"revision":previous.request.expected_revision+1,"command_digest":kiana_domain::json_digest(&json!(previous.request)),"execution_request_id":previous.authority.execution_request_id},"state":self.company_view(&context,&state).await?}),
            ));
        }
        if state.revision != request.expected_revision {
            return self
                .reject_company(&context, "company_revision_conflict")
                .await;
        }
        if let CompanyCommand::StartRun { packet_id, .. } = &request.command {
            if let Err(error) = self.swarm_parent_for_packet(&context, packet_id).await {
                return self.reject_company(&context, &error.to_string()).await;
            }
        }
        let mut proof = match self.company_proof(&context, &state, &request.command).await {
            Ok(proof) => proof,
            Err(PortError::Conflict(reason)) => {
                return self.reject_company(&context, &reason).await
            }
            Err(error) => return Err(error.into()),
        };
        if let Err(reason) = request.command.policy().validate_decision_for(
            proof.human_decision(),
            &context,
            &request.command,
            state.revision,
            company_now(),
        ) {
            return self.reject_company(&context, &reason).await;
        }
        // CO-09: go/no-go needs a recorded intake entry. A project either keeps the Initiative that
        // converted into it or the explicit direct-intake record that names the skipped Initiative
        // and preserves the objective ancestry. The check runs at the command boundary so legacy
        // events still replay through the domain transition, while a fresh approval cannot be
        // submitted without intake provenance. The versioned business charter path keeps its own
        // organization/assignment authority and is deliberately not gated here.
        if let CompanyCommand::ApproveProject { project_id, .. } = &request.command {
            if let Err(reason) = state.project_intake_provenance(project_id) {
                return self.reject_company(&context, reason).await;
            }
        }
        // CO-11: freezing the criterion coverage is a Sponsor approval that must match the
        // server-side frozen Charter baseline of this exact project. The boundary revalidates the
        // typed graph/approval before the transition, so a forged or drifted coverage cannot
        // change a related object.
        if let CompanyCommand::FreezeCriterionCoverage { coverage } = &request.command {
            if let Err(reason) = state.coverage_freeze_verdict(coverage) {
                return self.reject_company(&context, reason).await;
            }
        }
        // CO-12: admitting a Plan/WorkGraph revision is the approval transaction. The complete
        // candidate graph (missing dependencies, blocking cycles, cross-project edges, empty
        // required milestones, isolated required criteria, reading scope) and the frozen
        // Charter/coverage bindings are revalidated before the transition, so a batch draft may
        // be assembled in any order and still changes no related object when it is invalid.
        if let CompanyCommand::FreezePlanGraph { plan } = &request.command {
            if let Err(reason) = state.plan_graph_freeze_verdict(plan) {
                return self.reject_company(&context, reason).await;
            }
        }
        let command_id = CompanyCommandReceipt::command_id(
            &company_aggregate_id(&context),
            &request.idempotency_key,
        );
        if let Some(kind) = company_dispatch_kind(&request.command) {
            proof.dispatch_intent = Some(
                DispatchIntent::new(
                    command_id,
                    kind,
                    CompanyCommandReceipt::payload_digest(&request.command),
                    CompanyCommandReceipt::authority_digest(&context),
                    company_now(),
                )
                .map_err(|error| company_error(&error))?,
            );
        }
        let dispatch_intent = proof.dispatch_intent.clone();
        let authority = CompanyAuthority {
            actor_id: context.actor_id.clone().unwrap_or_default(),
            role_id: context.role_id.clone(),
            session_id: context.session_id.clone(),
            now_ms: company_now(),
            execution_request_id: RequestId::new(),
            execution_cell_id: match &request.command {
                CompanyCommand::RenewPacketClaim { packet_id } => state
                    .packets
                    .get(packet_id)
                    .and_then(|p| p.packet.claim.as_ref())
                    .map(|c| c.owner),
                _ => Some(CellId::new()),
            },
        };
        let next = match state.transition(&request.command, &authority, &proof) {
            Ok(next) => next,
            Err(reason) => return self.reject_company(&context, reason).await,
        };
        let artifact_persisted = self.artifact_store.is_some() && proof.artifact_version.is_some();
        if artifact_persisted && !self.events.supports_atomic_transitions() {
            return self
                .reject_company(&context, "company_artifact_event_atomicity_unsupported")
                .await;
        }
        // Record the cross-store publication window before the blob is staged. The intent binds
        // the reference to this logical command so a restart can reconcile the blob/fact pair
        // instead of guessing; writing it first also covers a crash between the blob commit and
        // the fact append, which would otherwise leave an orphan blob with no journal record at
        // all. An adapter without a journal keeps the legacy single-phase path.
        let publication_intent = self
            .record_artifact_publication_intent(&command_id, &context, &request, &proof)
            .await?;
        if let Err(error) = self.persist_company_artifact(&mut proof).await {
            // Nothing was published, so the window is closed explicitly instead of being left for
            // a reconciliation pass to classify. A failed abandon keeps it Pending and visible.
            self.abandon_artifact_publication_intent(publication_intent.as_ref())
                .await;
            return match error {
                PortError::Conflict(reason) => self.reject_company(&context, &reason).await,
                other => Err(other.into()),
            };
        }
        // Retryable staging returns the first immutable manifest, including its timestamp.
        // The pre-publication transition above remains the deny boundary; rebuild the response
        // projection from the finalized proof so it matches the fact that will be committed.
        let next = if artifact_persisted {
            state
                .transition(&request.command, &authority, &proof)
                .map_err(company_error)?
        } else {
            next
        };
        let event = CompanyEvent {
            schema: COMPANY_EVENT_SCHEMA.to_owned(),
            project_root: company_root(&context),
            owner_id: authority.actor_id.clone(),
            request: request.clone(),
            authority: authority.clone(),
            proof,
        };
        let committed = match self.commit_company(&context, event).await {
            Ok(event) => event,
            Err(CoreError::Port(PortError::Conflict(reason))) => {
                return self.reject_company(&context, &reason).await
            }
            Err(_) if artifact_persisted => {
                // The immutable blob may already be committed while the EventLog result is
                // uncertain. Preserve the retry boundary: the same idempotency key can replay
                // the fact or complete the missing append without publishing a new version.
                return Err(company_error(
                    "result_unknown:company_artifact_event_commit_unconfirmed",
                ));
            }
            Err(error) => return Err(error),
        };
        self.resolve_artifact_publication_intent(
            publication_intent.as_ref(),
            Some(committed.event_id),
        )
        .await;
        let receipt = CompanyCommandReceipt::new(
            &company_aggregate_id(&context),
            &request.idempotency_key,
            &request.command,
            &context,
            request.expected_revision,
            Some(next.revision),
            Some(committed.event_id),
            kiana_domain::CompanyReceiptStatus::Committed,
            dispatch_intent,
        )
        .map_err(|error| company_error(&error))?;
        if let CompanyCommand::Business { action, .. } = &request.command {
            if let Some(mut response) = self
                .company_business_effect(&context, &next, action)
                .await?
            {
                response.request_id = context.request_id;
                if !response.output.is_object() {
                    response.output = json!({"runtime_output":response.output});
                }
                response.output["company_receipt"] = json!(receipt);
                return Ok(response);
            }
        }
        if let CompanyCommand::StartRun {
            packet_id, sandbox, ..
        } = &request.command
        {
            let packet = next
                .packets
                .get(packet_id)
                .ok_or_else(|| company_error("company_packet_not_found"))?
                .packet
                .clone();
            let mut run_context = context.clone();
            run_context.request_id = authority.execution_request_id;
            // Make Company mode explicit at the existing spawn boundary.  Direct standalone
            // runs have no work_packet_id and therefore cannot inherit this Company scope.
            run_context.work_packet_id = Some(packet_id.clone());
            run_context.cell_id = packet.claim.as_ref().map(|claim| claim.owner);
            let runtime = self
                .spawn_from_packet(run_context, packet, sandbox.clone())
                .await;
            let mut response = match runtime {
                Ok(response) => response,
                Err(error) => CoreResponse {
                    request_id: context.request_id,
                    status: ExecutionStatus::ResultUnknown,
                    output: json!({"execution_request_id":authority.execution_request_id}),
                    error: Some(format!("company_run_result_unknown:{error}")),
                },
            };
            // Persist the observed outcome without running anything again. Subsequent continue/
            // approval actions can be folded by an explicit ReconcileRun command.
            match self
                .record_company_run_observation(&context, packet_id)
                .await
            {
                Ok(observed) => {
                    if !response.output.is_object() {
                        response.output = json!({"runtime_output":response.output});
                    }
                    response.output["company"] = json!({"event_id":committed.event_id,"state":self.company_view(&context,&observed).await?});
                    response.output["company_receipt"] = json!(receipt);
                }
                Err(error) => {
                    response.status = ExecutionStatus::ResultUnknown;
                    response.error = Some(format!("company_run_reconciliation_required:{error}"));
                }
            }
            if !response.output.is_object() {
                response.output = json!({"runtime_output":response.output});
            }
            response.output["company_receipt"] = json!(receipt);
            response.request_id = context.request_id;
            return Ok(response);
        }
        if let CompanyCommand::RequestCancelProject { project_id, reason } = &request.command {
            let all = self.events.read_all().await?;
            let mut cancellations = Vec::new();
            let mut unknown = false;
            for reservation in next
                .runs
                .values()
                .filter(|run| run.project_id == *project_id)
            {
                match observe_company_run(reservation, &context, &all) {
                    Ok(observed) if observed.status.is_terminal() => {
                        unknown |= observed.status == ExecutionStatus::ResultUnknown;
                        cancellations.push(
                            json!({"packet_id":reservation.packet_id,"status":observed.status}),
                        );
                    }
                    Ok(observed) => {
                        let mut cancel = context.clone();
                        cancel.request_id = RequestId::new();
                        cancel.session_id = observed.author_session_id.clone();
                        cancel.assign_role(&RoleSpec::builder());
                        let response = self
                            .cancel_run(cancel, observed.run_id, reason.clone())
                            .await;
                        match response {
                            Ok(response) => {
                                unknown |= response.status != ExecutionStatus::Cancelled;
                                cancellations.push(
                                    json!({"packet_id":reservation.packet_id,"response":response}),
                                );
                            }
                            Err(error) => {
                                unknown = true;
                                cancellations.push(json!({"packet_id":reservation.packet_id,"error":error.to_string()}));
                            }
                        }
                    }
                    Err(error) => {
                        unknown = true;
                        cancellations.push(
                            json!({"packet_id":reservation.packet_id,"error":error.to_string()}),
                        );
                    }
                }
            }
            return Ok(CoreResponse {
                request_id: context.request_id,
                status: if unknown {
                    ExecutionStatus::ResultUnknown
                } else {
                    ExecutionStatus::Accepted
                },
                output: json!({"schema":COMPANY_STATE_SCHEMA,"event_id":committed.event_id,"company_receipt":receipt,"state":self.company_view(&context,&next).await?,"cancellations":cancellations,"next_action":"confirm_cancel_project"}),
                error: unknown.then(|| "company_cancel_reconciliation_required".to_owned()),
            });
        }
        Ok(CoreResponse::completed(
            context.request_id,
            json!({"schema":COMPANY_STATE_SCHEMA,"event_id":committed.event_id,"replayed":false,"receipt":receipt,"state":self.company_view(&context,&next).await?}),
        ))
    }

    /// Once a workspace has Company projects, writes require its persisted packet reservation.
    /// Ad-hoc read-only runs remain useful for inspecting a charter or diagnosing a failure.
    pub(crate) async fn bind_company_run_scope(
        &self,
        context: &mut RequestContext,
        sandbox: &str,
        fresh: bool,
    ) -> Result<(), CoreError> {
        if sandbox == "read-only" {
            let (state, _) = self.load_company(context).await?;
            self.company_business_runtime_guard(context, &state)?;
            return Ok(());
        }
        let (state, _) = self.load_company(context).await?;
        if state.projects.is_empty() {
            return Ok(());
        }
        let run = state
            .runs
            .values()
            .find(|run| run.author_session_id == context.session_id)
            .ok_or_else(|| company_error("company_approved_packet_required"))?;
        let project = state
            .projects
            .get(&run.project_id)
            .ok_or_else(|| company_error("company_project_not_found"))?;
        if project.status != kiana_domain::ProjectStatus::Active
            || run.status.is_terminal()
            || (fresh && run.execution_request_id != context.request_id)
        {
            return Err(company_error("company_run_not_authorized"));
        }
        let packet = state
            .packets
            .get(&run.packet_id)
            .ok_or_else(|| company_error("company_packet_not_found"))?;
        self.swarm_parent_for_packet(context, &run.packet_id)
            .await?;
        if !fresh {
            let all = self.events.read_all().await?;
            let observed = observe_company_run(run, context, &all)?;
            if observed.status.is_terminal() {
                return Err(company_error("company_run_already_terminal"));
            }
        }
        context.work_packet_id = Some(run.packet_id.clone());
        context.path_allow = packet.packet.path_allow.clone();
        self.company_business_runtime_guard(context, &state)?;
        if let Some(claim) = &packet.packet.claim {
            if claim.owner_session_id != context.session_id || !claim.active(company_now()) {
                return Err(company_error("company_packet_claim_expired"));
            }
            context.cell_id = Some(claim.owner);
            self.renew_company_claim(context, &run.packet_id).await?;
        }
        Ok(())
    }

    pub(crate) async fn guard_company_packet(
        &self,
        context: &RequestContext,
        packet: &WorkPacket,
    ) -> Result<(), CoreError> {
        let (state, _) = self.load_company(context).await?;
        if state.projects.is_empty() {
            return Ok(());
        }
        let reserved = state
            .runs
            .get(&packet.id)
            .ok_or_else(|| company_error("company_approved_packet_required"))?;
        let approved = state
            .packets
            .get(&packet.id)
            .ok_or_else(|| company_error("company_packet_not_found"))?;
        if reserved.execution_request_id != context.request_id
            || reserved.author_session_id != context.session_id
            || reserved.status.is_terminal()
            || approved.packet != *packet
        {
            return Err(company_error("company_packet_reservation_mismatch"));
        }
        Ok(())
    }

    pub(crate) async fn guard_company_capability(
        &self,
        context: &RequestContext,
        request: &CapabilityRequest,
    ) -> Result<(), CoreError> {
        self.guard_workflow_capability(context, request).await?;
        let (business_state, _) = self.load_company(context).await?;
        self.company_business_runtime_guard(context, &business_state)?;
        if let Some(packet_id) = &context.work_packet_id {
            self.swarm_parent_for_packet(context, packet_id).await?;
        }
        // Administrative commands have their own human authority and approval gate;
        // they do not borrow a Builder packet's write grant.
        if context.cell_id.is_none()
            && request.arguments["operator_authorized"] == true
            && matches!(
                request.operation.as_str(),
                "memory.review"
                    | "data.governance"
                    | "extension.manage"
                    | "connector.manage"
                    | "connector.invoke"
                    | "connector.health"
                    | "connector.mcp_handshake"
                    | "workspace.checkpoint.restore"
            )
        {
            return Ok(());
        }
        if request.risk == RiskLevel::ReadOnly
            || (request.operation == "shell.exec"
                && request.arguments.get("sandbox").and_then(Value::as_str) == Some("read-only"))
        {
            return Ok(());
        }
        let (state, _) = self.load_company(context).await?;
        if state.projects.is_empty() {
            return Ok(());
        }
        let run = state
            .runs
            .values()
            .find(|run| run.author_session_id == context.session_id)
            .ok_or_else(|| company_error("company_approved_packet_required"))?;
        let project = state
            .projects
            .get(&run.project_id)
            .ok_or_else(|| company_error("company_project_not_found"))?;
        let packet = state
            .packets
            .get(&run.packet_id)
            .ok_or_else(|| company_error("company_packet_not_found"))?;
        if project.status != kiana_domain::ProjectStatus::Active
            || run.status.is_terminal()
            || context.work_packet_id.as_deref() != Some(run.packet_id.as_str())
            || context.path_allow != packet.packet.path_allow
        {
            return Err(company_error("company_capability_scope_inactive"));
        }
        self.company_business_runtime_guard(context, &state)?;
        if let Some(claim) = &packet.packet.claim {
            if claim.owner_session_id != context.session_id
                || context.cell_id != Some(claim.owner)
                || !claim.active(company_now())
            {
                return Err(company_error("company_packet_claim_expired"));
            }
            self.renew_company_claim(context, &run.packet_id).await?;
        }
        Ok(())
    }

    async fn renew_company_claim(
        &self,
        context: &RequestContext,
        packet_id: &str,
    ) -> Result<(), CoreError> {
        for _ in 0..3 {
            let (state, _) = self.load_company(context).await?;
            let claim = state
                .packets
                .get(packet_id)
                .and_then(|p| p.packet.claim.as_ref())
                .ok_or_else(|| company_error("company_packet_not_claimed"))?;
            let now_ms = company_now();
            if !claim.active(now_ms)
                || claim.owner_session_id != context.session_id
                || context.cell_id != Some(claim.owner)
            {
                return Err(company_error("company_packet_claim_expired"));
            }
            // Avoid an event on each read-only turn when the previous heartbeat remains fresh.
            if now_ms.saturating_sub(claim.heartbeat_at) < 15_000 {
                return Ok(());
            }
            let authority = CompanyAuthority {
                actor_id: context.actor_id.clone().unwrap_or_default(),
                role_id: context.role_id.clone(),
                session_id: context.session_id.clone(),
                now_ms,
                execution_request_id: RequestId::new(),
                execution_cell_id: Some(claim.owner),
            };
            let command = CompanyCommand::RenewPacketClaim {
                packet_id: packet_id.into(),
            };
            let proof = CompanyProof::default();
            state
                .transition(&command, &authority, &proof)
                .map_err(company_error)?;
            let event = CompanyEvent {
                schema: COMPANY_EVENT_SCHEMA.into(),
                project_root: company_root(context),
                owner_id: authority.actor_id.clone(),
                request: CompanyCommandRequest {
                    schema: COMPANY_COMMAND_SCHEMA.into(),
                    expected_revision: state.revision,
                    idempotency_key: format!("heartbeat:{packet_id}:{}", state.revision),
                    command,
                },
                authority,
                proof,
            };
            let mut heartbeat = context.clone();
            heartbeat.request_id = RequestId::new();
            match self.commit_company(&heartbeat, event).await {
                Ok(_) => return Ok(()),
                Err(CoreError::Port(PortError::Conflict(_))) => continue,
                Err(error) => return Err(error),
            }
        }
        Err(company_error("company_claim_contention"))
    }

    async fn reject_company(
        &self,
        context: &RequestContext,
        reason: &str,
    ) -> Result<CoreResponse, CoreError> {
        self.append_event(context.request_id,1,"company.command_rejected",json!({"reason":reason,"actor_id":context.actor_id,"project_root":company_root(context)})).await?;
        Ok(CoreResponse::blocked(context.request_id, reason))
    }

    pub(crate) async fn load_company(
        &self,
        context: &RequestContext,
    ) -> Result<(CompanyState, Vec<CompanyEvent>), CoreError> {
        let mut events = self
            .events
            .read_stream(COMPANY_AGGREGATE, &company_aggregate_id(context))
            .await?;
        events.sort_by_key(|event| event.stream_version.unwrap_or(0));
        let mut reducer = kiana_domain::CompanyReplayReducer::new(
            company_aggregate_id(context),
            company_root(context),
            context.actor_id.clone().unwrap_or_default(),
        );
        for event in &events {
            reducer
                .apply(event)
                .map_err(|error| company_error(&error))?;
        }
        Ok(reducer.into_parts())
    }

    async fn commit_company(
        &self,
        context: &RequestContext,
        event: CompanyEvent,
    ) -> Result<RuntimeEvent, CoreError> {
        let expected = event.request.expected_revision;
        let data = serde_json::to_value(&event)
            .map_err(|_| company_error("company_event_encode_failed"))?;
        let redacted = super::redaction::redact_event_value(&data);
        // Reject secret-bearing business contracts rather than silently changing their meaning
        // between validation and deterministic replay.
        if redacted != data {
            return Err(company_error("company_sensitive_payload_denied"));
        }
        let runtime = RuntimeEvent::new(
            context.request_id,
            1,
            format!("company.{}", event.request.command.event_name()),
            redacted,
        )?
        .with_stream_metadata(
            COMPANY_AGGREGATE,
            company_aggregate_id(context),
            expected
                .checked_add(1)
                .ok_or_else(|| company_error("company_revision_exhausted"))?,
        )
        .with_idempotency_key(format!(
            "company:{}:{}",
            company_aggregate_id(context),
            event.request.idempotency_key
        ));
        self.commit_protected_event(context, runtime, expected)
            .await
    }

    async fn record_company_run_observation(
        &self,
        context: &RequestContext,
        packet_id: &str,
    ) -> Result<CompanyState, CoreError> {
        for _ in 0..3 {
            let (state, _) = self.load_company(context).await?;
            let mut command = CompanyCommand::ReconcileRun {
                packet_id: packet_id.to_owned(),
            };
            let proof = self.company_proof(context, &state, &command).await?;
            if state
                .runs
                .get(packet_id)
                .is_some_and(|run| run.run_id.is_none())
                && proof.run.as_ref().is_some_and(|run| run.run_id.is_some())
            {
                command = CompanyCommand::RecordRunStarted {
                    packet_id: packet_id.to_owned(),
                };
            }
            let authority = CompanyAuthority {
                actor_id: context.actor_id.clone().unwrap_or_default(),
                role_id: context.role_id.clone(),
                session_id: context.session_id.clone(),
                now_ms: company_now(),
                execution_request_id: RequestId::new(),
                execution_cell_id: context.cell_id,
            };
            let next = state
                .transition(&command, &authority, &proof)
                .map_err(company_error)?;
            let request = CompanyCommandRequest {
                schema: COMPANY_COMMAND_SCHEMA.to_owned(),
                expected_revision: state.revision,
                idempotency_key: format!("observe:{}:{}", context.request_id, state.revision),
                command,
            };
            let event = CompanyEvent {
                schema: COMPANY_EVENT_SCHEMA.to_owned(),
                project_root: company_root(context),
                owner_id: authority.actor_id.clone(),
                request,
                authority,
                proof,
            };
            // Use a separate request identity so the response observation cannot collide with
            // either the Company's reservation or the runner's request-scoped sequence.
            let mut observation_context = context.clone();
            observation_context.request_id = RequestId::new();
            match self.commit_company(&observation_context, event).await {
                Ok(_) => return Ok(next),
                Err(CoreError::Port(PortError::Conflict(_))) => continue,
                Err(error) => return Err(error),
            }
        }
        Err(company_error("company_observation_contention"))
    }

    async fn company_proof(
        &self,
        context: &RequestContext,
        state: &CompanyState,
        command: &CompanyCommand,
    ) -> Result<CompanyProof, PortError> {
        let all = self.events.read_all().await?;
        let mut proof = CompanyProof::default();
        proof.business = self
            .company_business_proof(context, state, command, &all)
            .await?;
        proof.business.actor_is_human =
            DecisionActorKind::from_context(context) == DecisionActorKind::Human;
        let policy = command.policy();
        proof.human_decision = policy
            .decision(context, command, state.revision, company_now())
            .map_err(PortError::Failed)?;
        if let CompanyCommand::Business { action, .. } = command {
            if let CompanyBusinessAction::Closeout {
                action: closeout, ..
            } = action.as_ref()
            {
                proof.closeout = self
                    .business_closeout_proof(context, state, closeout, &all)
                    .await?;
            }
        }
        let revoked = self.company_revoked_paths(context).await?;
        let mut references = company_references(state, command);
        references.extend(
            proof
                .business
                .review_results
                .values()
                .flat_map(|result| result.evidence_refs.clone()),
        );
        references.sort();
        references.dedup();
        let artifact_request = match command {
            CompanyCommand::RegisterArtifact {
                artifact_id,
                relative_path,
            } => Some((artifact_id, relative_path)),
            CompanyCommand::Business { action, .. } => match action.as_ref() {
                kiana_domain::CompanyBusinessAction::RegisterEvidence {
                    artifact_id,
                    relative_path,
                    ..
                } => Some((artifact_id, relative_path)),
                _ => None,
            },
            _ => None,
        };
        if let Some((artifact_id, relative_path)) = artifact_request {
            if revoked.contains(".") || revoked.contains(relative_path) {
                return Err(company_conflict("company_artifact_source_revoked"));
            }
            let text = read_company_artifact(context, relative_path).await?;
            proof.artifact = Some(kiana_domain::CompanyArtifact {
                artifact_id: artifact_id.clone(),
                relative_path: relative_path.clone(),
                text,
                registered_at: company_now(),
                typed_version: None,
            });
            if let Ok(typed_id) =
                serde_json::from_value::<kiana_domain::ArtifactId>(json!(artifact_id))
            {
                let scope_digest = kiana_domain::json_digest(&json!({
                    "actor_id": context.actor_id,
                    "project_root": company_root(context),
                }));
                let artifact_text = proof
                    .artifact
                    .as_ref()
                    .map(|artifact| artifact.text.as_bytes())
                    .unwrap_or_default();
                let version = crate::artifacts::artifact_version_from_content(
                    typed_id,
                    1,
                    "kiana.company-artifact.v1",
                    artifact_text,
                    &scope_digest,
                    "company_command",
                    command.event_name(),
                    context.actor_id.as_deref().unwrap_or_default(),
                    company_now(),
                )
                .map_err(company_conflict)?;
                crate::artifacts::validate_artifact_reference_content(
                    &version.as_ref(),
                    artifact_text,
                )
                .map_err(company_conflict)?;
                proof.artifact_version = Some(version);
                if let Some(version) = proof.artifact_version.clone() {
                    if let Some(artifact) = proof.artifact.as_mut() {
                        artifact.typed_version = Some(version);
                    }
                }
            }
        }
        for reference in references {
            if let Some(id) = reference.strip_prefix("artifact:") {
                let artifact = state
                    .artifacts
                    .get(id)
                    .ok_or_else(|| company_conflict("company_artifact_not_registered"))?;
                if revoked.contains(".") || revoked.contains(&artifact.relative_path) {
                    return Err(company_conflict("company_artifact_source_revoked"));
                }
                let typed_reference = if let Some(version) = artifact.typed_version.as_ref() {
                    let typed_id = serde_json::from_value::<kiana_domain::ArtifactId>(json!(id))
                        .map_err(|_| company_conflict("company_artifact_reference_invalid"))?;
                    if version.artifact_id != typed_id {
                        return Err(company_conflict("company_artifact_reference_mismatch"));
                    }
                    let reference = version.as_ref();
                    crate::artifacts::validate_artifact_reference_content(
                        &reference,
                        artifact.text.as_bytes(),
                    )
                    .map_err(company_conflict)?;
                    Some(reference)
                } else {
                    None
                };
                let immutable = (typed_reference.is_some() && self.artifact_store.is_some())
                    || (matches!(command, CompanyCommand::Business { .. })
                        && state.business.artifacts.get(id).is_some_and(|metadata| {
                            metadata.content_hash
                                == kiana_domain::journal_sha256(artifact.text.as_bytes())
                        }));
                let current = if immutable {
                    if let (Some(store), Some(reference)) =
                        (self.artifact_store.as_ref(), typed_reference.as_ref())
                    {
                        let bytes = store.read_artifact(&reference).await?;
                        crate::artifacts::validate_artifact_reference_content(&reference, &bytes)
                            .map_err(company_conflict)?;
                        String::from_utf8(bytes)
                            .map_err(|_| company_conflict("company_artifact_content_invalid"))?
                    } else {
                        artifact.text.clone()
                    }
                } else {
                    read_company_artifact(context, &artifact.relative_path).await?
                };
                if current != artifact.text {
                    return Err(company_conflict("company_artifact_changed"));
                }
                proof.events.push(reference);
                continue;
            }
            let id = reference.strip_prefix("event:").unwrap_or(&reference);
            let event = all
                .iter()
                .find(|e| e.event_id.to_string() == id)
                .ok_or_else(|| company_conflict("company_evidence_not_found"))?;
            if !company_event_owned(event, context, &all) {
                return Err(company_conflict("company_evidence_owner_mismatch"));
            }
            proof.events.push(reference);
        }
        let packet_id = match command {
            CompanyCommand::ReclaimPacketClaim { packet_id }
                if state.runs.contains_key(packet_id) =>
            {
                Some(packet_id)
            }
            CompanyCommand::ReviewPacket { packet_id, .. }
            | CompanyCommand::ReconcileRun { packet_id }
            | CompanyCommand::RecordRunStarted { packet_id }
            | CompanyCommand::RequestAcceptance { packet_id, .. } => Some(packet_id),
            _ => None,
        };
        if let Some(packet_id) = packet_id {
            let reservation = state
                .runs
                .get(packet_id)
                .ok_or_else(|| company_conflict("company_run_not_reserved"))?;
            let observed = match observe_company_run(reservation, context, &all) {
                Ok(observed) => observed,
                Err(PortError::Conflict(reason))
                    if matches!(command, CompanyCommand::ReclaimPacketClaim { .. })
                        && reason == "company_run_reconciliation_unavailable" =>
                {
                    let mut unknown = reservation.clone();
                    unknown.status = ExecutionStatus::ResultUnknown;
                    unknown.evidence_refs = all
                        .iter()
                        .filter(|e| {
                            e.kind == "company.RunStartRequested"
                                && e.data
                                    .pointer("/request/command/packet_id")
                                    .and_then(Value::as_str)
                                    == Some(packet_id.as_str())
                                && company_event_owned(e, context, &all)
                        })
                        .map(|e| format!("event:{}", e.event_id))
                        .collect();
                    unknown
                }
                Err(error) => return Err(error),
            };
            proof.events.extend(observed.evidence_refs.clone());
            proof.run = Some(observed);
        }
        if let CompanyCommand::ConfirmCancelProject {
            project_id,
            incident: None,
        }
        | CompanyCommand::FailProject { project_id, .. } = command
        {
            proof.project_runs_stopped = true;
            for reservation in state.runs.values().filter(|r| r.project_id == *project_id) {
                let observed = observe_company_run(reservation, context, &all)?;
                if !observed.status.is_terminal()
                    || observed.status == ExecutionStatus::ResultUnknown
                {
                    proof.project_runs_stopped = false;
                }
                proof.events.extend(observed.evidence_refs);
            }
        }
        proof.events.sort();
        proof.events.dedup();
        Ok(proof)
    }

    /// Persist a typed Company artifact only after policy and state-transition validation. The
    /// adapter owns replay semantics: a retry with the same immutable reference and bytes returns
    /// the first stored version (including its original timestamp), while manifest drift fails.
    /// The fact may only cite a manifest that correlates with the bytes read during proof and that
    /// stays readable through the same port after commit; both preconditions are enforced here so
    /// a non-conforming adapter cannot publish an unverifiable reference. EventLog commit remains
    /// a separate operation, so an adapter failure after this fence can still leave an unreferenced
    /// blob and must not be reported as an atomic cross-store transaction.
    async fn persist_company_artifact(&self, proof: &mut CompanyProof) -> Result<(), PortError> {
        let Some(store) = self.artifact_store.as_ref() else {
            return Ok(());
        };
        let Some(version) = proof.artifact_version.clone() else {
            return Ok(());
        };
        let content = proof
            .artifact
            .as_ref()
            .ok_or_else(|| company_conflict("company_artifact_content_missing"))?
            .text
            .as_bytes()
            .to_vec();
        // Pin the reference, size and digest this command is admitting before delegating to the
        // adapter; a retry-aware adapter may return the first stored manifest (including its
        // original timestamp) but never a different manifest for the same (artifact_id, version).
        let admitted = version.as_ref();
        let admitted_size = content.len() as u64;
        let admitted_hash = kiana_domain::journal_sha256(&content);
        let persisted = store.stage_artifact_version(version, content).await?;
        if persisted.as_ref() != admitted
            || persisted.size_bytes != admitted_size
            || persisted.content_hash != admitted_hash
        {
            return Err(company_conflict(
                "company_artifact_staged_manifest_mismatch",
            ));
        }
        store
            .commit_artifact(persisted.as_ref(), Some(persisted.version))
            .await?;
        // "Contents persisted before a reference is admitted" only holds if the committed
        // reference can still be read back byte-identical through the same port at commit time.
        let stored = store
            .read_artifact(&persisted.as_ref())
            .await
            .map_err(|_| company_conflict("company_artifact_committed_blob_unverified"))?;
        if stored.len() as u64 != persisted.size_bytes
            || kiana_domain::journal_sha256(&stored) != persisted.content_hash
        {
            return Err(company_conflict(
                "company_artifact_committed_blob_unverified",
            ));
        }
        proof.artifact_version = Some(persisted.clone());
        if let Some(artifact) = proof.artifact.as_mut() {
            artifact.typed_version = Some(persisted);
        }
        Ok(())
    }

    /// Write the recovery intent that binds a committed blob to this logical Company command.
    /// The intent is written before the fact append; it carries identity and digests only and is
    /// never an authority or a fact source. Adapters without a journal return `Unavailable` and
    /// the command keeps its legacy single-phase behavior.
    async fn record_artifact_publication_intent(
        &self,
        command_id: &kiana_domain::RequestId,
        context: &RequestContext,
        request: &CompanyCommandRequest,
        proof: &CompanyProof,
    ) -> Result<Option<kiana_domain::ArtifactPublicationIntent>, CoreError> {
        let Some(store) = self.artifact_store.as_ref() else {
            return Ok(None);
        };
        let Some(version) = proof.artifact_version.as_ref() else {
            return Ok(None);
        };
        let intent = kiana_domain::ArtifactPublicationIntent::new(
            command_id.to_string(),
            request.idempotency_key.clone(),
            company_aggregate_id(context),
            version.as_ref(),
            company_now(),
        )
        .map_err(|error| company_error(&error))?;
        match store.record_publication_intent(&intent).await {
            Ok(()) => Ok(Some(intent)),
            Err(PortError::Unavailable(_)) => Ok(None),
            Err(error) => Err(error.into()),
        }
    }

    /// Close a recorded window that never produced a fact. Best effort: if the terminal append
    /// fails the intent stays Pending and a later reconciliation decides, which is strictly safer
    /// than claiming the window is closed.
    async fn abandon_artifact_publication_intent(
        &self,
        intent: Option<&kiana_domain::ArtifactPublicationIntent>,
    ) {
        let Some(store) = self.artifact_store.as_ref() else {
            return;
        };
        let Some(intent) = intent else {
            return;
        };
        let _ = store
            .resolve_publication_intent(
                &intent.command_id,
                kiana_domain::ArtifactPublicationState::Abandoned,
                None,
                company_now(),
            )
            .await;
    }

    /// Close the publication window after the Company fact is committed. A resolve failure leaves
    /// the intent Pending and is recovered by reconciliation; it must not turn a committed
    /// command into an error response.
    async fn resolve_artifact_publication_intent(
        &self,
        intent: Option<&kiana_domain::ArtifactPublicationIntent>,
        event_id: Option<kiana_domain::EventId>,
    ) {
        let Some(store) = self.artifact_store.as_ref() else {
            return;
        };
        let Some(intent) = intent else {
            return;
        };
        let _ = store
            .resolve_publication_intent(
                &intent.command_id,
                kiana_domain::ArtifactPublicationState::Committed,
                event_id,
                company_now(),
            )
            .await;
    }

    /// Reconcile unresolved artifact-publication intents against the canonical fact log.
    ///
    /// This is a read-and-classify operation. It may append a terminal marker to the store's
    /// recovery journal, but it never appends a Company fact, never widens authority, and never
    /// deletes or rewrites blob bytes. A pending intent whose fact is committed becomes
    /// `Published`; a readable blob with no fact is reported as `Unpublished` (an orphan) and is
    /// marked `Abandoned` so it stays visible instead of being re-cited; anything else stays
    /// `Unverifiable` and Pending for a later attempt.
    pub async fn reconcile_artifact_publications(
        &self,
    ) -> Result<Vec<kiana_domain::ArtifactPublicationReconciliation>, CoreError> {
        let Some(store) = self.artifact_store.as_ref() else {
            return Ok(Vec::new());
        };
        let pending = match store.pending_publication_intents().await {
            Ok(pending) => pending,
            Err(PortError::Unavailable(_)) => return Ok(Vec::new()),
            Err(error) => return Err(error.into()),
        };
        if pending.is_empty() {
            return Ok(Vec::new());
        }
        let all = self.events.read_all().await?;
        let mut report = Vec::with_capacity(pending.len());
        for intent in pending {
            let key = format!("company:{}:{}", intent.aggregate_id, intent.idempotency_key);
            let event_id = all
                .iter()
                .find(|event| {
                    event.kind.starts_with("company.")
                        && event.idempotency_key.as_deref() == Some(key.as_str())
                })
                .map(|event| event.event_id);
            let blob_readable = match store.read_artifact(&intent.artifact).await {
                Ok(bytes) => kiana_domain::journal_sha256(&bytes) == intent.artifact.content_hash,
                Err(_) => false,
            };
            let reason = match (event_id.is_some(), blob_readable) {
                (true, true) => "fact_committed_and_blob_verified",
                (false, true) => "orphan_committed_blob_without_fact",
                (true, false) => "fact_committed_without_readable_blob",
                (false, false) => "fact_and_blob_unconfirmed",
            };
            let reconciliation = kiana_domain::ArtifactPublicationReconciliation::new(
                intent.command_id.clone(),
                intent.artifact.clone(),
                event_id.is_some(),
                blob_readable,
                event_id,
                reason,
            )
            .map_err(|error| company_error(&error))?;
            if let Some(state) = reconciliation.terminal_state() {
                let _ = store
                    .resolve_publication_intent(
                        &reconciliation.command_id,
                        state,
                        reconciliation.event_id,
                        company_now(),
                    )
                    .await;
            }
            report.push(reconciliation);
        }
        Ok(report)
    }

    /// Compare a committed Company artifact with its current workspace source.
    ///
    /// Read-only projection: it re-reads the immutable reference through the injected store,
    /// reads the current file through the confined reader, and reports the difference. It never
    /// rewrites history, widens authority, or fetches a path the project did not register.
    pub async fn artifact_workspace_delta(
        &self,
        context: &RequestContext,
        artifact_id: &str,
    ) -> Result<kiana_domain::ArtifactWorkspaceDelta, CoreError> {
        company_context(context, false).map_err(company_conflict)?;
        let (state, _) = self.load_company(context).await?;
        let artifact = state
            .artifacts
            .get(artifact_id)
            .ok_or_else(|| company_conflict("company_artifact_not_registered"))?
            .clone();
        let reference = artifact
            .typed_version
            .as_ref()
            .map(kiana_domain::ArtifactVersion::as_ref)
            .ok_or_else(|| company_conflict("company_artifact_reference_invalid"))?;
        // The admitted original is durable in two places: the immutable store copy and the
        // committed Company fact that admitted it. The store copy is preferred whenever it is
        // readable, but a committed fact whose snapshot still hashes to the typed reference is a
        // valid historical source on its own, so the projection never has to trust the mutable
        // workspace or fail closed merely because the derived copy is gone.
        let mut limitations = Vec::new();
        let historical = match self.artifact_store.as_ref() {
            Some(store) => match store.read_artifact(&reference).await {
                Ok(bytes) => {
                    crate::artifacts::validate_artifact_reference_content(&reference, &bytes)
                        .map_err(company_conflict)?;
                    bytes
                }
                Err(_) => {
                    limitations.push("historical_source_committed_fact".to_owned());
                    Self::fact_backed_original(&artifact, &reference)?
                }
            },
            None => {
                limitations.push("historical_source_committed_fact".to_owned());
                Self::fact_backed_original(&artifact, &reference)?
            }
        };
        let root = company_root(context);
        let relative = artifact.relative_path.clone();
        let current = read_company_artifact(context, &relative).await;
        let present = match current.as_ref() {
            Ok(_) => true,
            Err(_) => {
                super::artifacts::project_artifact_exists(Path::new(&root), Path::new(&relative))
                    .map_err(company_conflict)?
            }
        };
        let (current_state, current_content_hash, current_size_bytes, added_lines, removed_lines) =
            match current {
                Ok(text) => {
                    let bytes = text.as_bytes();
                    let hash = kiana_domain::journal_sha256(bytes);
                    if hash == reference.content_hash {
                        (
                            kiana_domain::ArtifactWorkspaceDeltaState::Unchanged,
                            Some(hash),
                            Some(bytes.len() as u64),
                            0,
                            0,
                        )
                    } else {
                        let (added, removed) = match std::str::from_utf8(&historical) {
                            Ok(historical_text) => {
                                crate::artifacts::bounded_line_delta(historical_text, &text)
                            }
                            Err(_) => (0, 0),
                        };
                        (
                            kiana_domain::ArtifactWorkspaceDeltaState::Changed,
                            Some(hash),
                            Some(bytes.len() as u64),
                            added,
                            removed,
                        )
                    }
                }
                Err(_) if present => (
                    kiana_domain::ArtifactWorkspaceDeltaState::Unavailable,
                    None,
                    None,
                    0,
                    0,
                ),
                Err(_) => (
                    kiana_domain::ArtifactWorkspaceDeltaState::Missing,
                    None,
                    None,
                    0,
                    0,
                ),
            };
        if current_state == kiana_domain::ArtifactWorkspaceDeltaState::Unavailable {
            limitations.push("current_source_unavailable".to_owned());
        }
        if std::str::from_utf8(&historical).is_err() {
            limitations.push("historical_content_not_text".to_owned());
        }
        kiana_domain::ArtifactWorkspaceDelta::new(
            reference,
            relative,
            historical.len() as u64,
            current_state,
            current_content_hash,
            current_size_bytes,
            added_lines,
            removed_lines,
            limitations,
        )
        .map_err(|error| company_error(&error))
    }

    /// Re-derive the historical original from the committed Company fact.
    ///
    /// The fact is the durable commit point for the bounded snapshot that was admitted with the
    /// immutable reference, so it stays readable even when the store's derived copy is gone. The
    /// snapshot is never trusted as-is: it must hash to the same typed reference the fact cites,
    /// so a drifted or fabricated snapshot is refused instead of becoming a fabricated original.
    fn fact_backed_original(
        artifact: &kiana_domain::CompanyArtifact,
        reference: &kiana_domain::ArtifactRef,
    ) -> Result<Vec<u8>, PortError> {
        let bytes = artifact.text.as_bytes().to_vec();
        crate::artifacts::validate_artifact_reference_content(reference, &bytes)
            .map_err(company_conflict)?;
        Ok(bytes)
    }

    /// Read-only retention plan for committed blobs that no Company fact cites.
    ///
    /// The plan reads the artifact store's publication journal and classifies each abandoned
    /// window. It never reclaims bytes, never appends a Company fact and never widens authority; a
    /// store without a journal yields an explicitly unbacked, empty plan. Open (pending) windows
    /// are reported but excluded, because only reconciliation may prove a window is fact-free.
    pub async fn plan_artifact_publication_retention(
        &self,
        context: &RequestContext,
        policy: &kiana_domain::ArtifactPublicationRetentionPolicy,
    ) -> Result<kiana_domain::ArtifactPublicationRetentionPlan, CoreError> {
        company_context(context, false).map_err(company_conflict)?;
        policy.validate().map_err(|error| company_error(&error))?;
        self.artifact_publication_retention(policy, false).await
    }

    /// Apply orphan reclamation under an explicit retention policy.
    ///
    /// Bytes are removed only when the retention policy explicitly permits blob removal, the
    /// Company write context is satisfied, and the store independently re-confirms from its own
    /// journal that the reference is abandoned. Every other case is denied or retained, and the
    /// plan is returned either way so a caller never has to assume what happened. Reclamation
    /// never appends a Company fact, never widens authority and never rewrites a committed
    /// reference.
    pub async fn apply_artifact_publication_retention(
        &self,
        context: &RequestContext,
        policy: &kiana_domain::ArtifactPublicationRetentionPolicy,
    ) -> Result<kiana_domain::ArtifactPublicationRetentionPlan, CoreError> {
        company_context(context, true).map_err(company_conflict)?;
        policy.validate().map_err(|error| company_error(&error))?;
        self.artifact_publication_retention(policy, true).await
    }

    async fn artifact_publication_retention(
        &self,
        policy: &kiana_domain::ArtifactPublicationRetentionPolicy,
        apply: bool,
    ) -> Result<kiana_domain::ArtifactPublicationRetentionPlan, CoreError> {
        use kiana_domain::{
            ArtifactOrphanRetention, ArtifactOrphanRetentionDecision,
            ArtifactPublicationRetentionPlan, ArtifactPublicationState,
        };
        let now = company_now();
        let empty = |journal_available: bool| {
            ArtifactPublicationRetentionPlan::new(
                policy.clone(),
                journal_available,
                0,
                Vec::new(),
                now,
            )
            .map_err(|error| company_error(&error))
        };
        let Some(store) = self.artifact_store.as_ref() else {
            return empty(false);
        };
        let intents = match store.publication_intents().await {
            Ok(intents) => intents,
            Err(PortError::Unavailable(_)) => return empty(false),
            Err(error) => return Err(error.into()),
        };
        let pending_count = intents
            .iter()
            .filter(|intent| intent.is_pending())
            .count()
            .min(u32::MAX as usize) as u32;
        let mut candidates: Vec<&kiana_domain::ArtifactPublicationIntent> = intents
            .iter()
            .filter(|intent| intent.state == ArtifactPublicationState::Abandoned)
            .collect();
        // Deterministic order: oldest settled window first, then command id. The per-run cap is
        // applied against this order so repeated runs converge instead of thrashing.
        candidates.sort_by(|left, right| {
            left.recorded_at_unix_ms
                .cmp(&right.recorded_at_unix_ms)
                .then_with(|| left.command_id.cmp(&right.command_id))
        });
        let mut orphans = Vec::with_capacity(candidates.len());
        let mut selected = 0u32;
        for intent in candidates {
            let settled_at = intent
                .resolved_at_unix_ms
                .unwrap_or(intent.recorded_at_unix_ms);
            let age_ms = now.saturating_sub(settled_at);
            let blob_readable = match store.read_artifact(&intent.artifact).await {
                Ok(bytes) => kiana_domain::journal_sha256(&bytes) == intent.artifact.content_hash,
                Err(_) => false,
            };
            let (decision, reason) = if !blob_readable {
                (
                    ArtifactOrphanRetentionDecision::Retained,
                    "orphan_blob_absent",
                )
            } else if age_ms < policy.max_orphan_age_ms {
                (
                    ArtifactOrphanRetentionDecision::Retained,
                    "orphan_age_below_retention_window",
                )
            } else if selected >= policy.max_orphan_count {
                (
                    ArtifactOrphanRetentionDecision::Denied,
                    "orphan_retention_capacity_exceeded",
                )
            } else if !apply {
                selected += 1;
                (
                    ArtifactOrphanRetentionDecision::Eligible,
                    "orphan_eligible_for_retention",
                )
            } else if !policy.allow_blob_removal {
                (
                    ArtifactOrphanRetentionDecision::Denied,
                    "orphan_blob_removal_not_permitted",
                )
            } else {
                match store.remove_orphan_blob(&intent.artifact).await {
                    Ok(true) => {
                        selected += 1;
                        (
                            ArtifactOrphanRetentionDecision::Removed,
                            "orphan_blob_reclaimed",
                        )
                    }
                    Ok(false) => {
                        selected += 1;
                        (
                            ArtifactOrphanRetentionDecision::Removed,
                            "orphan_blob_already_absent",
                        )
                    }
                    Err(PortError::Unavailable(_)) => (
                        ArtifactOrphanRetentionDecision::Denied,
                        "artifact_orphan_removal_unsupported",
                    ),
                    Err(_) => (
                        ArtifactOrphanRetentionDecision::Denied,
                        "artifact_orphan_removal_failed",
                    ),
                }
            };
            orphans.push(
                ArtifactOrphanRetention::new(
                    intent.command_id.clone(),
                    intent.artifact.clone(),
                    intent.state,
                    age_ms,
                    decision,
                    reason,
                )
                .map_err(|error| company_error(&error))?,
            );
        }
        ArtifactPublicationRetentionPlan::new(policy.clone(), true, pending_count, orphans, now)
            .map_err(|error| company_error(&error))
    }
}

fn company_context(context: &RequestContext, write: bool) -> Result<(), &'static str> {
    crate::company_scope::validate_workspace_root(&context.project_root)?;
    if context
        .actor_id
        .as_deref()
        .is_none_or(|a| a.trim().is_empty())
    {
        return Err("company_actor_required");
    }
    if !context.project_trusted {
        return Err("project_untrusted");
    }
    if write
        && matches!(
            context.permission_profile,
            PermissionProfile::Safe | PermissionProfile::Ask
        )
    {
        return Err("company_write_requires_non_safe_profile");
    }
    let role = RoleSpec::lookup(&context.role_id).ok_or("role_unknown")?;
    if role.department_id != context.department_id {
        return Err("role_department_mismatch");
    }
    Ok(())
}

pub(crate) fn company_context_for_read(context: &RequestContext) -> Result<(), &'static str> {
    company_context(context, false)
}

/// Revalidate a server-resolved assignment at every Company authority boundary.
///
/// A `ResolvedAssignment` is a short-lived snapshot, not a grant that can be copied between
/// requests. Callers should resolve it again from [`kiana_ports::AssignmentDirectoryPort`] before
/// invoking this helper after a Continue, approval consumption or effect dispatch.
pub fn validate_company_assignment(
    context: &RequestContext,
    assignment: &kiana_domain::ResolvedAssignment,
    now_unix_ms: u64,
    write: bool,
) -> Result<(), &'static str> {
    company_context(context, write)?;
    assignment.validate().map_err(|_| "assignment_invalid")?;
    if assignment.principal.principal_id != context.actor_id.as_deref().unwrap_or_default() {
        return Err("assignment_actor_mismatch");
    }
    if assignment.role_id != context.role_id || assignment.department_id != context.department_id {
        return Err("assignment_role_mismatch");
    }
    if now_unix_ms < assignment.valid_from_unix_ms
        || now_unix_ms >= assignment.expires_at_unix_ms
        || now_unix_ms < assignment.resolved_at_unix_ms
        || now_unix_ms >= assignment.principal.expires_at_unix_ms
    {
        return Err("assignment_expired_or_missing");
    }
    Ok(())
}

fn company_now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|v| v.as_millis().min(u128::from(u64::MAX)) as u64)
        .unwrap_or(0)
}
fn company_root(context: &RequestContext) -> String {
    ControlPlane::canonical_project_root(&context.project_root)
        .to_string_lossy()
        .into_owned()
}
pub(crate) fn company_aggregate_id(context: &RequestContext) -> String {
    format!(
        "{}\n{}",
        context.actor_id.as_deref().unwrap_or_default(),
        company_root(context)
    )
}

fn company_dispatch_kind(command: &CompanyCommand) -> Option<&'static str> {
    match command {
        CompanyCommand::StartRun { .. } => Some("company.start_run"),
        CompanyCommand::Business { action, .. } => match action.as_ref() {
            CompanyBusinessAction::Closeout { action, .. } => match action.as_ref() {
                kiana_domain::BusinessCloseoutAction::DispatchDelivery { .. } => {
                    Some("company.delivery.dispatch")
                }
                kiana_domain::BusinessCloseoutAction::ReconcileDelivery { .. } => {
                    Some("company.delivery.reconcile")
                }
                kiana_domain::BusinessCloseoutAction::ReconcileCancel { .. } => {
                    Some("company.cancel.reconcile")
                }
                _ => None,
            },
            _ => None,
        },
        _ => None,
    }
}

fn company_error(reason: &str) -> CoreError {
    CoreError::Port(PortError::Failed(reason.to_owned()))
}
fn company_conflict(reason: &str) -> PortError {
    PortError::Conflict(reason.to_owned())
}

pub(crate) fn company_event_owned(
    event: &RuntimeEvent,
    context: &RequestContext,
    all: &[RuntimeEvent],
) -> bool {
    let actor = context.actor_id.as_deref().unwrap_or_default();
    let root = company_root(context);
    if event.aggregate_type.as_deref() == Some(COMPANY_AGGREGATE) {
        return event.aggregate_id.as_deref() == Some(company_aggregate_id(context).as_str());
    }
    let associated_run = event
        .data
        .get("run_id")
        .and_then(Value::as_str)
        .or_else(|| {
            (event.aggregate_type.as_deref() == Some("run"))
                .then_some(event.aggregate_id.as_deref())
                .flatten()
        });
    if let Some(run) = associated_run {
        if all.iter().any(|identity| {
            identity.kind == "run.authorized"
                && identity.data.get("run_id").and_then(Value::as_str) == Some(run)
                && identity.data.get("actor_id").and_then(Value::as_str) == Some(actor)
                && identity
                    .data
                    .get("project_root")
                    .and_then(Value::as_str)
                    .is_some_and(|value| {
                        ControlPlane::canonical_project_root(value).to_string_lossy() == root
                    })
        }) {
            return true;
        }
    }
    let identities = all.iter().filter(|e| {
        e.request_id == event.request_id
            && matches!(e.kind.as_str(), "run.authorized" | "request.accepted")
    });
    identities.into_iter().any(|identity| {
        identity.data.get("actor_id").and_then(Value::as_str) == Some(actor)
            && identity
                .data
                .get("project_root")
                .and_then(Value::as_str)
                .is_some_and(|value| {
                    ControlPlane::canonical_project_root(value).to_string_lossy() == root
                })
    })
}

pub(crate) fn observe_company_run(
    reservation: &CompanyRun,
    context: &RequestContext,
    all: &[RuntimeEvent],
) -> Result<CompanyRun, PortError> {
    let request_events = all
        .iter()
        .filter(|e| e.request_id == reservation.execution_request_id)
        .collect::<Vec<_>>();
    if request_events.is_empty() {
        return Err(company_conflict("company_run_reconciliation_unavailable"));
    }
    let identity = request_events.iter().find(|e| e.kind == "run.authorized");
    let mut observed = reservation.clone();
    if let Some(identity) = identity {
        if identity.data.get("session_id").and_then(Value::as_str)
            != Some(reservation.author_session_id.as_str())
            || identity.data.get("role_id").and_then(Value::as_str) != Some("builder")
            || !company_event_owned(identity, context, all)
        {
            return Err(company_conflict("company_run_owner_mismatch"));
        }
        let run_id = identity
            .data
            .get("run_id")
            .and_then(Value::as_str)
            .and_then(RunId::parse_str)
            .ok_or_else(|| company_conflict("company_run_id_missing"))?;
        let events = super::receipts::filter_run_events(all, run_id);
        let projection = project_run_state(run_id, &events)
            .map_err(|_| company_conflict("company_run_terminal_conflict"))?;
        observed.run_id = Some(run_id);
        observed.status = match projection.outcome {
            Some(RunOutcome::Completed) => ExecutionStatus::Completed,
            Some(RunOutcome::Failed) => ExecutionStatus::Failed,
            Some(RunOutcome::Cancelled) => ExecutionStatus::Cancelled,
            Some(RunOutcome::ResultUnknown) => ExecutionStatus::ResultUnknown,
            None if projection.phase == RunPhase::AwaitingApproval => {
                ExecutionStatus::AwaitingApproval
            }
            None => ExecutionStatus::Running,
        };
        observed.evidence_refs = events
            .iter()
            .map(|e| format!("event:{}", e.event_id))
            .collect();
    } else {
        observed.status = if request_events.iter().any(|e| e.kind == "run.rejected") {
            ExecutionStatus::Blocked
        } else {
            ExecutionStatus::ResultUnknown
        };
        observed.evidence_refs = request_events
            .iter()
            .map(|e| format!("event:{}", e.event_id))
            .collect();
    }
    observed.runtime_receipt = if observed.status.is_terminal() {
        Some(
            RuntimeReceiptRef::new(
                observed.execution_request_id,
                observed.status,
                observed.evidence_refs.clone(),
            )
            .map_err(|_| company_conflict("company_runtime_receipt_invalid"))?,
        )
    } else {
        None
    };
    Ok(observed)
}

fn company_references(state: &CompanyState, c: &CompanyCommand) -> Vec<String> {
    if let CompanyCommand::Business { action, .. } = c {
        let mut references = action.references();
        if let kiana_domain::CompanyBusinessAction::ApproveCharter { project_id, .. } =
            action.as_ref()
        {
            if let Some(project) = state.projects.get(project_id) {
                references.push(project.charter_ref.clone());
            }
        }
        return references;
    }
    if let CompanyCommand::ReviewPacket { evidence_refs, .. } = c {
        return evidence_refs.clone();
    }
    let mut refs = match c {
        CompanyCommand::ApproveProject {
            project_id,
            decision_ref,
        }
        | CompanyCommand::RejectProject {
            project_id,
            decision_ref,
        } => {
            let mut refs = vec![decision_ref.clone()];
            if let Some(p) = state.projects.get(project_id) {
                refs.push(p.charter_ref.clone());
            }
            refs
        }
        CompanyCommand::RequestAcceptance { evidence_refs, .. }
        | CompanyCommand::RecordReview { evidence_refs, .. }
        | CompanyCommand::Deliver { evidence_refs, .. }
        | CompanyCommand::ReconcileDelivery { evidence_refs, .. }
        | CompanyCommand::CloseProject { evidence_refs, .. }
        | CompanyCommand::AdvanceIncident { evidence_refs, .. }
        | CompanyCommand::FailProject { evidence_refs, .. } => evidence_refs.clone(),
        CompanyCommand::PrepareDelivery { delivery } => delivery.artifact_refs.clone(),
        CompanyCommand::ConfirmDelivery {
            handoff_receipt_ref,
            ..
        } => vec![handoff_receipt_ref.clone()],
        CompanyCommand::DecideAcceptance { waiver_ref, .. } => waiver_ref.iter().cloned().collect(),
        CompanyCommand::DecideChange { decision_ref, .. } => vec![decision_ref.clone()],
        // CO-11: the coverage approval must cite a verifiable planning/review record, so the
        // proof resolves the same typed reference the transition re-checks.
        CompanyCommand::FreezeCriterionCoverage { coverage } => {
            vec![coverage.approval.decision_ref.clone()]
        }
        CompanyCommand::RecordOutcome { observation, .. } => observation.evidence_refs.clone(),
        CompanyCommand::OpenIncident { incident }
        | CompanyCommand::MarkDeliveryUnknown { incident, .. } => incident.evidence_refs.clone(),
        CompanyCommand::ConfirmCancelProject {
            incident: Some(incident),
            ..
        } => incident.evidence_refs.clone(),
        _ => Vec::new(),
    };
    match c {
        CompanyCommand::RecordReview { acceptance_id, .. }
        | CompanyCommand::DecideAcceptance { acceptance_id, .. } => {
            if let Some(acceptance) = state.acceptances.get(acceptance_id) {
                refs.extend(acceptance.evidence_refs.clone());
            }
        }
        CompanyCommand::ConfirmDelivery { delivery_id, .. }
        | CompanyCommand::CloseProject { delivery_id, .. }
        | CompanyCommand::ReconcileDelivery { delivery_id, .. } => {
            if let Some(delivery) = state.deliveries.get(delivery_id) {
                refs.extend(delivery.artifact_refs.clone());
            }
        }
        _ => {}
    }
    refs.sort();
    refs.dedup();
    refs
}

async fn read_company_artifact(
    context: &RequestContext,
    relative: &str,
) -> Result<String, PortError> {
    if relative.is_empty()
        || !Path::new(relative)
            .components()
            .all(|c| matches!(c, Component::Normal(_)))
    {
        return Err(company_conflict("company_artifact_path_invalid"));
    }
    let root = company_root(context);
    let relative = relative.to_owned();
    let text = tokio::task::spawn_blocking(move || {
        super::artifacts::read_project_artifact(
            Path::new(&root),
            Path::new(&relative),
            "company_artifact_read_failed",
        )
    })
    .await
    .map_err(|_| PortError::Failed("company_artifact_read_join_failed".to_owned()))?
    .map_err(company_conflict)?;
    if text.is_empty() || text.len() > 65_536 {
        return Err(company_conflict("company_artifact_empty_or_too_large"));
    }
    if super::redaction::redact_event_value(&json!(text)) != json!(text) {
        return Err(company_conflict("company_artifact_sensitive_content"));
    }
    Ok(text)
}

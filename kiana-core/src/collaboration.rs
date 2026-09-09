use super::artifacts::*;
use super::events::*;
use super::lifecycle::*;
use super::receipts::*;
use super::*;

impl ControlPlane {
    pub async fn spawn_from_packet(
        &self,
        mut context: RequestContext,
        mut packet: WorkPacket,
        sandbox: Option<String>,
    ) -> Result<CoreResponse, CoreError> {
        let request_id = context.request_id;
        if let Err(reason) = packet.validate() {
            self.append_event(
                request_id,
                1,
                "run.rejected",
                json!({ "reason": reason, "command": "run.spawn" }),
            )
            .await?;
            return Ok(CoreResponse::blocked(request_id, reason));
        }
        if self.session_known(context.session_id.as_str()) {
            self.append_event(
                request_id,
                1,
                "run.rejected",
                json!({
                    "reason": "spawn_session_not_fresh",
                    "command": "run.spawn",
                    "session_id": context.session_id,
                }),
            )
            .await?;
            return Ok(CoreResponse::blocked(request_id, "spawn_session_not_fresh"));
        }
        if !matches!(
            packet.status,
            kiana_domain::WorkPacketStatus::Draft
                | kiana_domain::WorkPacketStatus::Approved
                | kiana_domain::WorkPacketStatus::Assigned
        ) {
            self.append_event(
                request_id,
                1,
                "run.rejected",
                json!({
                    "reason": "packet_status_not_spawnable",
                    "command": "run.spawn",
                    "packet_id": &packet.id,
                    "status": packet.status,
                }),
            )
            .await?;
            return Ok(CoreResponse::blocked(
                request_id,
                "packet_status_not_spawnable",
            ));
        }
        context.assign_role(&RoleSpec::builder());
        context.work_packet_id = Some(packet.id.clone());
        context.path_allow = packet.path_allow.clone();
        let session_id = context.session_id.as_str().to_owned();
        let project_root = context.project_root.clone();
        if let Err(reason) =
            self.acquire_builder_path_locks(&project_root, &session_id, &context.path_allow)
        {
            self.append_event(
                request_id,
                1,
                "run.rejected",
                json!({ "reason": reason, "command": "run.spawn", "session_id": session_id }),
            )
            .await?;
            return Ok(CoreResponse::blocked(request_id, reason));
        }
        let _path_lock_guard = BuilderPathLockGuard {
            control_plane: self,
            project_root: project_root.clone(),
            session_id: session_id.clone(),
        };
        let run_id = RunId::new();
        let admission = match self.reserve_packet_cell(&context, &packet, run_id).await {
            Ok(admission) => admission,
            Err(error) => {
                let error = spawn_error_reason(&error);
                self.append_event(
                    request_id,
                    1,
                    "run.rejected",
                    json!({
                        "reason": &error,
                        "command": "run.spawn",
                        "packet_id": &packet.id,
                    }),
                )
                .await?;
                return Ok(CoreResponse::blocked(request_id, error));
            }
        };
        let mut context = context;
        context.cell_id = Some(admission.cell.cell_id);
        packet.owner_cell_id = Some(admission.cell.cell_id);
        packet.budget_lease_id = Some(admission.budget.lease_id);
        packet.acceptor_id = context.actor_id.clone();
        let mut packet_sequence = 1u64;
        self.record_event(
            request_id,
            &mut packet_sequence,
            "cell.validated",
            cell_event_payload(&admission, "validated"),
        )
        .await?;
        let admission = match self
            .cell_registry
            .commit_spawn(admission.plan.plan_id)
            .await
        {
            Ok(admission) => admission,
            Err(error) => {
                let _ = self
                    .cell_registry
                    .abort_spawn(admission.plan.plan_id, "spawn_commit_failed")
                    .await;
                return Ok(CoreResponse::blocked(request_id, error.to_string()));
            }
        };
        self.record_event(
            request_id,
            &mut packet_sequence,
            "spawn.committed",
            spawn_event_payload(&admission),
        )
        .await?;
        let admission = match self
            .cell_registry
            .transition_cell(
                admission.cell.cell_id,
                CellLifecycle::Ready,
                CellLifecycle::Running,
            )
            .await
        {
            Ok(cell) => {
                let mut admission = admission;
                admission.cell = cell;
                admission
            }
            Err(error) => {
                let _ = self
                    .cell_registry
                    .abort_spawn(admission.plan.plan_id, "cell_start_failed")
                    .await;
                return Ok(CoreResponse::blocked(request_id, error.to_string()));
            }
        };
        self.record_event(
            request_id,
            &mut packet_sequence,
            "cell.started",
            cell_event_payload(&admission, "running"),
        )
        .await?;
        if packet.status == kiana_domain::WorkPacketStatus::Draft {
            packet.transition_status(kiana_domain::WorkPacketStatus::Approved)?;
            self.record_event(
                request_id,
                &mut packet_sequence,
                "packet.approved",
                json!({
                    "packet_id": &packet.id,
                    "status": packet.status,
                    "actor_id": context.actor_id,
                }),
            )
            .await?;
        }
        if packet.status == kiana_domain::WorkPacketStatus::Approved {
            packet.transition_status(kiana_domain::WorkPacketStatus::Assigned)?;
            self.record_event(
                request_id,
                &mut packet_sequence,
                "packet.assigned",
                json!({
                    "packet_id": &packet.id,
                    "status": packet.status,
                    "assignee_role": &packet.assignee_role,
                }),
            )
            .await?;
        }
        if packet.status == kiana_domain::WorkPacketStatus::Assigned {
            packet.transition_status(kiana_domain::WorkPacketStatus::Running)?;
            self.record_event(
                request_id,
                &mut packet_sequence,
                "packet.accepted",
                json!({
                    "packet_id": &packet.id,
                    "status": packet.status,
                    "acceptor_id": context.actor_id,
                }),
            )
            .await?;
        }
        let response = self
            .start_run_with_id(
                context.clone(),
                packet.as_prompt(),
                Vec::new(),
                sandbox,
                Some(run_id),
            )
            .await;
        let mut response = match response {
            Ok(response) => response,
            Err(error) => {
                let _ = self
                    .cell_registry
                    .abort_spawn(admission.plan.plan_id, "runner_start_failed")
                    .await;
                return Err(error);
            }
        };
        let cell_lifecycle = match response.status {
            ExecutionStatus::Completed => {
                Some((CellLifecycle::Running, CellLifecycle::ReadyToMerge))
            }
            ExecutionStatus::AwaitingApproval => {
                Some((CellLifecycle::Running, CellLifecycle::WaitingInput))
            }
            ExecutionStatus::Blocked | ExecutionStatus::Denied | ExecutionStatus::Failed => {
                Some((CellLifecycle::Running, CellLifecycle::Failed))
            }
            ExecutionStatus::Cancelled => {
                Some((CellLifecycle::Running, CellLifecycle::CancelRequested))
            }
            ExecutionStatus::ResultUnknown => {
                Some((CellLifecycle::Running, CellLifecycle::Quarantined))
            }
            ExecutionStatus::Accepted | ExecutionStatus::Running => None,
        };
        if let Some((expected, next)) = cell_lifecycle {
            let lifecycle = self
                .cell_registry
                .transition_cell(admission.cell.cell_id, expected, next)
                .await;
            match lifecycle {
                Ok(lifecycle) => {
                    let mut lifecycle_admission = admission.clone();
                    lifecycle_admission.cell = lifecycle.clone();
                    self.record_event(
                        request_id,
                        &mut packet_sequence,
                        cell_event_kind(lifecycle.lifecycle),
                        cell_event_payload(&lifecycle_admission, lifecycle.lifecycle.as_str()),
                    )
                    .await?;
                    let lifecycle = if response.status == ExecutionStatus::Cancelled {
                        let cancelled = self
                            .cell_registry
                            .transition_cell(
                                admission.cell.cell_id,
                                CellLifecycle::CancelRequested,
                                CellLifecycle::Cancelled,
                            )
                            .await?;
                        let mut cancelled_admission = admission.clone();
                        cancelled_admission.cell = cancelled.clone();
                        self.record_event(
                            request_id,
                            &mut packet_sequence,
                            "cell.cancelled",
                            cell_event_payload(&cancelled_admission, cancelled.lifecycle.as_str()),
                        )
                        .await?;
                        cancelled
                    } else {
                        lifecycle
                    };
                    if matches!(
                        lifecycle.lifecycle,
                        CellLifecycle::ReadyToMerge
                            | CellLifecycle::Failed
                            | CellLifecycle::Quarantined
                            | CellLifecycle::Cancelled
                    ) {
                        let retirement = self
                            .cell_registry
                            .retire_cell(admission.cell.cell_id, "packet_run_terminal")
                            .await;
                        match retirement {
                            Ok(retirement) => {
                                self.record_event(
                                    request_id,
                                    &mut packet_sequence,
                                    "cell.retired",
                                    retirement_event_payload(&retirement),
                                )
                                .await?;
                            }
                            Err(error) => {
                                return Ok(CoreResponse {
                                    request_id,
                                    status: ExecutionStatus::ResultUnknown,
                                    output: run_identity(&context, run_id, "read-only"),
                                    error: Some(format!("cell_retirement_failed:{error}")),
                                });
                            }
                        }
                    }
                }
                Err(error) => {
                    return Ok(CoreResponse {
                        request_id,
                        status: ExecutionStatus::ResultUnknown,
                        output: run_identity(&context, run_id, "read-only"),
                        error: Some(format!("cell_transition_failed:{error}")),
                    });
                }
            }
        }
        let next_status = match response.status {
            ExecutionStatus::Completed => kiana_domain::WorkPacketStatus::Succeeded,
            ExecutionStatus::AwaitingApproval => kiana_domain::WorkPacketStatus::AwaitingApproval,
            ExecutionStatus::Blocked => kiana_domain::WorkPacketStatus::Blocked,
            ExecutionStatus::Cancelled => kiana_domain::WorkPacketStatus::Cancelled,
            ExecutionStatus::Denied | ExecutionStatus::Failed | ExecutionStatus::ResultUnknown => {
                kiana_domain::WorkPacketStatus::Failed
            }
            ExecutionStatus::Accepted | ExecutionStatus::Running => packet.status,
        };
        if next_status != packet.status {
            packet.transition_status(next_status)?;
            let event_kind = match next_status {
                kiana_domain::WorkPacketStatus::Succeeded => "packet.succeeded",
                kiana_domain::WorkPacketStatus::AwaitingApproval => "packet.awaiting_approval",
                kiana_domain::WorkPacketStatus::Blocked => "packet.blocked",
                kiana_domain::WorkPacketStatus::Cancelled => "packet.cancelled",
                _ => "packet.failed",
            };
            self.record_event(
                request_id,
                &mut packet_sequence,
                event_kind,
                json!({
                    "packet_id": &packet.id,
                    "status": packet.status,
                    "run_id": response.output.get("run_id"),
                }),
            )
            .await?;
        }
        if let Some(output) = response.output.as_object_mut() {
            output.insert("packet_status".to_owned(), json!(packet.status));
        }
        Ok(response)
    }

    pub(crate) async fn reserve_packet_cell(
        &self,
        context: &RequestContext,
        packet: &WorkPacket,
        run_id: RunId,
    ) -> Result<kiana_ports::SpawnReservation, CoreError> {
        let template = self
            .cell_registry
            .resolve_template(ROLE_BUILDER, cell_registry::TEMPLATE_VERSION)
            .await?;
        let now = unix_ms();
        let deadline = packet
            .deadline_unix_ms
            .unwrap_or_else(|| now.saturating_add(template.ttl_seconds.saturating_mul(1_000)));
        if deadline <= now {
            return Err(CoreError::Port(PortError::Failed(
                "spawn_deadline_expired".to_owned(),
            )));
        }
        let grant_paths = if packet.path_allow.is_empty() {
            vec![".".to_owned()]
        } else {
            packet.path_allow.clone()
        };
        let budget = BudgetLease::new(
            template.estimated_cost.max(1),
            template.estimated_cost.max(1).saturating_mul(4096),
            template.ttl_seconds.saturating_mul(1_000),
            1,
            1,
        );
        let supervision = SupervisionLease {
            schema: SUPERVISION_LEASE_SCHEMA.to_owned(),
            lease_id: SupervisionLeaseId::new(),
            heartbeat_interval_seconds: template.heartbeat_interval_seconds,
            stall_threshold_seconds: template.ttl_seconds,
            retry_limit: 0,
            retries_used: 0,
        };
        let grant = CapabilityGrant {
            schema: CAPABILITY_GRANT_SCHEMA.to_owned(),
            grant_id: CapabilityGrantId::new(),
            capability: CapabilityKind::Other("coding".to_owned()),
            operation: "builder.packet".to_owned(),
            resources: vec!["workspace".to_owned()],
            paths: grant_paths,
            expires_at_unix_ms: deadline,
            approval_id: None,
            delegation_allowed: false,
        };
        let plan = SpawnPlan {
            schema: SPAWN_PLAN_SCHEMA.to_owned(),
            plan_id: SpawnPlanId::new(),
            parent_cell_id: None,
            reason_code: "packet_spawn".to_owned(),
            candidate_templates: vec![template.template_id],
            count: 1,
            partition: packet.id.clone(),
            input_refs: packet.inputs.clone(),
            output_contract: RUN_RESULT_SCHEMA.to_owned(),
            requested_capabilities: template.default_capabilities.clone(),
            budget_reservation: 1,
            deadline_unix_ms: deadline,
            rollback_policy: "abort".to_owned(),
            idempotency_key: format!("packet:{}:{}", context.project_root, packet.id),
            expected_utility: 0,
            status: SpawnPlanStatus::Proposed,
        };
        let cell = CellSpec {
            schema: CELL_SCHEMA.to_owned(),
            cell_id: CellId::new(),
            parent_cell_id: None,
            root_run_id: run_id,
            template_id: template.template_id,
            template_version: template.version.clone(),
            role_id: ROLE_BUILDER.to_owned(),
            objective: packet.goal.clone(),
            input_refs: packet.inputs.clone(),
            output_contract: plan.output_contract.clone(),
            partition_key: plan.partition.clone(),
            owned_paths: packet.path_allow.clone(),
            work_packet_id: Some(packet.id.clone()),
            owner_actor_id: context.actor_id.clone(),
            capability_grant_id: grant.grant_id,
            budget_lease_id: budget.lease_id,
            supervision_lease_id: supervision.lease_id,
            depth: 0,
            spawn_quota: template.max_children,
            lifecycle: CellLifecycle::Proposed,
        };
        let fingerprint = WorkFingerprint::from_parts(
            &packet.goal,
            &packet.inputs,
            &plan.partition,
            &plan.output_contract,
            "kiana.policy.v1",
        )
        .map_err(|reason| CoreError::Port(PortError::Failed(reason.to_owned())))?;
        Ok(self
            .cell_registry
            .reserve_spawn(SpawnReservationRequest {
                plan,
                cell,
                template,
                budget,
                grant,
                supervision,
                fingerprint,
                owned_paths: packet.path_allow.clone(),
            })
            .await?)
    }

    pub async fn review_author_run(
        &self,
        mut context: RequestContext,
        author_session_id: String,
        author_run_id: Option<RunId>,
    ) -> Result<CoreResponse, CoreError> {
        let request_id = context.request_id;
        let mut sequence = 1u64;
        self.record_event(
            request_id,
            &mut sequence,
            "request.accepted",
            json!({ "command": "run.review" }),
        )
        .await?;

        let author_session_id = author_session_id.trim().to_owned();
        if author_session_id.is_empty() {
            self.record_event(
                request_id,
                &mut sequence,
                "run.rejected",
                json!({ "reason": "review_author_required" }),
            )
            .await?;
            return Ok(CoreResponse::blocked(request_id, "review_author_required"));
        }

        let Some(reviewer) = RoleSpec::lookup(&context.role_id) else {
            self.record_event(
                request_id,
                &mut sequence,
                "run.rejected",
                json!({ "reason": "role_unknown" }),
            )
            .await?;
            return Ok(CoreResponse::blocked(request_id, "role_unknown"));
        };
        if reviewer.role_id != ROLE_REVIEWER {
            self.record_event(
                request_id,
                &mut sequence,
                "run.rejected",
                json!({ "reason": "review_role_must_be_reviewer" }),
            )
            .await?;
            return Ok(CoreResponse::blocked(
                request_id,
                "review_role_must_be_reviewer",
            ));
        }
        context.assign_role(&RoleSpec::reviewer());

        if context.session_id.as_str() == author_session_id {
            self.record_event(
                request_id,
                &mut sequence,
                "run.rejected",
                json!({
                    "reason": "review_author_session_denied",
                    "author_session_id": author_session_id,
                    "reviewer_session_id": context.session_id,
                }),
            )
            .await?;
            return Ok(CoreResponse::blocked(
                request_id,
                "review_author_session_denied",
            ));
        }
        if self.session_known(context.session_id.as_str()) {
            self.record_event(
                request_id,
                &mut sequence,
                "run.rejected",
                json!({ "reason": "review_session_not_fresh" }),
            )
            .await?;
            return Ok(CoreResponse::blocked(
                request_id,
                "review_session_not_fresh",
            ));
        }

        let events = self
            .events_for_author(&context, &author_session_id, author_run_id)
            .await?;
        if events.is_empty() || !events.iter().any(|event| event.kind == "run.completed") {
            self.record_event(
                request_id,
                &mut sequence,
                "run.rejected",
                json!({ "reason": "review_author_not_found" }),
            )
            .await?;
            return Ok(CoreResponse::blocked(request_id, "review_author_not_found"));
        }

        let author_role = events
            .iter()
            .rev()
            .find_map(|event| event.data.get("role_id").and_then(Value::as_str))
            .unwrap_or_default()
            .to_owned();
        if author_role != ROLE_BUILDER {
            self.record_event(
                request_id,
                &mut sequence,
                "run.rejected",
                json!({
                    "reason": "review_author_must_be_builder",
                    "author_role_id": author_role,
                }),
            )
            .await?;
            return Ok(CoreResponse::blocked(
                request_id,
                "review_author_must_be_builder",
            ));
        }

        let author_run_id = author_run_id
            .or_else(|| {
                events.iter().find_map(|event| {
                    (event.kind == "run.authorized")
                        .then(|| event.data.get("run_id"))
                        .flatten()
                        .and_then(Value::as_str)
                        .and_then(RunId::parse_str)
                })
            })
            .ok_or_else(|| {
                CoreError::Port(PortError::Failed("review_author_run_missing".to_owned()))
            })?;
        let files = files_changed_from_events(&events);
        let verdict = if files.is_empty() {
            "needs_change"
        } else {
            "pass"
        };
        let summary = if files.is_empty() {
            "author produced no files_changed"
        } else {
            "author files reviewed against builder receipt"
        };
        let packet = ReviewPacket::closed(
            format!("rv-{}", context.session_id),
            author_session_id.clone(),
            ROLE_BUILDER,
            context.session_id.to_string(),
            verdict,
            summary,
            files.clone(),
        );
        if let Err(reason) = packet.validate() {
            self.record_event(
                request_id,
                &mut sequence,
                "run.rejected",
                json!({ "reason": reason }),
            )
            .await?;
            return Ok(CoreResponse::blocked(request_id, reason));
        }
        if let Err(reason) = write_review_artifact(&context.project_root, &packet) {
            self.record_event(
                request_id,
                &mut sequence,
                "run.rejected",
                json!({ "reason": reason }),
            )
            .await?;
            return Ok(CoreResponse::blocked(request_id, reason));
        }
        let reviewer_session_id = kiana_domain::SessionId::new(context.session_id.as_str());
        let merge_receipt = if verdict == "pass" {
            let receipt = MergeReceipt {
                schema: kiana_domain::MERGE_RECEIPT_SCHEMA.to_owned(),
                receipt_id: kiana_domain::ReceiptId::new(),
                author_run_id,
                author_session_id: kiana_domain::SessionId::new(author_session_id.clone()),
                reviewer_session_id: reviewer_session_id.clone(),
                reviewer_verdict: verdict.to_owned(),
                files: files.clone(),
                accepted: true,
                provenance: vec![REVIEW_PACKET_PATH.to_owned(), "run.completed".to_owned()],
            };
            receipt
                .validate()
                .map_err(|reason| CoreError::Port(PortError::Failed(reason.to_owned())))?;
            write_merge_artifact(&context.project_root, &receipt)
                .map_err(|reason| CoreError::Port(PortError::Failed(reason.to_owned())))?;
            Some(receipt)
        } else {
            None
        };

        let run_id = RunId::parse_str(context.session_id.as_str()).unwrap_or_else(RunId::new);
        self.remember_session(&context, run_id);
        self.record_event(
            request_id,
            &mut sequence,
            "review.closed",
            json!({
                "author_session_id": author_session_id,
                "reviewer_session_id": context.session_id,
                "verdict": verdict,
                "builder_present": false,
                "merge_receipt_id": merge_receipt.as_ref().map(|receipt| receipt.receipt_id),
            }),
        )
        .await?;

        Ok(CoreResponse::completed(
            request_id,
            json!({
                "schema": REVIEW_RESULT_SCHEMA,
                "harness": HARNESS_ID,
                "run_id": run_id,
                "session_id": context.session_id,
                "role_id": ROLE_REVIEWER,
                "department_id": reviewer.department_id,
                "author_session_id": author_session_id,
                "author_role_id": ROLE_BUILDER,
                "input": "review",
                "verdict": verdict,
                "files_reviewed": files,
                "review_path": REVIEW_PACKET_PATH,
                "merge_path": merge_receipt.as_ref().map(|_| kiana_domain::MERGE_RECEIPT_PATH),
                "packet": packet,
                "merge_receipt": merge_receipt,
            }),
        ))
    }

    pub async fn close_author_run(
        &self,
        mut context: RequestContext,
        author_session_id: String,
        author_run_id: Option<RunId>,
    ) -> Result<CoreResponse, CoreError> {
        let request_id = context.request_id;
        let mut sequence = 1u64;
        self.record_event(
            request_id,
            &mut sequence,
            "request.accepted",
            json!({ "command": "run.close" }),
        )
        .await?;

        let author_session_id = author_session_id.trim().to_owned();
        if author_session_id.is_empty() {
            self.record_event(
                request_id,
                &mut sequence,
                "run.rejected",
                json!({ "reason": "close_author_required" }),
            )
            .await?;
            return Ok(CoreResponse::blocked(request_id, "close_author_required"));
        }
        let Some(closer) = RoleSpec::lookup(&context.role_id) else {
            self.record_event(
                request_id,
                &mut sequence,
                "run.rejected",
                json!({ "reason": "role_unknown" }),
            )
            .await?;
            return Ok(CoreResponse::blocked(request_id, "role_unknown"));
        };
        if closer.role_id != ROLE_CLOSER {
            self.record_event(
                request_id,
                &mut sequence,
                "run.rejected",
                json!({ "reason": "close_role_must_be_closer" }),
            )
            .await?;
            return Ok(CoreResponse::blocked(
                request_id,
                "close_role_must_be_closer",
            ));
        }
        context.assign_role(&RoleSpec::closer());
        if context.session_id.as_str() == author_session_id {
            self.record_event(
                request_id,
                &mut sequence,
                "run.rejected",
                json!({ "reason": "close_author_session_denied" }),
            )
            .await?;
            return Ok(CoreResponse::blocked(
                request_id,
                "close_author_session_denied",
            ));
        }
        if self.session_known(context.session_id.as_str()) {
            self.record_event(
                request_id,
                &mut sequence,
                "run.rejected",
                json!({ "reason": "close_session_not_fresh" }),
            )
            .await?;
            return Ok(CoreResponse::blocked(request_id, "close_session_not_fresh"));
        }

        let author_events = self
            .events_for_author(&context, &author_session_id, author_run_id)
            .await?;
        let Some(author_run_id) = author_run_id.or_else(|| {
            author_events.iter().find_map(|event| {
                (event.kind == "run.authorized")
                    .then(|| event.data.get("run_id"))
                    .flatten()
                    .and_then(Value::as_str)
                    .and_then(RunId::parse_str)
            })
        }) else {
            self.record_event(
                request_id,
                &mut sequence,
                "run.rejected",
                json!({ "reason": "close_author_not_found" }),
            )
            .await?;
            return Ok(CoreResponse::blocked(request_id, "close_author_not_found"));
        };
        if author_events.is_empty()
            || !author_events
                .iter()
                .any(|event| event.kind == "run.completed")
        {
            self.record_event(
                request_id,
                &mut sequence,
                "run.rejected",
                json!({ "reason": "close_author_not_completed" }),
            )
            .await?;
            return Ok(CoreResponse::blocked(
                request_id,
                "close_author_not_completed",
            ));
        }
        let review = match read_review_artifact(&context.project_root) {
            Ok(review) => review,
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
        if review.author_session_id != author_session_id {
            self.record_event(
                request_id,
                &mut sequence,
                "run.rejected",
                json!({ "reason": "close_review_author_mismatch" }),
            )
            .await?;
            return Ok(CoreResponse::blocked(
                request_id,
                "close_review_author_mismatch",
            ));
        }
        if review.author_role_id != ROLE_BUILDER || review.verdict != "pass" {
            self.record_event(
                request_id,
                &mut sequence,
                "run.rejected",
                json!({ "reason": "close_review_not_accepted" }),
            )
            .await?;
            return Ok(CoreResponse::blocked(
                request_id,
                "close_review_not_accepted",
            ));
        }
        let merge = match read_merge_artifact(&context.project_root) {
            Ok(merge) => merge,
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
        if merge.author_run_id != author_run_id
            || merge.author_session_id.as_str() != author_session_id
            || merge.reviewer_session_id.as_str() != review.reviewer_session_id
            || merge.files != review.files_reviewed
            || !merge.accepted
        {
            self.record_event(
                request_id,
                &mut sequence,
                "run.rejected",
                json!({ "reason": "close_merge_receipt_mismatch" }),
            )
            .await?;
            return Ok(CoreResponse::blocked(
                request_id,
                "close_merge_receipt_mismatch",
            ));
        }
        let reviewer_session_id = kiana_domain::SessionId::new(review.reviewer_session_id.clone());
        let receipt = ClosingReceipt {
            schema: kiana_domain::CLOSING_RECEIPT_SCHEMA.to_owned(),
            receipt_id: kiana_domain::ReceiptId::new(),
            project_id: None,
            author_run_id,
            author_session_id: kiana_domain::SessionId::new(author_session_id.clone()),
            reviewer_session_id: reviewer_session_id.clone(),
            closer_session_id: context.session_id.clone(),
            review_id: review.id.clone(),
            verdict: review.verdict.clone(),
            accepted: true,
            files_verified: review.files_reviewed.clone(),
            exceptions: Vec::new(),
            lessons_path: "lessons/LEARNED.md".to_owned(),
        };
        if let Err(reason) = receipt.validate() {
            self.record_event(
                request_id,
                &mut sequence,
                "run.rejected",
                json!({ "reason": reason }),
            )
            .await?;
            return Ok(CoreResponse::blocked(request_id, reason));
        }
        if let Err(reason) = write_closing_artifact(&context.project_root, &receipt) {
            self.record_event(
                request_id,
                &mut sequence,
                "run.rejected",
                json!({ "reason": reason }),
            )
            .await?;
            return Ok(CoreResponse::blocked(request_id, reason));
        }
        self.remember_session(&context, RunId::new());
        self.record_event(
            request_id,
            &mut sequence,
            "closing.completed",
            json!({
                "receipt_id": receipt.receipt_id,
                "author_run_id": author_run_id,
                "author_session_id": author_session_id,
                "reviewer_session_id": reviewer_session_id,
                "closer_session_id": context.session_id,
                "review_id": review.id,
                "files_verified": receipt.files_verified,
            }),
        )
        .await?;
        Ok(CoreResponse::completed(
            request_id,
            serde_json::to_value(receipt).map_err(|error| {
                CoreError::Port(PortError::Failed(format!(
                    "close_receipt_serialize:{error}"
                )))
            })?,
        ))
    }

    pub async fn convene_symposium(
        &self,
        mut context: RequestContext,
        goal: String,
        anti_meeting: bool,
        max_rounds: Option<u32>,
        sandbox: Option<String>,
    ) -> Result<CoreResponse, CoreError> {
        let request_id = context.request_id;
        let mut sequence = 1u64;
        self.record_event(
            request_id,
            &mut sequence,
            "request.accepted",
            json!({
                "command": "run.symposium",
                "anti_meeting": anti_meeting,
            }),
        )
        .await?;

        let goal = goal.trim().to_owned();
        if goal.is_empty() {
            self.record_event(
                request_id,
                &mut sequence,
                "run.rejected",
                json!({ "reason": "symposium_goal_required" }),
            )
            .await?;
            return Ok(CoreResponse::blocked(request_id, "symposium_goal_required"));
        }

        let Some(chair) = RoleSpec::lookup(&context.role_id) else {
            self.record_event(
                request_id,
                &mut sequence,
                "run.rejected",
                json!({ "reason": "role_unknown" }),
            )
            .await?;
            return Ok(CoreResponse::blocked(request_id, "role_unknown"));
        };
        if !chair.can_convene {
            let reason = if chair.department_id == DEPARTMENT_PLANNING {
                "symposium_chair_must_be_pm"
            } else {
                "symposium_chair_cannot_convene"
            };
            self.record_event(
                request_id,
                &mut sequence,
                "run.rejected",
                json!({ "reason": reason }),
            )
            .await?;
            return Ok(CoreResponse::blocked(request_id, reason));
        }
        context.assign_role(&chair);

        let max_rounds = match Symposium::validate_max_rounds(
            max_rounds.unwrap_or(Symposium::DEFAULT_MAX_ROUNDS),
        ) {
            Ok(value) => value,
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

        if chair.department_id == DEPARTMENT_PLANNING {
            match authorized_harness_sandbox(&context, sandbox.as_deref()) {
                Ok("workspace-write") => {}
                Ok(_) => {
                    self.record_event(
                        request_id,
                        &mut sequence,
                        "run.rejected",
                        json!({ "reason": "symposium_requires_workspace_write" }),
                    )
                    .await?;
                    return Ok(CoreResponse::blocked(
                        request_id,
                        "symposium_requires_workspace_write",
                    ));
                }
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
            }
        } else if !context.project_trusted
            || matches!(context.permission_profile, PermissionProfile::Safe)
        {
            self.record_event(
                request_id,
                &mut sequence,
                "run.rejected",
                json!({ "reason": "workspace_write_requires_trusted_non_safe_profile" }),
            )
            .await?;
            return Ok(CoreResponse::blocked(
                request_id,
                "workspace_write_requires_trusted_non_safe_profile",
            ));
        }

        let mut meeting = match Symposium::department(
            &chair.department_id,
            request_id.to_string(),
            goal,
            max_rounds,
        ) {
            Ok(meeting) => meeting,
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
        if let Err(reason) = meeting.validate() {
            self.record_event(
                request_id,
                &mut sequence,
                "run.rejected",
                json!({ "reason": reason }),
            )
            .await?;
            return Ok(CoreResponse::blocked(request_id, reason));
        }

        let mut speaker_sessions = Vec::new();
        if !anti_meeting {
            let attendees = meeting.attendees.clone();
            for round in 0..max_rounds {
                for role_id in &attendees {
                    let role = RoleSpec::lookup(role_id).unwrap_or_else(RoleSpec::pm);
                    let session_id = meeting.speaker_session_id(role_id);
                    let mut speaker = context.clone();
                    speaker.request_id = RequestId::new();
                    speaker.session_id = kiana_domain::SessionId::new(session_id.clone());
                    speaker.assign_role(&role);
                    speaker.permission_profile = PermissionProfile::Safe;
                    let prompt = meeting.speaker_prompt(role_id);
                    let turn = if round == 0 {
                        self.start_run(speaker, prompt, None).await?
                    } else {
                        self.continue_run(speaker, prompt, None, None).await?
                    };
                    if turn.status != ExecutionStatus::Completed {
                        let mut failed = turn;
                        failed.request_id = request_id;
                        return Ok(failed);
                    }
                    if let Some(text) = turn
                        .output
                        .get("output")
                        .and_then(|output| output.get("text"))
                        .and_then(Value::as_str)
                        .map(str::trim)
                        .filter(|value| !value.is_empty())
                    {
                        meeting
                            .blackboard
                            .claims
                            .push(SymposiumClaim::new(role_id, text));
                    }
                    if round == 0 {
                        speaker_sessions.push(json!({
                            "role_id": role_id,
                            "session_id": session_id,
                        }));
                    }
                }
            }
        }

        let (decision, packet) = match meeting.close(anti_meeting) {
            Ok(closed) => closed,
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

        if let Err(reason) =
            write_symposium_artifacts(&context.project_root, &meeting, &decision, packet.as_ref())
        {
            self.record_event(
                request_id,
                &mut sequence,
                "run.rejected",
                json!({ "reason": reason }),
            )
            .await?;
            return Ok(CoreResponse::blocked(request_id, reason));
        }

        self.record_event(
            request_id,
            &mut sequence,
            "symposium.closed",
            json!({
                "symposium_id": meeting.id,
                "department_id": meeting.department_id,
                "skipped_meeting": anti_meeting,
                "builder_present": meeting.builder_present(),
                "decision_id": decision.id,
                "work_packet_id": packet.as_ref().map(|packet| packet.id.clone()),
            }),
        )
        .await?;

        Ok(CoreResponse::completed(
            request_id,
            json!({
                "schema": SYMPOSIUM_RESULT_SCHEMA,
                "harness": HARNESS_ID,
                "symposium_id": meeting.id,
                "department_id": meeting.department_id,
                "chair": meeting.chair,
                "attendees": meeting.attendees,
                "status": meeting.status,
                "builder_present": meeting.builder_present(),
                "skipped_meeting": anti_meeting,
                "decision": decision,
                "packet": packet,
                "decision_path": meeting.decision_path(),
                "packet_path": packet.as_ref().map(|_| WORK_PACKET_PATH),
                "speaker_sessions": speaker_sessions,
                "blackboard": meeting.blackboard,
            }),
        ))
    }
}

fn unix_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis().min(u128::from(u64::MAX)) as u64)
        .unwrap_or(0)
}
fn spawn_error_reason(error: &CoreError) -> String {
    error.to_string()
}

fn cell_event_payload(reservation: &kiana_ports::SpawnReservation, lifecycle: &str) -> Value {
    json!({
        "schema": CELL_SCHEMA,
        "cell_id": reservation.cell.cell_id,
        "plan_id": reservation.plan.plan_id,
        "run_id": reservation.cell.root_run_id,
        "role_id": reservation.cell.role_id,
        "lifecycle": lifecycle,
        "owned_paths": reservation.owned_paths,
        "capability_grant_id": reservation.grant.grant_id,
        "budget_lease_id": reservation.budget.lease_id,
        "supervision_lease_id": reservation.supervision.lease_id,
        "replayed": reservation.replayed,
    })
}

fn spawn_event_payload(reservation: &kiana_ports::SpawnReservation) -> Value {
    json!({
        "schema": SPAWN_PLAN_SCHEMA,
        "plan_id": reservation.plan.plan_id,
        "run_id": reservation.cell.root_run_id,
        "cell_id": reservation.cell.cell_id,
        "status": reservation.plan.status,
        "idempotency_key": reservation.plan.idempotency_key,
        "fingerprint": reservation.fingerprint,
        "replayed": reservation.replayed,
    })
}

fn retirement_event_payload(retirement: &kiana_domain::RetirementRecord) -> Value {
    json!({
        "schema": kiana_domain::RETIREMENT_RECORD_SCHEMA,
        "cell_id": retirement.cell_id,
        "grant_id": retirement.grant_id,
        "budget_lease_id": retirement.budget_lease_id,
        "supervision_lease_id": retirement.supervision_lease_id,
        "released_paths": retirement.released_paths,
        "reason": retirement.reason,
        "retired_at_unix_ms": retirement.retired_at_unix_ms,
    })
}

fn cell_event_kind(lifecycle: kiana_domain::CellLifecycle) -> &'static str {
    match lifecycle {
        kiana_domain::CellLifecycle::Validated => "cell.validated",
        kiana_domain::CellLifecycle::Spawning => "cell.spawning",
        kiana_domain::CellLifecycle::Ready => "cell.ready",
        kiana_domain::CellLifecycle::Running => "cell.started",
        kiana_domain::CellLifecycle::WaitingInput => "cell.waiting_input",
        kiana_domain::CellLifecycle::CancelRequested => "cell.cancel_requested",
        kiana_domain::CellLifecycle::Cancelled => "cell.cancelled",
        kiana_domain::CellLifecycle::Blocked => "cell.blocked",
        kiana_domain::CellLifecycle::ReadyToMerge => "cell.ready_to_merge",
        kiana_domain::CellLifecycle::Quarantined => "cell.quarantined",
        kiana_domain::CellLifecycle::Failed => "cell.failed",
        kiana_domain::CellLifecycle::Retired => "cell.retired",
        _ => "cell.lifecycle_changed",
    }
}

use super::events::*;
use super::redaction::*;
use super::*;

fn effective_action_scope(
    context: &RequestContext,
    request: &CapabilityRequest,
) -> Result<kiana_domain::ScopeSet, CoreError> {
    let action_paths = kiana_domain::capability_request_paths(request)
        .into_iter()
        .map(|path| {
            kiana_domain::normalize_role_path(&path)
                .ok_or_else(|| action_error("action_path_scope_invalid"))
        })
        .collect::<Result<Vec<_>, _>>()?;
    let context_paths = if context.path_allow.is_empty()
        || context
            .path_allow
            .iter()
            .any(|path| matches!(path.trim(), "." | "*"))
    {
        kiana_domain::ScopeDimension::NotApplicable
    } else {
        kiana_domain::ScopeDimension::Restricted(
            context
                .path_allow
                .iter()
                .map(|path| {
                    kiana_domain::normalize_role_path(path)
                        .ok_or_else(|| action_error("context_path_scope_invalid"))
                })
                .collect::<Result<Vec<_>, _>>()?,
        )
    };
    let action_scope = kiana_domain::ScopeSet::new(
        kiana_domain::ScopeDimension::Restricted(vec![request.operation.clone()]),
        if action_paths.is_empty() {
            kiana_domain::ScopeDimension::NotApplicable
        } else {
            kiana_domain::ScopeDimension::Restricted(action_paths)
        },
        request
            .arguments
            .get("collection")
            .and_then(Value::as_str)
            .filter(|value| !value.trim().is_empty())
            .map(|value| kiana_domain::ScopeDimension::Restricted(vec![value.to_owned()]))
            .unwrap_or_default(),
        request
            .arguments
            .get("server")
            .and_then(Value::as_str)
            .filter(|value| !value.trim().is_empty())
            .map(|value| kiana_domain::ScopeDimension::Restricted(vec![value.to_owned()]))
            .unwrap_or_default(),
        kiana_domain::ScopeLimit::NotApplicable,
        kiana_domain::ScopeLimit::NotApplicable,
    )
    .map_err(|error| action_error(&format!("action_scope_invalid:{error}")))?;
    let context_scope = kiana_domain::ScopeSet::new(
        kiana_domain::ScopeDimension::NotApplicable,
        context_paths,
        kiana_domain::ScopeDimension::NotApplicable,
        kiana_domain::ScopeDimension::NotApplicable,
        kiana_domain::ScopeLimit::NotApplicable,
        kiana_domain::ScopeLimit::NotApplicable,
    )
    .map_err(|error| action_error(&format!("context_scope_invalid:{error}")))?;
    action_scope
        .intersect(&context_scope)
        .map_err(|error| action_error(&format!("scope_intersection_invalid:{error}")))
}

fn build_execution_scope(
    context: &RequestContext,
    request: &CapabilityRequest,
    permission_scope: &kiana_domain::ScopeSet,
) -> Result<kiana_domain::ExecutionScope, CoreError> {
    let mut principal = kiana_domain::AuthenticatedPrincipalRef::local();
    principal.principal_id = context.actor_id.clone().unwrap_or_default();
    principal.principal_digest = principal.digest();
    principal
        .validate()
        .map_err(|error| action_error(&format!("execution_scope_principal_invalid:{error}")))?;
    let canonical_root = ControlPlane::canonical_project_root(&context.project_root);
    let project = kiana_domain::ProjectIdentity::new(
        context.project_root.clone(),
        canonical_root.to_string_lossy().into_owned(),
        None,
        None,
        kiana_domain::json_digest(&json!({"trusted":context.project_trusted})),
    )
    .map_err(|error| action_error(&format!("execution_scope_project_invalid:{error}")))?;
    let mut grant_refs = request.capability_grant_id.into_iter().collect::<Vec<_>>();
    grant_refs.sort_by_key(|grant| grant.to_string());
    let action_digest = {
        let mut without_scope = request.clone();
        without_scope.execution_scope = None;
        kiana_domain::capability_action_digest(&without_scope)
    };
    let path_values = match &permission_scope.paths {
        kiana_domain::ScopeDimension::Restricted(values) => values.clone(),
        kiana_domain::ScopeDimension::NotApplicable => Vec::new(),
    };
    let collection = request
        .arguments
        .get("collection")
        .and_then(Value::as_str)
        .filter(|value| !value.trim().is_empty())
        .map(str::to_owned);
    let server = request
        .arguments
        .get("server")
        .and_then(Value::as_str)
        .filter(|value| !value.trim().is_empty())
        .map(str::to_owned);
    let mut scope = kiana_domain::ExecutionScope {
        schema: kiana_domain::EXECUTION_SCOPE_SCHEMA.to_owned(),
        version: kiana_domain::EXECUTION_SCOPE_SCHEMA_VERSION,
        principal,
        project,
        session_id: context.session_id.clone(),
        run_id: request
            .arguments
            .get("run_id")
            .and_then(Value::as_str)
            .and_then(RunId::parse_str),
        turn_id: request
            .arguments
            .get("turn_id")
            .and_then(|value| serde_json::from_value::<kiana_domain::TurnId>(value.clone()).ok()),
        cell_id: context.cell_id,
        grant_refs,
        budget_lease_id: request.budget_lease_id,
        work_packet_id: context.work_packet_id.clone(),
        environment_id: "kiana-local".to_owned(),
        workspace_revision: None,
        permission_scope: permission_scope.clone(),
        read_roots: vec![canonical_root.to_string_lossy().into_owned()],
        write_roots: if request.risk == RiskLevel::ReadOnly {
            Vec::new()
        } else if path_values.is_empty() {
            vec![".".to_owned()]
        } else {
            path_values.clone()
        },
        read_denies: Vec::new(),
        write_denies: Vec::new(),
        memory_scopes: collection.into_iter().collect(),
        server_scopes: server.into_iter().collect(),
        network_policy: Vec::new(),
        authority_epoch: 1,
        trust_revision: kiana_domain::json_digest(&json!({"trusted":context.project_trusted})),
        data_epoch: 1,
        cancellation_epoch: 1,
        deadline_unix_ms: u64::MAX,
        fencing_token: 1,
        catalog_digest: kiana_domain::capability_action_catalog_digest(),
        action_digest,
        permission_scope_digest: permission_scope.digest(),
        scope_digest: String::new(),
    };
    scope.scope_digest = scope.digest();
    scope
        .validate()
        .map_err(|error| action_error(&format!("execution_scope_invalid:{error}")))?;
    Ok(scope)
}

impl ControlPlane {
    pub(crate) async fn bind_cell_scope(
        &self,
        context: &RequestContext,
        request: &mut CapabilityRequest,
    ) -> Result<(), CoreError> {
        self.guard_company_capability(context, request).await?;
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

    pub(crate) async fn prepare_capability_action(
        &self,
        context: &RequestContext,
        request: CapabilityRequest,
        sandbox: Option<&str>,
        from_runner: bool,
    ) -> Result<CapabilityRequest, CoreError> {
        self.prepare_capability_action_cancellable(context, request, sandbox, from_runner, None)
            .await
    }

    pub(crate) async fn prepare_capability_action_cancellable(
        &self,
        context: &RequestContext,
        mut request: CapabilityRequest,
        sandbox: Option<&str>,
        from_runner: bool,
        cancellation: Option<&watch::Receiver<bool>>,
    ) -> Result<CapabilityRequest, CoreError> {
        if !request.arguments.is_object() {
            return Err(action_error("action_arguments_object_required"));
        }
        let operation = kiana_domain::canonical_action_operation(&request.operation)
            .ok_or_else(|| action_error("action_operation_unknown"))?;
        if from_runner && kiana_domain::model_tool_name(operation).is_none() {
            return Err(action_error("action_not_model_visible"));
        }
        if context.cell_id.is_none()
            && (request.cell_id.is_some()
                || request.capability_grant_id.is_some()
                || request.budget_lease_id.is_some())
        {
            return Err(action_error("cell_capability_scope_incomplete"));
        }
        request.operation = operation.to_owned();
        // Context and registry state own these fields. Handler payloads cannot mint authority.
        let arguments = request.arguments.as_object_mut().expect("checked object");
        for key in [
            "authorization_id",
            "permission_profile",
            "cell_id",
            "capability_grant_id",
            "budget_lease_id",
            "work_packet_id",
            "approval_id",
            "dispatch_permit",
            "execution_context",
        ] {
            arguments.remove(key);
        }
        if from_runner || context.cell_id.is_some() {
            arguments.remove("operator_authorized");
        }
        if !from_runner {
            // Direct/operator calls cannot select a Harness Run or Turn through compatibility
            // arguments; only the lifecycle/runner path may carry these server references.
            arguments.remove("run_id");
            arguments.remove("turn_id");
        }
        // Execution scope is always server-derived after the action and context are normalized.
        request.execution_scope = None;
        if kiana_domain::model_tool_name(operation).is_some() {
            let selected = sandbox
                .or_else(|| arguments.get("sandbox").and_then(Value::as_str))
                .unwrap_or("read-only");
            let selected = crate::lifecycle::authorized_harness_sandbox(context, Some(selected))
                .map_err(action_error)?
                .to_owned();
            arguments.insert("sandbox".to_owned(), json!(selected));
        }
        request.cell_id = None;
        request.capability_grant_id = None;
        request.budget_lease_id = None;
        stamp_request_identity(&mut request, context);
        kiana_domain::normalize_capability_action(&mut request).map_err(action_error)?;
        let cancellation = cancellation.cloned().unwrap_or_else(|| {
            let (_sender, receiver) = watch::channel(false);
            receiver
        });
        request = self
            .pre_tool_hooks
            .prepare_action(context, &request, cancellation)
            .await
            .map_err(CoreError::from)?;
        kiana_domain::normalize_capability_action(&mut request).map_err(action_error)?;
        let effective_scope = effective_action_scope(context, &request)?;
        self.bind_cell_scope(context, &mut request).await?;
        request.execution_scope = Some(build_execution_scope(context, &request, &effective_scope)?);
        let action = kiana_domain::PreparedAction::new(request).map_err(action_error)?;
        self.pin_action_authority(context, action.request()).await?;
        Ok(action.into_request())
    }

    /// Each engine contributes restrictions. Only the exact human-approved requirements
    /// may be discharged; an Allow from a different layer cannot erase an Ask or Deny.
    pub(crate) async fn authorize_capability_action(
        &self,
        context: &RequestContext,
        request: &CapabilityRequest,
        approval: Option<(ApprovalId, &str)>,
    ) -> Result<(PolicyDecision, GateDecision), CoreError> {
        self.authorize_capability_action_cancellable(context, request, approval, None)
            .await
    }

    pub(crate) async fn authorize_capability_action_cancellable(
        &self,
        context: &RequestContext,
        request: &CapabilityRequest,
        approval: Option<(ApprovalId, &str)>,
        cancellation: Option<&watch::Receiver<bool>>,
    ) -> Result<(PolicyDecision, GateDecision), CoreError> {
        let policy = self.evaluate_policy(context, request);
        let mut gate = self.evaluate_gate(request, &policy);
        if !matches!(gate, GateDecision::Denied { .. }) {
            let decision = if let Some(cancellation) = cancellation {
                self.pre_tool_hooks
                    .decide_cancellable(context, request, cancellation.clone())
                    .await
            } else {
                self.pre_tool_hooks.decide(context, request).await
            };
            gate = match decision {
                Ok(PreToolHookDecision::Allow) => gate,
                Ok(PreToolHookDecision::Block(reason)) => GateDecision::Denied {
                    reason: format!("hook_blocked:{reason}"),
                },
                Ok(PreToolHookDecision::Ask { reason }) => {
                    let required = format!("hook_approval_required:{reason}");
                    match gate {
                        GateDecision::AwaitingApproval { reason } => {
                            GateDecision::AwaitingApproval {
                                reason: merge_requirements(&reason, &required),
                            }
                        }
                        _ => GateDecision::AwaitingApproval { reason: required },
                    }
                }
                Err(error) => GateDecision::Denied {
                    reason: format!("hook_blocked:{error}"),
                },
            };
        }
        if let Some((approval_id, original_reason)) = approval {
            if let GateDecision::AwaitingApproval { reason } = &gate {
                let approved = approval_requirements(original_reason);
                if approval_requirements(reason)
                    .iter()
                    .all(|requirement| approved.contains(requirement))
                {
                    gate = GateDecision::Allowed {
                        authorization_id: format!("approval:{approval_id}"),
                    };
                }
            } else if matches!(gate, GateDecision::Allowed { .. }) {
                gate = GateDecision::Allowed {
                    authorization_id: format!("approval:{approval_id}"),
                };
            }
        }
        Ok((policy, gate))
    }

    /// Activation and the visible wait state share one commit. A staged challenge alone
    /// is never published as authority to an entrypoint or a resumed Runner.
    pub(crate) async fn stage_capability_action(
        &self,
        context: &RequestContext,
        request: &CapabilityRequest,
        reason: &str,
        run_id: Option<RunId>,
        event_request_id: RequestId,
        sequence: &mut u64,
    ) -> Result<kiana_domain::ApprovalChallenge, CoreError> {
        let mut approval_context = context.clone();
        approval_context.request_id = request.request_id;
        let challenge = self
            .approvals
            .stage(&approval_context, request.clone(), reason)
            .await?;
        let command_id = kiana_domain::derived_request_id(
            "approval.activate_with_pause",
            &challenge.approval_id.to_string(),
        );
        let command_digest = kiana_domain::json_digest(
            &json!({"approval_id":challenge.approval_id,
            "run_id":run_id,"event_request_id":event_request_id,"action_digest":kiana_domain::capability_action_digest(request)}),
        );
        let command_digest = command_digest.trim_start_matches("sha256:").to_owned();
        let event_count = if run_id.is_some() { 2 } else { 1 };
        if let Some(receipt) = self.events.read_command(&command_id).await? {
            if receipt.command_digest != command_digest {
                return Err(action_error("approval_activation_command_conflict"));
            }
            *sequence = sequence.saturating_add(event_count);
            return Ok(challenge);
        }
        let activation = self
            .approvals
            .prepare_activation(challenge.approval_id, command_id)
            .await?;
        let (aggregate_type, aggregate_id) = match run_id {
            Some(run_id) => ("run", run_id.to_string()),
            None => ("request", event_request_id.to_string()),
        };
        let history = self
            .events
            .read_stream(aggregate_type, &aggregate_id)
            .await?;
        if run_id.is_some() {
            let turn = history
                .iter()
                .rposition(|event| event.kind == "run.prompt")
                .unwrap_or(0);
            if history[turn..].iter().any(|event| {
                matches!(
                    event.kind.as_str(),
                    "run.cancelling"
                        | "run.cancelled"
                        | "run.completed"
                        | "run.failed"
                        | "run.result_unknown"
                )
            }) {
                return Err(action_error("approval_run_already_terminal"));
            }
        }
        let version = history
            .iter()
            .filter_map(|event| event.stream_version)
            .max()
            .unwrap_or(0);
        let mut expected_versions = activation.authority_versions;
        for required in [
            activation.expected_version,
            kiana_domain::AggregateVersion::new(aggregate_type, &aggregate_id, version),
        ] {
            if let Some(existing) = expected_versions.iter().find(|v| {
                v.aggregate_type == required.aggregate_type
                    && v.aggregate_id == required.aggregate_id
            }) {
                if existing.version != required.version {
                    return Err(action_error("approval_activation_authority_conflict"));
                }
            } else {
                expected_versions.push(required);
            }
        }
        let mut data = json!({"approval_id":challenge.approval_id,"request_hash":challenge.request_hash,
            "session_id":context.session_id,"actor_id":context.actor_id,"expires_at_unix_ms":challenge.expires_at_unix_ms,
            "capability_request_id":request.request_id,"action_digest":kiana_domain::capability_action_digest(request),
            "attempt":1,"effect_started":false,"effect_known":true,"zero_effect":true,
            "stop_state":"not_requested","fenced":false});
        if let Some(run_id) = run_id {
            data["run_id"] = json!(run_id);
        }
        let requested = RuntimeEvent::new(
            event_request_id,
            *sequence,
            "approval.requested",
            redact_event_value(&data),
        )?
        .with_stream_metadata(aggregate_type, &aggregate_id, version + 1);
        let mut events = vec![activation.event, requested];
        if let Some(run_id) = run_id {
            events.push(
                RuntimeEvent::new(
                    event_request_id,
                    sequence.saturating_add(1),
                    "run.awaiting_approval",
                    json!({"run_id":run_id,"approval_id":challenge.approval_id,
                        "capability_request_id":request.request_id,"attempt":1,"effect_started":false,
                        "effect_known":true,"zero_effect":true,"stop_state":"not_requested","fenced":false}),
                )?
                .with_stream_metadata(aggregate_type, &aggregate_id, version + 2),
            );
        }
        let outcome = self
            .events
            .commit_transition(kiana_domain::TransitionBatch {
                command_id,
                command_digest: command_digest.clone(),
                expected_versions,
                events,
            })
            .await?;
        match outcome {
            kiana_domain::CommitOutcome::Committed { .. }
            | kiana_domain::CommitOutcome::Replayed { .. } => {}
            kiana_domain::CommitOutcome::Conflict { .. } => {
                return Err(action_error("approval_activation_conflict"))
            }
            kiana_domain::CommitOutcome::Unknown { .. } => {
                if self
                    .events
                    .read_command(&command_id)
                    .await?
                    .is_none_or(|receipt| receipt.command_digest != command_digest)
                {
                    return Err(action_error(
                        "result_unknown:approval_activation_unconfirmed",
                    ));
                }
            }
        }
        if let Some(run_id) = run_id {
            self.invalidate_invocation_projection(run_id);
        }
        *sequence = sequence.saturating_add(event_count);
        Ok(challenge)
    }

    pub(crate) async fn cancel_pending_tools(
        &self,
        run_id: RunId,
        event_request_id: RequestId,
        sequence: &mut u64,
        first: Option<&CapabilityRequest>,
        reason: &str,
    ) -> Result<(), CoreError> {
        let cancelled = self
            .runner
            .send(RunnerCommand::Cancel {
                run_id,
                reason: reason.to_owned(),
            })
            .await;
        let mut recording_error = None;
        if let Some(request) = first {
            let mut result = CapabilityResult::failure(request.request_id, reason);
            result.output["not_executed"] = json!(true);
            let result = kiana_domain::normalize_capability_result(request.request_id, result);
            recording_error = self.record_event(event_request_id, sequence, "run.tool_result", json!({
                "run_id":run_id,"capability_request_id":request.request_id,"call_id":request.arguments["call_id"],
                "result":result.output,"cancelled":kiana_domain::CapabilityErrorCode::from_reason(reason) == kiana_domain::CapabilityErrorCode::Cancelled,"not_executed":true,
                "attempt":1,"effect_started":false,"effect_known":true,"zero_effect":true,
                "stop_state":"confirmed","stop_confirmed":true,"fenced":false,
            })).await.err();
        }
        if let Ok(events) = cancelled {
            for event in events {
                if let RunnerEvent::ToolCancelled {
                    run_id: event_run,
                    request_id,
                    call_id,
                    result,
                } = event
                {
                    if event_run == run_id
                        && !first.is_some_and(|request| request.request_id == request_id)
                    {
                        if let Err(error) = self.record_event(event_request_id, sequence, "run.tool_result", json!({
                            "run_id":run_id,"capability_request_id":request_id,"call_id":call_id,
                            "result":result,"cancelled":true,"not_executed":true,
                            "attempt":1,"effect_started":false,"effect_known":true,"zero_effect":true,
                            "stop_state":"confirmed","stop_confirmed":true,"fenced":false,
                        })).await { recording_error = Some(error); }
                    }
                }
            }
        }
        match recording_error {
            Some(error) => Err(error),
            None => Ok(()),
        }
    }

    /// All three ingress paths reserve resources, dispatch once, then finalize the same way.
    pub(crate) async fn dispatch_capability_action(
        &self,
        context: &RequestContext,
        run_id: Option<RunId>,
        request: &CapabilityRequest,
        authorization_id: String,
        cancellation: watch::Receiver<bool>,
        event_request_id: RequestId,
        sequence: &mut u64,
    ) -> Result<FinalizedCapabilityAction, CoreError> {
        let mut lease = None;
        let execution = if *cancellation.borrow() {
            let mut result =
                CapabilityResult::failure(request.request_id, "cancelled:before_dispatch");
            result.output["not_executed"] = json!(true);
            Ok(result)
        } else {
            let prepared = async {
                if let Some(run_id) = run_id {
                    self.checkpoint_before_write(context, run_id, request, *sequence)
                        .await?;
                }
                let authorized =
                    AuthorizedCapabilityRequest::new(authorization_id, request.clone())?;
                lease = self.begin_cell_capability_from_request(request).await?;
                Ok::<_, CoreError>(authorized)
            }
            .await;
            match prepared {
                Ok(authorized) => {
                    self.dispatch_authorized(context, run_id, authorized, cancellation.clone())
                        .await
                }
                Err(error) => {
                    let mut result =
                        CapabilityResult::failure(request.request_id, error.to_string());
                    result.output["not_executed"] = json!(true);
                    result.output["dispatch_rejected"] = json!(true);
                    Ok(result)
                }
            }
        };
        self.finalize_capability_action(
            context,
            run_id,
            request,
            execution,
            lease,
            &cancellation,
            event_request_id,
            sequence,
        )
        .await
    }

    async fn finalize_capability_action(
        &self,
        context: &RequestContext,
        run_id: Option<RunId>,
        request: &CapabilityRequest,
        execution: Result<CapabilityResult, PortError>,
        lease: Option<CapabilityLease>,
        cancellation: &watch::Receiver<bool>,
        event_request_id: RequestId,
        sequence: &mut u64,
    ) -> Result<FinalizedCapabilityAction, CoreError> {
        let result = execution.unwrap_or_else(|error| {
            let raw = match &error {
                PortError::Failed(reason)
                | PortError::Conflict(reason)
                | PortError::Unavailable(reason) => reason.as_str(),
            };
            let cancelled_before_dispatch =
                matches!(raw, "cancelled:before_dispatch" | "cancelled:before_broker");
            // An error without a committed handler result does not prove whether an effect occurred.
            // Only the dispatcher's explicit pre-execution cancellation proves no handler started.
            let mut result = CapabilityResult::failure(
                request.request_id,
                if cancelled_before_dispatch {
                    raw.to_owned()
                } else {
                    format!("result_unknown:{}", redact_event_text(&error.to_string()))
                },
            );
            result.output["dispatch_rejected"] = json!(true);
            if cancelled_before_dispatch {
                result.output["not_executed"] = json!(true);
            }
            result
        });
        let mut result = kiana_domain::normalize_capability_result(request.request_id, result);
        if *cancellation.borrow()
            && result.failure_code() != Some(kiana_domain::CapabilityErrorCode::ResultUnknown)
        {
            let stopped = result.output["stop_confirmed"] == true
                || result.output["not_executed"] == true
                || match run_id {
                    Some(run_id) => self.await_capability_stop(run_id).await,
                    None => false,
                };
            result.success = false;
            if !result.output.is_object() {
                result.output = json!({"detail":result.output});
            }
            result.output["error"] = json!(if stopped {
                "cancelled:user"
            } else {
                "result_unknown:cancel_stop_unconfirmed"
            });
            result = kiana_domain::normalize_capability_result(request.request_id, result);
        }
        let mut finalized = FinalizedCapabilityAction::new(redact_capability_result(result));
        let payload = match run_id {
            Some(run_id) => {
                capability_event_payload(&finalized.result.output, request, context, run_id)
            }
            None => direct_capability_event_payload(&finalized.result.output, request),
        };
        let kind = match finalized.status {
            ExecutionStatus::Completed => "capability.completed",
            ExecutionStatus::Cancelled => "capability.cancelled",
            ExecutionStatus::ResultUnknown => "capability.result_unknown",
            _ => "capability.failed",
        };
        if self
            .record_event(event_request_id, sequence, kind, payload)
            .await
            .is_err()
        {
            finalized = FinalizedCapabilityAction::unknown(
                request.request_id,
                "result_event_persistence_failed",
            );
        }
        if let Some(lease) = lease {
            let outcome = match finalized.status {
                ExecutionStatus::Completed => CapabilityOutcome::Succeeded,
                ExecutionStatus::ResultUnknown => CapabilityOutcome::Unknown,
                _ => CapabilityOutcome::Failed,
            };
            if self.finish_cell_capability(lease, outcome).await.is_err() {
                finalized = FinalizedCapabilityAction::unknown(
                    request.request_id,
                    "cell_capability_settlement_failed",
                );
            }
        }
        Ok(finalized)
    }

    pub(crate) async fn broker_harness_capability(
        &self,
        context: &RequestContext,
        request_id: RequestId,
        run_id: RunId,
        sandbox: &str,
        sequence: &mut u64,
        request: CapabilityRequest,
        cancel_rx: &watch::Receiver<bool>,
    ) -> Result<Result<Option<Vec<RunnerEvent>>, String>, CoreError> {
        let mut request = request;
        if let Some(arguments) = request.arguments.as_object_mut() {
            arguments.insert("run_id".to_owned(), json!(run_id));
            arguments.insert(
                "turn_id".to_owned(),
                json!(kiana_domain::TurnId::from_uuid(request_id.as_uuid())),
            );
        }
        let original = request.clone();
        let request = match self
            .prepare_capability_action_cancellable(
                context,
                request,
                Some(sandbox),
                true,
                Some(cancel_rx),
            )
            .await
        {
            Ok(request) => request,
            Err(error) => {
                let reason = redact_event_text(&error.to_string());
                // Preparation may reject malformed, untrusted, or out-of-sandbox input before
                // the normal request fact is written. Persist only the redacted request facts
                // so restart projection can account for the denied invocation without guessing.
                self.record_event(
                    request_id,
                    sequence,
                    "run.capability_requested",
                    json!({"run_id":run_id,
                        "request_id":original.request_id,
                        "capability":original.capability,
                        "operation":original.operation,
                        "risk":original.risk,
                        "cell_id":original.cell_id,
                        "capability_grant_id":original.capability_grant_id,
                        "budget_lease_id":original.budget_lease_id,
                        "attempt":1,"effect_started":false,"effect_known":true,
                        "zero_effect":true,"stop_state":"not_requested","fenced":false,
                        "action_digest":kiana_domain::capability_action_digest(&original),
                        "turn_id":kiana_domain::TurnId::from_uuid(request_id.as_uuid()),
                        "invocation_id":kiana_domain::InvocationId::from_uuid(original.request_id.as_uuid()),
                        "arguments":redact_event_value(&original.arguments)}),
                )
                .await?;
                self.record_event(
                    request_id,
                    sequence,
                    "run.capability_blocked",
                    json!({"run_id":run_id,"capability_request_id":original.request_id,"error":reason,
                        "attempt":1,"effect_started":false,"effect_known":true,"zero_effect":true,
                        "stop_state":"not_requested","fenced":false}),
                )
                .await?;
                self.cancel_pending_tools(run_id, request_id, sequence, Some(&original), &reason)
                    .await?;
                return Ok(Err(reason));
            }
        };
        self.record_event(
            request_id,
            sequence,
            "run.tool_call",
            json!({"run_id":run_id,
            "capability_request_id":request.request_id,"call_id":request.arguments["call_id"],
            "turn_id":kiana_domain::TurnId::from_uuid(request_id.as_uuid()),
            "invocation_id":kiana_domain::InvocationId::from_uuid(request.request_id.as_uuid()),
            "execution_scope":request.execution_scope,
            "tool":request.capability,"operation":request.operation}),
        )
        .await?;
        self.record_event(request_id,sequence,"run.capability_requested",json!({"run_id":run_id,
            "request_id":request.request_id,"capability":request.capability,"operation":request.operation,"risk":request.risk,
            "cell_id":request.cell_id,"capability_grant_id":request.capability_grant_id,"budget_lease_id":request.budget_lease_id,
            "attempt":1,"effect_started":false,"effect_known":true,"zero_effect":true,
            "stop_state":"not_requested","fenced":false,
            "action_digest":kiana_domain::capability_action_digest(&request),
            "turn_id":kiana_domain::TurnId::from_uuid(request_id.as_uuid()),
            "invocation_id":kiana_domain::InvocationId::from_uuid(request.request_id.as_uuid()),
            "execution_scope":request.execution_scope,
            "arguments":redact_event_value(&request.arguments)})).await?;
        if *cancel_rx.borrow() {
            self.cancel_pending_tools(
                run_id,
                request_id,
                sequence,
                Some(&request),
                "cancelled:user",
            )
            .await?;
            return Ok(Err("cancelled:user".to_owned()));
        }
        let (policy, gate) = self
            .authorize_capability_action_cancellable(context, &request, None, Some(cancel_rx))
            .await?;
        self.record_event(
            request_id,
            sequence,
            "capability.decision",
            json!({"run_id":run_id,"capability_request_id":request.request_id,"policy":policy,"gate":gate,
                "attempt":1,"effect_started":false,"effect_known":true,"zero_effect":true,
                "stop_state":"not_requested","fenced":false}),
        )
        .await?;
        let authorization_id = match gate {
            GateDecision::Denied { reason } => {
                self.record_event(
                    request_id,
                    sequence,
                    "run.capability_blocked",
                    json!({"run_id":run_id,"capability_request_id":request.request_id,"reason":reason,
                        "attempt":1,"effect_started":false,"effect_known":true,"zero_effect":true,
                        "stop_state":"not_requested","fenced":false}),
                )
                .await?;
                self.cancel_pending_tools(run_id, request_id, sequence, Some(&request), &reason)
                    .await?;
                return Ok(Err(reason));
            }
            GateDecision::AwaitingApproval { reason } => {
                let challenge = match self
                    .stage_capability_action(
                        context,
                        &request,
                        &reason,
                        Some(run_id),
                        request_id,
                        sequence,
                    )
                    .await
                {
                    Ok(challenge) => challenge,
                    Err(error) => {
                        let reason = redact_event_text(&error.to_string());
                        self.cancel_pending_tools(
                            run_id,
                            request_id,
                            sequence,
                            Some(&request),
                            &reason,
                        )
                        .await?;
                        return Ok(Err(reason));
                    }
                };
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
                            event_sequence: *sequence,
                            run_id,
                            request,
                            context: context.clone(),
                            sandbox: sandbox.to_owned(),
                        },
                    );
                return Ok(Ok(None));
            }
            GateDecision::Allowed { authorization_id } => authorization_id,
        };
        let finalized = self
            .dispatch_capability_action(
                context,
                Some(run_id),
                &request,
                authorization_id,
                cancel_rx.clone(),
                request_id,
                sequence,
            )
            .await?;
        if matches!(
            finalized.status,
            ExecutionStatus::Cancelled | ExecutionStatus::ResultUnknown
        ) || finalized.result.output["dispatch_rejected"] == true
        {
            let reason = finalized
                .error
                .unwrap_or_else(|| "result_unknown:capability_result".to_owned());
            self.cancel_pending_tools(
                run_id,
                request_id,
                sequence,
                (finalized.result.output["not_executed"] == true).then_some(&request),
                &reason,
            )
            .await?;
            return Ok(Err(reason));
        }
        match self
            .deliver_capability_result(run_id, finalized.result)
            .await
        {
            Ok(events) => Ok(Ok(Some(events))),
            Err(error) => Ok(Err(redact_event_text(&error.to_string()))),
        }
    }
}

pub(crate) struct FinalizedCapabilityAction {
    pub(crate) result: CapabilityResult,
    pub(crate) status: ExecutionStatus,
    pub(crate) error: Option<String>,
}
impl FinalizedCapabilityAction {
    fn new(result: CapabilityResult) -> Self {
        let status = match result.failure_code() {
            None => ExecutionStatus::Completed,
            Some(kiana_domain::CapabilityErrorCode::Cancelled) => ExecutionStatus::Cancelled,
            Some(
                kiana_domain::CapabilityErrorCode::ResultUnknown
                | kiana_domain::CapabilityErrorCode::CompensationRequired,
            ) => ExecutionStatus::ResultUnknown,
            _ => ExecutionStatus::Failed,
        };
        let error = (!result.success).then(|| {
            result.output["error"]
                .as_str()
                .unwrap_or("capability_failed")
                .to_owned()
        });
        Self {
            result,
            status,
            error,
        }
    }
    fn unknown(request_id: RequestId, reason: &str) -> Self {
        Self::new(kiana_domain::normalize_capability_result(
            request_id,
            CapabilityResult::failure_with_code(
                request_id,
                kiana_domain::CapabilityErrorCode::ResultUnknown,
                Some(reason),
            ),
        ))
    }
}
fn action_error(reason: &str) -> CoreError {
    PortError::Failed(reason.to_owned()).into()
}
pub(crate) fn approval_requirements(reason: &str) -> Vec<String> {
    reason
        .strip_prefix("approval_requirements:")
        .and_then(|value| serde_json::from_str(value).ok())
        .unwrap_or_else(|| vec![reason.to_owned()])
}
pub(crate) fn merge_requirements(left: &str, right: &str) -> String {
    let mut requirements = approval_requirements(left);
    requirements.extend(approval_requirements(right));
    requirements.sort();
    requirements.dedup();
    format!(
        "approval_requirements:{}",
        serde_json::to_string(&requirements).expect("string array")
    )
}

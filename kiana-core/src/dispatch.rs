use super::*;
use kiana_domain::{
    derived_request_id, json_digest, AggregateVersion, CommitOutcome, DispatchPermit, ExecutionId,
    InvocationId, TransitionBatch, TurnId, DISPATCH_PERMIT_SCHEMA, DISPATCH_PERMIT_VERSION,
};

pub fn project_root_identity(root: &str) -> Result<Value, PortError> {
    let canonical = Path::new(root)
        .canonicalize()
        .map_err(|_| dispatch_error("project_root_unavailable"))?;
    let metadata =
        fs::metadata(&canonical).map_err(|_| dispatch_error("project_root_unavailable"))?;
    if !metadata.is_dir() {
        return Err(dispatch_error("project_root_not_directory"));
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        Ok(json!({"canonical_root":canonical,"device":metadata.dev(),"inode":metadata.ino()}))
    }
    #[cfg(not(unix))]
    {
        let _ = metadata;
        Ok(json!({"canonical_root":canonical,"identity_strength":"path_only"}))
    }
}
fn dispatch_error(code: &str) -> PortError {
    PortError::Failed(code.to_owned())
}
fn now_ms() -> Result<u64, PortError> {
    let value = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|_| dispatch_error("clock_untrusted"))?
        .as_millis();
    u64::try_from(value).map_err(|_| dispatch_error("clock_overflow"))
}

fn typed_invocation_identity(
    run_id: Option<RunId>,
    turn_id: Option<TurnId>,
    invocation_id: InvocationId,
    execution_id: ExecutionId,
    request: &CapabilityRequest,
    attempt: u32,
) -> Result<Option<Value>, PortError> {
    let Some(run_id) = run_id else {
        // Direct commands have an explicit Command scope and must not receive a fabricated
        // Harness Run/Turn identity.
        return Ok(None);
    };
    let turn_id = turn_id.ok_or_else(|| dispatch_error("turn_identity_required"))?;
    let call_id = request.arguments["call_id"].as_str().map(str::to_owned);
    let identity = kiana_domain::InvocationIdentity::new(
        run_id,
        turn_id,
        invocation_id,
        execution_id,
        call_id,
        attempt,
    )
    .map_err(|error| dispatch_error(&format!("invocation_identity_invalid:{error}")))?;
    serde_json::to_value(identity)
        .map(Some)
        .map_err(|error| dispatch_error(&format!("invocation_identity_encode:{error}")))
}
pub(crate) async fn commit_confirmed(
    events: &dyn EventStorePort,
    mut batch: TransitionBatch,
) -> Result<bool, PortError> {
    // Domain display digests include their algorithm; the journal wire field is bare hex.
    if let Some(hex) = batch.command_digest.strip_prefix("sha256:") {
        batch.command_digest = hex.to_owned();
    }
    if !events.supports_atomic_transitions() {
        return Err(PortError::Unavailable(
            "control_journal_required".to_owned(),
        ));
    }
    match events.commit_transition(batch).await? {
        CommitOutcome::Committed { .. } => Ok(false),
        CommitOutcome::Replayed { .. } => Ok(true),
        CommitOutcome::Conflict { .. } => Err(PortError::Conflict(
            "control_transition_conflict".to_owned(),
        )),
        CommitOutcome::Unknown { .. } => {
            Err(dispatch_error("result_unknown:control_commit_unconfirmed"))
        }
    }
}

/// Reusable by the composition root; it possesses no model loop or handler authority.
pub struct JournalPermitVerifier {
    events: Arc<dyn EventStorePort>,
}
impl JournalPermitVerifier {
    pub fn new(events: Arc<dyn EventStorePort>) -> Self {
        Self { events }
    }
}

#[async_trait::async_trait]
impl kiana_ports::ExecutionPermitVerifierPort for JournalPermitVerifier {
    async fn verify_and_consume(
        &self,
        request: &AuthorizedCapabilityRequest,
    ) -> Result<(), PortError> {
        let id = request
            .authorization_id
            .strip_prefix("permit:")
            .ok_or_else(|| dispatch_error("execution_permit_required"))?;
        let records = self.events.read_stream("execution_permit", id).await?;
        if records.len() != 1 || records[0].kind != "execution.prepared" {
            return Err(dispatch_error("execution_permit_unavailable"));
        }
        let permit = DispatchPermit::from_json(&records[0].data["permit"])
            .map_err(|_| dispatch_error("execution_permit_invalid"))?;
        let now = now_ms()?;
        let project_identity = project_root_identity(&permit.context.project_root)?;
        if permit.execution_id.to_string() != id
            || permit.version != DISPATCH_PERMIT_VERSION
            || permit
                .validate_for_request(&request.request, &project_identity, now)
                .is_err()
        {
            return Err(dispatch_error("execution_permit_scope_or_expiry_mismatch"));
        }
        let mut expected = permit.authority_versions.clone();
        expected.push(AggregateVersion {
            aggregate_type: "execution_permit".to_owned(),
            aggregate_id: id.to_owned(),
            version: 1,
        });
        let command_id = derived_request_id("permit.consume", id);
        let invocation = typed_invocation_identity(
            permit.run_id,
            permit.turn_id,
            permit.invocation_id,
            permit.execution_id,
            &request.request,
            1,
        )?;
        let event=RuntimeEvent::new(command_id,1,"invocation.dispatching",json!({
            "run_id":permit.run_id,"turn_id":permit.turn_id,"invocation_id":permit.invocation_id,
            "execution_id":permit.execution_id,"capability_request_id":permit.request_id,
            "call_id":request.request.arguments["call_id"],"operation":request.request.operation,
            "args_fingerprint":permit.action_digest,"decision_id":permit.decision_id,
            "attempt":1,"started":false,"effect_started":false,"effect_known":true,
            "zero_effect":true,"stop_state":"not_requested","fenced":true,
            "boundary":"broker_admission","invocation":invocation,
        })).map_err(|e|dispatch_error(&e.to_string()))?.with_stream_metadata("execution_permit",id,2);
        let batch = TransitionBatch {
            command_id,
            command_digest: json_digest(&json!({"consume":permit})),
            expected_versions: expected,
            events: vec![event],
        };
        let committed = commit_confirmed(self.events.as_ref(), batch).await;
        if committed? {
            return Err(dispatch_error("execution_permit_already_consumed"));
        }
        Ok(())
    }
}

impl ControlPlane {
    pub(crate) async fn deliver_capability_result(
        &self,
        run_id: RunId,
        result: CapabilityResult,
    ) -> Result<Vec<RunnerEvent>, PortError> {
        let command_id = derived_request_id("result.deliver", &result.request_id.to_string());
        let run = self.events.read_stream("run", &run_id.to_string()).await?;
        let version = run
            .iter()
            .filter_map(|event| event.stream_version)
            .max()
            .unwrap_or(0);
        let turn = run
            .iter()
            .rposition(|event| event.kind == "run.prompt")
            .unwrap_or(0);
        if run[turn..].iter().any(|event| {
            matches!(
                event.kind.as_str(),
                "run.cancelling"
                    | "run.cancelled"
                    | "run.result_unknown"
                    | "run.failed"
                    | "run.completed"
            )
        }) {
            return Err(dispatch_error("cancelled:result_delivery_run_inactive"));
        }
        let event=RuntimeEvent::new(command_id,1,"result.delivery_claimed",json!({"run_id":run_id,
            "capability_request_id":result.request_id,"result_digest":json_digest(&json!(result)),"delivery_policy":"single_advance"}))
            .map_err(|e|dispatch_error(&e.to_string()))?.with_stream_metadata("result_delivery",result.request_id.to_string(),1);
        let batch = TransitionBatch {
            command_id,
            command_digest: json_digest(&json!({"run_id":run_id,"result":result})),
            expected_versions: vec![
                AggregateVersion::new("result_delivery", result.request_id.to_string(), 0),
                AggregateVersion::new("run", run_id.to_string(), version),
            ],
            events: vec![event],
        };
        if commit_confirmed(self.events.as_ref(), batch).await? {
            return Err(dispatch_error(
                "result_unknown:result_delivery_already_claimed",
            ));
        }
        self.runner
            .send(RunnerCommand::CapabilityResult { run_id, result })
            .await
            .map_err(|e| dispatch_error(&format!("result_unknown:result_delivery_unconfirmed:{e}")))
    }

    pub(crate) async fn pin_action_authority(
        &self,
        context: &RequestContext,
        request: &CapabilityRequest,
    ) -> Result<(), CoreError> {
        let root = Self::canonical_project_root(&context.project_root);
        let key = json_digest(&json!({"project_root":root}));
        let authority = self.events.read_stream("authority", &key).await?;
        let current = authority
            .last()
            .ok_or_else(|| dispatch_error("action_authority_missing"))?;
        if current.data["project_trusted"] != json!(context.project_trusted) {
            return Err(dispatch_error("action_trust_changed").into());
        }
        let mut scope = context.clone();
        scope.request_id = request.request_id;
        let digest = json_digest(
            &json!({"scope":scope,"action_digest":kiana_domain::capability_action_digest(request)}),
        );
        let existing = self
            .events
            .read_stream("action", &request.request_id.to_string())
            .await?;
        let version = current.stream_version.unwrap_or(0);
        if let Some(pin) = existing.first() {
            if pin.data["action_digest"] != digest || pin.data["authority_version"] != version {
                return Err(dispatch_error("action_authority_changed").into());
            }
            return Ok(());
        }
        let command_id = derived_request_id("action.pin", &request.request_id.to_string());
        let event = RuntimeEvent::new(
            command_id,
            1,
            "action.authority_pinned",
            json!({"action_digest":digest,"authority_version":version,
            "request_id":request.request_id,"authority_key":key,"actor_id":context.actor_id}),
        )?
        .with_stream_metadata("action", request.request_id.to_string(), 1);
        let batch = TransitionBatch {
            command_id,
            command_digest: digest,
            expected_versions: vec![
                AggregateVersion::new("action", request.request_id.to_string(), 0),
                AggregateVersion::new("authority", key, version),
            ],
            events: vec![event],
        };
        commit_confirmed(self.events.as_ref(), batch).await?;
        Ok(())
    }

    async fn dispatch_authority_versions(
        &self,
        context: &RequestContext,
        run_id: Option<RunId>,
    ) -> Result<Vec<AggregateVersion>, PortError> {
        let root = Self::canonical_project_root(&context.project_root);
        let assignment = json_digest(&json!({"session_id":context.session_id,"project_root":root}));
        let mut scopes = vec![
            ("session_assignment".to_owned(), assignment),
            (
                "authority".to_owned(),
                json_digest(&json!({"project_root":root})),
            ),
            (
                "resource_quarantine".to_owned(),
                json_digest(&json!({"project_root":root})),
            ),
        ];
        if let Some(id) = run_id {
            scopes.push(("run".to_owned(), id.to_string()));
        }
        // Domain changes and revocations are also read dependencies, even for root commands.
        scopes.push((
            "company".to_owned(),
            super::company::company_aggregate_id(context),
        ));
        let mut versions = Vec::new();
        let authority_key = json_digest(&json!({"project_root":root}));
        let authority = self.events.read_stream("authority", &authority_key).await?;
        let current_authority = authority
            .last()
            .ok_or_else(|| dispatch_error("dispatch_authority_missing"))?;
        if current_authority.data["project_trusted"] != json!(context.project_trusted) {
            return Err(dispatch_error("dispatch_trust_changed"));
        }
        let authority_revision = format!(
            "{}:{}",
            current_authority.stream_version.unwrap_or(0),
            current_authority.data["revision_digest"]
                .as_str()
                .unwrap_or_default()
        );
        for (aggregate_type, aggregate_id) in scopes {
            let events = self
                .events
                .read_stream(&aggregate_type, &aggregate_id)
                .await?;
            if aggregate_type == "resource_quarantine" {
                let mut quarantined = std::collections::HashSet::new();
                for event in &events {
                    if let Some(id) = event.data["run_id"].as_str() {
                        match event.kind.as_str() {
                            "resource.quarantined" => {
                                quarantined.insert(id.to_owned());
                            }
                            "resource.released" => {
                                quarantined.remove(id);
                            }
                            _ => return Err(dispatch_error("resource_quarantine_record_invalid")),
                        }
                    }
                }
                if !quarantined.is_empty() {
                    return Err(dispatch_error("project_resources_quarantined"));
                }
            }
            if aggregate_type == "run" {
                if events
                    .iter()
                    .find(|event| event.kind == "run.authorized")
                    .is_none_or(|event| event.data["authority_revision"] != authority_revision)
                {
                    return Err(dispatch_error("run_dispatch_authority_changed"));
                }
                let turn = events
                    .iter()
                    .rposition(|event| event.kind == "run.prompt")
                    .unwrap_or(0);
                if events[turn..].iter().any(|event| {
                    matches!(
                        event.kind.as_str(),
                        "run.cancelling"
                            | "run.cancelled"
                            | "run.result_unknown"
                            | "run.failed"
                            | "run.completed"
                    )
                }) {
                    return Err(dispatch_error("run_dispatch_after_terminal_or_cancel"));
                }
            }
            versions.push(AggregateVersion {
                aggregate_type,
                aggregate_id,
                version: events
                    .iter()
                    .filter_map(|e| e.stream_version)
                    .max()
                    .unwrap_or(0),
            });
        }
        Ok(versions)
    }

    pub(crate) async fn dispatch_authorized(
        &self,
        context: &RequestContext,
        run_id: Option<RunId>,
        mut authorized: AuthorizedCapabilityRequest,
        cancellation: watch::Receiver<bool>,
    ) -> Result<CapabilityResult, PortError> {
        if *cancellation.borrow() {
            return Err(dispatch_error("cancelled:before_dispatch"));
        }
        let request = &authorized.request;
        let invocation_id = InvocationId::from_uuid(request.request_id.as_uuid());
        let dispatch_command_id =
            derived_request_id("execution.prepare", &request.request_id.to_string());
        if self
            .events
            .read_command(&dispatch_command_id)
            .await?
            .is_some()
        {
            return Err(PortError::Conflict(
                "result_unknown:invocation_already_prepared".to_owned(),
            ));
        }
        let execution_id = ExecutionId::new();
        let mut authority_versions = self.dispatch_authority_versions(context, run_id).await?;
        // Revalidate business scope after pinning the read set; concurrent changes fail the commit.
        self.guard_company_capability(context, request)
            .await
            .map_err(|e| dispatch_error(&e.to_string()))?;
        if let PolicyDecision::Deny { reason } = self.evaluate_policy(context, request) {
            return Err(dispatch_error(&reason));
        }
        let pin = self
            .events
            .read_stream("action", &request.request_id.to_string())
            .await?;
        let pinned = pin
            .first()
            .ok_or_else(|| dispatch_error("action_authority_not_pinned"))?;
        let mut scope = context.clone();
        scope.request_id = request.request_id;
        if pinned.data["action_digest"]
            != json_digest(
                &json!({"scope":scope,"action_digest":kiana_domain::capability_action_digest(request)}),
            )
            || !authority_versions.iter().any(|version| {
                version.aggregate_type == "authority"
                    && pinned.data["authority_version"] == version.version
            })
        {
            return Err(dispatch_error("action_authority_changed"));
        }
        authority_versions.push(AggregateVersion::new(
            "action",
            request.request_id.to_string(),
            pin.iter()
                .filter_map(|event| event.stream_version)
                .max()
                .unwrap_or(0),
        ));
        let mut events = Vec::new();
        let mut expiry = now_ms()?.saturating_add(60_000);
        let approval_id = authorized
            .authorization_id
            .strip_prefix("approval:")
            .map(|id| {
                serde_json::from_value::<ApprovalId>(json!(id))
                    .map_err(|_| dispatch_error("approval_id_invalid"))
            })
            .transpose()?;
        if let Some(approval_id) = approval_id {
            let prepared = self
                .approvals
                .prepare_consumption(context, approval_id, dispatch_command_id)
                .await?;
            if prepared.pending.request != *request {
                return Err(dispatch_error("approval_action_changed"));
            }
            expiry = expiry.min(prepared.pending.challenge.expires_at_unix_ms);
            for value in prepared.authority_versions {
                if let Some(observed) = authority_versions.iter().find(|existing| {
                    existing.aggregate_type == value.aggregate_type
                        && existing.aggregate_id == value.aggregate_id
                }) {
                    if observed.version != value.version {
                        return Err(dispatch_error("approval_authority_changed"));
                    }
                } else {
                    authority_versions.push(value);
                }
            }
            authority_versions.push(prepared.expected_version);
            events.push(prepared.event);
        }
        let turn_id = if let Some(run_id) = run_id {
            self.events
                .read_stream("run", &run_id.to_string())
                .await?
                .iter()
                .rev()
                .find(|event| event.kind == "run.prompt")
                .map(|event| TurnId::from_uuid(event.request_id.as_uuid()))
        } else {
            None
        };
        let mut permit_authority = authority_versions.clone();
        // Consumption itself advances approval; the Broker must observe that committed version.
        for value in &mut permit_authority {
            if events.iter().any(|event| {
                event.aggregate_type.as_deref() == Some(value.aggregate_type.as_str())
                    && event.aggregate_id.as_deref() == Some(value.aggregate_id.as_str())
            }) {
                value.version += 1;
            }
        }
        let issued_at_unix_ms = now_ms()?;
        let mut permit = DispatchPermit {
            schema: DISPATCH_PERMIT_SCHEMA.to_owned(),
            version: DISPATCH_PERMIT_VERSION,
            execution_id,
            invocation_id,
            request_id: request.request_id,
            run_id,
            turn_id,
            decision_id: authorized.authorization_id.clone(),
            approval_id,
            context: context.clone(),
            action_digest: kiana_domain::capability_action_digest(request),
            project_identity: project_root_identity(&context.project_root)?,
            authority_versions: permit_authority,
            issued_at_unix_ms,
            expires_at_unix_ms: expiry,
            permit_digest: String::new(),
        };
        permit.permit_digest = permit.digest();
        permit.validate().map_err(|error| dispatch_error(&error))?;
        authority_versions.push(AggregateVersion {
            aggregate_type: "execution_permit".to_owned(),
            aggregate_id: execution_id.to_string(),
            version: 0,
        });
        let invocation =
            typed_invocation_identity(run_id, turn_id, invocation_id, execution_id, request, 1)?;
        let event=RuntimeEvent::new(dispatch_command_id,1,"execution.prepared",json!({"permit":permit,"cell_reservation":
            if let Some(id)=request.cell_id {self.cell_registry.reservation_for_cell(id).await?.map(|r|json!({"budget":r.budget,"grant":r.grant,"cell":r.cell}))}else{None}
        ,"invocation":invocation})).map_err(|e|dispatch_error(&e.to_string()))?.with_stream_metadata("execution_permit",execution_id.to_string(),1);
        let mut event = event;
        event.data = super::redaction::redact_event_value(&event.data);
        events.push(event);
        let batch = TransitionBatch {
            command_id: dispatch_command_id,
            command_digest: json_digest(
                &json!({"context":context,"run_id":run_id,"request":request}),
            ),
            expected_versions: authority_versions,
            events,
        };
        let committed = commit_confirmed(self.events.as_ref(), batch).await;
        if let Some(run_id) = run_id {
            self.invalidate_invocation_projection(run_id);
        }
        if committed? {
            return Err(dispatch_error("result_unknown:invocation_already_prepared"));
        }
        self.prepare_data_revocation(&authorized.request)
            .await
            .map_err(|e| dispatch_error(&e.to_string()))?;
        self.prepare_checkpoint_restore(&authorized.request)
            .await
            .map_err(|e| dispatch_error(&e.to_string()))?;
        if *cancellation.borrow() {
            return Err(dispatch_error("cancelled:before_broker"));
        }
        authorized.authorization_id = format!("permit:{execution_id}");
        let executed_request = authorized.request.clone();
        self.commit_invocation_executing(&permit, &executed_request)
            .await?;
        let (stop_tx, _rx) = watch::channel(None);
        if let Some(id) = run_id {
            self.capability_stops
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .insert(id, stop_tx.clone());
        }
        let result = self
            .capabilities
            .execute_cancellable(authorized, cancellation.clone())
            .await;
        let result = match result {
            Ok(result) if result.request_id == permit.request_id => {
                super::redaction::redact_capability_result(result)
            }
            Ok(_) => CapabilityResult::failure(
                permit.request_id,
                "result_unknown:capability_result_mismatch",
            ),
            Err(error) => CapabilityResult::failure(
                permit.request_id,
                format!(
                    "result_unknown:{}",
                    super::redaction::redact_event_text(&error.to_string())
                ),
            ),
        };
        let result = kiana_domain::normalize_capability_result(permit.request_id, result);
        let result = match self
            .finish_checkpoint_restore(&executed_request, &result)
            .await
        {
            Ok(()) => result,
            Err(error) => {
                CapabilityResult::failure(permit.request_id, format!("result_unknown:{error}"))
            }
        };
        let mut result = kiana_domain::normalize_capability_result(permit.request_id, result);
        if *cancellation.borrow()
            && result.failure_code() != Some(kiana_domain::CapabilityErrorCode::ResultUnknown)
        {
            if !result.output.is_object() {
                result.output = json!({"detail":result.output});
            }
            result.output["completed_before_cancel"] = json!(result.success);
            result.output["cancelled"] = json!(true);
            result.output["stop_confirmed"] = json!(true);
            result = kiana_domain::normalize_capability_result(permit.request_id, result);
        }
        let unknown =
            result.failure_code() == Some(kiana_domain::CapabilityErrorCode::ResultUnknown);
        let confirmed = !unknown
            && (result.output["cancelled"] != true || result.output["stop_confirmed"] == true);
        let stop_requested = *cancellation.borrow();
        stop_tx.send_replace(Some(confirmed));
        let invocation = typed_invocation_identity(
            permit.run_id,
            permit.turn_id,
            permit.invocation_id,
            permit.execution_id,
            &executed_request,
            1,
        )?;
        let stream = self
            .events
            .read_stream("execution_permit", &execution_id.to_string())
            .await?;
        let version = stream
            .iter()
            .filter_map(|event| event.stream_version)
            .max()
            .unwrap_or(0);
        let final_id = derived_request_id("execution.result", &execution_id.to_string());
        let event=RuntimeEvent::new(final_id,1,"execution.result_committed",json!({
            "run_id":run_id,"turn_id":turn_id,"invocation_id":invocation_id,"execution_id":execution_id,
            "capability_request_id":permit.request_id,"result":result,"attempt":1,
            "effect_started":true,"effect_known":!unknown,"zero_effect":false,
            "stop_state":if stop_requested { if confirmed {"confirmed"} else {"unconfirmed"} } else {"not_requested"},
            "stop_requested":stop_requested,"stop_confirmed":stop_requested.then_some(confirmed),"fenced":unknown,
            "invocation":invocation,
        })).map_err(|e|dispatch_error(&e.to_string()))?.with_stream_metadata("execution_permit",execution_id.to_string(),version+1);
        let batch = TransitionBatch {
            command_id: final_id,
            command_digest: json_digest(&json!(result)),
            expected_versions: vec![AggregateVersion {
                aggregate_type: "execution_permit".to_owned(),
                aggregate_id: execution_id.to_string(),
                version,
            }],
            events: vec![event],
        };
        if let Some(run_id) = run_id {
            self.invalidate_invocation_projection(run_id);
        }
        commit_confirmed(self.events.as_ref(), batch)
            .await
            .map_err(|e| dispatch_error(&format!("result_unknown:result_commit_failed:{e}")))?;
        Ok(result)
    }

    /// Record the handler boundary before invoking a capability.  If this CAS cannot be
    /// committed, the handler is never called and the prepared permit remains fenced for
    /// reconciliation.  This keeps execution evidence causally after admission/permit facts.
    async fn commit_invocation_executing(
        &self,
        permit: &DispatchPermit,
        request: &CapabilityRequest,
    ) -> Result<(), PortError> {
        let id = permit.execution_id.to_string();
        let stream = self.events.read_stream("execution_permit", &id).await?;
        let version = stream
            .iter()
            .filter_map(|event| event.stream_version)
            .max()
            .unwrap_or(0);
        let command_id = derived_request_id("execution.start", &id);
        if self.events.read_command(&command_id).await?.is_some() {
            return Err(dispatch_error(
                "result_unknown:execution_start_already_recorded",
            ));
        }
        let invocation = typed_invocation_identity(
            permit.run_id,
            permit.turn_id,
            permit.invocation_id,
            permit.execution_id,
            request,
            1,
        )?;
        let event = RuntimeEvent::new(
            command_id,
            1,
            "invocation.executing",
            json!({
                "run_id": permit.run_id,
                "turn_id": permit.turn_id,
                "invocation_id": permit.invocation_id,
                "execution_id": permit.execution_id,
                "capability_request_id": permit.request_id,
                "operation": request.operation,
                "attempt": 1,
                "started": true,
                "effect_started": true,
                "effect_known": true,
                "zero_effect": false,
                "stop_state": "not_requested",
                "fenced": true,
                "boundary": "handler_execution",
                "invocation": invocation,
            }),
        )
        .map_err(|error| dispatch_error(&error.to_string()))?
        .with_stream_metadata("execution_permit", id.clone(), version.saturating_add(1));
        let batch = TransitionBatch {
            command_id,
            command_digest: json_digest(&json!({
                "execution_id": permit.execution_id,
                "request_id": permit.request_id,
                "action_digest": permit.action_digest,
            })),
            expected_versions: vec![AggregateVersion::new("execution_permit", id, version)],
            events: vec![event],
        };
        if commit_confirmed(self.events.as_ref(), batch).await? {
            return Err(dispatch_error(
                "result_unknown:execution_start_already_recorded",
            ));
        }
        Ok(())
    }
}

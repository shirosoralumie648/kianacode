use super::redaction::*;
use super::*;

impl ControlPlane {
    pub(crate) async fn record_event(
        &self,
        request_id: kiana_domain::RequestId,
        sequence: &mut u64,
        kind: &str,
        data: Value,
    ) -> Result<(), CoreError> {
        self.append_event(request_id, *sequence, kind, data).await?;
        *sequence += 1;
        Ok(())
    }

    pub(crate) async fn append_event(
        &self,
        request_id: kiana_domain::RequestId,
        sequence: u64,
        kind: &str,
        data: Value,
    ) -> Result<(), CoreError> {
        // EventLog is the canonical boundary: generic runner output must be redacted before it
        // can become durable fact or feed a receipt projection.
        let data = redact_event_value(&data);
        let (aggregate_type, aggregate_id) = aggregate_for_event(request_id, &data);
        let base_idempotency_key =
            format!("{request_id}:{aggregate_type}:{aggregate_id}:{sequence}:{kind}");
        let idempotency_key = base_idempotency_key;
        for _ in 0..4 {
            let current_version = self
                .events
                .read_stream(&aggregate_type, &aggregate_id)
                .await?
                .iter()
                .map(|event| event.stream_version.unwrap_or(event.sequence))
                .max()
                .unwrap_or(0);
            let event = RuntimeEvent::new(request_id, sequence, kind, data.clone())?
                .with_stream_metadata(
                    aggregate_type.clone(),
                    aggregate_id.clone(),
                    current_version.saturating_add(1),
                )
                .with_idempotency_key(idempotency_key.clone());
            match self
                .events
                .append_idempotent_expected(event, Some(current_version))
                .await
            {
                Ok(_) => return Ok(()),
                Err(PortError::Conflict(reason))
                    if reason == "event_stream_version_mismatch"
                        || reason == "event_sequence_not_monotonic" => {}
                Err(error) => return Err(error.into()),
            }
        }
        Err(CoreError::Port(PortError::Conflict(
            "event_append_contention".to_owned(),
        )))
    }

    pub(crate) async fn record_terminal_event(
        &self,
        request_id: kiana_domain::RequestId,
        sequence: &mut u64,
        run_id: RunId,
        kind: &str,
        data: Value,
    ) -> Result<bool, CoreError> {
        let scope = self
            .active_terminal_scopes
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .get(&run_id)
            .cloned();
        let _scope_lock = match scope.as_ref() {
            Some(scope) => Some(scope.recorded.lock().await),
            None => None,
        };
        for _ in 0..8 {
            let prior = self.events.read_stream("run", &run_id.to_string()).await?;
            let turn = prior
                .iter()
                .rposition(|event| event.kind == "run.prompt")
                .unwrap_or(0);
            if prior[turn..].iter().any(|event| {
                matches!(
                    event.kind.as_str(),
                    "run.completed" | "run.failed" | "run.cancelled" | "run.result_unknown"
                )
            }) {
                return Ok(false);
            }
            let version = prior
                .iter()
                .filter_map(|event| event.stream_version)
                .max()
                .unwrap_or(0);
            let event = RuntimeEvent::new(request_id, *sequence, kind, redact_event_value(&data))?
                .with_stream_metadata("run", run_id.to_string(), version + 1)
                .with_idempotency_key(format!("run:{run_id}:turn:{turn}:terminal"));
            let mut expected_versions = vec![kiana_domain::AggregateVersion {
                aggregate_type: "run".to_owned(),
                aggregate_id: run_id.to_string(),
                version,
            }];
            let mut terminal_events = vec![event];
            if kind == "run.result_unknown" {
                if let Some(root) = prior
                    .iter()
                    .find(|event| event.kind == "run.authorized")
                    .and_then(|event| event.data["project_root"].as_str())
                {
                    let key = kiana_domain::json_digest(
                        &json!({"project_root":Self::canonical_project_root(root)}),
                    );
                    let records = self.events.read_stream("resource_quarantine", &key).await?;
                    let version = records
                        .iter()
                        .filter_map(|event| event.stream_version)
                        .max()
                        .unwrap_or(0);
                    expected_versions.push(kiana_domain::AggregateVersion {
                        aggregate_type: "resource_quarantine".to_owned(),
                        aggregate_id: key.clone(),
                        version,
                    });
                    terminal_events.push(RuntimeEvent::new(request_id,*sequence+1,"resource.quarantined",json!({"run_id":run_id,"project_root":root,"reason":data["error"],"release_requires_stop_evidence":true}))?
                        .with_stream_metadata("resource_quarantine",key,version+1));
                }
            }
            let command_id =
                kiana_domain::derived_request_id("run.terminal", &format!("{run_id}:{turn}"));
            let batch = kiana_domain::TransitionBatch {
                command_id,
                command_digest: kiana_domain::json_digest(&json!({"kind":kind,"data":data})),
                expected_versions,
                events: terminal_events,
            };
            match super::dispatch::commit_confirmed(self.events.as_ref(), batch).await {
                Ok(_) => {
                    *sequence += 1;
                    self.queue_terminal_distillation(run_id, kind).await;
                    return Ok(true);
                }
                Err(PortError::Conflict(_)) => continue,
                Err(error) => return Err(error.into()),
            }
        }
        Err(PortError::Conflict("run_terminal_contention".to_owned()).into())
    }

    pub(crate) fn begin_terminal_scope(&self, run_id: RunId) -> TerminalScopeGuard<'_> {
        self.active_terminal_scopes
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .insert(
                run_id,
                Arc::new(RunTerminalScope {
                    recorded: AsyncMutex::new(false),
                }),
            );
        TerminalScopeGuard {
            control_plane: self,
            run_id,
        }
    }

    pub(crate) fn end_terminal_scope(&self, run_id: RunId) {
        self.active_terminal_scopes
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .remove(&run_id);
    }
}

pub(crate) fn aggregate_for_event(
    request_id: kiana_domain::RequestId,
    data: &Value,
) -> (String, String) {
    if let Some(packet_id) = data
        .get("packet_id")
        .and_then(Value::as_str)
        .filter(|packet_id| !packet_id.trim().is_empty())
    {
        return ("work_packet".to_owned(), packet_id.to_owned());
    }
    if let Some(run_id) = data
        .get("run_id")
        .and_then(Value::as_str)
        .filter(|run_id| !run_id.trim().is_empty())
    {
        return ("run".to_owned(), run_id.to_owned());
    }
    ("request".to_owned(), request_id.to_string())
}
pub(crate) fn run_identity(context: &RequestContext, run_id: RunId, sandbox: &str) -> Value {
    let worker = RoleSpec::lookup(&context.role_id).unwrap_or_else(RoleSpec::builder);
    with_work_packet(
        json!({
            "schema": RUN_RESULT_SCHEMA,
            "run_id": run_id,
            "session_id": context.session_id,
            "harness": HARNESS_ID,
            "sandbox": sandbox,
            "actor_id": context.actor_id,
            "role_id": worker.role_id,
            "department_id": worker.department_id,
            "prompt_hash": worker.prompt_hash,
        }),
        context,
    )
}
pub(crate) fn with_work_packet(mut receipt: Value, context: &RequestContext) -> Value {
    if let Some(work_packet_id) = context
        .work_packet_id
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        receipt["work_packet_id"] = json!(work_packet_id);
        receipt["input"] = json!("work_packet");
    }
    receipt
}
pub(crate) fn capability_event_payload(
    output: &Value,
    request: &CapabilityRequest,
    context: &RequestContext,
    run_id: RunId,
) -> Value {
    let mut payload = redact_event_value(output);
    if !payload.is_object() {
        payload = json!({ "output": payload });
    }
    let object = payload
        .as_object_mut()
        .expect("capability event payload is normalized to an object");
    object.insert("run_id".to_owned(), json!(run_id));
    object.insert("session_id".to_owned(), json!(context.session_id));
    object.insert("capability".to_owned(), json!(request.capability));
    object.insert("operation".to_owned(), json!(request.operation));
    object.insert("cell_id".to_owned(), json!(request.cell_id));
    object.insert(
        "capability_grant_id".to_owned(),
        json!(request.capability_grant_id),
    );
    object.insert("budget_lease_id".to_owned(), json!(request.budget_lease_id));
    object.insert(
        "capability_request_id".to_owned(),
        json!(request.request_id),
    );
    payload
}

pub(crate) fn direct_capability_event_payload(
    output: &Value,
    request: &CapabilityRequest,
) -> Value {
    let mut payload = redact_event_value(output);
    if !payload.is_object() {
        payload = json!({ "output": payload });
    }
    let object = payload
        .as_object_mut()
        .expect("direct capability payload is normalized to an object");
    object.insert("capability".to_owned(), json!(request.capability));
    object.insert("operation".to_owned(), json!(request.operation));
    object.insert("cell_id".to_owned(), json!(request.cell_id));
    object.insert(
        "capability_grant_id".to_owned(),
        json!(request.capability_grant_id),
    );
    object.insert("budget_lease_id".to_owned(), json!(request.budget_lease_id));
    object.insert(
        "capability_request_id".to_owned(),
        json!(request.request_id),
    );
    payload
}
pub(crate) fn stamp_request_identity(request: &mut CapabilityRequest, context: &RequestContext) {
    let Some(arguments) = request.arguments.as_object_mut() else {
        return;
    };
    arguments.insert("role_id".to_owned(), json!(context.role_id));
    arguments.insert("department_id".to_owned(), json!(context.department_id));
    arguments.insert("session_id".to_owned(), json!(context.session_id.as_str()));
    arguments.insert("project_root".to_owned(), json!(context.project_root));
    arguments.insert("actor_id".to_owned(), json!(context.actor_id));
    arguments.insert("project_trusted".to_owned(), json!(context.project_trusted));
    let role = RoleSpec::lookup(&context.role_id);
    let role_paths = role.map(|role| role.path_allow).unwrap_or_default();
    let paths = if context.path_allow.is_empty() {
        role_paths
    } else {
        let mut intersection = Vec::new();
        for allowed in &context.path_allow {
            if kiana_domain::allow_list_covers(&role_paths, allowed) {
                intersection.push(allowed.clone());
            }
        }
        for allowed in &role_paths {
            if kiana_domain::allow_list_covers(&context.path_allow, allowed) {
                intersection.push(allowed.clone());
            }
        }
        intersection.sort();
        intersection.dedup();
        intersection
    };
    arguments.insert("path_allow".to_owned(), json!(paths));
}

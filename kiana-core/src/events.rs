use super::redaction::*;
use super::*;
use kiana_domain::CapabilityErrorCode;
use serde::de::DeserializeOwned;

fn optional_event_link<T: DeserializeOwned>(
    data: &Value,
    field: &str,
) -> Result<Option<T>, CoreError> {
    match data.get(field) {
        None | Some(Value::Null) => Ok(None),
        Some(value) => serde_json::from_value(value.clone())
            .map(Some)
            .map_err(|_| PortError::Failed(format!("event_{field}_invalid")).into()),
    }
}

fn result_unknown_value(value: &Value) -> bool {
    value
        .get("error_code")
        .and_then(Value::as_str)
        .map(CapabilityErrorCode::from_reason)
        .or_else(|| {
            value
                .get("error")
                .and_then(Value::as_str)
                .map(CapabilityErrorCode::from_reason)
        })
        .is_some_and(|code| {
            matches!(
                code,
                CapabilityErrorCode::ResultUnknown | CapabilityErrorCode::CompensationRequired
            )
        })
}

fn stamp_event_links(
    event: RuntimeEvent,
    request_id: RequestId,
    data: &Value,
) -> Result<RuntimeEvent, CoreError> {
    Ok(event.with_identity_links(
        optional_event_link(data, "command_id")?,
        optional_event_link(data, "correlation_id")?.or(Some(request_id)),
        optional_event_link(data, "causation_event_id")?,
        optional_event_link(data, "parent_event_id")?,
    ))
}

fn payload_depth(value: &Value, depth: usize) -> bool {
    if depth > kiana_domain::MAX_REDACTION_DEPTH {
        return false;
    }
    match value {
        Value::Array(items) => items.iter().all(|item| payload_depth(item, depth + 1)),
        Value::Object(fields) => fields
            .iter()
            .all(|(key, value)| !key.contains('\0') && payload_depth(value, depth + 1)),
        Value::String(text) => !text.contains('\0'),
        _ => true,
    }
}

fn event_artifact_refs(data: &Value) -> Result<Vec<String>, CoreError> {
    let mut refs = Vec::new();
    if let Some(reference) = data.get("artifact_ref") {
        let reference = reference
            .as_str()
            .filter(|value| !value.trim().is_empty())
            .ok_or_else(|| PortError::Failed("event_artifact_ref_invalid".to_owned()))?;
        refs.push(reference.to_owned());
    }
    if let Some(values) = data.get("artifact_refs") {
        let values = values
            .as_array()
            .ok_or_else(|| PortError::Failed("event_artifact_refs_invalid".to_owned()))?;
        for value in values {
            let reference = value
                .as_str()
                .filter(|value| !value.trim().is_empty())
                .ok_or_else(|| PortError::Failed("event_artifact_refs_invalid".to_owned()))?;
            refs.push(reference.to_owned());
        }
    }
    refs.sort_unstable();
    refs.dedup();
    if refs.len() > 256 || refs.iter().any(|reference| reference.len() > 4_096) {
        return Err(PortError::Failed("event_artifact_refs_invalid".to_owned()).into());
    }
    Ok(refs)
}

fn prepare_event_payload(
    data: &Value,
) -> Result<(Value, String, Option<u64>, Vec<String>), CoreError> {
    let redacted = redact_event_value(data);
    if !payload_depth(&redacted, 0) {
        return Err(PortError::Failed("event_payload_depth_limit".to_owned()).into());
    }
    let bytes = serde_json::to_vec(&redacted)
        .map_err(|_| PortError::Failed("event_payload_encode_failed".to_owned()))?;
    if bytes.len() > kiana_domain::MAX_JOURNAL_EVENT_BYTES {
        return Err(PortError::Failed("event_payload_size_limit".to_owned()).into());
    }
    if redact_event_value(&redacted) != redacted {
        return Err(PortError::Failed("event_redaction_not_stable".to_owned()).into());
    }
    let data_epoch = match redacted.get("data_epoch") {
        None | Some(Value::Null) => None,
        Some(value) => Some(
            value
                .as_u64()
                .ok_or_else(|| PortError::Failed("event_data_epoch_invalid".to_owned()))?,
        ),
    };
    let artifact_refs = event_artifact_refs(&redacted)?;
    let profile = kiana_domain::RedactionProfile::for_signal(kiana_domain::RedactionSignal::Audit);
    Ok((redacted, profile.profile_digest, data_epoch, artifact_refs))
}

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
        let (data, redaction_profile, data_epoch, artifact_refs) = prepare_event_payload(&data)?;
        let (aggregate_type, aggregate_id) = aggregate_for_event(request_id, &data);
        // Invalidate before attempting the CAS append as well as after success: an adapter may
        // report an ambiguous error after durably writing the event, and a stale fold must never
        // survive that uncertainty.
        let affected_run = data
            .get("run_id")
            .and_then(Value::as_str)
            .and_then(RunId::parse_str)
            .or_else(|| {
                (aggregate_type == "run")
                    .then(|| RunId::parse_str(&aggregate_id))
                    .flatten()
            });
        if let Some(run_id) = affected_run {
            self.invalidate_invocation_projection(run_id);
        }
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
            let event = stamp_event_links(
                RuntimeEvent::new(request_id, sequence, kind, data.clone())?
                    .with_stream_metadata(
                        aggregate_type.clone(),
                        aggregate_id.clone(),
                        current_version.saturating_add(1),
                    )
                    .with_idempotency_key(idempotency_key.clone()),
                request_id,
                &data,
            )?
            .with_redaction_metadata(
                redaction_profile.clone(),
                false,
                data_epoch,
                artifact_refs.clone(),
            );
            match self
                .events
                .append_idempotent_expected(event, Some(current_version))
                .await
            {
                Ok(_) => {
                    if let Some(run_id) = affected_run {
                        self.invalidate_invocation_projection(run_id);
                    }
                    return Ok(());
                }
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
            let (redacted_data, redaction_profile, data_epoch, artifact_refs) =
                prepare_event_payload(&data)?;
            let event = stamp_event_links(
                RuntimeEvent::new(request_id, *sequence, kind, redacted_data.clone())?
                    .with_stream_metadata("run", run_id.to_string(), version + 1)
                    .with_idempotency_key(format!("run:{run_id}:turn:{turn}:terminal")),
                request_id,
                &redacted_data,
            )?
            .with_redaction_metadata(
                redaction_profile,
                false,
                data_epoch,
                artifact_refs,
            );
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
                    self.invalidate_invocation_projection(run_id);
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
    if let Some(message_id) = data
        .get("message_id")
        .and_then(Value::as_str)
        .filter(|message_id| !message_id.trim().is_empty())
    {
        return ("communication".to_owned(), message_id.to_owned());
    }
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
    let worker = RoleSpec::lookup(&context.role_id);
    let role_id = worker
        .as_ref()
        .map(|role| role.role_id.clone())
        .unwrap_or_else(|| context.role_id.clone());
    let department_id = worker
        .as_ref()
        .map(|role| role.department_id.clone())
        .unwrap_or_else(|| context.department_id.clone());
    with_work_packet(
        json!({
            "schema": RUN_RESULT_SCHEMA,
            "run_id": run_id,
            "session_id": context.session_id,
            "harness": HARNESS_ID,
            "sandbox": sandbox,
            "actor_id": context.actor_id,
            "role_id": role_id,
            "department_id": department_id,
            "prompt_hash": worker.as_ref().map(|role| role.prompt_hash.clone()),
            "role_spec_schema": worker.as_ref().map(|role| role.schema.clone()),
            "role_version": worker.as_ref().map(|role| role.version),
            "role_catalog_schema": kiana_domain::ROLE_CATALOG_SCHEMA,
            "role_catalog_version": kiana_domain::SchemaVersion::new(1, 0),
            "input_schema": worker.as_ref().map(|role| role.input_schema.clone()),
            "output_schema": worker.as_ref().map(|role| role.output_schema.clone()),
            "model_profile": worker.as_ref().map(|role| role.model_profile.clone()),
            "role_resolution": worker.is_some(),
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
    let result_unknown = result_unknown_value(&payload);
    let object = payload
        .as_object_mut()
        .expect("capability event payload is normalized to an object");
    let not_executed = object.get("not_executed") == Some(&json!(true));
    let stop_confirmed = object.get("stop_confirmed").and_then(Value::as_bool);
    object.entry("attempt".to_owned()).or_insert(json!(1));
    object
        .entry("effect_started".to_owned())
        .or_insert(json!(!not_executed));
    object
        .entry("effect_known".to_owned())
        .or_insert(json!(!result_unknown));
    object
        .entry("zero_effect".to_owned())
        .or_insert(json!(not_executed));
    object
        .entry("fenced".to_owned())
        .or_insert(json!(result_unknown));
    if let Some(stop_confirmed) = stop_confirmed {
        object
            .entry("stop_state".to_owned())
            .or_insert(json!(if stop_confirmed {
                "confirmed"
            } else {
                "unconfirmed"
            }));
    } else {
        object
            .entry("stop_state".to_owned())
            .or_insert(json!("not_requested"));
    }
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
    let result_unknown = result_unknown_value(&payload);
    let object = payload
        .as_object_mut()
        .expect("direct capability payload is normalized to an object");
    let not_executed = object.get("not_executed") == Some(&json!(true));
    object.entry("attempt".to_owned()).or_insert(json!(1));
    object
        .entry("effect_started".to_owned())
        .or_insert(json!(!not_executed));
    object
        .entry("effect_known".to_owned())
        .or_insert(json!(!result_unknown));
    object
        .entry("zero_effect".to_owned())
        .or_insert(json!(not_executed));
    object
        .entry("fenced".to_owned())
        .or_insert(json!(result_unknown));
    object
        .entry("stop_state".to_owned())
        .or_insert(json!("not_requested"));
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
            if kiana_domain::enforce_path_containment(&role_paths, allowed).is_ok() {
                intersection.push(allowed.clone());
            }
        }
        for allowed in &role_paths {
            if kiana_domain::enforce_path_containment(&context.path_allow, allowed).is_ok() {
                intersection.push(allowed.clone());
            }
        }
        intersection.sort();
        intersection.dedup();
        intersection
    };
    arguments.insert("path_allow".to_owned(), json!(paths));
}

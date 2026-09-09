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
        let idempotency_key =
            format!("{request_id}:{aggregate_type}:{aggregate_id}:{sequence}:{kind}");
        let current_version = self
            .events
            .read_stream(&aggregate_type, &aggregate_id)
            .await?
            .iter()
            .map(|event| event.stream_version.unwrap_or(event.sequence))
            .max()
            .unwrap_or(0);
        self.events
            .append_idempotent_expected(
                RuntimeEvent::new(request_id, sequence, kind, data)?
                    .with_stream_metadata(
                        aggregate_type,
                        aggregate_id,
                        current_version.saturating_add(1),
                    )
                    .with_idempotency_key(idempotency_key),
                Some(current_version),
            )
            .await?;
        Ok(())
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
}

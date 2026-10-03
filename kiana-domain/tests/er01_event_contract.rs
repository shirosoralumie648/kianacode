use kiana_domain::{
    check_event_schema_version, event_kind_spec, event_migration, unknown_event_policy,
    validate_event_payload, validate_runtime_event, EventKindSpec, RequestId, RuntimeEvent,
    SchemaVersion, UnknownEventPolicy, EVENT_KIND_SPECS, RUNTIME_EVENT_SCHEMA,
};
use serde_json::json;

#[test]
fn event_kind_registry_is_machine_readable_and_bounded() {
    assert!(!EVENT_KIND_SPECS.is_empty());
    for spec in EVENT_KIND_SPECS {
        assert_eq!(spec.schema, RUNTIME_EVENT_SCHEMA);
        assert_eq!(spec.owner_crate, "kiana-domain");
        assert!(!spec.kind.is_empty());
        assert!(!spec.aggregate_type.is_empty());
        assert!(!spec.secret_policy.is_empty());
        let _: EventKindSpec = *spec;
    }
    assert!(event_kind_spec("run.completed").is_some_and(|spec| spec.terminal));
    assert!(event_kind_spec("run.prompt").is_some_and(|spec| !spec.terminal));
}

#[test]
fn request_event_identity_is_required_in_the_envelope_not_duplicated_in_payload() {
    for kind in ["request.accepted", "request.rejected"] {
        let spec = event_kind_spec(kind).unwrap();
        assert!(spec.required_ids.is_empty(), "{kind}");
    }

    let accepted = RuntimeEvent::new(
        RequestId::new(),
        1,
        "request.accepted",
        json!({"command": "run.start"}),
    )
    .unwrap();
    validate_runtime_event(&accepted).unwrap();

    let rejected = RuntimeEvent::new(
        RequestId::new(),
        1,
        "request.rejected",
        json!({"reason": "prompt_required"}),
    )
    .unwrap();
    validate_runtime_event(&rejected).unwrap();
}

#[test]
fn unknown_required_kind_and_schema_downgrade_fail_closed() {
    assert_eq!(
        unknown_event_policy("future.opaque").unwrap(),
        UnknownEventPolicy::PreserveOpaqueWithoutExecution
    );
    assert_eq!(
        unknown_event_policy("run.future").unwrap_err(),
        "unknown_required_event_kind"
    );
    assert_eq!(
        check_event_schema_version("run.completed", &SchemaVersion::new(0, 0)).unwrap_err(),
        "event_schema_version_incompatible"
    );
    assert_eq!(
        event_migration("run.completed", 0, 1),
        Some("legacy_run_event_v0_to_v1")
    );
    assert!(event_migration("future.opaque", 0, 1).is_none());
}

#[test]
fn payload_unknown_field_is_not_silently_dropped() {
    let run_id = kiana_domain::RunId::new();
    let valid = json!({"run_id":run_id,"error":"failed","turn_id":kiana_domain::TurnId::new()});
    validate_event_payload("run.failed", &valid).unwrap();
    let mut unknown = valid;
    unknown["unexpected"] = json!(true);
    assert_eq!(
        validate_event_payload("run.failed", &unknown).unwrap_err(),
        "event_payload_unknown_field"
    );
    let event = RuntimeEvent::new(
        RequestId::new(),
        1,
        "future.opaque",
        json!({"untrusted":true}),
    )
    .unwrap();
    validate_runtime_event(&event).unwrap();
    let required = RuntimeEvent::new(RequestId::new(), 1, "run.future", json!({})).unwrap();
    assert_eq!(
        validate_runtime_event(&required).unwrap_err(),
        "unknown_required_event_kind"
    );
}

#[test]
fn execution_prepared_contract_requires_server_identity_envelope() {
    let spec = event_kind_spec("execution.prepared").unwrap();
    for field in [
        "run_id",
        "turn_id",
        "invocation_id",
        "execution_id",
        "capability_request_id",
        "action_digest",
        "attempt",
        "permit",
        "cell_reservation",
        "invocation",
    ] {
        assert!(
            spec.allowed_fields.contains(&field),
            "execution.prepared field not allowed: {field}"
        );
    }
    for field in [
        "execution_id",
        "invocation_id",
        "capability_request_id",
        "action_digest",
        "attempt",
    ] {
        assert!(
            spec.required_ids.contains(&field),
            "execution.prepared identity field not required: {field}"
        );
    }

    let payload = json!({
        "run_id":null,
        "turn_id":null,
        "invocation_id":kiana_domain::InvocationId::new(),
        "execution_id":kiana_domain::ExecutionId::new(),
        "capability_request_id":kiana_domain::RequestId::new(),
        "action_digest":format!("sha256:{}", "a".repeat(64)),
        "attempt":1,
        "permit":{},
        "cell_reservation":null,
        "invocation":null,
    });
    validate_event_payload("execution.prepared", &payload).unwrap();

    let mut missing_execution = payload;
    missing_execution
        .as_object_mut()
        .unwrap()
        .remove("execution_id");
    assert_eq!(
        validate_event_payload("execution.prepared", &missing_execution).unwrap_err(),
        "event_required_id_missing:execution_id"
    );
}

#[test]
fn result_event_contracts_accept_only_their_result_fields() {
    let invocation = event_kind_spec("invocation.executing").unwrap();
    let execution_result = event_kind_spec("execution.result_committed").unwrap();
    let capability_result = event_kind_spec("capability.completed").unwrap();

    for field in invocation.allowed_fields {
        assert!(execution_result.allowed_fields.contains(field));
        assert!(capability_result.allowed_fields.contains(field));
    }
    for field in ["outcome_state", "outcome_ready", "result_receipt"] {
        assert!(
            execution_result.allowed_fields.contains(&field),
            "execution.result_committed field not allowed: {field}"
        );
    }
    assert_eq!(
        execution_result.allowed_fields.len(),
        invocation.allowed_fields.len() + 3
    );
    for field in ["result_receipt", "result_source"] {
        assert!(
            capability_result.allowed_fields.contains(&field),
            "capability terminal result field not allowed: {field}"
        );
    }
    assert!(
        capability_result.allowed_fields.contains(&"session_id"),
        "run-scoped capability terminal result must retain its session identity"
    );
    assert!(
        capability_result.allowed_fields.contains(&"stdout"),
        "capability terminal result must retain the broker stdout field"
    );
    assert!(
        capability_result.allowed_fields.contains(&"outcome"),
        "capability terminal result must retain normalized outcome dimensions"
    );
    assert_eq!(
        capability_result.allowed_fields.len(),
        invocation.allowed_fields.len() + 5
    );

    let mut execution_payload = json!({
        "run_id":kiana_domain::RunId::new(),
        "capability_request_id":kiana_domain::RequestId::new(),
        "outcome_state":"succeeded",
        "outcome_ready":true,
        "result_receipt":{"receipt_digest":format!("sha256:{}", "a".repeat(64))},
        "unknown_result_field":true,
    });
    assert_eq!(
        validate_event_payload("execution.result_committed", &execution_payload).unwrap_err(),
        "event_payload_unknown_field"
    );
    execution_payload
        .as_object_mut()
        .unwrap()
        .remove("unknown_result_field");
    validate_event_payload("execution.result_committed", &execution_payload).unwrap();

    let capability_payload = json!({
        "run_id":kiana_domain::RunId::new(),
        "session_id":"session-1",
        "capability_request_id":kiana_domain::RequestId::new(),
        "stdout":"scope captured",
        "outcome": {"schema":"kiana.capability-outcome.v1"},
        "result_receipt":{"receipt_digest":format!("sha256:{}", "b".repeat(64))},
        "result_source":"model-claimed-user-source",
    });
    for kind in [
        "capability.completed",
        "capability.failed",
        "capability.cancelled",
        "capability.result_unknown",
    ] {
        validate_event_payload(kind, &capability_payload).unwrap();
    }

    let mut unknown_capability_result = capability_payload;
    unknown_capability_result["unknown_result_field"] = json!(true);
    assert_eq!(
        validate_event_payload("capability.completed", &unknown_capability_result).unwrap_err(),
        "event_payload_unknown_field"
    );
}

#[test]
fn capability_requested_and_result_delivery_contracts_match_producers() {
    let invocation = event_kind_spec("invocation.executing").unwrap();
    let capability_requested = event_kind_spec("run.capability_requested").unwrap();
    assert!(capability_requested.allowed_fields.contains(&"request_id"));
    assert!(capability_requested
        .allowed_fields
        .contains(&"capability_request_id"));
    assert!(capability_requested
        .required_ids
        .contains(&"capability_request_id"));

    let mut capability_payload = json!({
        "run_id": kiana_domain::RunId::new(),
        "request_id": kiana_domain::RequestId::new(),
        "capability_request_id": kiana_domain::RequestId::new(),
        "capability": "shell",
        "operation": "shell.exec",
        "risk": "low",
        "attempt": 1,
        "effect_started": false,
        "effect_known": true,
        "zero_effect": true,
        "stop_state": "not_requested",
        "fenced": false,
    });
    validate_event_payload("run.capability_requested", &capability_payload).unwrap();
    capability_payload
        .as_object_mut()
        .unwrap()
        .remove("capability_request_id");
    assert_eq!(
        validate_event_payload("run.capability_requested", &capability_payload).unwrap_err(),
        "event_required_id_missing:capability_request_id"
    );

    let delivery = event_kind_spec("result.delivery_claimed").unwrap();
    for field in [
        "result_digest",
        "receipt_digest",
        "outcome_state",
        "outcome_ready",
        "delivery_policy",
    ] {
        assert!(
            delivery.allowed_fields.contains(&field),
            "result.delivery_claimed field not allowed: {field}"
        );
    }
    assert_eq!(
        delivery.allowed_fields.len(),
        invocation.allowed_fields.len() + 5
    );

    let mut delivery_payload = json!({
        "run_id": kiana_domain::RunId::new(),
        "capability_request_id": kiana_domain::RequestId::new(),
        "result_digest": format!("sha256:{}", "a".repeat(64)),
        "receipt_digest": format!("sha256:{}", "b".repeat(64)),
        "outcome_state": "succeeded",
        "outcome_ready": true,
        "delivery_policy": "single_advance",
    });
    validate_event_payload("result.delivery_claimed", &delivery_payload).unwrap();
    delivery_payload["unknown_delivery_field"] = json!(true);
    assert_eq!(
        validate_event_payload("result.delivery_claimed", &delivery_payload).unwrap_err(),
        "event_payload_unknown_field"
    );
}

#[test]
fn run_tool_contracts_match_cancel_and_dispatch_producers() {
    let tool_call = event_kind_spec("run.tool_call").unwrap();
    assert_eq!(
        tool_call.allowed_fields,
        &[
            "run_id",
            "capability_request_id",
            "call_id",
            "turn_id",
            "step_id",
            "invocation_id",
            "execution_scope",
            "tool",
            "operation",
        ][..]
    );
    let mut call_payload = json!({
        "run_id": kiana_domain::RunId::new(),
        "capability_request_id": kiana_domain::RequestId::new(),
        "call_id": "call-1",
        "turn_id": kiana_domain::TurnId::new(),
        "step_id": kiana_domain::StepId::new(),
        "invocation_id": kiana_domain::InvocationId::new(),
        "execution_scope": null,
        "tool": "shell",
        "operation": "shell.exec",
    });
    validate_event_payload("run.tool_call", &call_payload).unwrap();
    call_payload["arguments"] = json!({"command":"pwd"});
    assert_eq!(
        validate_event_payload("run.tool_call", &call_payload).unwrap_err(),
        "event_payload_unknown_field"
    );
    call_payload.as_object_mut().unwrap().remove("arguments");
    call_payload
        .as_object_mut()
        .unwrap()
        .remove("capability_request_id");
    assert_eq!(
        validate_event_payload("run.tool_call", &call_payload).unwrap_err(),
        "event_required_id_missing:capability_request_id"
    );

    let tool_result = event_kind_spec("run.tool_result").unwrap();
    assert_eq!(
        tool_result.allowed_fields,
        &[
            "run_id",
            "capability_request_id",
            "call_id",
            "result",
            "cancelled",
            "not_executed",
            "attempt",
            "effect_started",
            "effect_known",
            "zero_effect",
            "stop_state",
            "stop_confirmed",
            "fenced",
        ][..]
    );
    let mut result_payload = json!({
        "run_id": kiana_domain::RunId::new(),
        "capability_request_id": kiana_domain::RequestId::new(),
        "call_id": "call-1",
        "result": {"error":"cancelled:user","not_executed":true},
        "cancelled": true,
        "not_executed": true,
        "attempt": 1,
        "effect_started": false,
        "effect_known": true,
        "zero_effect": true,
        "stop_state": "confirmed",
        "stop_confirmed": true,
        "fenced": false,
    });
    validate_event_payload("run.tool_result", &result_payload).unwrap();
    result_payload["unknown_tool_result_field"] = json!(true);
    assert_eq!(
        validate_event_payload("run.tool_result", &result_payload).unwrap_err(),
        "event_payload_unknown_field"
    );
}

#[test]
fn run_predecessor_contract_matches_lifecycle_producer() {
    let spec = event_kind_spec("run.predecessor").unwrap();
    assert_eq!(spec.required_ids, &["run_id"][..]);
    assert_eq!(
        spec.allowed_fields,
        &[
            "run_id",
            "previous_run_id",
            "turn_id",
            "turn",
            "session_id",
            "semantics",
        ][..]
    );

    let mut payload = json!({
        "run_id": kiana_domain::RunId::new(),
        "previous_run_id": kiana_domain::RunId::new(),
        "turn_id": kiana_domain::TurnId::new(),
        "turn": {},
        "session_id": "session-1",
        "semantics": "new_turn_v2",
    });
    validate_event_payload("run.predecessor", &payload).unwrap();

    let mut missing_run = payload.clone();
    missing_run.as_object_mut().unwrap().remove("run_id");
    assert_eq!(
        validate_event_payload("run.predecessor", &missing_run).unwrap_err(),
        "event_required_id_missing:run_id"
    );

    payload["unexpected_predecessor_field"] = json!(true);
    assert_eq!(
        validate_event_payload("run.predecessor", &payload).unwrap_err(),
        "event_payload_unknown_field"
    );
}

#[test]
fn run_resume_prepared_contract_matches_recovery_producer() {
    let spec = event_kind_spec("run.resume_prepared").unwrap();
    assert_eq!(spec.required_ids, &["run_id"][..]);
    assert_eq!(
        spec.allowed_fields,
        &[
            "run_id",
            "session_id",
            "actor_id",
            "snapshot_event_id",
            "turn_id",
            "turn",
        ][..]
    );

    let mut payload = json!({
        "run_id": kiana_domain::RunId::new(),
        "session_id": "session-1",
        "actor_id": "actor-1",
        "snapshot_event_id": kiana_domain::EventId::new(),
        "turn_id": kiana_domain::TurnId::new(),
        "turn": {},
    });
    validate_event_payload("run.resume_prepared", &payload).unwrap();

    let mut missing_run = payload.clone();
    missing_run.as_object_mut().unwrap().remove("run_id");
    assert_eq!(
        validate_event_payload("run.resume_prepared", &missing_run).unwrap_err(),
        "event_required_id_missing:run_id"
    );

    payload["unexpected_resume_field"] = json!(true);
    assert_eq!(
        validate_event_payload("run.resume_prepared", &payload).unwrap_err(),
        "event_payload_unknown_field"
    );
}

#[test]
fn run_compacted_contract_matches_lifecycle_producer() {
    let spec = event_kind_spec("run.compacted").unwrap();
    assert_eq!(spec.required_ids, &["run_id"][..]);
    assert!(!spec.terminal);
    assert_eq!(
        event_migration("run.compacted", 0, 1),
        Some("legacy_run_event_v0_to_v1")
    );
    assert_eq!(
        spec.allowed_fields,
        &[
            "schema",
            "run_id",
            "tokens_before",
            "tokens_after",
            "summary_present",
        ][..]
    );

    let mut payload = json!({
        "schema": "kiana.compact.v1",
        "run_id": kiana_domain::RunId::new(),
        "tokens_before": 100,
        "tokens_after": 40,
        "summary_present": true,
    });
    validate_event_payload("run.compacted", &payload).unwrap();

    let mut missing_run = payload.clone();
    missing_run.as_object_mut().unwrap().remove("run_id");
    assert_eq!(
        validate_event_payload("run.compacted", &missing_run).unwrap_err(),
        "event_required_id_missing:run_id"
    );

    payload["unexpected_compaction_field"] = json!(true);
    assert_eq!(
        validate_event_payload("run.compacted", &payload).unwrap_err(),
        "event_payload_unknown_field"
    );
}

#[test]
fn run_receipt_contract_bounds_top_level_projection() {
    let spec = event_kind_spec("run.receipt").unwrap();
    assert_eq!(spec.required_ids, &["run_id"][..]);
    assert!(!spec.terminal);
    assert_eq!(
        event_migration("run.receipt", 0, 1),
        Some("legacy_run_event_v0_to_v1")
    );
    assert_eq!(spec.allowed_fields.len(), 39);

    let mut payload = json!({
        "schema": "kiana.run-result.v1",
        "run_id": kiana_domain::RunId::new(),
        "session_id": "session-1",
        "harness": "kiana-harness",
        "sandbox": "read-only",
        "actor_id": "actor-1",
        "role_id": "builder",
        "department_id": "engineering",
        "role_resolution": true,
        "role_spec_schema": "kiana.role.v1",
        "role_version": 1,
        "role_catalog_schema": "kiana.role-catalog.v1",
        "role_catalog_version": {},
        "input_schema": "kiana.input.v1",
        "output_schema": "kiana.output.v1",
        "model_profile": "offline",
        "max_steps_per_turn": 8,
        "prompt_hash": null,
        "model_turns": [],
        "cost_ledger": [],
        "files_changed": [],
        "memory_hits": 0,
        "retrieval_receipts": [],
        "memory_proposals": [],
        "invocations": null,
        "invocation_projection_error": null,
        "run_receipt": {},
        "receipt_data_binding": {},
        "projection": {},
        "execution_receipts": [],
        "aggregation": {},
        "cost_breakdown": null,
        "effect_usage": null,
        "compact": {},
        "capabilities": {},
        "observability": {},
        "output": {},
        "work_packet_id": "packet-1",
        "input": "work_packet",
    });
    validate_event_payload("run.receipt", &payload).unwrap();

    let mut missing_run = payload.clone();
    missing_run.as_object_mut().unwrap().remove("run_id");
    assert_eq!(
        validate_event_payload("run.receipt", &missing_run).unwrap_err(),
        "event_required_id_missing:run_id"
    );

    payload["unexpected_receipt_field"] = json!(true);
    assert_eq!(
        validate_event_payload("run.receipt", &payload).unwrap_err(),
        "event_payload_unknown_field"
    );
}

#[test]
fn recovery_credential_contract_matches_typed_producer() {
    let spec = event_kind_spec("recovery.credential").unwrap();
    assert_eq!(spec.aggregate_type, "run");
    assert_eq!(spec.required_ids, &["run_id"][..]);
    assert!(!spec.terminal);
    assert!(event_migration("recovery.credential", 0, 1).is_none());
    assert_eq!(spec.allowed_fields, &["run_id", "recovery"][..]);

    let mut payload = json!({
        "run_id": kiana_domain::RunId::new(),
        "recovery": {"schema": "kiana.credential-recovery-event.v1"},
    });
    validate_event_payload("recovery.credential", &payload).unwrap();

    let mut missing_run = payload.clone();
    missing_run.as_object_mut().unwrap().remove("run_id");
    assert_eq!(
        validate_event_payload("recovery.credential", &missing_run).unwrap_err(),
        "event_required_id_missing:run_id"
    );

    payload["unexpected_recovery_field"] = json!(true);
    assert_eq!(
        validate_event_payload("recovery.credential", &payload).unwrap_err(),
        "event_payload_unknown_field"
    );
}

#[test]
fn communication_lifecycle_contracts_match_producers() {
    let handoff_fields = [
        "message",
        "message_id",
        "lifecycle",
        "accepted",
        "reason",
        "authority_granted",
        "project_root",
        "actor_id",
        "session_id",
        "request_id",
    ];
    for (kind, accepted) in [
        ("communication.handoff_acknowledged", true),
        ("communication.handoff_rejected", false),
    ] {
        let spec = event_kind_spec(kind).unwrap();
        assert_eq!(spec.aggregate_type, "communication");
        assert_eq!(spec.required_ids, &["message_id"][..]);
        assert!(!spec.terminal);
        assert!(spec.migration.is_none());
        assert_eq!(spec.allowed_fields, &handoff_fields[..]);

        let mut payload = json!({
            "message": {},
            "message_id": "message-1",
            "lifecycle": {},
            "accepted": accepted,
            "reason": "operator_decision",
            "authority_granted": false,
            "project_root": "/workspace/project",
            "actor_id": "actor-1",
            "session_id": "session-1",
            "request_id": kiana_domain::RequestId::new(),
        });
        validate_event_payload(kind, &payload).unwrap();

        let mut missing_id = payload.clone();
        missing_id.as_object_mut().unwrap().remove("message_id");
        assert_eq!(
            validate_event_payload(kind, &missing_id).unwrap_err(),
            "event_required_id_missing:message_id"
        );

        payload["unexpected_communication_field"] = json!(true);
        assert_eq!(
            validate_event_payload(kind, &payload).unwrap_err(),
            "event_payload_unknown_field"
        );
    }

    let incident = event_kind_spec("communication.incident_escalated").unwrap();
    assert_eq!(incident.aggregate_type, "communication");
    assert_eq!(incident.required_ids, &["message_id"][..]);
    assert!(!incident.terminal);
    assert!(incident.migration.is_none());
    assert_eq!(
        incident.allowed_fields,
        &[
            "message",
            "message_id",
            "lifecycle",
            "evidence_refs",
            "reason",
            "authority_granted",
            "project_root",
            "actor_id",
            "session_id",
            "request_id",
        ][..]
    );
    let mut incident_payload = json!({
        "message": {},
        "message_id": "incident-1",
        "lifecycle": {},
        "evidence_refs": ["evidence-1"],
        "reason": "severity_high",
        "authority_granted": false,
        "project_root": "/workspace/project",
        "actor_id": "actor-1",
        "session_id": "session-1",
        "request_id": kiana_domain::RequestId::new(),
    });
    validate_event_payload("communication.incident_escalated", &incident_payload).unwrap();
    incident_payload["unexpected_communication_field"] = json!(true);
    assert_eq!(
        validate_event_payload("communication.incident_escalated", &incident_payload).unwrap_err(),
        "event_payload_unknown_field"
    );
}

#[test]
fn communication_send_contracts_match_shared_producer() {
    let fields = [
        "message",
        "message_id",
        "lifecycle",
        "authority_granted",
        "project_root",
        "actor_id",
        "session_id",
        "request_id",
    ];
    let kinds = [
        "communication.chat",
        "communication.command",
        "communication.handoff",
        "communication.decision",
        "communication.status_report",
        "communication.evidence",
        "communication.incident",
    ];
    for kind in kinds {
        let spec = event_kind_spec(kind).unwrap();
        assert_eq!(spec.aggregate_type, "communication");
        assert_eq!(spec.required_ids, &["message"][..]);
        assert!(!spec.terminal);
        assert!(spec.migration.is_none());
        assert_eq!(spec.allowed_fields, &fields[..]);

        let mut payload = json!({
            "message": {},
            "message_id": "message-1",
            "lifecycle": {},
            "authority_granted": false,
            "project_root": "/workspace/project",
            "actor_id": "actor-1",
            "session_id": "session-1",
            "request_id": kiana_domain::RequestId::new(),
        });
        validate_event_payload(kind, &payload).unwrap();

        let mut missing_message = payload.clone();
        missing_message.as_object_mut().unwrap().remove("message");
        assert_eq!(
            validate_event_payload(kind, &missing_message).unwrap_err(),
            "event_required_id_missing:message"
        );

        payload["unexpected_send_field"] = json!(true);
        assert_eq!(
            validate_event_payload(kind, &payload).unwrap_err(),
            "event_payload_unknown_field"
        );
    }
}

#[test]
fn connector_health_and_handshake_contracts_match_daemon_producers() {
    let health = event_kind_spec("connector.health_checked").unwrap();
    assert_eq!(health.aggregate_type, "connector");
    assert_eq!(
        health.required_ids,
        &["request_id", "connector_id", "binding_id"][..]
    );
    assert!(!health.terminal);
    assert!(health.migration.is_none());
    assert_eq!(
        health.allowed_fields,
        &[
            "schema",
            "request_id",
            "connector_id",
            "binding_id",
            "status",
            "health",
            "probe_kind",
            "checked_at_unix_ms",
            "project_root",
            "actor_id",
            "authorization_id",
            "request_fingerprint",
        ][..]
    );
    let mut health_payload = json!({
        "schema": "kiana.connector-health-event.v1",
        "request_id": kiana_domain::RequestId::new(),
        "connector_id": "connector-demo",
        "binding_id": "binding-demo",
        "status": "connectivity_only",
        "health": {},
        "probe_kind": "read_only",
        "checked_at_unix_ms": 1_700_000_000_000u64,
        "project_root": "/workspace/project",
        "actor_id": "actor-1",
        "authorization_id": "permit-1",
        "request_fingerprint": "sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
    });
    validate_event_payload("connector.health_checked", &health_payload).unwrap();
    let mut missing_health_id = health_payload.clone();
    missing_health_id
        .as_object_mut()
        .unwrap()
        .remove("binding_id");
    assert_eq!(
        validate_event_payload("connector.health_checked", &missing_health_id).unwrap_err(),
        "event_required_id_missing:binding_id"
    );
    health_payload["unknown_connector_field"] = json!(true);
    assert_eq!(
        validate_event_payload("connector.health_checked", &health_payload).unwrap_err(),
        "event_payload_unknown_field"
    );

    let handshake = event_kind_spec("connector.mcp_handshake").unwrap();
    assert_eq!(handshake.aggregate_type, "connector");
    assert_eq!(
        handshake.required_ids,
        &["request_id", "connector_id", "binding_id"][..]
    );
    assert!(!handshake.terminal);
    assert!(handshake.migration.is_none());
    assert_eq!(
        handshake.allowed_fields,
        &[
            "schema",
            "request_id",
            "connector_id",
            "binding_id",
            "server",
            "actor_id",
            "project_root",
            "authorization_id",
            "handshake",
            "proof_level",
        ][..]
    );
    let mut handshake_payload = json!({
        "schema": "kiana.connector-mcp-handshake-event.v1",
        "request_id": kiana_domain::RequestId::new(),
        "connector_id": "connector-demo",
        "binding_id": "binding-demo",
        "server": "server-demo",
        "actor_id": "actor-1",
        "project_root": "/workspace/project",
        "authorization_id": "permit-1",
        "handshake": {},
        "proof_level": "source",
    });
    validate_event_payload("connector.mcp_handshake", &handshake_payload).unwrap();
    let mut missing_handshake_id = handshake_payload.clone();
    missing_handshake_id
        .as_object_mut()
        .unwrap()
        .remove("connector_id");
    assert_eq!(
        validate_event_payload("connector.mcp_handshake", &missing_handshake_id).unwrap_err(),
        "event_required_id_missing:connector_id"
    );
    handshake_payload["unknown_connector_field"] = json!(true);
    assert_eq!(
        validate_event_payload("connector.mcp_handshake", &handshake_payload).unwrap_err(),
        "event_payload_unknown_field"
    );
}

#[test]
fn session_assignment_contract_matches_producer() {
    let spec = event_kind_spec("session.assigned").unwrap();
    assert_eq!(spec.aggregate_type, "session_assignment");
    assert_eq!(spec.required_ids, &["session_id"][..]);
    assert!(!spec.terminal);
    assert_eq!(
        event_migration("session.assigned", 0, 1),
        Some("legacy_session_event_v0_to_v1")
    );
    assert_eq!(
        spec.allowed_fields,
        &[
            "schema",
            "session_id",
            "actor_id",
            "project_root",
            "role_id",
            "department_id",
            "prompt_hash",
            "model_profile",
            "role_spec_schema",
            "role_version",
            "role_catalog_schema",
            "role_catalog_version",
            "role_input_schema",
            "role_output_schema",
            "authority_epoch",
            "principal",
            "project_identity",
            "assignment",
        ][..]
    );
    let mut payload = json!({
        "schema": "kiana.session-assignment.v1",
        "session_id": "session-1",
        "actor_id": "actor-1",
        "project_root": "/workspace/project",
        "role_id": "builder",
        "department_id": "engineering",
        "prompt_hash": "sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
        "model_profile": "offline",
        "role_spec_schema": "kiana.role.v1",
        "role_version": 1,
        "role_catalog_schema": "kiana.role-catalog.v1",
        "role_catalog_version": {},
        "role_input_schema": "kiana.input.v1",
        "role_output_schema": "kiana.output.v1",
        "authority_epoch": 1,
        "principal": {},
        "project_identity": {},
        "assignment": {},
    });
    validate_event_payload("session.assigned", &payload).unwrap();

    let mut missing_session = payload.clone();
    missing_session
        .as_object_mut()
        .unwrap()
        .remove("session_id");
    assert_eq!(
        validate_event_payload("session.assigned", &missing_session).unwrap_err(),
        "event_required_id_missing:session_id"
    );
    payload["unexpected_session_field"] = json!(true);
    assert_eq!(
        validate_event_payload("session.assigned", &payload).unwrap_err(),
        "event_payload_unknown_field"
    );
}

#[test]
fn capability_blocked_contract_matches_direct_deny_producers() {
    let blocked = event_kind_spec("capability.blocked").unwrap();
    assert_eq!(blocked.aggregate_type, "request");
    assert!(blocked.required_ids.is_empty());
    assert_eq!(
        blocked.allowed_fields,
        &[
            "error",
            "attempt",
            "effect_started",
            "effect_known",
            "zero_effect",
            "stop_state",
            "fenced",
        ][..]
    );
    assert!(event_migration("capability.blocked", 0, 1).is_none());

    validate_event_payload("capability.blocked", &json!({"error":"action_invalid"})).unwrap();
    validate_event_payload(
        "capability.blocked",
        &json!({
            "error":"approval_scope_mismatch",
            "attempt":1,
            "effect_started":false,
            "effect_known":true,
            "zero_effect":true,
            "stop_state":"not_requested",
            "fenced":false,
        }),
    )
    .unwrap();

    assert_eq!(
        validate_event_payload(
            "capability.blocked",
            &json!({"error":"denied","run_id":"run"})
        )
        .unwrap_err(),
        "event_payload_unknown_field"
    );
}

#[test]
fn capability_decision_contract_accepts_policy_and_gate_and_rejects_unknown_fields() {
    let spec = event_kind_spec("capability.decision").unwrap();
    assert!(spec.required_ids.is_empty());
    assert!(spec.allowed_fields.contains(&"policy"));
    assert!(spec.allowed_fields.contains(&"gate"));
    assert_eq!(
        spec.allowed_fields.len(),
        event_kind_spec("invocation.executing")
            .unwrap()
            .allowed_fields
            .len()
            + 2
    );

    let mut payload = json!({
        "run_id": kiana_domain::RunId::new(),
        "capability_request_id": kiana_domain::RequestId::new(),
        "policy": {"decision":"allowed"},
        "gate": {"decision":"allowed"},
        "attempt": 1,
        "effect_started": false,
        "effect_known": true,
        "zero_effect": true,
        "stop_state": "not_requested",
        "fenced": false,
    });
    validate_event_payload("capability.decision", &payload).unwrap();

    let mut direct_payload = payload.clone();
    direct_payload.as_object_mut().unwrap().remove("run_id");
    direct_payload
        .as_object_mut()
        .unwrap()
        .remove("capability_request_id");
    validate_event_payload("capability.decision", &direct_payload).unwrap();

    let mut partial_identity = direct_payload;
    partial_identity["run_id"] = json!(kiana_domain::RunId::new());
    assert_eq!(
        validate_event_payload("capability.decision", &partial_identity).unwrap_err(),
        "event_capability_identity_pair_incomplete"
    );

    payload["unexpected_decision_field"] = json!(true);
    assert_eq!(
        validate_event_payload("capability.decision", &payload).unwrap_err(),
        "event_payload_unknown_field"
    );
}

#[test]
fn approval_staged_contract_matches_journal_producer_and_rejects_unknown_fields() {
    let spec = event_kind_spec("approval.staged").unwrap();
    assert_eq!(spec.aggregate_type, "approval");
    assert_eq!(spec.required_ids, &["approval_id"][..]);
    assert_eq!(
        spec.allowed_fields,
        &["schema", "approval_id", "subject", "state", "at_unix_ms"][..]
    );
    assert!(!spec.terminal);
    assert_eq!(
        event_migration("approval.staged", 0, 1),
        Some("legacy_approval_event_v0_to_v1")
    );

    let mut payload = json!({
        "schema": "kiana.approval.v1",
        "approval_id": kiana_domain::ApprovalId::new(),
        "subject": {},
        "state": "staged",
        "at_unix_ms": 1,
    });
    validate_event_payload("approval.staged", &payload).unwrap();

    let mut missing_id = payload.clone();
    missing_id.as_object_mut().unwrap().remove("approval_id");
    assert_eq!(
        validate_event_payload("approval.staged", &missing_id).unwrap_err(),
        "event_required_id_missing:approval_id"
    );

    payload["unexpected_approval_field"] = json!(true);
    assert_eq!(
        validate_event_payload("approval.staged", &payload).unwrap_err(),
        "event_payload_unknown_field"
    );
}

#[test]
fn approval_activated_contract_matches_transition_producer_and_rejects_unknown_fields() {
    let spec = event_kind_spec("approval.activated").unwrap();
    assert_eq!(spec.aggregate_type, "approval");
    assert_eq!(spec.required_ids, &["approval_id"][..]);
    assert_eq!(
        spec.allowed_fields,
        &[
            "schema",
            "approval_id",
            "previous_state",
            "state",
            "request_hash",
            "at_unix_ms",
            "activation_command_id",
        ][..]
    );
    assert!(!spec.terminal);
    assert_eq!(
        event_migration("approval.activated", 0, 1),
        Some("legacy_approval_event_v0_to_v1")
    );

    let mut payload = json!({
        "schema": "kiana.approval.v1",
        "approval_id": kiana_domain::ApprovalId::new(),
        "previous_state": "staged",
        "state": "active",
        "request_hash": format!("sha256:{}", "a".repeat(64)),
        "at_unix_ms": 1,
        "activation_command_id": kiana_domain::RequestId::new(),
    });
    validate_event_payload("approval.activated", &payload).unwrap();

    let mut missing_id = payload.clone();
    missing_id.as_object_mut().unwrap().remove("approval_id");
    assert_eq!(
        validate_event_payload("approval.activated", &missing_id).unwrap_err(),
        "event_required_id_missing:approval_id"
    );

    payload["unexpected_approval_field"] = json!(true);
    assert_eq!(
        validate_event_payload("approval.activated", &payload).unwrap_err(),
        "event_payload_unknown_field"
    );
}

#[test]
fn approval_requested_contract_matches_capability_producer_and_rejects_unknown_fields() {
    let spec = event_kind_spec("approval.requested").unwrap();
    assert_eq!(spec.aggregate_type, "approval");
    assert_eq!(spec.required_ids, &["approval_id"][..]);
    assert!(spec.allowed_fields.contains(&"action_digest"));
    assert_eq!(
        spec.allowed_fields,
        &[
            "approval_id",
            "request_hash",
            "session_id",
            "actor_id",
            "run_id",
            "expires_at_unix_ms",
            "capability_request_id",
            "attempt",
            "effect_started",
            "effect_known",
            "zero_effect",
            "stop_state",
            "fenced",
            "resume_binding",
            "action_digest",
        ][..]
    );
    assert!(!spec.terminal);
    assert_eq!(
        event_migration("approval.requested", 0, 1),
        Some("legacy_approval_event_v0_to_v1")
    );

    let mut payload = json!({
        "approval_id": kiana_domain::ApprovalId::new(),
        "request_hash": format!("sha256:{}", "a".repeat(64)),
        "session_id": "session",
        "actor_id": "operator",
        "expires_at_unix_ms": 10,
        "capability_request_id": kiana_domain::RequestId::new(),
        "action_digest": format!("sha256:{}", "b".repeat(64)),
        "attempt": 1,
        "effect_started": false,
        "effect_known": true,
        "zero_effect": true,
        "stop_state": "not_requested",
        "fenced": false,
    });
    validate_event_payload("approval.requested", &payload).unwrap();

    // The same producer adds these two fields only for Run-bound continuation.
    let mut run_bound = payload.clone();
    run_bound["run_id"] = json!(kiana_domain::RunId::new());
    run_bound["resume_binding"] = json!({});
    validate_event_payload("approval.requested", &run_bound).unwrap();

    let mut missing_id = payload.clone();
    missing_id.as_object_mut().unwrap().remove("approval_id");
    assert_eq!(
        validate_event_payload("approval.requested", &missing_id).unwrap_err(),
        "event_required_id_missing:approval_id"
    );

    payload["unexpected_approval_field"] = json!(true);
    assert_eq!(
        validate_event_payload("approval.requested", &payload).unwrap_err(),
        "event_payload_unknown_field"
    );

    for field in [
        "subject_request_id",
        "operation",
        "decision",
        "scope",
        "error",
    ] {
        let mut foreign = run_bound.clone();
        foreign[field] = json!(true);
        assert_eq!(
            validate_event_payload("approval.requested", &foreign).unwrap_err(),
            "event_payload_unknown_field",
            "approval.requested must not admit the unrelated field {field}"
        );
    }
}

#[test]
fn approval_approved_contract_matches_decision_producer_and_rejects_unknown_fields() {
    let spec = event_kind_spec("approval.approved").unwrap();
    assert_eq!(spec.aggregate_type, "approval");
    assert_eq!(spec.required_ids, &["approval_id"][..]);
    assert_eq!(
        spec.allowed_fields,
        &[
            "schema",
            "approval_id",
            "previous_state",
            "state",
            "request_hash",
            "at_unix_ms",
            "decision",
            "decision_command_id",
            "decided_by",
            "decision_fact",
        ][..]
    );
    assert!(!spec.terminal);
    assert_eq!(
        event_migration("approval.approved", 0, 1),
        Some("legacy_approval_event_v0_to_v1")
    );

    let mut payload = json!({
        "schema": "kiana.approval.v1",
        "approval_id": kiana_domain::ApprovalId::new(),
        "previous_state": "active",
        "state": "approved",
        "request_hash": format!("sha256:{}", "a".repeat(64)),
        "at_unix_ms": 1,
        "decision": "approve",
        "decision_command_id": kiana_domain::RequestId::new(),
        "decided_by": "operator",
        "decision_fact": {},
    });
    validate_event_payload("approval.approved", &payload).unwrap();

    let mut missing_id = payload.clone();
    missing_id.as_object_mut().unwrap().remove("approval_id");
    assert_eq!(
        validate_event_payload("approval.approved", &missing_id).unwrap_err(),
        "event_required_id_missing:approval_id"
    );

    payload["unexpected_approval_field"] = json!(true);
    assert_eq!(
        validate_event_payload("approval.approved", &payload).unwrap_err(),
        "event_payload_unknown_field"
    );
}

#[test]
fn approval_denied_contract_matches_decision_producer_and_rejects_unknown_fields() {
    let spec = event_kind_spec("approval.denied").unwrap();
    assert_eq!(spec.aggregate_type, "approval");
    assert_eq!(spec.required_ids, &["approval_id"][..]);
    assert_eq!(
        spec.allowed_fields,
        &[
            "schema",
            "approval_id",
            "previous_state",
            "state",
            "request_hash",
            "at_unix_ms",
            "decision",
            "decision_command_id",
            "decided_by",
            "decision_fact",
        ][..]
    );
    assert!(spec.terminal);
    assert_eq!(
        event_migration("approval.denied", 0, 1),
        Some("legacy_approval_event_v0_to_v1")
    );

    let mut payload = json!({
        "schema": "kiana.approval.v1",
        "approval_id": kiana_domain::ApprovalId::new(),
        "previous_state": "active",
        "state": "denied",
        "request_hash": format!("sha256:{}", "a".repeat(64)),
        "at_unix_ms": 1,
        "decision": "deny",
        "decision_command_id": kiana_domain::RequestId::new(),
        "decided_by": "operator",
        "decision_fact": {},
    });
    validate_event_payload("approval.denied", &payload).unwrap();

    let mut missing_id = payload.clone();
    missing_id.as_object_mut().unwrap().remove("approval_id");
    assert_eq!(
        validate_event_payload("approval.denied", &missing_id).unwrap_err(),
        "event_required_id_missing:approval_id"
    );

    payload["unexpected_approval_field"] = json!(true);
    assert_eq!(
        validate_event_payload("approval.denied", &payload).unwrap_err(),
        "event_payload_unknown_field"
    );
}

#[test]
fn approval_expired_contract_matches_expiry_producer_and_rejects_unknown_fields() {
    let spec = event_kind_spec("approval.expired").unwrap();
    assert_eq!(spec.aggregate_type, "approval");
    assert_eq!(spec.required_ids, &["approval_id"][..]);
    assert_eq!(
        spec.allowed_fields,
        &[
            "schema",
            "approval_id",
            "previous_state",
            "state",
            "request_hash",
            "at_unix_ms",
            "reason",
        ][..]
    );
    assert!(spec.terminal);
    assert_eq!(
        event_migration("approval.expired", 0, 1),
        Some("legacy_approval_event_v0_to_v1")
    );

    let mut payload = json!({
        "schema": "kiana.approval.v1",
        "approval_id": kiana_domain::ApprovalId::new(),
        "previous_state": "active",
        "state": "expired",
        "request_hash": format!("sha256:{}", "a".repeat(64)),
        "at_unix_ms": 1,
        "reason": "approval_expired",
    });
    validate_event_payload("approval.expired", &payload).unwrap();

    let mut missing_id = payload.clone();
    missing_id.as_object_mut().unwrap().remove("approval_id");
    assert_eq!(
        validate_event_payload("approval.expired", &missing_id).unwrap_err(),
        "event_required_id_missing:approval_id"
    );

    payload["unexpected_approval_field"] = json!(true);
    assert_eq!(
        validate_event_payload("approval.expired", &payload).unwrap_err(),
        "event_payload_unknown_field"
    );
}

#[test]
fn approval_cancelled_contract_matches_both_cancellation_producers() {
    let spec = event_kind_spec("approval.cancelled").unwrap();
    assert_eq!(spec.aggregate_type, "approval");
    assert_eq!(spec.required_ids, &["approval_id"][..]);
    assert_eq!(
        spec.allowed_fields,
        &[
            "schema",
            "approval_id",
            "previous_state",
            "state",
            "request_hash",
            "at_unix_ms",
            "reason",
            "revoked_by",
            "source",
        ][..]
    );
    assert!(spec.terminal);
    assert_eq!(
        event_migration("approval.cancelled", 0, 1),
        Some("legacy_approval_event_v0_to_v1")
    );

    let common = json!({
        "schema": "kiana.approval.v1",
        "approval_id": kiana_domain::ApprovalId::new(),
        "previous_state": "active",
        "state": "cancelled",
        "request_hash": format!("sha256:{}", "a".repeat(64)),
        "at_unix_ms": 1,
        "reason": "project_untrusted",
    });
    let mut direct = common.clone();
    direct["revoked_by"] = json!("operator");
    validate_event_payload("approval.cancelled", &direct).unwrap();

    let mut project = common;
    project["source"] = json!("project_invalidation");
    validate_event_payload("approval.cancelled", &project).unwrap();

    let mut missing_id = direct.clone();
    missing_id.as_object_mut().unwrap().remove("approval_id");
    assert_eq!(
        validate_event_payload("approval.cancelled", &missing_id).unwrap_err(),
        "event_required_id_missing:approval_id"
    );

    direct["unexpected_approval_field"] = json!(true);
    assert_eq!(
        validate_event_payload("approval.cancelled", &direct).unwrap_err(),
        "event_payload_unknown_field"
    );
}

#[test]
fn approval_consumed_contract_matches_consumption_producer_and_is_terminal() {
    let spec = event_kind_spec("approval.consumed").unwrap();
    assert_eq!(spec.aggregate_type, "approval");
    assert_eq!(spec.required_ids, &["approval_id"][..]);
    assert_eq!(
        spec.allowed_fields,
        &[
            "schema",
            "approval_id",
            "previous_state",
            "state",
            "request_hash",
            "at_unix_ms",
            "dispatch_command_id",
            "decision_command_id",
            "decided_by",
            "consumption_fact",
        ][..]
    );
    assert!(spec.terminal);
    assert_eq!(
        event_migration("approval.consumed", 0, 1),
        Some("legacy_approval_event_v0_to_v1")
    );

    let mut payload = json!({
        "schema": "kiana.approval.v1",
        "approval_id": kiana_domain::ApprovalId::new(),
        "previous_state": "approved",
        "state": "consumed",
        "request_hash": format!("sha256:{}", "a".repeat(64)),
        "at_unix_ms": 1,
        "dispatch_command_id": kiana_domain::RequestId::new(),
        "decision_command_id": kiana_domain::RequestId::new(),
        "decided_by": "operator",
        "consumption_fact": {},
    });
    validate_event_payload("approval.consumed", &payload).unwrap();

    let mut missing_id = payload.clone();
    missing_id.as_object_mut().unwrap().remove("approval_id");
    assert_eq!(
        validate_event_payload("approval.consumed", &missing_id).unwrap_err(),
        "event_required_id_missing:approval_id"
    );

    payload["unexpected_approval_field"] = json!(true);
    assert_eq!(
        validate_event_payload("approval.consumed", &payload).unwrap_err(),
        "event_payload_unknown_field"
    );
}

#[test]
fn approval_continuation_unavailable_contract_matches_recovery_producer() {
    let spec = event_kind_spec("approval.continuation_unavailable").unwrap();
    assert_eq!(spec.aggregate_type, "approval");
    assert_eq!(spec.required_ids, &["approval_id", "run_id"][..]);
    assert_eq!(spec.allowed_fields, &["approval_id", "run_id", "error"][..]);
    assert!(spec.terminal);
    assert_eq!(
        event_migration("approval.continuation_unavailable", 0, 1),
        Some("legacy_approval_event_v0_to_v1")
    );

    let mut payload = json!({
        "approval_id": kiana_domain::ApprovalId::new(),
        "run_id": kiana_domain::RunId::new(),
        "error": "approval_continuation_unavailable",
    });
    validate_event_payload("approval.continuation_unavailable", &payload).unwrap();

    let mut missing_run = payload.clone();
    missing_run.as_object_mut().unwrap().remove("run_id");
    assert_eq!(
        validate_event_payload("approval.continuation_unavailable", &missing_run).unwrap_err(),
        "event_required_id_missing:run_id"
    );

    payload["unexpected_approval_field"] = json!(true);
    assert_eq!(
        validate_event_payload("approval.continuation_unavailable", &payload).unwrap_err(),
        "event_payload_unknown_field"
    );
}

#[test]
fn action_authority_pinned_contract_matches_dispatch_pin_and_rejects_drift() {
    let spec = event_kind_spec("action.authority_pinned").unwrap();
    assert_eq!(spec.aggregate_type, "action");
    assert_eq!(spec.required_ids, &["request_id", "action_digest"][..]);
    assert_eq!(
        spec.allowed_fields,
        &[
            "request_id",
            "action_digest",
            "authority_version",
            "authority_key",
            "actor_id",
            "run_id",
            "turn_id",
            "invocation_id",
            "execution_id",
            "capability_request_id",
            "call_id",
            "operation",
            "attempt",
            "result",
            "effect_started",
            "effect_known",
            "zero_effect",
            "stop_state",
            "fenced",
        ][..]
    );
    assert!(!spec.terminal);
    assert_eq!(
        event_migration("action.authority_pinned", 0, 1),
        Some("legacy_action_event_v0_to_v1")
    );

    let mut payload = json!({
        "action_digest": format!("sha256:{}", "a".repeat(64)),
        "authority_version": 3,
        "request_id": kiana_domain::RequestId::new(),
        "authority_key": "project:trusted",
        "actor_id": "operator",
    });
    validate_event_payload("action.authority_pinned", &payload).unwrap();

    for id in ["request_id", "action_digest"] {
        let mut missing_id = payload.clone();
        missing_id.as_object_mut().unwrap().remove(id);
        assert_eq!(
            validate_event_payload("action.authority_pinned", &missing_id).unwrap_err(),
            format!("event_required_id_missing:{id}")
        );
    }

    payload["unexpected_authority_field"] = json!(true);
    assert_eq!(
        validate_event_payload("action.authority_pinned", &payload).unwrap_err(),
        "event_payload_unknown_field"
    );
}

#[test]
fn run_lifecycle_contracts_match_real_producers_and_reject_drift() {
    let run_id = kiana_domain::RunId::new();
    let prompt = json!({
        "run_id": run_id,
        "session_id": "session-1",
        "turn_id": kiana_domain::TurnId::new(),
        "turn": {},
        "text": "do the work",
    });
    let delta = json!({"run_id": run_id, "text": "partial output"});
    let authorized = json!({
        "run_id": run_id,
        "session_id": "session-1",
        "actor_id": "operator",
        "project_root": "/workspace",
        "role_id": "builder",
        "department_id": "executing",
        "harness": "kiana-harness-v1",
        "sandbox": "workspace-write",
        "capability_mode": "brokered",
        "max_steps_per_turn": 8,
        "runtime_budget": {},
        "authority_revision": 2,
        "authority_epoch": 3,
        "turn_id": kiana_domain::TurnId::new(),
        "turn": {},
        "role_prompt_hash": "sha256:prompt",
        "model_profile": "default",
        "role_spec_schema": "kiana.role.v1",
        "role_version": 1,
        "role_catalog_schema": "kiana.role-catalog.v1",
        "role_catalog_version": {},
        "role_input_schema": {},
        "role_output_schema": {},
    });
    let cases = [
        ("run.authorized", authorized),
        ("run.started", json!({"run_id":run_id})),
        ("run.prompt", prompt),
        ("run.delta", delta),
    ];

    for (kind, payload) in cases {
        let spec = event_kind_spec(kind).unwrap();
        assert_eq!(spec.aggregate_type, "run", "{kind}");
        assert_eq!(spec.required_ids, &["run_id"][..], "{kind}");
        assert!(!spec.terminal, "{kind}");
        assert_eq!(
            event_migration(kind, 0, 1),
            Some("legacy_run_event_v0_to_v1")
        );
        validate_event_payload(kind, &payload).unwrap_or_else(|error| panic!("{kind}: {error}"));

        let mut missing_run_id = payload.clone();
        missing_run_id.as_object_mut().unwrap().remove("run_id");
        assert_eq!(
            validate_event_payload(kind, &missing_run_id).unwrap_err(),
            "event_required_id_missing:run_id",
            "{kind}"
        );

        let mut unknown_field = payload;
        unknown_field["unregistered_lifecycle_field"] = json!(true);
        assert_eq!(
            validate_event_payload(kind, &unknown_field).unwrap_err(),
            "event_payload_unknown_field",
            "{kind}"
        );
    }

    assert_eq!(
        event_kind_spec("run.authorized").unwrap().allowed_fields,
        &[
            "run_id",
            "session_id",
            "actor_id",
            "project_root",
            "role_id",
            "department_id",
            "harness",
            "sandbox",
            "capability_mode",
            "max_steps_per_turn",
            "runtime_budget",
            "authority_revision",
            "authority_epoch",
            "turn_id",
            "turn",
            "role_prompt_hash",
            "model_profile",
            "role_spec_schema",
            "role_version",
            "role_catalog_schema",
            "role_catalog_version",
            "role_input_schema",
            "role_output_schema",
            "decision",
        ][..]
    );
    assert_eq!(
        event_kind_spec("run.started").unwrap().allowed_fields,
        &["run_id", "sequence"][..]
    );
    for kind in ["run.prompt", "run.delta"] {
        assert!(event_kind_spec(kind).is_some());
    }
    assert_eq!(
        event_kind_spec("run.prompt").unwrap().allowed_fields,
        &["run_id", "session_id", "turn_id", "turn", "text"][..]
    );
    assert_eq!(
        event_kind_spec("run.delta").unwrap().allowed_fields,
        &["run_id", "text"][..]
    );
}

#[test]
fn run_input_snapshot_and_clarification_contracts_match_producers() {
    let run_id = kiana_domain::RunId::new();
    let input_id = kiana_domain::InputId::new();
    let interaction_id = kiana_domain::InteractionId::new();
    let turn_id = kiana_domain::TurnId::new();
    let cases = [
        (
            "run.input.accepted",
            &["run_id", "input_id"][..],
            &[
                "run_id",
                "input_id",
                "source",
                "target",
                "target_turn_id",
                "text",
                "received_sequence",
                "disposition",
            ][..],
            json!({
                "run_id": run_id,
                "input_id": input_id,
                "source": "control_plane",
                "target": "next-step",
                "target_turn_id": turn_id,
                "text": "safe input",
                "received_sequence": 1,
                "disposition": "accepted",
            }),
        ),
        (
            "run.input.claimed",
            &["run_id", "input_id"][..],
            &[
                "run_id",
                "input_id",
                "source",
                "target",
                "target_turn_id",
                "received_sequence",
                "disposition",
                "error",
            ][..],
            json!({
                "run_id": run_id,
                "input_id": input_id,
                "source": "control_plane",
                "target": "next-step",
                "target_turn_id": turn_id,
                "received_sequence": 1,
                "disposition": "rejected",
                "error": "input_rejected",
            }),
        ),
        (
            "run.snapshot",
            &["run_id"][..],
            &["run_id", "snapshot"][..],
            json!({"run_id":run_id,"snapshot":{}}),
        ),
        (
            "run.clarification.requested",
            &["run_id", "interaction_id", "turn_id"][..],
            &[
                "run_id",
                "interaction_id",
                "turn_id",
                "request",
                "wait",
                "status",
            ][..],
            json!({
                "run_id": run_id,
                "interaction_id": interaction_id,
                "turn_id": turn_id,
                "request": {},
                "wait": {},
                "status": "waiting",
            }),
        ),
    ];

    for (kind, required_ids, allowed_fields, payload) in cases {
        let spec = event_kind_spec(kind).unwrap();
        assert_eq!(spec.aggregate_type, "run", "{kind}");
        assert_eq!(spec.required_ids, required_ids, "{kind}");
        assert_eq!(spec.allowed_fields, allowed_fields, "{kind}");
        assert!(!spec.terminal, "{kind}");
        assert_eq!(
            event_migration(kind, 0, 1),
            Some("legacy_run_event_v0_to_v1"),
            "{kind}"
        );
        validate_event_payload(kind, &payload).unwrap_or_else(|error| panic!("{kind}: {error}"));

        for &id in required_ids {
            let mut missing_id = payload.clone();
            missing_id.as_object_mut().unwrap().remove(id);
            assert_eq!(
                validate_event_payload(kind, &missing_id).unwrap_err(),
                format!("event_required_id_missing:{id}"),
                "{kind}"
            );
        }

        let mut unknown_field = payload;
        unknown_field["unregistered_input_field"] = json!(true);
        assert_eq!(
            validate_event_payload(kind, &unknown_field).unwrap_err(),
            "event_payload_unknown_field",
            "{kind}"
        );
    }
}

#[test]
fn run_capability_and_approval_contracts_match_all_producers() {
    let run_id = kiana_domain::RunId::new();
    let request_id = kiana_domain::RequestId::new();
    let approval_id = kiana_domain::ApprovalId::new();
    let normal_request = json!({
        "run_id": run_id,
        "request_id": request_id,
        "capability_request_id": request_id,
        "capability": "shell",
        "operation": "shell.exec",
        "risk": "low",
        "cell_id": null,
        "capability_grant_id": null,
        "budget_lease_id": null,
        "attempt": 1,
        "effect_started": false,
        "effect_known": true,
        "zero_effect": true,
        "stop_state": "not_requested",
        "fenced": false,
        "action_digest": format!("sha256:{}", "a".repeat(64)),
        "turn_id": kiana_domain::TurnId::new(),
        "step_id": kiana_domain::StepId::new(),
        "invocation_id": kiana_domain::InvocationId::new(),
        "arguments": {},
        "execution_scope": {},
    });
    let mut preparation_denied_request = normal_request.clone();
    preparation_denied_request
        .as_object_mut()
        .unwrap()
        .remove("execution_scope");

    let cases = [
        (
            "run.capability_requested",
            &["run_id", "capability_request_id"][..],
            &[
                "run_id",
                "request_id",
                "capability_request_id",
                "capability",
                "operation",
                "risk",
                "cell_id",
                "capability_grant_id",
                "budget_lease_id",
                "attempt",
                "effect_started",
                "effect_known",
                "zero_effect",
                "stop_state",
                "fenced",
                "action_digest",
                "turn_id",
                "step_id",
                "invocation_id",
                "arguments",
                "execution_scope",
            ][..],
            false,
            normal_request,
        ),
        (
            "run.capability_requested",
            &["run_id", "capability_request_id"][..],
            &[
                "run_id",
                "request_id",
                "capability_request_id",
                "capability",
                "operation",
                "risk",
                "cell_id",
                "capability_grant_id",
                "budget_lease_id",
                "attempt",
                "effect_started",
                "effect_known",
                "zero_effect",
                "stop_state",
                "fenced",
                "action_digest",
                "turn_id",
                "step_id",
                "invocation_id",
                "arguments",
                "execution_scope",
            ][..],
            false,
            preparation_denied_request,
        ),
        (
            "run.capability_blocked",
            &["run_id", "capability_request_id"][..],
            &[
                "run_id",
                "capability_request_id",
                "error",
                "reason",
                "attempt",
                "effect_started",
                "effect_known",
                "zero_effect",
                "stop_state",
                "fenced",
            ][..],
            true,
            json!({
                "run_id": run_id,
                "capability_request_id": request_id,
                "error": "prepare_rejected",
                "attempt": 1,
                "effect_started": false,
                "effect_known": true,
                "zero_effect": true,
                "stop_state": "not_requested",
                "fenced": false,
            }),
        ),
        (
            "run.capability_blocked",
            &["run_id", "capability_request_id"][..],
            &[
                "run_id",
                "capability_request_id",
                "error",
                "reason",
                "attempt",
                "effect_started",
                "effect_known",
                "zero_effect",
                "stop_state",
                "fenced",
            ][..],
            true,
            json!({
                "run_id": run_id,
                "capability_request_id": request_id,
                "reason": "policy_denied",
                "attempt": 1,
                "effect_started": false,
                "effect_known": true,
                "zero_effect": true,
                "stop_state": "not_requested",
                "fenced": false,
            }),
        ),
        (
            "run.awaiting_approval",
            &["run_id", "capability_request_id", "approval_id"][..],
            &[
                "run_id",
                "approval_id",
                "capability_request_id",
                "attempt",
                "effect_started",
                "effect_known",
                "zero_effect",
                "stop_state",
                "fenced",
                "resume_binding",
            ][..],
            false,
            json!({
                "run_id": run_id,
                "approval_id": approval_id,
                "capability_request_id": request_id,
                "attempt": 1,
                "effect_started": false,
                "effect_known": true,
                "zero_effect": true,
                "stop_state": "not_requested",
                "fenced": false,
                "resume_binding": {},
            }),
        ),
    ];

    for (kind, required_ids, allowed_fields, terminal, payload) in cases {
        let spec = event_kind_spec(kind).unwrap();
        assert_eq!(spec.aggregate_type, "run", "{kind}");
        assert_eq!(spec.required_ids, required_ids, "{kind}");
        assert_eq!(spec.allowed_fields, allowed_fields, "{kind}");
        assert_eq!(spec.terminal, terminal, "{kind}");
        assert_eq!(
            event_migration(kind, 0, 1),
            Some("legacy_run_event_v0_to_v1"),
            "{kind}"
        );
        validate_event_payload(kind, &payload).unwrap_or_else(|error| panic!("{kind}: {error}"));

        for &id in required_ids {
            let mut missing_id = payload.clone();
            missing_id.as_object_mut().unwrap().remove(id);
            assert_eq!(
                validate_event_payload(kind, &missing_id).unwrap_err(),
                format!("event_required_id_missing:{id}"),
                "{kind}"
            );
        }

        let mut unknown_field = payload;
        unknown_field["unregistered_capability_field"] = json!(true);
        assert_eq!(
            validate_event_payload(kind, &unknown_field).unwrap_err(),
            "event_payload_unknown_field",
            "{kind}"
        );
    }
}

#[test]
fn run_cancelling_contract_matches_lifecycle_and_project_invalidation_producers() {
    let run_id = kiana_domain::RunId::new();
    let lifecycle_payload = json!({
        "run_id": run_id,
        "reason": "user",
        "cancellation_state": "stopping",
        "cancellation_reason": "user",
        "cancel_actor_id": "operator",
        "cancellation_targets": [kiana_domain::RequestId::new()],
        "cancellation_at_unix_ms": 123,
        "cancellation_fact": {},
    });
    let project_invalidation_payload = json!({
        "run_id": run_id,
        "reason": "data.revocation_requested",
        "project_root": "/workspace/project",
    });
    let required_ids = &["run_id"][..];
    let allowed_fields = &[
        "run_id",
        "reason",
        "cancellation_state",
        "cancellation_reason",
        "cancel_actor_id",
        "cancellation_targets",
        "cancellation_at_unix_ms",
        "cancellation_fact",
        "project_root",
    ][..];

    let spec = event_kind_spec("run.cancelling").unwrap();
    assert_eq!(spec.aggregate_type, "run");
    assert_eq!(spec.required_ids, required_ids);
    assert_eq!(spec.allowed_fields, allowed_fields);
    assert!(!spec.terminal);
    assert_eq!(
        event_migration("run.cancelling", 0, 1),
        Some("legacy_run_event_v0_to_v1")
    );

    for payload in [lifecycle_payload, project_invalidation_payload] {
        validate_event_payload("run.cancelling", &payload).unwrap();

        for &id in required_ids {
            let mut missing_id = payload.clone();
            missing_id.as_object_mut().unwrap().remove(id);
            assert_eq!(
                validate_event_payload("run.cancelling", &missing_id).unwrap_err(),
                format!("event_required_id_missing:{id}")
            );
        }

        let mut unknown_field = payload;
        unknown_field["unregistered_cancellation_field"] = json!(true);
        assert_eq!(
            validate_event_payload("run.cancelling", &unknown_field).unwrap_err(),
            "event_payload_unknown_field"
        );
    }
}

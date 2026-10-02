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
    assert_eq!(
        capability_result.allowed_fields.len(),
        invocation.allowed_fields.len() + 2
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
        "capability_request_id":kiana_domain::RequestId::new(),
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
        spec.allowed_fields.len(),
        event_kind_spec("approval.approved")
            .unwrap()
            .allowed_fields
            .len()
            + 1
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

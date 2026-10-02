#[test]
fn event_contract_registry_and_migration_boundary_are_source_owned() {
    let contracts = include_str!("../../kiana-domain/src/event_contracts.rs");
    let domain_contracts = include_str!("../../kiana-domain/src/contracts.rs");
    let states = include_str!("../../kiana-domain/src/states.rs");
    let journal = include_str!("../../kiana-domain/src/journal.rs");
    let protocol = include_str!("../../kiana-protocol/src/lib.rs");
    let approvals = include_str!("../../kiana-core/src/approvals.rs");
    let capabilities = include_str!("../../kiana-core/src/capabilities.rs");
    let journal_approvals = include_str!("../../kiana-daemon/src/journal_approvals.rs");
    let baseline = include_str!("../../docs/roadmap/event-receipt-schema-baseline.md");

    for marker in [
        "EventKindSpec",
        "EVENT_KIND_SPECS",
        "EVENT_MIGRATIONS",
        "unknown_required_event_kind",
        "event_schema_version_incompatible",
        "event_payload_unknown_field",
        "validate_runtime_event",
        "secret_policy",
        "required_ids",
        "CAPABILITY_DECISION_IDS",
        "CAPABILITY_BLOCKED_FIELDS",
        "CAPABILITY_DECISION_FIELDS",
        "APPROVAL_STAGED_FIELDS",
        "APPROVAL_ACTIVATED_FIELDS",
        "APPROVAL_REQUESTED_FIELDS",
        "APPROVAL_APPROVED_FIELDS",
        "APPROVAL_DENIED_FIELDS",
        "APPROVAL_EXPIRED_FIELDS",
        "APPROVAL_CANCELLED_FIELDS",
        "result_source",
    ] {
        assert!(
            contracts.contains(marker),
            "missing event contract marker {marker}"
        );
    }
    assert!(domain_contracts.contains("kiana.runtime-event.v1"));
    assert!(states.contains("pub struct RuntimeEvent"));
    assert!(states.contains("pub event_id: EventId"));
    assert!(journal.contains("JOURNAL_FRAME_SCHEMA"));
    assert!(protocol.contains("RuntimeEvent") || protocol.contains("EventKind"));
    assert!(baseline.contains("unknown_required_event_kind_fails_closed"));
    assert!(baseline.contains("event_schema_version_cannot_downgrade"));
    assert!(baseline.contains("event_payload_unknown_field_is_not_silently_dropped"));
    assert!(baseline.contains("capability_blocked_contract_matches_direct_deny_producers"));
    assert!(baseline.contains("capability_decision_contract_accepts_policy_and_gate"));
    assert!(baseline.contains("event_capability_identity_pair_incomplete"));
    assert!(baseline.contains("approval_staged_contract_matches_journal_producer"));
    assert!(baseline.contains("approval_activated_contract_matches_transition_producer"));
    assert!(baseline.contains("approval_requested_contract_matches_capability_producer"));
    assert!(baseline.contains("approval_approved_contract_matches_decision_producer"));
    assert!(baseline.contains("approval_denied_contract_matches_decision_producer"));
    assert!(baseline.contains("approval_expired_contract_matches_expiry_producer"));
    assert!(baseline.contains("approval_cancelled_contract_matches_both_cancellation_producers"));
    assert!(baseline.contains("result_source"));
    assert!(baseline.contains("legacy decode"));
    assert!(approvals.contains("json!({\"error\":reason})"));
    assert!(approvals.contains("\"attempt\":1,\"effect_started\":false"));
    assert!(approvals.contains("\"capability.blocked\""));
    assert!(approvals.contains("\"policy\":policy,\"gate\":gate"));
    assert!(approvals
        .contains("json!({\"policy\":policy,\"gate\":gate,\n            \"action_digest\""));
    assert!(approvals.contains(
        "json!({\"run_id\":invocation.run_id,\"capability_request_id\":request.request_id,\"policy\":policy,\"gate\":gate,"
    ));
    assert!(approvals.matches("\"capability.decision\"").count() >= 2);
    assert!(journal_approvals.contains("\"approval.staged\""));
    assert!(journal_approvals.contains(
        "\"schema\":APPROVAL_SCHEMA,\"approval_id\":id,\"subject\":subject,\"state\":\"staged\",\"at_unix_ms\":now"
    ));
    assert!(journal_approvals.contains(".with_stream_metadata(APPROVAL_STREAM, id.to_string(), 1)"));
    assert!(journal_approvals.contains("ApprovalState::Active => \"approval.activated\""));
    assert!(journal_approvals.contains("json!({\"activation_command_id\":command_id})"));
    assert!(journal_approvals.contains(
        "\"schema\":APPROVAL_SCHEMA,\"approval_id\":id,\"previous_state\":record.state,\"state\":next,\n        \"request_hash\":record.subject.preview.challenge.request_hash,\"at_unix_ms\":now"
    ));
    assert!(journal_approvals
        .contains(".with_stream_metadata(APPROVAL_STREAM, id.to_string(), record.version + 1)"));
    assert!(capabilities.contains("\"approval.requested\""));
    assert!(capabilities.contains(
        "\"capability_request_id\":request.request_id,\"action_digest\":kiana_domain::capability_action_digest(request),"
    ));
    assert!(capabilities.contains("Some(run_id) => (\"run\", run_id.to_string())"));
    assert!(capabilities.contains("None => (\"request\", event_request_id.to_string())"));
    assert!(
        capabilities.contains(".with_stream_metadata(aggregate_type, &aggregate_id, version + 1)")
    );
    assert!(journal_approvals.contains("ApprovalState::Approved => \"approval.approved\""));
    assert!(journal_approvals.contains(
        "json!({\"decision\":decision,\n            \"decision_command_id\":context.request_id,\"decided_by\":context.actor_id})"
    ));
    assert!(journal_approvals.contains("with_fact(event, \"decision_fact\", &decision_fact)"));
    assert!(journal_approvals.contains("ApprovalState::Denied => \"approval.denied\""));
    assert!(journal_approvals.contains("ApprovalState::Expired => \"approval.expired\""));
    assert!(journal_approvals.contains("json!({\"reason\":reason})"));
    assert!(journal_approvals.contains("ApprovalState::Cancelled => \"approval.cancelled\""));
    assert!(journal_approvals
        .contains("json!({\"reason\":redact_text(reason),\"revoked_by\":context.actor_id})"));
    assert!(journal_approvals
        .contains("json!({\"reason\":redact_text(reason),\"source\":\"project_invalidation\"})"));
}

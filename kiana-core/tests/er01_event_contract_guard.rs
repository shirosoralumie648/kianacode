#[test]
fn event_contract_registry_and_migration_boundary_are_source_owned() {
    let contracts = include_str!("../../kiana-domain/src/event_contracts.rs");
    let domain_contracts = include_str!("../../kiana-domain/src/contracts.rs");
    let states = include_str!("../../kiana-domain/src/states.rs");
    let journal = include_str!("../../kiana-domain/src/journal.rs");
    let protocol = include_str!("../../kiana-protocol/src/lib.rs");
    let approvals = include_str!("../../kiana-core/src/approvals.rs");
    let capabilities = include_str!("../../kiana-core/src/capabilities.rs");
    let dispatch = include_str!("../../kiana-core/src/dispatch.rs");
    let lifecycle = include_str!("../../kiana-core/src/lifecycle.rs");
    let data_governance = include_str!("../../kiana-core/src/data_governance.rs");
    let recovery = include_str!("../../kiana-core/src/recovery.rs");
    let receipts = include_str!("../../kiana-core/src/receipts.rs");
    let events = include_str!("../../kiana-core/src/events.rs");
    let credential_recovery =
        include_str!("../../kiana-domain/src/credential_recovery_evidence.rs");
    let communication = include_str!("../../kiana-core/src/communication.rs");
    let daemon_connectors = include_str!("../../kiana-daemon/src/connectors.rs");
    let sessions = include_str!("../../kiana-core/src/sessions.rs");
    let journal_approvals = include_str!("../../kiana-daemon/src/journal_approvals.rs");
    let event_store_core = include_str!("../../kiana-eventlog/src/event_store_core.rs");
    let baseline = include_str!("../../docs/roadmap/event-receipt-schema-baseline.md");

    assert!(baseline.contains("run_input_snapshot_and_clarification_contracts_match_producers"));
    for (kind, expected_count) in [
        ("run.input.accepted", 1),
        ("run.input.claimed", 1),
        ("run.clarification.requested", 1),
    ] {
        assert_eq!(
            lifecycle.matches(&format!("\"{kind}\",\n")).count(),
            expected_count,
            "unexpected lifecycle producer count for {kind}"
        );
    }
    for (kind, fields, ids) in [
        ("run.snapshot", "RUN_SNAPSHOT_FIELDS", "RUN_IDS"),
        (
            "run.input.accepted",
            "RUN_INPUT_ACCEPTED_FIELDS",
            "INPUT_IDS",
        ),
        ("run.input.claimed", "RUN_INPUT_CLAIMED_FIELDS", "INPUT_IDS"),
        (
            "run.clarification.requested",
            "RUN_CLARIFICATION_REQUESTED_FIELDS",
            "CLARIFICATION_IDS",
        ),
    ] {
        let spec = format!("\"{kind}\",\n        \"run\",\n        {ids},\n        {fields},");
        assert!(
            contracts.contains(&spec),
            "run event {kind} must use exact fields and IDs"
        );
    }
    for marker in [
        "\"source\": source,",
        "\"target\": target,",
        "\"target_turn_id\": target_turn_id,",
        "\"text\": safe_text,",
        "\"received_sequence\": accepted_sequence,",
        "\"disposition\": \"accepted\",",
        "\"run_id\":run_id,\"input_id\":input_id,\"source\":\"control_plane\",\"target\":target,",
        "\"target_turn_id\":target_turn_id,\"received_sequence\":accepted_sequence,\"disposition\":\"rejected\",\"error\":error",
        "\"run_id\":run_id,\"snapshot\":safe",
        "\"interaction_id\": request.interaction_id,",
        "\"turn_id\": request.turn_id,",
        "\"request\": &request,",
        "\"wait\": &wait,",
        "\"status\": kiana_domain::CLARIFICATION_WAITING_STATUS,",
    ] {
        let source = if marker.contains("snapshot") {
            recovery
        } else {
            lifecycle
        };
        assert!(
            source.contains(marker),
            "missing run input/clarification producer field {marker}"
        );
    }
    for marker in [
        "EventKindSpec",
        "RUN_REJECTED_FIELDS",
        "event_rejected_reason_required",
        "run.rejected",
        "EventSchemaResolution",
        "EVENT_KIND_SPECS",
        "EVENT_MIGRATIONS",
        "accepts_version",
        "validate_version",
        "upcast_event_payload",
        "event_migration_unavailable",
        "event_migration_unimplemented",
        "resolve_event_payload",
        "unknown_required_event_kind",
        "event_schema_version_downgrade",
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
        "APPROVAL_CONSUMED_FIELDS",
        "APPROVAL_CONTINUATION_UNAVAILABLE_IDS",
        "APPROVAL_CONTINUATION_UNAVAILABLE_FIELDS",
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
    for marker in [
        "EventSchemaResolution",
        "resolve_event_payload",
        "check_event_schema_version",
    ] {
        assert!(
            protocol.contains(marker),
            "missing protocol event boundary: {marker}"
        );
    }
    assert!(baseline.contains("run_rejected_contract_accepts_request_and_run_bound_shapes"));
    assert!(events.contains("pub(crate) fn aggregate_for_event("));
    assert!(events.contains("if let Some(run_id) = data"));
    assert!(events.contains("(\"request\".to_owned(), request_id.to_string())"));
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
    assert!(baseline.contains("approval_consumed_contract_matches_consumption_producer"));
    assert!(
        baseline.contains("approval_continuation_unavailable_contract_matches_recovery_producer")
    );
    assert!(baseline.contains("result_source"));
    assert!(baseline.contains("legacy decode"));
    assert!(contracts.contains("const ACTION_IDS: &[&str] = &[\"request_id\", \"action_digest\"]"));
    assert!(contracts.contains("\"action.authority_pinned\""));
    assert!(contracts.contains("Some(\"legacy_action_event_v0_to_v1\")"));
    assert_eq!(
        dispatch.matches("\"action.authority_pinned\"").count(),
        1,
        "action.authority_pinned must have one dispatch producer"
    );
    for marker in [
        "let command_id = derived_request_id(\"action.pin\", &request.request_id.to_string())",
        "\"action_digest\":digest,\"authority_version\":version",
        "\"request_id\":request.request_id,\"authority_key\":key,\"actor_id\":context.actor_id",
        ".with_stream_metadata(\"action\", request.request_id.to_string(), 1)",
        "AggregateVersion::new(\"action\", request.request_id.to_string(), 0)",
        "AggregateVersion::new(\"authority\", key, version)",
    ] {
        assert!(
            dispatch.contains(marker),
            "missing action pin producer boundary {marker}"
        );
    }
    assert!(baseline
        .contains("action_authority_pinned_contract_matches_dispatch_pin_and_rejects_drift"));
    for (kind, expected_count) in [
        ("run.authorized", 1),
        ("run.started", 1),
        ("run.prompt", 2),
        ("run.delta", 1),
    ] {
        assert_eq!(
            // A query predicate also names the kind; only a writer passes it as an argument.
            lifecycle.matches(&format!("\"{kind}\",\n")).count(),
            expected_count,
            "unexpected lifecycle producer count for {kind}"
        );
    }
    for (kind, fields) in [
        ("run.authorized", "RUN_AUTHORIZED_FIELDS"),
        ("run.started", "RUN_STARTED_FIELDS"),
        ("run.prompt", "RUN_PROMPT_FIELDS"),
        ("run.delta", "RUN_DELTA_FIELDS"),
    ] {
        let spec = format!("\"{kind}\",\n        \"run\",\n        RUN_IDS,\n        {fields},");
        assert!(
            contracts.contains(&spec),
            "run event {kind} must use its own exact field set"
        );
    }
    for marker in [
        "RUN_AUTHORIZED_FIELDS",
        "RUN_STARTED_FIELDS",
        "RUN_PROMPT_FIELDS",
        "RUN_DELTA_FIELDS",
        "run_lifecycle_contracts_match_real_producers_and_reject_drift",
    ] {
        assert!(
            contracts.contains(marker) || baseline.contains(marker),
            "missing run lifecycle contract marker {marker}"
        );
    }
    for marker in [
        "\"run_id\": run_id,",
        "\"session_id\": context.session_id,",
        "\"actor_id\": context.actor_id,",
        "\"project_root\": context.project_root,",
        "\"role_id\": context.role_id,",
        "\"department_id\": context.department_id,",
        "\"harness\": HARNESS_ID,",
        "\"sandbox\": sandbox,",
        "\"capability_mode\": \"brokered\",",
        "\"max_steps_per_turn\": max_steps_per_turn,",
        "\"runtime_budget\":runtime_budget,",
        "\"authority_revision\":authority_revision,",
        "\"authority_epoch\":authority_epoch,",
        "\"turn_id\":kiana_domain::TurnId::from_uuid(request_id.as_uuid()),",
        "\"turn\":turn,",
        "\"role_prompt_hash\": role.prompt_hash,",
        "\"model_profile\": role.model_profile,",
        "\"role_spec_schema\": role.schema,",
        "\"role_version\": role.version,",
        "\"role_catalog_schema\": kiana_domain::ROLE_CATALOG_SCHEMA,",
        "\"role_catalog_version\": kiana_domain::SchemaVersion::new(1, 0),",
        "\"role_input_schema\": role.input_schema,",
        "\"role_output_schema\": role.output_schema,",
        "json!({ \"run_id\": run_id })",
        "\"run_id\": run_id,\n                \"session_id\": context.session_id,\n                \"turn_id\": turn_id,\n                \"turn\": turn.clone(),\n                \"text\": &prompt,",
        "\"run_id\": run_id,\n                \"session_id\": context.session_id,\n                \"turn_id\": legacy_turn.turn_id,\n                \"turn\": legacy_turn.clone(),\n                \"text\": &prompt,",
        "json!({ \"run_id\": run_id, \"text\": text })",
    ] {
        assert!(
            lifecycle.contains(marker),
            "missing lifecycle run producer field {marker}"
        );
    }
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
    assert!(journal_approvals.contains("ApprovalState::Consumed => \"approval.consumed\""));
    assert!(journal_approvals.contains(
        "json!({\"dispatch_command_id\":dispatch_command_id,\n            \"decision_command_id\":record.decision_command_id,\"decided_by\":record.decided_by})"
    ));
    assert!(journal_approvals.contains("with_fact(event, \"consumption_fact\", &consumption_fact)"));
    assert!(states.contains("Self::Denied | Self::Expired | Self::Cancelled | Self::Consumed"));
    assert!(approvals.contains("\"approval.continuation_unavailable\""));
    assert!(approvals.contains(
        "\"approval_id\": approval_id,\n                        \"run_id\": persisted_approval.run_id,\n                        \"error\": \"approval_continuation_unavailable\","
    ));
    assert!(capabilities.contains(
        "\"request_id\":original.request_id,\n                        \"capability_request_id\":original.request_id,"
    ));
    assert!(capabilities.contains(
        "\"request_id\":request.request_id,\"capability_request_id\":request.request_id,"
    ));
    assert!(dispatch.contains("\"result.delivery_claimed\""));
    for marker in [
        "\"result_digest\":json_digest",
        "\"receipt_digest\":receipt_digest",
        "\"outcome_state\":outcome_state",
        "\"outcome_ready\":true",
        "\"delivery_policy\":\"single_advance\"",
    ] {
        assert!(
            dispatch.contains(marker),
            "missing result delivery producer field {marker}"
        );
    }
    assert!(capabilities.contains("\"run.tool_call\""));
    assert!(capabilities.contains("\"tool\":request.capability,\"operation\":request.operation"));
    for marker in [
        "\"result\":result.output,\"cancelled\":",
        "\"not_executed\":true",
        "\"stop_confirmed\":true,\"fenced\":false",
    ] {
        assert!(
            capabilities.contains(marker),
            "missing capability tool producer field {marker}"
        );
    }
    for marker in [
        "\"result\":{\"error\":\"cancelled:approval_pending\",\"not_executed\":true,\"replay_safe\":true},\"not_executed\":true",
        "\"result\":result,\"cancelled\":true,\"not_executed\":true",
    ] {
        assert!(lifecycle.contains(marker), "missing lifecycle tool-result producer field {marker}");
    }
    for marker in [
        "\"result\":{\"error\":format!(\"cancelled:{reason}\"),\"not_executed\":true,\"replay_safe\":true},\"not_executed\":true",
        "\"capability_request_id\":capability_id,\"call_id\":call_id,\"result\":result,\"not_executed\":true",
    ] {
        assert!(
            data_governance.contains(marker),
            "missing governance tool-result producer field {marker}"
        );
    }
    assert!(recovery.contains("\"run.resume_prepared\""));
    for marker in [
        "\"run_id\":run_id,\"session_id\":context.session_id,\"actor_id\":context.actor_id,",
        "\"snapshot_event_id\":event.event_id,",
        "\"turn_id\":resume_turn.turn_id,\"turn\":resume_turn,",
    ] {
        assert!(
            recovery.contains(marker),
            "missing resume producer field {marker}"
        );
    }
    assert!(lifecycle.contains("\"run.predecessor\""));
    for marker in [
        "\"run_id\": run_id,",
        "\"previous_run_id\": previous_run_id,",
        "\"turn_id\": turn_id,",
        "\"turn\": turn,",
        "\"session_id\": context.session_id,",
        "\"semantics\": \"new_turn_v2\",",
    ] {
        assert!(
            lifecycle.contains(marker),
            "missing predecessor producer field {marker}"
        );
    }
    assert!(lifecycle.contains("\"run.compacted\""));
    for marker in [
        "\"schema\": COMPACT_SCHEMA,",
        "\"run_id\": run_id,",
        "\"tokens_before\": tokens_before,",
        "\"tokens_after\": tokens_after,",
        "\"summary_present\": summary_present,",
    ] {
        assert!(
            lifecycle.contains(marker),
            "missing compaction producer field {marker}"
        );
    }
    assert!(
        lifecycle.contains("record_event(request_id, sequence, \"run.receipt\", receipt.clone())")
    );
    assert!(receipts.contains("let receipt = with_work_packet("));
    for marker in [
        "\"schema\": RUN_RESULT_SCHEMA",
        "\"run_id\": run_id",
        "\"session_id\": context.session_id",
        "\"harness\": HARNESS_ID",
        "\"sandbox\": sandbox",
        "\"actor_id\": context.actor_id",
        "\"role_id\": role_id",
        "\"department_id\": department_id",
        "\"role_resolution\": worker.is_some()",
        "\"role_spec_schema\":",
        "\"role_version\":",
        "\"role_catalog_schema\":",
        "\"role_catalog_version\":",
        "\"input_schema\":",
        "\"output_schema\":",
        "\"model_profile\":",
        "\"max_steps_per_turn\":",
        "\"prompt_hash\":",
        "\"model_turns\":",
        "\"cost_ledger\":",
        "\"files_changed\":",
        "\"memory_hits\":",
        "\"retrieval_receipts\":",
        "\"memory_proposals\":",
        "\"invocations\":",
        "\"invocation_projection_error\":",
        "\"run_receipt\":",
        "\"receipt_data_binding\":",
        "\"projection\":",
        "\"execution_receipts\":",
        "\"aggregation\":",
        "\"cost_breakdown\":",
        "\"effect_usage\":",
        "\"compact\":",
        "\"capabilities\":",
        "\"observability\":",
        "\"output\": output",
    ] {
        assert!(
            receipts.contains(marker),
            "missing receipt top-level field {marker}"
        );
    }
    assert!(events.contains("receipt[\"work_packet_id\"] = json!(work_packet_id)"));
    assert!(events.contains("receipt[\"input\"] = json!(\"work_packet\")"));
    assert!(contracts.contains("const RECOVERY_IDS: &[&str] = &[\"run_id\"]"));
    assert!(contracts.contains("const RECOVERY_FIELDS: &[&str] = &[\"run_id\", \"recovery\"]"));
    assert!(contracts.contains("\"recovery.credential\""));
    assert!(credential_recovery.contains("CREDENTIAL_RECOVERY_EVENT_KIND"));
    assert!(credential_recovery.contains("json!({\"run_id\": self.run_ref, \"recovery\": self})"));
    assert!(credential_recovery.contains("validate_runtime_event(&event)?"));
    assert!(event_store_core.contains("kiana_domain::validate_runtime_event(event)"));
    assert!(event_store_core.contains("event_contract_"));
    for marker in [
        "COMMUNICATION_HANDOFF_LIFECYCLE_FIELDS",
        "COMMUNICATION_INCIDENT_LIFECYCLE_FIELDS",
        "\"communication.handoff_acknowledged\"",
        "\"communication.handoff_rejected\"",
        "\"communication.incident_escalated\"",
    ] {
        assert!(
            contracts.contains(marker),
            "missing communication contract marker {marker}"
        );
    }
    for marker in [
        "\"message_id\": message_id",
        "\"accepted\": to_status == CommunicationLifecycleStatus::Acknowledged",
        "\"reason\": reason",
        "\"evidence_refs\": lifecycle.evidence_refs.clone()",
        "\"authority_granted\": false",
        "\"request_id\": context.request_id",
    ] {
        assert!(
            communication.contains(marker),
            "missing communication producer field {marker}"
        );
    }
    for marker in [
        "COMMUNICATION_SEND_FIELDS",
        "\"communication.chat\"",
        "\"communication.command\"",
        "\"communication.handoff\"",
        "\"communication.decision\"",
        "\"communication.status_report\"",
        "\"communication.evidence\"",
        "\"communication.incident\"",
    ] {
        assert!(
            contracts.contains(marker),
            "missing communication send marker {marker}"
        );
    }
    for marker in [
        "let kind = communication_event_kind(message.kind)",
        "\"message\": message.clone(),",
        "\"message_id\": message.message_id,",
        "\"lifecycle\": lifecycle,",
        "\"authority_granted\": false,",
        "\"project_root\": context.project_root,",
        "\"actor_id\": context.actor_id,",
        "\"session_id\": context.session_id,",
        "\"request_id\": context.request_id,",
    ] {
        assert!(
            communication.contains(marker),
            "missing communication send producer field {marker}"
        );
    }
    for marker in [
        "CONNECTOR_HEALTH_IDS",
        "CONNECTOR_HEALTH_FIELDS",
        "CONNECTOR_MCP_HANDSHAKE_IDS",
        "CONNECTOR_MCP_HANDSHAKE_FIELDS",
        "\"connector.health_checked\"",
        "\"connector.mcp_handshake\"",
    ] {
        assert!(
            contracts.contains(marker),
            "missing connector contract marker {marker}"
        );
    }
    for marker in [
        "CONNECTOR_HEALTH_EVENT_KIND",
        "\"schema\":\"kiana.connector-health-event.v1\"",
        "\"connector_id\":&fact.connector_id",
        "\"binding_id\":&fact.binding_id",
        "\"health\":fact",
        "\"request_fingerprint\":request_fingerprint",
        "CONNECTOR_MCP_HANDSHAKE_EVENT_KIND",
        "\"schema\": \"kiana.connector-mcp-handshake-event.v1\"",
        "\"connector_id\": snapshot.definition.connector_id",
        "\"binding_id\": binding_id",
        "\"handshake\": handshake",
        "\"proof_level\": \"source\"",
    ] {
        assert!(
            daemon_connectors.contains(marker),
            "missing connector daemon producer field {marker}"
        );
    }
    for marker in [
        "SESSION_ASSIGNMENT_IDS",
        "SESSION_ASSIGNMENT_FIELDS",
        "\"session.assigned\"",
    ] {
        assert!(
            contracts.contains(marker),
            "missing session contract marker {marker}"
        );
    }
    for marker in [
        "\"schema\":\"kiana.session-assignment.v1\"",
        "\"session_id\":context.session_id",
        "\"actor_id\":context.actor_id",
        "\"project_root\":Self::canonical_project_root(&context.project_root)",
        "\"role_id\":role.role_id",
        "\"department_id\":role.department_id",
        "\"prompt_hash\":role.prompt_hash",
        "\"model_profile\":role.model_profile",
        "\"role_spec_schema\":role.schema",
        "\"role_version\":role.version",
        "\"role_catalog_schema\":kiana_domain::ROLE_CATALOG_SCHEMA",
        "\"role_input_schema\":role.input_schema",
        "\"role_output_schema\":role.output_schema",
        "\"authority_epoch\":authority_epoch",
        "\"principal\":principal",
        "\"project_identity\":project",
        "\"assignment\":typed",
        "\"session.assigned\"",
    ] {
        assert!(
            sessions.contains(marker),
            "missing session producer field {marker}"
        );
    }
    for (kind, expected_count) in [
        ("run.capability_requested", 2),
        ("run.capability_blocked", 2),
        ("run.awaiting_approval", 1),
    ] {
        assert_eq!(
            capabilities.matches(&format!("\"{kind}\"")).count(),
            expected_count,
            "unexpected capability producer count for {kind}"
        );
    }
    for (kind, fields, ids, terminal) in [
        (
            "run.capability_requested",
            "RUN_CAPABILITY_REQUESTED_FIELDS",
            "INVOCATION_IDS",
            false,
        ),
        (
            "run.capability_blocked",
            "RUN_CAPABILITY_BLOCKED_FIELDS",
            "INVOCATION_IDS",
            true,
        ),
        (
            "run.awaiting_approval",
            "RUN_AWAITING_APPROVAL_FIELDS",
            "RUN_AWAITING_APPROVAL_IDS",
            false,
        ),
    ] {
        let spec = format!(
            "\"{kind}\",\n        \"run\",\n        {ids},\n        {fields},\n        {terminal},"
        );
        assert!(
            contracts.contains(&spec),
            "run event {kind} must use its producer contract"
        );
    }
    for marker in [
        "const RUN_CAPABILITY_REQUESTED_FIELDS: &[&str]",
        "const RUN_CAPABILITY_BLOCKED_FIELDS: &[&str]",
        "const RUN_AWAITING_APPROVAL_IDS: &[&str] = &[\"run_id\", \"capability_request_id\", \"approval_id\"]",
        "const RUN_AWAITING_APPROVAL_FIELDS: &[&str]",
        "Terminal closes this capability attempt only, not the enclosing run.",
    ] {
        assert!(
            contracts.contains(marker),
            "missing run capability contract marker {marker}"
        );
    }
    for marker in [
        "\"request_id\":original.request_id,",
        "\"capability_request_id\":original.request_id,",
        "\"cell_id\":original.cell_id,",
        "\"capability_grant_id\":original.capability_grant_id,",
        "\"budget_lease_id\":original.budget_lease_id,",
        "\"invocation_id\":kiana_domain::InvocationId::from_uuid(original.request_id.as_uuid()),",
        "\"arguments\":redact_event_value(&original.arguments)",
        "\"request_id\":request.request_id,\"capability_request_id\":request.request_id,",
        "\"execution_scope\":request.execution_scope,",
        "\"error\":reason,",
        "\"reason\":reason,",
        "\"run_id\":run_id,\"approval_id\":challenge.approval_id,",
        "\"resume_binding\":resume_binding",
        ".with_stream_metadata(aggregate_type, &aggregate_id, version + 2)",
    ] {
        assert!(
            capabilities.contains(marker),
            "missing capability producer field or boundary {marker}"
        );
    }
    assert!(capabilities.contains("Some(run_id) => (\"run\", run_id.to_string())"));
    assert_eq!(
        lifecycle
            .matches("RuntimeEvent::new(context.request_id, *sequence, \"run.cancelling\", data)")
            .count(),
        1,
        "expected one lifecycle cancellation writer"
    );
    assert_eq!(
        data_governance.matches("\"run.cancelling\"").count(),
        1,
        "expected one project invalidation cancellation writer"
    );
    assert!(contracts.contains("const RUN_CANCELLING_FIELDS: &[&str]"));
    assert!(contracts.contains(
        "\"run.cancelling\",\n        \"run\",\n        RUN_IDS,\n        RUN_CANCELLING_FIELDS,\n        false,"
    ));
    for marker in [
        "\"run_id\": run_id,",
        "\"reason\": reason,",
        "\"cancellation_state\": \"stopping\",",
        "\"cancellation_reason\": reason,",
        "\"cancel_actor_id\": context.actor_id,",
        "\"cancellation_targets\": canonical_targets,",
        "\"cancellation_at_unix_ms\": at_unix_ms,",
        "\"cancellation_fact\": fact,",
        ".with_stream_metadata(\"run\", run_id.to_string(), version + 1)",
    ] {
        assert!(
            lifecycle.contains(marker),
            "missing typed cancellation producer marker {marker}"
        );
    }
    assert!(data_governance
        .contains("json!({\"run_id\":run_id,\"reason\":kind,\"project_root\":root})"));
}

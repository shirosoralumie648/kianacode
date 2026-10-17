#[path = "support/source_guard.rs"]
mod source_guard;
use source_guard::{compact_code, item_source};

#[test]
fn sc31_audit_record_is_server_derived_and_eventlog_bound() {
    let observability = include_str!("../../kiana-domain/src/observability.rs");
    let reducer = include_str!("../../kiana-domain/src/audit.rs");
    let contracts = include_str!("../../kiana-domain/src/contracts.rs");
    let protocol = include_str!("../../kiana-protocol/src/lib.rs");
    let eventlog_boundary = include_str!("../../kiana-eventlog/src/audit_contract.rs");
    let eventlog_plan = include_str!("../../kiana-eventlog/src/event_store_core.rs");
    let journal_plan = include_str!("../../kiana-eventlog/src/journal_core.rs");

    for marker in [
        "AUDIT_RECORD_SCHEMA",
        "AUDIT_EVENT_SCHEMA",
        "AuditRecordEvent",
        "append_only",
        "payload_recoverable",
        "redaction_profile",
        "audit_event_source_binding_mismatch",
        "from_wire_value",
        "require_audit_wire_fields",
        "AUDIT_EVENT_WIRE_FIELDS",
        "AUDIT_EVENT_WIRE_REQUIRED_FIELDS",
        "AUDIT_RECORD_WIRE_FIELDS",
        "AUDIT_RECORD_WIRE_REQUIRED_FIELDS",
        "audit_wire_required_field_missing",
        "audit_wire_unknown_field",
    ] {
        assert!(
            observability.contains(marker),
            "SC-31 domain marker missing: {marker}"
        );
    }
    for marker in [
        "SERVER_AUDIT_ACTOR",
        "record.reason",
        "classify_audit_event",
        "audit_event_kind_untrusted",
        "audit_record_payload_untrusted",
    ] {
        assert!(
            reducer.contains(marker),
            "SC-31 reducer marker missing: {marker}"
        );
    }
    assert!(contracts.contains("kiana.audit-event.v1"));
    assert!(protocol.contains("AuditRecordEvent"));
    for marker in [
        "AuditRecordEvent",
        "from_wire_value",
        "eventlog_audit_kind_untrusted",
        "eventlog_audit_payload_invalid",
        "eventlog_audit_field_missing",
        "eventlog_audit_redaction_binding_mismatch",
        "eventlog_audit_idempotency_binding_mismatch",
        "payload_recoverable",
        "artifact_refs",
    ] {
        assert!(
            eventlog_boundary.contains(marker),
            "SC-31 EventLog boundary marker missing: {marker}"
        );
    }
    assert!(eventlog_plan.contains("audit_contract::validate_runtime_event"));
    let plan = item_source(journal_plan, "pub fn plan_transition(");
    assert!(compact_code(&plan).contains("validate_event_for_storage(event)?"));
    let storage = item_source(eventlog_plan, "pub(crate) fn validate_event_for_storage(");
    assert!(compact_code(&storage)
        .contains("audit_contract::validate_runtime_event(event).map_err(PortError::Failed)?"));
    assert!(!eventlog_boundary.contains("EventStorePort"));
    assert!(!eventlog_boundary.contains("std::fs"));
}

/// The SC-31 fixtures, refusal codes and lane wiring are bound to one another. `include_str!`
/// hits are text checks, not runtime proof.
#[test]
fn sc31_deny_first_fixtures_codes_and_lane_are_wired() {
    let observability = include_str!("../../kiana-domain/src/observability.rs");
    let event_contracts = include_str!("../../kiana-domain/src/event_contracts.rs");
    let audit_contract = include_str!("../../kiana-eventlog/src/audit_contract.rs");
    let domain_fixture = include_str!("../../kiana-domain/tests/sc31_audit_record.rs");
    let eventlog_fixture = include_str!("../../kiana-eventlog/tests/sc31_audit_event.rs");
    let protocol_fixture = include_str!("../../kiana-protocol/tests/sc31_audit_record_wire.rs");
    let workflow = include_str!("../../.github/workflows/sc31-audit-record.yml");
    let baseline = include_str!("../../docs/roadmap/sc31-audit-record-baseline.md");
    let card = include_str!("../../docs/roadmap/security-compliance.md");
    let roadmap = include_str!("../../docs/roadmap.md");
    let current_status = include_str!("../../CURRENT_STATUS.md");

    // Fixture names, deny-first order.
    for fixture in [
        "audit_fact_without_actor_decision_reason_or_source_cursor_is_refused",
        "raw_secret_or_prompt_text_never_enters_an_audit_record",
        "prompt_and_transcript_fields_cannot_be_smuggled_into_an_audit_envelope",
        "audit_event_versions_follow_the_registered_contract",
    ] {
        assert!(
            domain_fixture.contains(fixture),
            "SC-31 domain fixture missing: {fixture}"
        );
    }
    for fixture in [
        "missing_audit_identity_fields_are_refused_before_any_storage_mutation",
        "secrets_prompts_and_version_drift_never_enter_the_audit_log",
    ] {
        assert!(
            eventlog_fixture.contains(fixture),
            "SC-31 EventLog fixture missing: {fixture}"
        );
    }
    for fixture in [
        "protocol_reexports_the_registered_append_only_audit_envelope",
        "protocol_wire_decode_refuses_missing_identity_secret_and_prompt_fields",
        "protocol_runtime_frame_binds_redaction_recoverability_and_idempotency",
    ] {
        assert!(
            protocol_fixture.contains(fixture),
            "SC-31 protocol fixture missing: {fixture}"
        );
    }

    // Stable refusal codes: missing actor/decision/reason/source cursor, raw secret/prompt
    // material and version drift stay pinned by name in the fixtures that prove them.
    for code in [
        "audit_wire_required_field_missing:record.actor_ref",
        "audit_wire_required_field_missing:record.decision",
        "audit_wire_required_field_missing:record.reason",
        "audit_wire_required_field_missing:source_cursor",
        "audit_wire_unknown_field:prompt",
        "audit_wire_unknown_field:record.transcript",
        "audit_wire_payload_invalid",
        "audit_actor_ref_required",
        "audit_reason_required",
        "observability_source_event_ids_required",
        "audit_reason_unredacted",
        "audit_attribute_key_unredacted",
        "audit_attribute_value_unredacted",
        "audit_event_schema_version_incompatible",
    ] {
        assert!(
            domain_fixture.contains(code) || protocol_fixture.contains(code),
            "SC-31 fixture no longer pins refusal code: {code}"
        );
    }
    for code in [
        "eventlog_audit_field_missing:record.actor_ref",
        "eventlog_audit_field_missing:source_cursor",
        "eventlog_audit_field_missing:record",
        "eventlog_audit_envelope:audit_actor_ref_required",
        "eventlog_audit_envelope:audit_reason_required",
        "eventlog_audit_envelope:audit_reason_unredacted",
        "eventlog_audit_envelope:audit_event_schema_version_incompatible",
        "eventlog_audit_payload_invalid",
    ] {
        assert!(
            eventlog_fixture.contains(code),
            "SC-31 EventLog fixture no longer pins refusal code: {code}"
        );
    }
    assert!(
        audit_contract.contains("AuditRecordEvent::from_wire_value"),
        "the EventLog boundary must decode through the strict domain wire decode"
    );

    // Single authority: the generic domain event registry must not claim the audit payload, and
    // the audit contract has no update/delete operation to bypass append-only history.
    assert!(!event_contracts.contains("\"audit.record\""));
    assert!(!observability.contains("fn update_audit_record"));
    assert!(!observability.contains("fn delete_audit_record"));
    assert!(!audit_contract.contains("fn update_audit"));
    assert!(!audit_contract.contains("fn delete_audit"));

    // Manual lane stays dispatch-only, locked, serial and fail-closed.
    for marker in [
        "workflow_dispatch:",
        "permissions:",
        "contents: read",
        "cargo fetch --locked",
        "cargo fmt --all --check",
        "cargo test -p kiana-core --test sc31_audit_record_guard --locked -- --test-threads=1",
        "cargo test -p kiana-domain --test sc31_audit_record --locked -- --test-threads=1",
        "cargo test -p kiana-eventlog --test sc31_audit_event --locked -- --test-threads=1",
        "cargo test -p kiana-protocol --test sc31_audit_record_wire --locked -- --test-threads=1",
        "--no-run",
    ] {
        assert!(
            workflow.contains(marker),
            "SC-31 lane marker missing: {marker}"
        );
    }
    for forbidden in [
        "push:",
        "pull_request:",
        "schedule:",
        "continue-on-error",
        "--ignored",
    ] {
        assert!(
            !workflow.contains(forbidden),
            "SC-31 lane must stay manual and fail-closed: {forbidden}"
        );
    }
    assert!(
        !domain_fixture.contains("#[ignore]") && !eventlog_fixture.contains("#[ignore]"),
        "SC-31 fixtures must not be ignored"
    );

    // Auto shards still claim the integration targets; eventlog/protocol are whole-crate shards.
    let shards: serde_json::Value =
        serde_json::from_str(include_str!("../../scripts/ci/test-shards.json"))
            .expect("shard registry json");
    let domain_targets = shards["kiana-domain-s4/4"]["targets"]
        .as_array()
        .expect("domain shard targets");
    assert!(domain_targets
        .iter()
        .any(|target| target.as_str() == Some("sc31_audit_record")));
    let core_targets = shards["kiana-core-s6/6"]["targets"]
        .as_array()
        .expect("core shard targets");
    assert!(core_targets
        .iter()
        .any(|target| target.as_str() == Some("sc31_audit_record_guard")));
    assert!(shards["kiana-eventlog"]["targets"].is_null());
    assert!(shards["kiana-protocol"]["targets"].is_null());

    // Bookkeeping anchors stay discoverable.
    assert!(baseline.contains("2026-10-17 completion slice (CI-only)"));
    assert!(baseline.contains("from_wire_value"));
    assert!(card.contains("<a id=\"step-sc-31\"></a>SC-31"));
    assert!(roadmap.contains("[`SC-31`](roadmap/security-compliance.md#step-sc-31)"));
    assert!(current_status.contains("### 2026-10-17 SC-31"));
}

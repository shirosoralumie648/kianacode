//! CP-26 source guard for decision explanation, audit correlation and evidence redaction.

fn require(source: &str, markers: &[&str], label: &str) {
    for marker in markers {
        assert!(
            source.contains(marker),
            "CP-26 {label} marker missing: {marker}"
        );
    }
}

#[test]
fn cp_decision_trace_links_every_effect_to_its_authority() {
    let audit = include_str!("../../kiana-domain/src/audit.rs");
    let record = include_str!("../../kiana-domain/src/observability.rs");
    let correlation = include_str!("../../kiana-domain/src/correlation.rs");
    let security_context = include_str!("../src/security_context.rs");
    let fence = include_str!("../src/security_fence.rs");
    let approval = include_str!("../src/approval_binding.rs");
    let events = include_str!("../src/events.rs");
    let dispatch = include_str!("../src/dispatch.rs");
    let receipts = include_str!("../src/receipts.rs");
    let projection = include_str!("../src/audit_projection.rs");
    let daemon = include_str!("../../kiana-daemon/src/execution_control.rs");

    require(
        audit,
        &[
            "SERVER_AUDIT_ACTOR",
            "classify_audit_event",
            "target_binding",
            "authority_epoch",
            "data_epoch",
            "request_id",
            "command_id",
            "correlation_ref",
            "causation_ref",
            "action_digest",
            "input_digest",
            "reason_code",
            "redaction_profile",
            "audit_record_payload_untrusted",
            "audit_decision_conflict",
            "append_audit_records",
        ],
        "audit reducer",
    );
    require(
        record,
        &[
            "AuditRecord",
            "source_event_ids",
            "authority_epoch",
            "data_epoch",
            "record_digest",
            "retention_class",
            "attributes",
            "validate",
        ],
        "audit record",
    );
    require(
        correlation,
        &[
            "CorrelationContext",
            "correlation_id",
            "causation",
            "command_id",
            "request_id",
            "attempt",
            "actor_ref",
            "authority_epoch",
            "data_epoch",
            "validate_for_request",
        ],
        "correlation context",
    );
    require(
        security_context,
        &[
            "SecurityContext",
            "policy_revision",
            "authority_epoch",
            "data_epoch",
            "context_digest",
            "validate_request_assertions",
            "require_trusted_for_effect",
        ],
        "security context",
    );
    require(
        fence,
        &[
            "issue_authority_fence",
            "validate_authority_fence",
            "authority_epoch",
            "policy_revision",
            "config_revision",
            "FACT_FENCE_MISMATCH",
        ],
        "authority fence",
    );
    require(
        approval,
        &[
            "ApprovalBinding",
            "subject_request_id",
            "target_digest",
            "payload_digest",
            "scope_digest",
            "grant_id",
            "authority_epoch",
            "policy_revision",
            "binding_digest",
            "validate_request",
            "PolicyApprovalBindingMismatch",
        ],
        "approval binding",
    );
    require(
        events,
        &[
            "stamp_event_links",
            "correlation_id",
            "causation_event_id",
            "prepare_event_payload",
            "redact_event_value",
            "with_redaction_metadata",
            "with_idempotency_key",
            "append_event",
        ],
        "event commit boundary",
    );
    require(
        dispatch,
        &[
            "commit_confirmed",
            "execution.prepared",
            "execution.result_committed",
            "attempt",
            "action_digest",
            "effect_known",
            "result_receipt",
            "validate_for_request",
        ],
        "dispatch evidence",
    );
    require(
        receipts,
        &[
            "typed_run_receipt",
            "typed_execution_receipts",
            "terminal_reason",
            "policy_revision",
            "authority_epoch",
            "provider_receipt_refs",
            "verification",
            "redact_event_value(&receipt)",
        ],
        "Receipt evidence",
    );
    require(
        projection,
        &[
            "rebuild_audit_projection",
            "query_audit",
            "source_cursor",
            "filter_digest",
            "audit_query_cursor_stale",
            "owned_runs",
            "owned_approvals",
            "raw RuntimeEvent",
        ],
        "audit query",
    );
    require(
        daemon,
        &[
            "authority_revoked",
            "result_unknown",
            "capture_complete",
            "redactor",
            "RuntimeEvent",
            "stop_confirmed",
        ],
        "daemon effect telemetry",
    );
}

#[test]
fn cp_secret_never_appears_in_error_event_receipt_or_explain() {
    let reasons = include_str!("../../kiana-domain/src/security_reasons.rs");
    let audit = include_str!("../../kiana-domain/src/audit.rs");
    let redaction = include_str!("../src/redaction.rs");
    let events = include_str!("../src/events.rs");
    let receipts = include_str!("../src/receipts.rs");
    let export = include_str!("../src/audit_export.rs");
    let protocol = include_str!("../../kiana-protocol/src/lib.rs");
    let diagnostics = include_str!("../../kiana-entrypoints/src/provider_diagnostics.rs");

    require(
        reasons,
        &[
            "detail_digest",
            "evidence_refs",
            "reason_digest",
            "classify_security_reason",
            "UnknownUnclassified",
            "deliberately a digest",
        ],
        "security explanation",
    );
    require(
        redaction,
        &[
            "redact_event_text",
            "redact_event_value",
            "[REDACTED]",
            "secret_ref",
        ],
        "EventLog redaction",
    );
    require(
        events,
        &[
            "prepare_event_payload",
            "event_redaction_not_stable",
            "payload_depth",
            "MAX_JOURNAL_EVENT_BYTES",
        ],
        "event safety",
    );
    require(
        receipts,
        &[
            "redact_event_text",
            "redact_event_value",
            "redaction_profile",
            "receipt_data_revoked",
            "retained_event_ids",
        ],
        "Receipt redaction",
    );
    require(
        export,
        &[
            "redacted_record",
            "render_content",
            "audit_export_secret_sentinel",
            "PermissionProfile::Safe",
            "AuditDeliveryState",
        ],
        "audit export boundary",
    );
    require(
        protocol,
        &[
            "SecurityReason",
            "credential_ref",
            "provider-credential-probe",
            "redacted",
        ],
        "wire diagnostic boundary",
    );
    require(
        diagnostics,
        &[
            "redact_text",
            "access_token",
            "client_secret",
            "credential_value",
            "redacted",
        ],
        "provider diagnostics boundary",
    );

    for source in [reasons, audit, receipts, export, protocol] {
        for forbidden in ["raw_prompt", "raw_output", "secret_payload"] {
            assert!(
                !source.contains(forbidden),
                "CP-26 explain/receipt source contains forbidden raw field: {forbidden}"
            );
        }
    }
}

#[test]
fn cp_explain_does_not_consume_grant_or_approval() {
    let projection = include_str!("../src/audit_projection.rs");
    let export = include_str!("../src/audit_export.rs");
    let reasons = include_str!("../../kiana-domain/src/security_reasons.rs");
    let client = include_str!("../../kiana-client/src/lib.rs");
    let daemon = include_str!("../../kiana-daemon/src/lib.rs");

    require(
        projection,
        &[
            "pub async fn query_audit",
            "read_all_events",
            "AuditQueryInput",
            "read-only",
            "audit_query_unauthenticated",
        ],
        "read-only explain query",
    );
    require(
        export,
        &[
            "query_audit",
            "PermissionProfile::Safe",
            "audit_export_requires_explicit_permission",
            "AuditExportManifest",
        ],
        "controlled explain export",
    );
    require(
        reasons,
        &[
            "explanation value, not an authorization decision",
            "SecurityReason",
            "policy",
            "detail_digest",
        ],
        "reason explain contract",
    );
    require(
        client,
        &["audit_query", "audit_export", "AuditQueryRequest"],
        "client explain facade",
    );
    require(
        daemon,
        &[
            "RequestBody::AuditQuery",
            "RequestBody::AuditExport",
            "PermissionProfile::Safe",
        ],
        "daemon explain route",
    );

    for source in [projection, export, reasons] {
        for forbidden in [
            "issue_permit(",
            "consume_approval(",
            "decide_approval(",
            "execute_authorized_request(",
            "CapabilityBroker",
        ] {
            assert!(
                !source.contains(forbidden),
                "read-only explain path contains authority mutation: {forbidden}"
            );
        }
    }
}

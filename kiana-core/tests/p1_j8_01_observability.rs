#[test]
fn observability_fields_are_traceable_and_secret_free() {
    let receipts = include_str!("../src/receipts.rs");
    let capability = include_str!("../src/capability_attempt_projection.rs");
    let model = include_str!("../src/model_attempt_projection.rs");
    let spans = include_str!("../src/span_projection.rs");
    let domain = include_str!("../../kiana-domain/src/observability.rs");
    let redaction = include_str!("../src/redaction.rs");
    let policy = include_str!("../../kiana-policy/src/security.rs");
    let baseline = include_str!("../../docs/roadmap/p1-j8-01-observability-baseline.md");
    for marker in [
        "observability_from_events",
        "policy_observations_from_events",
        "policy_verdict",
        "gate_verdict",
        "tool_args_hash",
        "action_digest",
        "cancellation_observations_from_events",
        "persistence_revision",
        "source_cursor",
        "source_event_ids",
        "project_model_attempts",
        "project_capability_attempts",
        "project_spans",
        "ModelAttemptRecord",
        "CapabilityAttemptRecord",
        "retry_class",
        "usage_complete",
        "stop_confirmed",
        "redact_event_text",
        "redact_event_value",
        "DecisionTrace",
        "policy-decision-trace.v1",
    ] {
        assert!(
            receipts.contains(marker)
                || capability.contains(marker)
                || model.contains(marker)
                || spans.contains(marker)
                || domain.contains(marker)
                || redaction.contains(marker)
                || policy.contains(marker)
                || baseline.contains(marker),
            "observability marker missing: {marker}"
        );
    }
    assert!(receipts.contains("\"observability\": observability_from_events(run_id, events)"));
    assert!(receipts.contains("redact_event_text"));
    assert!(receipts.contains("serde_json::to_value(records)"));
    assert!(!receipts.contains("broker.execute"));
    assert!(!receipts.contains("ProviderClient::send"));
}

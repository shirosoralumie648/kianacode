#[test]
fn trigger_admission_binds_source_owner_approval_and_occurrence_keys() {
    let domain = include_str!("../../kiana-domain/src/automation.rs");
    let planner = include_str!("../../kiana-workflow/src/durable.rs");
    let core = include_str!("../src/automation.rs");
    for marker in [
        "TRIGGER_DEFINITION_SCHEMA",
        "validate_shape",
        "trigger_event_kind_invalid",
        "trigger_schedule_overflow",
        "trigger_definition_shape_invalid",
        "trigger_firing_key_invalid",
        "deny_unknown_fields",
    ] {
        assert!(
            domain.contains(marker) || planner.contains(marker),
            "trigger marker missing: {marker}"
        );
    }
    for marker in [
        "trigger_owner_or_role_denied",
        "trigger_approval_required",
        "trigger_approval_event_required",
        "trigger_event_evidence_invalid",
        "trigger_budget_exhausted",
        "trigger_already_running",
        "trigger_queue_full",
        "t.fired",
        "event_ref",
    ] {
        assert!(
            planner.contains(marker),
            "planner trigger marker missing: {marker}"
        );
    }
    assert!(core.contains("workflow_proof"));
    assert!(core.contains("workflow_evidence_owner_mismatch"));
    assert!(!planner.contains("CapabilityBroker"));
    assert!(!planner.contains("tokio::spawn"));
}

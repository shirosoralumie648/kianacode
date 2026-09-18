#[test]
fn workflow_definition_admission_is_pure_and_version_bound() {
    let domain = include_str!("../../kiana-domain/src/automation.rs");
    let planner = include_str!("../../kiana-workflow/src/durable.rs");
    let core = include_str!("../src/automation.rs");
    for marker in [
        "WORKFLOW_DEFINITION_SCHEMA",
        "pub fn digest(&self)",
        "validate_digest",
        "deny_unknown_fields",
    ] {
        assert!(
            domain.contains(marker),
            "domain definition marker missing: {marker}"
        );
    }
    for marker in [
        "validate_definition",
        "workflow_dependency_graph_invalid",
        "workflow_artifact_dependency_missing",
        "workflow_definition_version_immutable",
        "workflow_definition_role_denied",
        "workflow_packet_required",
        "definition_key",
    ] {
        assert!(
            planner.contains(marker),
            "planner definition marker missing: {marker}"
        );
    }
    assert!(core.contains("plan_command"));
    assert!(core.contains("commit_workflow"));
    assert!(core.contains("authorize_and_execute"));
    assert!(!planner.contains("tokio::spawn"));
    assert!(!planner.contains("SystemTime"));
}

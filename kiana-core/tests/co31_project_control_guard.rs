#[test]
fn project_control_keeps_cancel_and_dispatch_fences_on_the_single_control_plane() {
    let contract = include_str!("../../kiana-domain/src/project_control.rs");
    let company = include_str!("../../kiana-domain/src/company.rs");
    let cancellation = include_str!("../../kiana-domain/src/cancellation.rs");
    let core = include_str!("../src/project_control.rs");
    let lifecycle = include_str!("../src/lifecycle.rs");
    let cell_registry = include_str!("../src/cell_registry.rs");
    for marker in [
        "PROJECT_CONTROL_SCHEMA",
        "ProjectControlAction",
        "ProjectControlChild",
        "ProjectControlPlan",
        "ProjectControlLedger",
        "project_cancel_with_unconfirmed_child",
        "project_result_unknown_requires_reconciliation",
        "project_control_resume_fence_invalid",
        "record_project_control",
        "PauseProject",
        "ResumeProject",
        "RequestCancelProject",
        "ConfirmCancelProject",
        "RunCancellationFact",
        "DispatchIntent",
        "cancel_run",
        "retire_cell",
    ] {
        assert!(
            contract.contains(marker)
                || company.contains(marker)
                || cancellation.contains(marker)
                || core.contains(marker)
                || lifecycle.contains(marker)
                || cell_registry.contains(marker),
            "CO-31 marker missing: {marker}"
        );
    }
    assert!(!contract.contains("CapabilityBroker"));
    assert!(!contract.contains("Runner"));
}

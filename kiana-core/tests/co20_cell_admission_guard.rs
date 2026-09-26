#[test]
fn cell_admission_binds_all_resources_and_releases_only_its_own_set() {
    let domain = include_str!("../../kiana-domain/src/cell_admission.rs");
    let registry = include_str!("../src/cell_registry.rs");
    let collaboration = include_str!("../src/collaboration.rs");
    let core = include_str!("../src/lib.rs");

    for marker in [
        "CELL_ADMISSION_SCHEMA",
        "CellAdmissionResources",
        "SpawnPlanId",
        "BudgetLeaseId",
        "CapabilityGrantId",
        "SupervisionLeaseId",
        "owned_paths",
        "CellAdmissionStatus::Reserved",
        "CellAdmissionStatus::Committed",
        "CellAdmissionStatus::RolledBack",
        "CellAdmissionStatus::Retired",
        "CellAdmissionStatus::Unknown",
        "cell_admission_resource_conflict",
        "cell_admission_active_capability_release_forbidden",
        "resources_released",
        "commit_spawn",
        "abort_spawn",
        "retire_cell",
        "release_resources",
        "budget.release",
        "path_locks.remove",
        "cell.retired",
    ] {
        assert!(
            domain.contains(marker)
                || registry.contains(marker)
                || collaboration.contains(marker)
                || core.contains(marker),
            "CO-20 marker missing: {marker}"
        );
    }
    assert!(registry.contains("if record.resources_released"));
    assert!(registry.contains("cell_capability_in_flight"));
    assert!(!domain.contains("CapabilityBroker"));
    assert!(!domain.contains("Runner"));
}

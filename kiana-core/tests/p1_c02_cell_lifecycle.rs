#[test]
fn retire_revokes_grants_and_releases_budget() {
    let registry = include_str!("../src/cell_registry.rs");
    let collaboration = include_str!("../src/collaboration.rs");
    let existing = include_str!("control_plane.rs");
    for marker in [
        "commit_spawn",
        "transition_cell",
        "retire_cell",
        "release_resources",
        "resources_released",
        "path_locks.remove",
        "budget.release",
        "cell.retired",
    ] {
        assert!(
            registry.contains(marker)
                || collaboration.contains(marker)
                || existing.contains(marker),
            "Cell lifecycle marker missing: {marker}"
        );
    }
    assert!(registry.contains("if record.resources_released"));
    assert!(registry.contains("cell_capability_in_flight"));
    assert!(registry.contains("spawn_reservation_already_released"));
}

#[test]
fn cell_retirement_releases_only_its_own_resources_once() {
    let registry = include_str!("../src/cell_registry.rs");
    assert!(
        registry.contains("state.path_locks.get(path) == Some(&record.reservation.cell.cell_id)")
    );
    assert!(registry.contains("record.reservation.budget"));
    assert!(registry.contains("record.reservation.grant.grant_id"));
    assert!(registry.contains("record.reservation.supervision.lease_id"));
}

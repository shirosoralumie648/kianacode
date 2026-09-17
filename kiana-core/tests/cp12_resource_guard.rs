#[test]
fn cp12_resources_use_canonical_paths_and_fencing_without_a_second_execution_path() {
    let domain = include_str!("../../kiana-domain/src/resource_leases.rs");
    let sessions = include_str!("../src/sessions.rs");
    let core = include_str!("../src/resource_leases.rs");
    let ports = include_str!("../../kiana-ports/src/lib.rs");
    let cell = include_str!("../src/cell_registry.rs");
    for marker in [
        "ResourceLease",
        "fence_token",
        "validate_current",
        "validate_successor",
        "canonical_resource_set",
        "path_lock_conflict",
        "O_NOFOLLOW",
        "issue_resource_lease",
        "validate_resource_lease",
        "authority_epoch",
        "fencing_token",
    ] {
        assert!(
            domain.contains(marker)
                || sessions.contains(marker)
                || core.contains(marker)
                || ports.contains(marker)
                || cell.contains(marker),
            "CP-12 marker missing: {marker}"
        );
    }
    assert!(sessions.contains("canonical_resource_set"));
    assert!(sessions.contains("LOCK_NB"));
    assert!(core.contains("authority_epoch"));
    assert!(ports.contains("fencing_token"));
    for forbidden in [
        "authorize_and_execute_from_lease",
        "default_allow",
        "ignore_path",
    ] {
        assert!(
            !domain.contains(forbidden) && !core.contains(forbidden),
            "resource lease must not enable {forbidden}"
        );
    }
}

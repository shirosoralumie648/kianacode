#[test]
fn authority_snapshot_and_daemon_assignment_paths_are_server_owned() {
    let authority = include_str!("../src/security_authority.rs");
    let daemon = include_str!("../../kiana-daemon/src/lib.rs");
    let domain = include_str!("../../kiana-domain/src/trust_snapshots.rs");
    for marker in [
        "SecurityAuthoritySnapshot",
        "ProjectTrustSnapshot",
        "DepartmentSnapshot",
        "validate_request",
        "require_trusted_for_effect",
        "AUTH_PROJECT_MISMATCH",
        "AUTH_ROLE_MISMATCH",
    ] {
        assert!(
            authority.contains(marker),
            "authority marker missing: {marker}"
        );
    }
    for marker in [
        "resolve_assignment_for_project",
        "context_from_assignment",
        "project_trust_snapshot",
        "DepartmentSnapshot::from_spec",
        "SecurityAuthoritySnapshot::from_parts",
        "self.principal.identity",
    ] {
        assert!(
            daemon.contains(marker),
            "daemon assignment marker missing: {marker}"
        );
    }
    for marker in [
        "ProjectTrustSnapshot",
        "DEPARTMENT_SNAPSHOT_SCHEMA",
        "department_snapshot_roles_noncanonical",
        "deny_unknown_fields",
    ] {
        assert!(
            domain.contains(marker),
            "domain snapshot marker missing: {marker}"
        );
    }
    assert!(!authority.contains("CapabilityBroker"));
    assert!(!authority.contains("std::process"));
}

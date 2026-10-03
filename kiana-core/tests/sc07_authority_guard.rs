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
    assert!(
        authority.contains("context.project_trusted && !self.project_trust.trusted"),
        "wire trust may not upgrade a server-owned untrusted snapshot"
    );
    assert!(
        authority.contains("self.project_trust\n            .trusted"),
        "effect gate must read the server-owned trust snapshot"
    );
    for marker in [
        "project.project_id != self.project_trust.project_id",
        "\"canonical_root\": project.canonical_root",
        "project.trust_revision != self.project_trust.trust_revision",
        "context.project_root != project.root",
    ] {
        assert!(
            authority.contains(marker),
            "request project binding marker missing: {marker}"
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
    assert!(
        daemon.contains("let project = self.project_identity(&context.project_root)?;")
            && daemon.contains("authority\n            .validate_request(&context, &project)"),
        "daemon assignment helper must validate the request against its resolved project identity"
    );
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

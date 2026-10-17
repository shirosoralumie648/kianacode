#[test]
fn authority_snapshot_and_daemon_assignment_paths_are_server_owned() {
    let authority = include_str!("../src/security_authority.rs");
    let daemon = include_str!("../../kiana-daemon/src/lib.rs");
    let assignment = include_str!("../../kiana-domain/src/assignment.rs");
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
        "SecurityAuthorityProvenance",
        "SECURITY_AUTHORITY_PROVENANCE_SCHEMA",
        "trust_source",
        "trust_revision",
        "role_source",
        "role_revision",
        "department_revision",
        "provenance_digest",
    ] {
        assert!(
            authority.contains(marker),
            "authority provenance marker missing: {marker}"
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
        "authority_snapshot_for_assignment",
        "authority_provenance",
        "project_trust_snapshot",
        "DepartmentSnapshot::from_spec",
        "SecurityAuthoritySnapshot::from_parts",
        "SecurityAuthorityProvenance::from_authority",
        "ASSIGNMENT_DIRECTORY_SOURCE",
        "self.principal.identity",
    ] {
        assert!(
            daemon.contains(marker),
            "daemon assignment marker missing: {marker}"
        );
    }
    assert!(
        daemon.contains("fn authority_snapshot_for_assignment")
            && daemon.contains("let project = self.project_identity(project_root)?;")
            && daemon.contains("assignment.project_id != project.project_id")
            && daemon.contains("authority\n            .validate_request(&context, &project)"),
        "daemon assignment helper must reject foreign projects before authority validation"
    );
    for marker in [
        "role_assignment_identity_invalid",
        "project_assignment_identity_invalid",
        "resolved_assignment_identity_invalid",
    ] {
        assert!(
            assignment.contains(marker),
            "assignment identity marker missing: {marker}"
        );
    }
    assert!(
        assignment.contains("self.assignment_id.as_uuid().is_nil()")
            && authority.contains("self.assignment.validate()?"),
        "authority snapshots must inherit nil assignment identity denial"
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

#[test]
fn company_authority_has_assignment_revalidation_boundary() {
    let domain = include_str!("../../kiana-domain/src/assignment.rs");
    let ports = include_str!("../../kiana-ports/src/lib.rs");
    let daemon = include_str!("../../kiana-daemon/src/lib.rs");
    let company = include_str!("../src/company.rs");

    for marker in [
        "RoleAssignmentStatus",
        "AssignmentDirectory::resolve",
        "assignment_expired_or_missing",
        "authority_epoch",
    ] {
        assert!(
            domain.contains(marker),
            "domain assignment marker missing: {marker}"
        );
    }
    for marker in [
        "trait AssignmentDirectoryPort",
        "resolve_assignment",
        "revoke_assignment",
        "IdentityAssignmentPort",
    ] {
        assert!(
            ports.contains(marker),
            "identity port marker missing: {marker}"
        );
    }
    for marker in [
        "authenticated_principal",
        "resolve_assignment_for_project",
        "context_from_assignment",
        "project_identity",
    ] {
        assert!(
            daemon.contains(marker),
            "daemon assignment marker missing: {marker}"
        );
    }
    for marker in [
        "validate_company_assignment",
        "assignment_actor_mismatch",
        "assignment_role_mismatch",
        "assignment_expired_or_missing",
    ] {
        assert!(
            company.contains(marker),
            "core assignment guard missing: {marker}"
        );
    }
}

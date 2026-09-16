use kiana_domain::{
    AssignmentDirectory, AssignmentId, AuthenticatedPrincipalRef, OrganizationId,
    ProjectAssignment, ProjectAssignmentId, ProjectId, RoleAssignment, RoleAssignmentStatus,
};

fn fixtures() -> (
    AssignmentDirectory,
    AuthenticatedPrincipalRef,
    OrganizationId,
    ProjectId,
    AssignmentId,
) {
    let principal = AuthenticatedPrincipalRef::local();
    let organization_id = OrganizationId::new();
    let project_id = ProjectId::new();
    let assignment_id = AssignmentId::new();
    let role = RoleAssignment::new(
        assignment_id,
        principal.clone(),
        organization_id,
        "builder",
        "executing",
        vec![project_id],
        1,
        200,
        RoleAssignmentStatus::Active,
        1,
        1,
    )
    .unwrap();
    let project = ProjectAssignment::new(
        ProjectAssignmentId::new(),
        assignment_id,
        principal.clone(),
        organization_id,
        project_id,
        1,
        200,
        false,
        1,
        1,
    )
    .unwrap();
    let mut directory = AssignmentDirectory::new();
    directory.register_role(role).unwrap();
    directory.register_project(project).unwrap();
    (
        directory,
        principal,
        organization_id,
        project_id,
        assignment_id,
    )
}

#[test]
fn active_assignment_survives_reopen_with_the_same_scope() {
    let (directory, principal, organization_id, project_id, _) = fixtures();
    let resolved = directory
        .resolve(&principal, organization_id, project_id, "builder", 100)
        .unwrap();
    resolved.validate().unwrap();
    let encoded = serde_json::to_value(&resolved).unwrap();
    let decoded: kiana_domain::ResolvedAssignment = serde_json::from_value(encoded).unwrap();
    assert_eq!(decoded, resolved);
    assert_eq!(resolved.department_id, "executing");
}

#[test]
fn revoked_assignment_blocks_company_command_and_approved_continuation() {
    let (mut directory, principal, organization_id, project_id, assignment_id) = fixtures();
    directory.revoke_role(assignment_id).unwrap();
    assert_eq!(
        directory
            .resolve(&principal, organization_id, project_id, "builder", 100)
            .unwrap_err(),
        "assignment_expired_or_missing"
    );
    assert!(directory
        .resolve(&principal, organization_id, project_id, "builder", 250)
        .is_err());
}

#[test]
fn client_role_name_cannot_impersonate_sponsor() {
    let (directory, principal, organization_id, project_id, _) = fixtures();
    assert_eq!(
        directory
            .resolve(&principal, organization_id, project_id, "sponsor", 100)
            .unwrap_err(),
        "assignment_role_mismatch"
    );
}

#[test]
fn assignment_expiry_and_unknown_fields_fail_closed() {
    let (directory, principal, organization_id, project_id, _) = fixtures();
    assert!(directory
        .resolve(&principal, organization_id, project_id, "builder", 200)
        .is_err());
    let role = directory.role_assignments().next().unwrap();
    let mut forged = serde_json::to_value(role).unwrap();
    forged["unknown"] = serde_json::json!(true);
    assert!(serde_json::from_value::<RoleAssignment>(forged).is_err());
}

#[test]
fn duplicate_and_out_of_window_project_assignments_do_not_mutate_directory() {
    let (mut directory, principal, organization_id, project_id, assignment_id) = fixtures();
    let role = directory.role_assignments().next().unwrap().clone();
    assert_eq!(
        directory.register_role(role.clone()).unwrap_err(),
        "role_assignment_duplicate"
    );
    assert_eq!(directory.role_assignments().count(), 1);

    let out_of_window = ProjectAssignment::new(
        ProjectAssignmentId::new(),
        assignment_id,
        principal,
        organization_id,
        project_id,
        1,
        201,
        false,
        1,
        1,
    )
    .unwrap();
    assert_eq!(
        directory.register_project(out_of_window).unwrap_err(),
        "project_assignment_window_mismatch"
    );
    assert_eq!(directory.project_assignments().count(), 1);
}

#[test]
fn resolved_digest_and_role_department_binding_fail_closed() {
    let (directory, principal, organization_id, project_id, _) = fixtures();
    let resolved = directory
        .resolve(&principal, organization_id, project_id, "builder", 100)
        .unwrap();
    let mut forged = resolved.clone();
    forged.department_id = "planning".to_owned();
    assert_eq!(
        forged.validate().unwrap_err(),
        "resolved_assignment_role_invalid"
    );
    let mut forged = resolved;
    forged.resolution_digest =
        "sha256:0000000000000000000000000000000000000000000000000000000000000000".to_owned();
    assert_eq!(
        forged.validate().unwrap_err(),
        "resolved_assignment_digest_mismatch"
    );
}

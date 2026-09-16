use kiana_core::validate_company_assignment;
use kiana_domain::{
    AssignmentDirectory, AssignmentId, AuthenticatedPrincipalRef, OrganizationId,
    ProjectAssignment, ProjectAssignmentId, ProjectId, RequestContext, RoleAssignment,
    RoleAssignmentStatus,
};

fn fixture() -> (kiana_domain::ResolvedAssignment, RequestContext) {
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
    let assignment = directory
        .resolve(&principal, organization_id, project_id, "builder", 100)
        .unwrap();
    let mut context = RequestContext::local("co03", "/tmp/kiana-co03");
    context.project_trusted = true;
    context.assign_role(&kiana_domain::RoleSpec::builder());
    context.actor_id = Some(principal.principal_id.clone());
    (assignment, context)
}

#[test]
fn company_assignment_guard_rejects_expiry_and_actor_or_role_drift() {
    let (assignment, context) = fixture();
    assert!(validate_company_assignment(&context, &assignment, 100, false).is_ok());
    assert_eq!(
        validate_company_assignment(&context, &assignment, 200, false).unwrap_err(),
        "assignment_expired_or_missing"
    );

    let mut wrong_actor = context.clone();
    wrong_actor.actor_id = Some("forged-actor".to_owned());
    assert_eq!(
        validate_company_assignment(&wrong_actor, &assignment, 100, false).unwrap_err(),
        "assignment_actor_mismatch"
    );

    let mut wrong_role = context;
    wrong_role.role_id = "sponsor".to_owned();
    wrong_role.department_id = "initiating".to_owned();
    assert_eq!(
        validate_company_assignment(&wrong_role, &assignment, 100, false).unwrap_err(),
        "assignment_role_mismatch"
    );
}

use kiana_core::{SecurityAuthoritySnapshot, SECURITY_AUTHORITY_SNAPSHOT_SCHEMA};
use kiana_domain::{
    json_digest, AssignmentDirectory, AssignmentId, AuthenticatedPrincipalRef, DepartmentSnapshot,
    DepartmentSpec, OrganizationId, ProjectAssignment, ProjectAssignmentId, ProjectIdentity,
    ProjectTrustSnapshot, RequestContext, RoleAssignment, RoleAssignmentStatus, SecurityContextId,
};
use serde_json::json;

fn fixture() -> (SecurityAuthoritySnapshot, RequestContext) {
    let principal = AuthenticatedPrincipalRef::local();
    let project = ProjectIdentity::new(
        "/repo",
        "/repo",
        None,
        None,
        json_digest(&json!({"trusted":true})),
    )
    .unwrap();
    let organization_id = OrganizationId::new();
    let assignment_id = AssignmentId::new();
    let role = RoleAssignment::new(
        assignment_id,
        principal.clone(),
        organization_id,
        "builder",
        "executing",
        vec![project.project_id],
        1,
        1_000,
        RoleAssignmentStatus::Active,
        1,
        2,
    )
    .unwrap();
    let project_assignment = ProjectAssignment::new(
        ProjectAssignmentId::new(),
        assignment_id,
        principal.clone(),
        organization_id,
        project.project_id,
        1,
        1_000,
        false,
        1,
        2,
    )
    .unwrap();
    let mut directory = AssignmentDirectory::new();
    directory.register_role(role).unwrap();
    directory.register_project(project_assignment).unwrap();
    let resolved = directory
        .resolve(
            &principal,
            organization_id,
            project.project_id,
            "builder",
            100,
        )
        .unwrap();
    let trust = ProjectTrustSnapshot::from_project(&project, true, "fixture", 1).unwrap();
    let department = DepartmentSnapshot::from_spec(&DepartmentSpec::executing(), 1, 2).unwrap();
    let snapshot = SecurityAuthoritySnapshot::from_parts(
        SecurityContextId::new(),
        principal.clone(),
        trust,
        resolved,
        department,
        2,
    )
    .unwrap();
    let mut context = RequestContext::local("sc07", "/repo");
    context.project_trusted = true;
    (snapshot, context)
}

#[test]
fn authority_snapshot_round_trips_and_validates_scope() {
    let (snapshot, context) = fixture();
    assert_eq!(snapshot.schema, SECURITY_AUTHORITY_SNAPSHOT_SCHEMA);
    assert!(snapshot.validate().is_ok());
    assert!(snapshot.validate_request(&context).is_ok());
    assert!(snapshot.require_trusted_for_effect().is_ok());
    assert_eq!(
        SecurityAuthoritySnapshot::from_json(&snapshot.to_json().unwrap()).unwrap(),
        snapshot
    );
}

#[test]
fn authority_snapshot_rejects_foreign_role_project_and_untrusted_effect() {
    let (snapshot, mut context) = fixture();
    context.role_id = "reviewer".to_owned();
    assert_eq!(
        snapshot.validate_request(&context).unwrap_err(),
        "AUTH_ROLE_MISMATCH"
    );
    context.role_id = "builder".to_owned();
    context.project_trusted = false;
    assert!(snapshot.validate_request(&context).is_ok());
    assert_eq!(
        snapshot.require_trusted_for_effect().unwrap_err(),
        "AUTH_PROJECT_UNTRUSTED"
    );

    let mut tampered = snapshot.to_json().unwrap();
    tampered["assignment"]["project_id"] = json!(kiana_domain::ProjectId::new());
    assert!(SecurityAuthoritySnapshot::from_json(&tampered).is_err());
}

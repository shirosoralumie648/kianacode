use kiana_domain::{
    json_digest, AssignmentDirectory, AssignmentId, AuthenticatedPrincipalRef, DepartmentSnapshot,
    DepartmentSpec, OrganizationId, ProjectAssignment, ProjectAssignmentId, ProjectIdentity,
    ProjectTrustSnapshot, RoleAssignment, RoleAssignmentStatus, SessionId,
};
use serde_json::json;

fn project() -> ProjectIdentity {
    ProjectIdentity::new(
        "/repo",
        "/repo",
        None,
        None,
        json_digest(&json!({"trust":"stored"})),
    )
    .unwrap()
}

#[test]
fn project_trust_snapshot_is_server_scoped_and_strict() {
    let project = project();
    let trust =
        ProjectTrustSnapshot::from_project(&project, true, "stored-project-trust", 2).unwrap();
    assert!(trust.trusted);
    assert_eq!(trust.project_id, project.project_id);
    assert!(trust.validate().is_ok());
    assert_eq!(
        ProjectTrustSnapshot::from_json(&trust.to_json().unwrap()).unwrap(),
        trust
    );

    let mut tampered = trust.to_json().unwrap();
    tampered["project_id"] = json!(kiana_domain::ProjectId::new());
    assert!(ProjectTrustSnapshot::from_json(&tampered).is_err());
    tampered["raw_token"] = json!("bearer secret");
    assert!(ProjectTrustSnapshot::from_json(&tampered).is_err());
}

#[test]
fn department_snapshot_is_canonical_and_role_bound() {
    let spec = DepartmentSpec::executing();
    let snapshot = DepartmentSnapshot::from_spec(&spec, 3, 4).unwrap();
    assert!(snapshot.contains_role("builder"));
    assert!(snapshot.validate().is_ok());
    assert_eq!(
        DepartmentSnapshot::from_json(&serde_json::to_value(&snapshot).unwrap()).unwrap(),
        snapshot
    );

    let mut tampered = serde_json::to_value(&snapshot).unwrap();
    tampered["role_ids"] = json!(["reviewer"]);
    assert!(DepartmentSnapshot::from_json(&tampered).is_err());
}

#[test]
fn assignment_directory_resolution_binds_principal_project_role_and_epoch() {
    let principal = AuthenticatedPrincipalRef::local();
    let project = project();
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
        2,
        5,
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
        2,
        5,
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
    assert_eq!(resolved.principal, principal);
    assert_eq!(resolved.project_id, project.project_id);
    assert_eq!(resolved.role_id, "builder");
    assert_eq!(resolved.department_id, "executing");
    assert_eq!(resolved.authority_epoch, 5);
    assert!(SessionId::new("scope").as_str() == "scope");
}

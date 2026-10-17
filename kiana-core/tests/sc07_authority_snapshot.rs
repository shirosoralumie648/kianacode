use kiana_core::{
    SecurityAuthorityProvenance, SecurityAuthoritySnapshot, SECURITY_AUTHORITY_SNAPSHOT_SCHEMA,
};
use kiana_domain::{
    json_digest, AssignmentDirectory, AssignmentId, AuthenticatedPrincipalRef, DepartmentSnapshot,
    DepartmentSpec, OrganizationId, ProjectAssignment, ProjectAssignmentId, ProjectId,
    ProjectIdentity, ProjectTrustSnapshot, RequestContext, RoleAssignment, RoleAssignmentStatus,
    SecurityContextId,
};
use serde_json::json;

fn fixture(trusted: bool) -> (SecurityAuthoritySnapshot, RequestContext, ProjectIdentity) {
    let principal = AuthenticatedPrincipalRef::local();
    let project = ProjectIdentity::new(
        "/repo",
        "/repo",
        None,
        None,
        json_digest(&json!({"trusted":trusted})),
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
    let trust = ProjectTrustSnapshot::from_project(&project, trusted, "fixture", 1).unwrap();
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
    context.project_trusted = trusted;
    (snapshot, context, project)
}

#[test]
fn authority_snapshot_round_trips_and_validates_scope() {
    let (snapshot, context, project) = fixture(true);
    assert_eq!(snapshot.schema, SECURITY_AUTHORITY_SNAPSHOT_SCHEMA);
    assert!(snapshot.validate().is_ok());
    assert!(snapshot.validate_request(&context, &project).is_ok());
    assert!(snapshot.require_trusted_for_effect().is_ok());
    assert_eq!(
        SecurityAuthoritySnapshot::from_json(&snapshot.to_json().unwrap()).unwrap(),
        snapshot
    );
}

#[test]
fn authority_snapshot_rejects_foreign_role_project_and_untrusted_effect() {
    let (snapshot, mut context, project) = fixture(true);
    context.role_id = "reviewer".to_owned();
    assert_eq!(
        snapshot.validate_request(&context, &project).unwrap_err(),
        "AUTH_ROLE_MISMATCH"
    );
    let (untrusted_snapshot, mut untrusted_context, untrusted_project) = fixture(false);
    assert!(untrusted_snapshot
        .validate_request(&untrusted_context, &untrusted_project)
        .is_ok());
    untrusted_context.project_trusted = true;
    assert_eq!(
        untrusted_snapshot
            .validate_request(&untrusted_context, &untrusted_project)
            .unwrap_err(),
        "AUTH_CALLER_UNTRUSTED"
    );
    assert_eq!(
        untrusted_snapshot.require_trusted_for_effect().unwrap_err(),
        "AUTH_PROJECT_UNTRUSTED"
    );

    let mut foreign_context = context.clone();
    foreign_context.role_id = "builder".to_owned();
    foreign_context.project_root = "/foreign".to_owned();
    assert_eq!(
        snapshot
            .validate_request(&foreign_context, &project)
            .unwrap_err(),
        "AUTH_PROJECT_MISMATCH"
    );
    let foreign_project = ProjectIdentity::new(
        "/foreign",
        "/foreign",
        None,
        None,
        json_digest(&json!({"trusted":true})),
    )
    .unwrap();
    let mut matching_foreign_context = context;
    matching_foreign_context.project_root = "/foreign".to_owned();
    assert_eq!(
        snapshot
            .validate_request(&matching_foreign_context, &foreign_project)
            .unwrap_err(),
        "AUTH_PROJECT_MISMATCH"
    );

    let mut tampered = snapshot.to_json().unwrap();
    tampered["assignment"]["project_id"] = json!(kiana_domain::ProjectId::new());
    assert!(SecurityAuthoritySnapshot::from_json(&tampered).is_err());
}

#[test]
fn authority_provenance_exposes_server_sources_and_revisions() {
    let (snapshot, _, _) = fixture(true);
    let provenance = SecurityAuthorityProvenance::from_authority(
        &snapshot,
        "fixture.assignment_directory",
    )
    .unwrap();
    assert_eq!(provenance.project_id, snapshot.assignment.project_id);
    assert_eq!(
        provenance.organization_id,
        snapshot.assignment.organization_id
    );
    assert_eq!(provenance.trust_source, "fixture");
    assert_eq!(
        provenance.trust_revision,
        snapshot.project_trust.trust_revision
    );
    assert_eq!(
        provenance.trust_snapshot_revision,
        snapshot.project_trust.revision
    );
    assert!(provenance.trust_trusted);
    assert_eq!(provenance.role_source, "fixture.assignment_directory");
    assert_eq!(provenance.role_id, "builder");
    assert_eq!(
        provenance.role_revision,
        snapshot.assignment.assignment_revision
    );
    assert_eq!(provenance.department_id, "executing");
    assert_eq!(
        provenance.department_revision,
        snapshot.department.revision
    );
    assert_eq!(provenance.authority_epoch, snapshot.authority_epoch);
    assert_eq!(
        SecurityAuthorityProvenance::from_json(&provenance.to_json().unwrap()).unwrap(),
        provenance
    );
    let mut tampered = provenance.to_json().unwrap();
    tampered["role_revision"] = json!(0);
    assert!(SecurityAuthorityProvenance::from_json(&tampered).is_err());
    let mut foreign_source = provenance.to_json().unwrap();
    foreign_source["role_source"] = json!("");
    assert!(SecurityAuthorityProvenance::from_json(&foreign_source).is_err());
}

#[test]
fn authority_snapshot_rejects_nil_assignment_identity_after_digest_recomputation() {
    let (snapshot, _, _) = fixture(true);
    let mut assignment = snapshot.assignment.clone();
    assignment.project_id = ProjectId::parse_str("00000000-0000-0000-0000-000000000000").unwrap();
    assignment.resolution_digest = assignment.digest();
    assert_eq!(
        SecurityAuthoritySnapshot::from_parts(
            snapshot.security_context_id,
            snapshot.principal.clone(),
            snapshot.project_trust.clone(),
            assignment,
            snapshot.department.clone(),
            snapshot.authority_epoch,
        )
        .unwrap_err(),
        "resolved_assignment_identity_invalid"
    );
}

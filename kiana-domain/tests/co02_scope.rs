use kiana_domain::{
    CompanyScopeRegistry, OrganizationBinding, OrganizationId, ProjectBinding, ProjectId,
    WorkspaceBinding,
};

fn registry() -> (
    CompanyScopeRegistry,
    OrganizationId,
    kiana_domain::WorkspaceId,
) {
    let organization_id = OrganizationId::new();
    let organization = OrganizationBinding::new(organization_id, "Platform", "owner", 1).unwrap();
    let workspace = WorkspaceBinding::new(
        organization_id,
        "/tmp/co02-shared-workspace",
        "trust-rev-1",
        1,
    )
    .unwrap();
    let workspace_id = workspace.workspace_id;
    let mut registry = CompanyScopeRegistry::new();
    registry.register_organization(organization).unwrap();
    registry.add_member(organization_id, "member").unwrap();
    registry.bind_workspace(workspace).unwrap();
    (registry, organization_id, workspace_id)
}

#[test]
fn two_business_projects_share_a_workspace_without_sharing_authority() {
    let (mut registry, organization_id, workspace_id) = registry();
    let first = ProjectId::new();
    let second = ProjectId::new();
    registry
        .bind_project(
            ProjectBinding::new(first, organization_id, workspace_id, "owner", 1).unwrap(),
        )
        .unwrap();
    registry
        .bind_project(
            ProjectBinding::new(second, organization_id, workspace_id, "member", 1).unwrap(),
        )
        .unwrap();

    let first_scope = registry
        .resolve("member", organization_id, first, workspace_id)
        .unwrap();
    let second_scope = registry
        .resolve("member", organization_id, second, workspace_id)
        .unwrap();
    assert_eq!(first_scope.workspace_id, second_scope.workspace_id);
    assert_ne!(first_scope.project_id, second_scope.project_id);
    assert_ne!(first_scope.scope_digest, second_scope.scope_digest);
}

#[test]
fn company_scope_rejects_foreign_project_and_ambiguous_legacy_root() {
    let (mut registry, organization_id, workspace_id) = registry();
    let first = ProjectId::new();
    let second = ProjectId::new();
    registry
        .bind_project(
            ProjectBinding::new(first, organization_id, workspace_id, "owner", 1).unwrap(),
        )
        .unwrap();
    registry
        .bind_project(
            ProjectBinding::new(second, organization_id, workspace_id, "owner", 1).unwrap(),
        )
        .unwrap();
    let other_org = OrganizationId::new();
    assert!(registry
        .resolve("member", other_org, first, workspace_id)
        .is_err());

    registry
        .import_legacy_stream(
            "member",
            organization_id,
            first,
            "/tmp/co02-shared-workspace",
        )
        .unwrap();
    assert!(registry
        .import_legacy_stream(
            "member",
            organization_id,
            second,
            "/tmp/co02-shared-workspace",
        )
        .is_err());
}

#[test]
fn company_scope_rejects_noncanonical_root_and_tampered_digest() {
    let organization_id = OrganizationId::new();
    assert!(WorkspaceBinding::new(organization_id, "relative/root", "trust", 1).is_err());
    let (registry, organization_id, workspace_id) = registry();
    let project_id = ProjectId::new();
    let mut registry = registry;
    registry
        .bind_project(
            ProjectBinding::new(project_id, organization_id, workspace_id, "owner", 1).unwrap(),
        )
        .unwrap();
    let mut scope = registry
        .resolve("owner", organization_id, project_id, workspace_id)
        .unwrap();
    scope.principal_id = "forged".to_owned();
    assert_eq!(
        scope.validate().unwrap_err(),
        "company_scope_digest_mismatch"
    );
}

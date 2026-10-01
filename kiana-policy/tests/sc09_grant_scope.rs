use kiana_domain::{
    CapabilityGrant, CapabilityGrantId, CapabilityKind, CapabilityRequest, GrantId, PrincipalId,
    ProjectId, RequestId, RiskLevel, ScopeDimension, ScopeLimit, ScopeSet,
};
use kiana_policy::{GrantScope, GRANT_SCOPE_SCHEMA};
use serde_json::json;

fn scope(operations: &[&str], paths: &[&str], network: &[&str]) -> ScopeSet {
    ScopeSet::new(
        ScopeDimension::Restricted(operations.iter().map(|v| (*v).to_owned()).collect()),
        if paths.is_empty() {
            ScopeDimension::NotApplicable
        } else {
            ScopeDimension::Restricted(paths.iter().map(|v| (*v).to_owned()).collect())
        },
        ScopeDimension::NotApplicable,
        if network.is_empty() {
            ScopeDimension::NotApplicable
        } else {
            ScopeDimension::Restricted(network.iter().map(|v| (*v).to_owned()).collect())
        },
        ScopeLimit::Restricted(10),
        ScopeLimit::Restricted(2),
    )
    .unwrap()
}

fn parent_grant() -> GrantScope {
    GrantScope::new(
        GrantId::new(),
        None,
        PrincipalId::new(),
        ProjectId::new(),
        scope(&["read", "write"], &["src"], &["api"]),
        vec![
            CapabilityKind::Filesystem,
            CapabilityKind::Network,
            CapabilityKind::Secret,
        ],
        true,
        true,
        true,
        4,
        1_000,
    )
    .unwrap()
}

#[test]
fn grant_scope_intersects_all_layers_without_union_or_transfer() {
    let parent = parent_grant();
    let child = GrantScope::new(
        GrantId::new(),
        Some(parent.grant_id),
        parent.principal_id,
        parent.project_id,
        scope(&["read"], &["src/lib"], &["api"]),
        vec![CapabilityKind::Filesystem],
        false,
        false,
        false,
        4,
        500,
    )
    .unwrap();
    let derived = parent.intersect(&child).unwrap();
    assert_eq!(derived.schema, GRANT_SCOPE_SCHEMA);
    assert_eq!(derived.capabilities, vec![CapabilityKind::Filesystem]);
    assert!(!derived.allow_secret);
    assert!(!derived.allow_external);
    assert!(parent.contains(&derived).unwrap());
    assert!(!derived.contains(&parent).unwrap());
    assert!(GrantScope::intersect_all(&[parent, child]).is_ok());
}

#[test]
fn grant_scope_rejects_empty_capability_intersection_cross_scope_and_mixed_dimensions() {
    let parent = parent_grant();
    let process_only = GrantScope::new(
        GrantId::new(),
        None,
        parent.principal_id,
        parent.project_id,
        scope(&["read"], &[], &["api"]),
        vec![CapabilityKind::Process],
        false,
        false,
        false,
        4,
        500,
    )
    .unwrap();
    assert_eq!(
        GrantScope::new(
            GrantId::new(),
            None,
            parent.principal_id,
            parent.project_id,
            scope(&["read"], &[], &[]),
            vec![CapabilityKind::Filesystem],
            true,
            false,
            false,
            4,
            500,
        )
        .unwrap_err(),
        "grant_scope_secret_dimension_mismatch"
    );
    assert_eq!(
        parent.intersect(&process_only).unwrap_err(),
        "grant_scope_capability_intersection_empty"
    );

    let foreign = GrantScope::new(
        GrantId::new(),
        None,
        PrincipalId::new(),
        parent.project_id,
        scope(&["read"], &[], &[]),
        vec![CapabilityKind::Filesystem],
        false,
        false,
        false,
        4,
        500,
    )
    .unwrap();
    assert_eq!(
        parent.intersect(&foreign).unwrap_err(),
        "grant_scope_principal_mismatch"
    );

    let mut foreign_project = parent_grant();
    foreign_project.project_id = ProjectId::new();
    foreign_project.grant_digest = foreign_project.digest();
    assert_eq!(
        parent.intersect(&foreign_project).unwrap_err(),
        "grant_scope_project_mismatch"
    );

    let mut foreign_epoch = parent_grant();
    foreign_epoch.authority_epoch = 5;
    foreign_epoch.grant_digest = foreign_epoch.digest();
    assert_eq!(
        parent.intersect(&foreign_epoch).unwrap_err(),
        "grant_scope_authority_epoch_mismatch"
    );

    assert_eq!(
        GrantScope::new(
            GrantId::new(),
            None,
            parent.principal_id,
            parent.project_id,
            scope(&["read"], &[], &[]),
            vec![CapabilityKind::Filesystem],
            false,
            true,
            false,
            4,
            500,
        )
        .unwrap_err(),
        "grant_scope_external_dimension_mismatch"
    );
}

#[test]
fn grant_scope_allows_only_explicit_capability_scope_and_expiry() {
    let mut parent = parent_grant();
    parent.allow_external = false;
    parent.grant_digest = parent.digest();
    let mut request = CapabilityRequest::new(
        RequestId::new(),
        CapabilityKind::Filesystem,
        "read",
        json!({"path":"src/lib.rs"}),
    )
    .with_risk(RiskLevel::ReadOnly);
    assert!(parent.allows_request(&request, 500).unwrap());
    request.arguments["path"] = json!("tests/outside.rs");
    assert!(!parent.allows_request(&request, 500).unwrap());
    request.arguments["path"] = json!("src/lib.rs");
    request.risk = RiskLevel::ExternalSideEffect;
    assert!(!parent.allows_request(&request, 500).unwrap());
    assert!(!parent.allows_request(&request, 1_000).unwrap());

    request.risk = RiskLevel::ReadOnly;
    request.arguments["path"] = json!({"nested":"src/lib.rs"});
    assert!(!parent.allows_request(&request, 500).unwrap());
}

#[test]
fn grant_scope_intersection_preserves_expiry_and_delegation_narrowing() {
    let mut parent = parent_grant();
    parent.delegation_allowed = false;
    parent.grant_digest = parent.digest();
    let child = GrantScope::new(
        GrantId::new(),
        Some(parent.grant_id),
        parent.principal_id,
        parent.project_id,
        scope(&["read"], &[], &[]),
        vec![CapabilityKind::Filesystem],
        false,
        false,
        true,
        4,
        2_000,
    )
    .unwrap();
    let derived = parent.intersect(&child).unwrap();
    assert!(!derived.delegation_allowed);
    assert_eq!(derived.expires_at_unix_ms, parent.expires_at_unix_ms);
    assert!(parent.contains(&derived).unwrap());
    assert!(!parent.contains(&child).unwrap());

    let mut forged_order = parent_grant();
    forged_order.capabilities.reverse();
    forged_order.grant_digest = forged_order.digest();
    assert_eq!(
        forged_order.validate().unwrap_err(),
        "grant_scope_capabilities_noncanonical"
    );
}

#[test]
fn grant_scope_intersect_all_validates_a_single_layer() {
    let mut tampered = parent_grant();
    tampered.grant_digest =
        "sha256:0000000000000000000000000000000000000000000000000000000000000000".to_owned();

    assert_eq!(
        GrantScope::intersect_all(std::slice::from_ref(&tampered)).unwrap_err(),
        "grant_scope_digest_mismatch"
    );
}

#[test]
fn historical_capability_grant_adapts_to_narrow_scope() {
    let grant = CapabilityGrant {
        schema: kiana_domain::CAPABILITY_GRANT_SCHEMA.to_owned(),
        grant_id: CapabilityGrantId::new(),
        capability: CapabilityKind::Filesystem,
        operation: "read".to_owned(),
        resources: vec!["workspace".to_owned()],
        paths: vec!["src".to_owned()],
        expires_at_unix_ms: 100,
        approval_id: None,
        delegation_allowed: false,
    };
    let scope =
        GrantScope::from_capability_grant(&grant, PrincipalId::new(), ProjectId::new(), 1).unwrap();
    assert_eq!(
        scope.scope.operations,
        ScopeDimension::Restricted(vec!["read".to_owned()])
    );
    assert!(!scope.allow_secret);
    assert!(!scope.allow_external);
}

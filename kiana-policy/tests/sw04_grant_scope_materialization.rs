use kiana_domain::{
    CapabilityGrant, CapabilityGrantId, CapabilityKind, GrantId, PrincipalId, ProjectId,
    ScopeDimension, ScopeLimit, ScopeSet, CAPABILITY_GRANT_SCHEMA,
};
use kiana_policy::GrantScope;

fn scope(paths: &[&str]) -> ScopeSet {
    ScopeSet::new(
        ScopeDimension::Restricted(vec!["builder.packet".to_owned()]),
        if paths.is_empty() {
            ScopeDimension::NotApplicable
        } else {
            ScopeDimension::Restricted(paths.iter().map(|path| (*path).to_owned()).collect())
        },
        ScopeDimension::NotApplicable,
        ScopeDimension::NotApplicable,
        ScopeLimit::NotApplicable,
        ScopeLimit::NotApplicable,
    )
    .unwrap()
}

#[test]
fn scope_materialization_keeps_partition_and_cannot_become_secret() {
    let principal = PrincipalId::new();
    let project = ProjectId::new();
    let parent = GrantScope::new(
        GrantId::new(),
        None,
        principal,
        project,
        scope(&["src"]),
        vec![CapabilityKind::Other("coding".to_owned())],
        false,
        false,
        true,
        9,
        10_000,
    )
    .unwrap();
    let packet = GrantScope::new(
        GrantId::new(),
        Some(parent.grant_id),
        principal,
        project,
        scope(&["src/lib.rs"]),
        vec![CapabilityKind::Other("coding".to_owned())],
        false,
        false,
        false,
        9,
        5_000,
    )
    .unwrap();
    let derived = parent.intersect(&packet).unwrap();
    let grant = derived
        .to_capability_grant(
            CapabilityKind::Other("coding".to_owned()),
            "builder.packet",
            vec!["workspace".to_owned()],
            None,
        )
        .unwrap();
    assert_eq!(grant.paths, ["src/lib.rs"]);
    assert!(!grant.delegation_allowed);
    assert!(GrantScope::intersect_all(&[parent, packet]).is_ok());

    let secret = CapabilityGrant {
        schema: CAPABILITY_GRANT_SCHEMA.to_owned(),
        grant_id: CapabilityGrantId::new(),
        capability: CapabilityKind::Secret,
        operation: "provider.use".to_owned(),
        resources: vec!["credential".to_owned()],
        paths: vec![".".to_owned()],
        expires_at_unix_ms: 5_000,
        approval_id: None,
        delegation_allowed: true,
    };
    let secret_scope = GrantScope::from_capability_grant(&secret, principal, project, 9).unwrap();
    assert!(secret_scope
        .to_capability_grant(CapabilityKind::Secret, "provider.use", vec![], None)
        .is_ok());
    assert!(derived
        .to_capability_grant(CapabilityKind::Secret, "provider.use", vec![], None)
        .is_err());
}

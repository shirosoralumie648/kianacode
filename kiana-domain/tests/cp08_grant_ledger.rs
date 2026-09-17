use kiana_domain::{
    AuthenticatedPrincipalRef, GrantAuthorityEnvelope, GrantAuthorityStatus, GrantId, GrantLedger,
    ProjectId, ScopeDimension, ScopeLimit, ScopeSet,
};

fn scope(operations: &[&str], paths: &[&str]) -> ScopeSet {
    ScopeSet::new(
        ScopeDimension::Restricted(operations.iter().map(|value| (*value).to_owned()).collect()),
        ScopeDimension::Restricted(paths.iter().map(|value| (*value).to_owned()).collect()),
        ScopeDimension::NotApplicable,
        ScopeDimension::NotApplicable,
        ScopeLimit::Restricted(10),
        ScopeLimit::Restricted(2),
    )
    .unwrap()
}

fn root() -> GrantAuthorityEnvelope {
    let principal = AuthenticatedPrincipalRef::local();
    GrantAuthorityEnvelope::new(
        GrantId::new(),
        None,
        principal.clone(),
        ProjectId::new(),
        principal,
        true,
        scope(&["read", "write"], &["src"]),
        1,
        1,
        1_000,
    )
    .unwrap()
}

#[test]
fn root_and_child_grants_are_strictly_bounded_and_snapshot_replays() {
    let root = root();
    let child = GrantAuthorityEnvelope::new(
        GrantId::new(),
        Some(root.grant_id),
        root.principal.clone(),
        root.project_id,
        root.issuer.clone(),
        false,
        scope(&["read"], &["src/lib"]),
        1,
        1,
        500,
    )
    .unwrap();
    assert!(root.contains(&child).unwrap());
    let mut ledger = GrantLedger::new();
    ledger.bind(root.clone(), 1).unwrap();
    ledger.bind(child.clone(), 2).unwrap();
    assert!(ledger.active_grant(child.grant_id, 100, 1).unwrap());
    let snapshot = ledger.snapshot().unwrap();
    assert!(snapshot.validate().is_ok());
    let restored = GrantLedger::restore(snapshot).unwrap();
    assert_eq!(restored.source_sequence(), 2);
    assert!(restored.active_grant(child.grant_id, 100, 1).unwrap());
}

#[test]
fn grant_ledger_rejects_widening_duplicate_gap_and_foreign_parent() {
    let root = root();
    let mut ledger = GrantLedger::new();
    ledger.bind(root.clone(), 1).unwrap();
    assert_eq!(
        ledger.bind(root.clone(), 2).unwrap_err(),
        "grant_ledger_duplicate_id"
    );

    let foreign_child = GrantAuthorityEnvelope::new(
        GrantId::new(),
        Some(root.grant_id),
        AuthenticatedPrincipalRef::local(),
        root.project_id,
        root.issuer.clone(),
        false,
        scope(&["read"], &["src"]),
        1,
        1,
        500,
    )
    .unwrap();
    assert_eq!(
        ledger.bind(foreign_child, 3).unwrap_err(),
        "grant_ledger_sequence_gap_or_regression"
    );

    let widening = GrantAuthorityEnvelope::new(
        GrantId::new(),
        Some(root.grant_id),
        root.principal.clone(),
        root.project_id,
        root.issuer.clone(),
        false,
        scope(&["read", "write"], &["src", "tests"]),
        1,
        1,
        500,
    )
    .unwrap();
    assert_eq!(
        ledger.bind(widening, 2).unwrap_err(),
        "grant_ledger_child_scope_widened"
    );
}

#[test]
fn revoking_an_ancestor_fences_descendants_and_preserves_unknown_state() {
    let root = root();
    let child = GrantAuthorityEnvelope::new(
        GrantId::new(),
        Some(root.grant_id),
        root.principal.clone(),
        root.project_id,
        root.issuer.clone(),
        false,
        scope(&["read"], &["src/lib"]),
        1,
        1,
        500,
    )
    .unwrap();
    let mut ledger = GrantLedger::new();
    ledger.bind(root.clone(), 1).unwrap();
    ledger.bind(child.clone(), 2).unwrap();
    ledger.revoke(root.grant_id, 2, 3).unwrap();
    assert!(!ledger.active_grant(child.grant_id, 100, 2).unwrap());
    let snapshot = ledger.snapshot().unwrap();
    assert!(snapshot
        .grants
        .iter()
        .any(|grant| grant.status == GrantAuthorityStatus::Revoked));
    assert!(GrantLedger::restore(snapshot).is_ok());
}

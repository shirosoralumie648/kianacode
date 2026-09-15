use kiana_domain::{
    ScopeDimension, ScopeLimit, ScopeSet, SCOPE_SET_SCHEMA, SCOPE_SET_SCHEMA_VERSION,
};

fn parent_scope() -> ScopeSet {
    ScopeSet::new(
        ScopeDimension::Restricted(vec!["apply_patch".to_owned(), "shell.exec".to_owned()]),
        ScopeDimension::Restricted(vec!["src".to_owned()]),
        ScopeDimension::Restricted(vec!["project".to_owned()]),
        ScopeDimension::Restricted(vec!["mcp.local".to_owned()]),
        ScopeLimit::Restricted(100),
        ScopeLimit::Restricted(3),
    )
    .unwrap()
}

#[test]
fn scope_intersection_is_monotonic_and_path_aware() {
    let parent = parent_scope();
    let child = ScopeSet::new(
        ScopeDimension::Restricted(vec!["shell.exec".to_owned()]),
        ScopeDimension::Restricted(vec!["src/lib".to_owned()]),
        ScopeDimension::Restricted(vec!["project".to_owned()]),
        ScopeDimension::Restricted(vec!["mcp.local".to_owned()]),
        ScopeLimit::Restricted(50),
        ScopeLimit::Restricted(2),
    )
    .unwrap();
    assert!(child.is_subset_of(&parent).unwrap());
    let intersection = parent.intersect(&child).unwrap();
    assert!(intersection.is_subset_of(&parent).unwrap());
    assert!(intersection.is_subset_of(&child).unwrap());
    assert!(intersection.allows_operation("shell.exec"));
    assert!(intersection.allows_path("src/lib/main.rs"));
    assert!(!intersection.allows_operation("mcp.call"));
}

#[test]
fn not_applicable_does_not_mean_empty_restricted_scope() {
    let unrestricted = ScopeSet::unrestricted();
    let parent = parent_scope();
    assert_eq!(unrestricted.intersect(&parent).unwrap(), parent);
    let empty = ScopeSet::new(
        ScopeDimension::Restricted(Vec::new()),
        ScopeDimension::NotApplicable,
        ScopeDimension::NotApplicable,
        ScopeDimension::NotApplicable,
        ScopeLimit::NotApplicable,
        ScopeLimit::NotApplicable,
    )
    .unwrap();
    assert_eq!(
        parent.intersect(&empty).unwrap_err(),
        "scope_intersection_empty"
    );
}

#[test]
fn scope_digest_version_and_unknown_fields_fail_closed() {
    let scope = parent_scope();
    assert_eq!(scope.schema, SCOPE_SET_SCHEMA);
    assert_eq!(scope.version, SCOPE_SET_SCHEMA_VERSION);
    scope.validate().unwrap();
    let mut unknown = serde_json::to_value(&scope).unwrap();
    unknown["unexpected"] = serde_json::json!(true);
    assert!(serde_json::from_value::<ScopeSet>(unknown).is_err());
    let mut forged = scope.clone();
    forged.budget = ScopeLimit::Restricted(1_000);
    assert_eq!(forged.validate().unwrap_err(), "scope_set_digest_mismatch");
}

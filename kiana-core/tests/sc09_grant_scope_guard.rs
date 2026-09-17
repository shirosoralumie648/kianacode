#[test]
fn grant_scope_reuses_scope_set_and_cannot_create_a_broker_or_union_path() {
    let scope = include_str!("../../kiana-policy/src/grant_scope.rs");
    let policy = include_str!("../../kiana-policy/src/lib.rs");
    let domain = include_str!("../../kiana-domain/src/scope.rs");
    for marker in [
        "GrantScope",
        "intersect_all",
        "capability_intersection_empty",
        "grant_scope_principal_mismatch",
        "allow_secret",
        "allow_external",
        "ScopeSet::intersect",
        "is_subset_of",
    ] {
        assert!(
            scope.contains(marker),
            "grant scope marker missing: {marker}"
        );
    }
    assert!(policy.contains("pub use grant_scope::*;"));
    assert!(domain.contains("pub fn intersect_all"));
    for forbidden in [
        "CapabilityBroker",
        "DaemonHost",
        "authorize_and_execute",
        "union",
    ] {
        assert!(
            !scope.contains(forbidden),
            "grant scope must not {forbidden}"
        );
    }
}

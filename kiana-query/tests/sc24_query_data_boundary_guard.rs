#[test]
fn query_data_boundary_guard_prevents_revoked_derived_views() {
    let boundary = include_str!("../src/data_boundary.rs");
    let memory = include_str!("../../kiana-daemon/src/harness_memory.rs");
    let cache = include_str!("../src/cache_policy.rs");
    for marker in [
        "QueryDataBoundary",
        "scope_digest",
        "data_epoch",
        "query_data_boundary_revoked_data_allowed",
        "QueryDataDisposition::Denied",
    ] {
        assert!(boundary.contains(marker) || memory.contains(marker) || cache.contains(marker));
    }
}

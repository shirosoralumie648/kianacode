#[test]
fn daemon_cache_path_does_not_treat_cache_failure_as_empty_facts() {
    let query = include_str!("../../kiana-query/src/cache_policy.rs");
    let index = include_str!("../../kiana-query/src/index.rs");
    let daemon = include_str!("../src/context_query.rs");
    assert!(query.contains("business_result_from_cache"));
    assert!(index.contains("read_cached_index"));
    assert!(daemon.contains("ContextIndex") || daemon.contains("context index"));
}

#[test]
fn pd21_cache_guard_keeps_cache_outside_business_authority() {
    let policy = include_str!("../src/cache_policy.rs");
    let index = include_str!("../src/index.rs");
    for marker in [
        "ContextCacheStatus",
        "business_result_from_cache",
        "cache_invalid_rebuilt",
        "cache_source_changed",
        "decision:",
    ] {
        assert!(
            policy.contains(marker) || index.contains(marker),
            "missing marker: {marker}"
        );
    }
    assert!(!policy.contains("CapabilityBroker"));
    assert!(!policy.contains("ProviderClient"));
}

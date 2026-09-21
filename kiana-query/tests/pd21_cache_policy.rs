use kiana_query::{ContextCacheDecision, ContextCacheStatus};

#[test]
fn cache_miss_stale_and_degraded_never_supply_business_results() {
    let miss = ContextCacheDecision::from_report("created", 0, 3, 0, 0);
    assert_eq!(miss.status, ContextCacheStatus::Miss);
    assert!(!miss.business_result_from_cache);
    miss.validate().unwrap();

    let stale = ContextCacheDecision::from_report("updated", 2, 0, 1, 0);
    assert_eq!(stale.status, ContextCacheStatus::Stale);
    assert!(!stale.business_result_from_cache);
    stale.validate().unwrap();

    let degraded = ContextCacheDecision::from_report("recovered", 0, 3, 0, 0);
    assert_eq!(degraded.status, ContextCacheStatus::Degraded);
    assert!(!degraded.business_result_from_cache);
    degraded.validate().unwrap();
}

#[test]
fn only_unchanged_reuse_is_a_cache_hit() {
    let hit = ContextCacheDecision::from_report("updated", 3, 0, 0, 0);
    assert_eq!(hit.status, ContextCacheStatus::Hit);
    assert!(hit.cache_read_used);
    assert!(hit.business_result_from_cache);
    hit.validate().unwrap();
}

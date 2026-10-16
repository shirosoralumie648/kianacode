//! PD-21 缓存报告映射的行为 fixture。
//!
//! 这些用例直接给策略函数传入合成状态和计数，不读取真实索引缓存、不扫描项目，也不调用
//! 治理传播构造器。它们验证几个代表性输入会映射到预期 DTO，不证明生产端实际复用了旧结果。

use kiana_query::{ContextCacheDecision, ContextCacheStatus};

/// 新缓存、内容已变化的缓存和损坏后重建都不能声明业务结果来自旧缓存。
#[test]
fn cache_miss_stale_and_degraded_never_supply_business_results() {
    // `created` 表示此前没有缓存；当前结果来自新构建。
    let miss = ContextCacheDecision::from_report("created", 0, 3, 0, 0);
    assert_eq!(miss.status, ContextCacheStatus::Miss);
    assert!(!miss.business_result_from_cache);
    miss.validate().unwrap();

    // 至少有文件变更时，即使另外有文件摘要相同，也应映射为 Stale。
    let stale = ContextCacheDecision::from_report("updated", 2, 0, 1, 0);
    assert_eq!(stale.status, ContextCacheStatus::Stale);
    assert!(!stale.business_result_from_cache);
    stale.validate().unwrap();

    // `recovered` 表示旧缓存无法读取后生成替代报告；状态降级但不宣称复用旧结果。
    let degraded = ContextCacheDecision::from_report("recovered", 0, 3, 0, 0);
    assert_eq!(degraded.status, ContextCacheStatus::Degraded);
    assert!(!degraded.business_result_from_cache);
    degraded.validate().unwrap();
}

/// 仅当更新报告有未变化文件且没有增删改时，当前映射才产生 Hit。
///
/// 这里验证的是传入计数到 DTO 的映射；由于 fixture 不执行 `index.rs` 的持久构建顺序，
/// 不代表那些文件内容实际从旧缓存读出。
#[test]
fn only_unchanged_reuse_is_a_cache_hit() {
    let hit = ContextCacheDecision::from_report("updated", 3, 0, 0, 0);
    assert_eq!(hit.status, ContextCacheStatus::Hit);
    assert!(hit.cache_read_used);
    assert!(hit.business_result_from_cache);
    hit.validate().unwrap();
}

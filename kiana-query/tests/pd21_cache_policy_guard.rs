//! PD-21 缓存状态 DTO 的静态源码边界守卫。
//!
//! `include_str!` 将策略与索引模块当作文本，检查关键状态和字段仍存在，并检查策略源码
//! 未出现列出的执行层耦合词；它不构造缓存，也不执行真实请求/授权路径。

#[test]
fn pd21_cache_guard_keeps_cache_outside_business_authority() {
    let policy = include_str!("../src/cache_policy.rs");
    let index = include_str!("../src/index.rs");
    // 这些 marker 证明所需字段/原因文本仍出现在两个源码文件之一，不验证运行时调用顺序。
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
    // 这是针对单个源文件的词面约束，不等于整个 workspace 的依赖图证明。
    assert!(!policy.contains("CapabilityBroker"));
    assert!(!policy.contains("ProviderClient"));
}

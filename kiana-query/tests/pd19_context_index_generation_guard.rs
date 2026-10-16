//! PD-19 Query source-level manifest/generation 边界守卫。
//!
//! 本用例用 `include_str!` 搜索 source fingerprint、Ready 发布和 build 调用标记，
//! 并检查已知 Provider/Capability 字符串缺失；它不会实际扫描/发布索引，也不能证明所有入口都使用该 envelope。

/// 确认源码保留 generation-bound publication 的主要边界标记。
#[test]
fn pd19_source_guard_keeps_index_reads_generation_bound() {
    let generation = include_str!("../src/index_generation.rs");
    let index = include_str!("../src/index.rs");
    for marker in [
        "ContextIndexSourceManifest",
        "source_fingerprint",
        "Freshness::Stale",
        "IndexGenerationState",
        "write_context_index_generation_atomic",
        "build_context_index_generation",
    ] {
        assert!(
            generation.contains(marker) || index.contains(marker),
            "missing marker: {marker}"
        );
    }
    assert!(index.contains("build_context_index"));
    assert!(!generation.contains("ProviderClient"));
    assert!(!generation.contains("CapabilityBroker"));
}

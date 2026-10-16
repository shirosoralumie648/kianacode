//! P1-L4-01 代码智能的源码标记守卫。
//!
//! `include_str!` 在编译时把 RepoMap、索引、Daemon/Core 查询源码和 baseline 读成文本，断言
//! 某些实现词汇仍在且特定调用端没有出现越界执行组件名称。它不运行 scanner、索引或 HTTP 路由，
//! 因此只能证明源码标记，不证明结果正确、freshness 有效或生产入口已接线。

#[test]
/// 检查上下文结果所需的 snapshot/source/freshness 标记和源码边界字符串。
fn code_intelligence_results_carry_snapshot_source_and_freshness() {
    let repo_map = include_str!("../src/repo_map.rs");
    let index = include_str!("../src/index.rs");
    let daemon = include_str!("../../kiana-daemon/src/context_query.rs");
    let core = include_str!("../../kiana-core/src/context_query.rs");
    let baseline = include_str!("../../docs/roadmap/p1-l4-01-code-intelligence-baseline.md");
    for marker in [
        "RepoMap",
        "content_hash",
        "build_repo_map",
        "canonicalize",
        "ContextIndex",
        "ContextSearchResults",
        "ContextVectorSearchResults",
        "ContextPack",
        "ContextArtifactDependencyGraph",
        "stable_hash",
        "vector_search",
        "source_snapshot",
        "local_workspace",
        "freshness",
        "captured_at_read",
        "runtime_version",
        "collect_provenance_sources",
        "artifact_graph",
        "line_number",
        "start_line",
        "end_line",
        "snapshot",
    ] {
        assert!(
            repo_map.contains(marker)
                || index.contains(marker)
                || daemon.contains(marker)
                || core.contains(marker)
                || baseline.contains(marker),
            "code intelligence marker missing: {marker}"
        );
    }
    assert!(repo_map.contains("sha256"));
    assert!(index.contains("content_hash: stable_hash"));
    assert!(daemon.contains("freshness\":\"captured_at_read\""));
    assert!(daemon.contains("source\":\"local_workspace\""));
    assert!(daemon.contains("collect_provenance_sources"));
    assert!(index.contains("canonical_root"));
    assert!(index.contains("canonical_root"));
    assert!(!index.contains("CapabilityBroker"));
    assert!(!index.contains("ProviderClient"));
    assert!(!daemon.contains("ModelClient"));
}

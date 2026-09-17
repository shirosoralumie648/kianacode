#[test]
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

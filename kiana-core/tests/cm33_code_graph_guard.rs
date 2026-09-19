#[test]
fn code_graph_is_source_scoped_temporal_and_rebuildable() {
    let domain = include_str!("../../kiana-domain/src/code_graph.rs");
    let repo_map = include_str!("../../kiana-query/src/repo_map.rs");
    let dependencies = include_str!("../../kiana-domain/src/source_dependencies.rs");

    for marker in [
        "CodeGraphEdge",
        "CODE_GRAPH_EDGE_SCHEMA",
        "CodeGraphRebuildPlan",
        "CODE_GRAPH_REBUILD_SCHEMA",
        "SourceRef",
        "scope_digest",
        "valid_from_ms",
        "valid_to_ms",
        "invalidated_at_ms",
        "CodeGraphTemporalState",
        "affected_edge_ids",
        "retained_edge_ids",
        "affected_edge_ids",
        "data_epoch",
        "SourceKind::WorkspaceFile",
        "source_id",
        "RepoMap",
        "SourceDependencyGraph",
    ] {
        assert!(
            domain.contains(marker) || repo_map.contains(marker) || dependencies.contains(marker),
            "CM-33 source marker missing: {marker}"
        );
    }
    assert!(domain.contains("edge.source.source_id == source_id"));
    assert!(!domain.contains("CapabilityBroker"));
    assert!(!domain.contains("ModelClient"));
}

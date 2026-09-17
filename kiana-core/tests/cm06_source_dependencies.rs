#[test]
fn revoking_source_invalidates_all_derived_context() {
    let domain = include_str!("../../kiana-domain/src/source_dependencies.rs");
    let governance = include_str!("../src/data_governance.rs");
    let memory = include_str!("../src/memory_proposals.rs");
    let fixture = include_str!("../../kiana-domain/tests/cm06_source_dependencies.rs");
    for marker in [
        "SourceDependencyGraph",
        "DependencyNodeKind",
        "DependencyEdge",
        "SourceInvalidation",
        "SOURCE_DEPENDENCY_GRAPH_SCHEMA",
        "SOURCE_INVALIDATION_SCHEMA",
        "affected_by_source",
        "revoke_source",
        "data_epoch",
        "revoked_sources",
        "memory",
        "evidence",
        "event",
        "artifact",
        "file",
        "context",
        "index",
        "summary",
        "plan",
        "source_dependency_epoch_rollback",
        "source_dependency_edge_duplicate",
        "revoking_source_invalidates_all_derived_context",
    ] {
        assert!(
            domain.contains(marker)
                || governance.contains(marker)
                || memory.contains(marker)
                || fixture.contains(marker),
            "dependency marker missing: {marker}"
        );
    }
    assert!(domain.contains("let mut reverse"));
    assert!(domain.contains("VecDeque"));
    assert!(domain.contains("self.data_epoch = next_epoch"));
    assert!(governance.contains("data_epoch"));
    assert!(governance.contains("revoked_sources"));
    assert!(!domain.contains("authorize_and_execute"));
}

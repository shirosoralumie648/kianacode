#[test]
fn invalidation_binds_identity_content_cache_inputs_and_tombstones() {
    let domain = include_str!("../../kiana-domain/src/index_invalidation.rs");
    let query = include_str!("../../kiana-query/src/index_invalidation.rs");
    for marker in [
        "WorkspaceChangeKind",
        "Renamed",
        "Removed",
        "tombstones",
        "IndexCacheKey",
        "worktree_digest",
        "dirty_manifest_digest",
        "parser_digest",
        "chunker_digest",
        "config_digest",
        "effective_digest",
        "source_generation",
        "target_generation",
        "modified_unix_ms",
    ] {
        assert!(
            domain.contains(marker) || query.contains(marker),
            "CM-11 marker missing: {marker}"
        );
    }
    assert!(domain.contains("content_digest"));
    assert!(!query.contains("ModelClient"));
    assert!(!query.contains("CapabilityBroker"));
}

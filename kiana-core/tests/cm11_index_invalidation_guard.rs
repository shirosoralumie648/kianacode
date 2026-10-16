//! CM-11 Core 静态源码守卫，用来固定 snapshot identity、cache-key 与 tombstone 词汇边界。
//!
//! `include_str!` 只验证相关字段/状态标记仍出现在 Domain、Query 和 WorkspaceSnapshot 源码里，
//! 不执行 snapshot diff、cache eviction 或真实 index rebuild。

/// 检查 invalidation 计划保持 identity/content/cache/tombstone 所需的源级标记。
#[test]
fn invalidation_binds_identity_content_cache_inputs_and_tombstones() {
    let domain = include_str!("../../kiana-domain/src/index_invalidation.rs");
    let query = include_str!("../../kiana-query/src/index_invalidation.rs");
    let workspace_snapshot = include_str!("../../kiana-domain/src/workspace_snapshot.rs");
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
            domain.contains(marker)
                || query.contains(marker)
                || workspace_snapshot.contains(marker),
            "CM-11 marker missing: {marker}"
        );
    }
    assert!(domain.contains("content_digest"));
    assert!(!query.contains("ModelClient"));
    assert!(!query.contains("CapabilityBroker"));
}

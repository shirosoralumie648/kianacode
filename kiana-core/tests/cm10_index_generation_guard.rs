//! CM-10 Core 静态源码边界守卫。
//!
//! `include_str!` 把 Domain 和 Query 源码嵌入为字符串，检查单 manifest、Ready reader、原子发布标记和已知禁用依赖。
//! 它不运行文件发布流程，也不证明跨进程一致性、掉电 durability 或运行时没有其他路径。

/// 检查 generation 状态机与 temp-write/sync/rename 发布边界仍出现在预期源码中。
#[test]
fn index_generation_has_one_manifest_and_atomic_publication_boundary() {
    let domain = include_str!("../../kiana-domain/src/index_generation.rs");
    let query = include_str!("../../kiana-query/src/index_generation.rs");
    for marker in [
        "IndexManifest",
        "IndexGenerationState",
        "IndexComponentKind",
        "source_manifest_digest",
        "IndexGenerationStatus::Ready",
        "reader",
        "index_generation_commit_fence_mismatch",
        "last_failure",
    ] {
        assert!(
            domain.contains(marker) || query.contains(marker),
            "CM-10 marker missing: {marker}"
        );
    }
    for marker in ["create_new", "sync_all", "rename", "read_index_manifest"] {
        assert!(
            query.contains(marker),
            "atomic publication marker missing: {marker}"
        );
    }
    assert!(!query.contains("ModelClient"));
    assert!(!query.contains("CapabilityBroker"));
}

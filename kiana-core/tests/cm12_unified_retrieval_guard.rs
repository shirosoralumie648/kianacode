//! CM-12 的源码边界守卫：检查统一算法、Query adapter 与既有兼容入口仍留在预期位置。
//!
//! `include_str!` 把文件源码作为编译期文本读入，下面的 `contains()` / `!contains()` 只检查字面标记；
//! 它不会执行排序、进入真实权限服务、调用索引或验证排名质量。守卫成功最多说明这些源码锚点存在，
//! 不是产品主路径接线或 live 检索证明。

#[test]
/// 确认 Domain 暴露统一评分合同，旧 Daemon/CLI 路径和 Query adapter 的边界仍可被源码定位。
fn unified_retrieval_keeps_acl_first_and_one_algorithm_profile() {
    let domain = include_str!("../../kiana-domain/src/unified_retrieval.rs");
    let query = include_str!("../../kiana-query/src/unified_retrieval.rs");
    let daemon = include_str!("../../kiana-daemon/src/memory_retrieval.rs");
    let commands = include_str!("../../kiana-commands/src/memory.rs");
    // 这些标记覆盖 DTO、ACL/scope 输入及 sparse/dense/RRF/MMR 解释字段。
    for marker in [
        "RetrievalProfile",
        "RetrievalRequest",
        "RetrievalItem",
        "acl_filtered",
        "permission_scope_digest",
        "bm25_k1",
        "dense_vector",
        "rrf_k",
        "mmr_lambda",
        "acl_filtered_count",
        "exact_score",
        "sparse_rank",
        "dense_rank",
    ] {
        assert!(
            domain.contains(marker) || query.contains(marker),
            "CM-12 marker missing: {marker}"
        );
    }
    // 旧入口 marker 只说明兼容实现仍有对应函数；不能据此认定它调用了新 Domain 算法。
    assert!(daemon.contains("rank_records"));
    assert!(commands.contains("search_memory_records"));
    // Domain 排名器保持纯计算边界；这些负向检查只扫描实现源码文本。
    assert!(!domain.contains("ModelClient"));
    assert!(!query.contains("CapabilityBroker"));
}

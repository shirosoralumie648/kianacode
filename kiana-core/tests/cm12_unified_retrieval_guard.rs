//! CM-12 的源码边界守卫：检查统一算法、Query adapter 与两个产品适配器的委托边界仍留在预期位置。
//!
//! `include_str!` 把文件源码作为编译期文本读入，下面的 `contains()` / `!contains()` 只检查字面标记；
//! 它不会执行排序、进入真实权限服务、调用索引或验证排名质量。守卫成功最多说明这些源码锚点存在，
//! 不是产品主路径接线或 live 检索证明。

#[test]
/// 确认 Domain 暴露统一评分合同，Daemon/CLI 适配器委托同一实现，且没有第二套本地公式。
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
    // CM-12：两个产品适配器必须委托统一算法——daemon 经 `UnifiedRetrievalPort`（Domain `rank_retrieval()`
    // 的唯一 `RetrievalPort` 实现），CLI 直接调用 Domain `rank_retrieval()`；两者都读 `RetrievalProfile::unified()`。
    assert!(daemon.contains("UnifiedRetrievalPort"));
    assert!(daemon.contains("rank_records"));
    assert!(daemon.contains("RetrievalProfile::unified"));
    assert!(commands.contains("search_memory_records"));
    assert!(commands.contains("rank_retrieval"));
    assert!(commands.contains("RetrievalProfile::unified"));
    // 反向锚点：适配器不得再内联第二套 RRF/MMR 公式（CM-12 的整改对象）。
    for source in [daemon, commands] {
        assert!(!source.contains("1.0 / (61.0 + rank as f64)"));
        assert!(!source.contains("0.7 * rrf"));
    }
    // Domain 排名器保持纯计算边界；这些负向检查只扫描实现源码文本。
    assert!(!domain.contains("ModelClient"));
    assert!(!query.contains("CapabilityBroker"));
}

//! CM-14 的源码边界守卫：检查检索结果证据/health 合同与两个产品适配器的输出边界。
//!
//! `include_str!` 把文件源码作为编译期文本读入，下面的 `contains()` / `!contains()` 只检查字面标记；
//! 它不会执行检索、进入真实权限服务、调用索引或验证排名质量。守卫成功最多说明这些源码锚点存在，
//! 不是产品主路径接线或 live 检索证明。

#[test]
fn retrieval_results_keep_generation_provenance_and_safe_health_semantics() {
    let source = include_str!("../../kiana-domain/src/retrieval_evidence.rs");
    let candidate = include_str!("../../kiana-domain/src/retrieval_context.rs");
    for marker in [
        "RetrievalEvidence",
        "source_snapshot_digest",
        "source_revision",
        "generation",
        "rank_components",
        "RetrievalHealthStatus",
        "Denied",
        "Unavailable",
        "retryable",
        "retry_safe",
        "empty_reason",
        "RetrievalResponse",
        "Freshness",
        "EvidenceStatus",
    ] {
        assert!(
            source.contains(marker) || candidate.contains(marker),
            "CM-14 marker missing: {marker}"
        );
    }
    assert!(!source.contains("ModelClient"));
    assert!(!source.contains("CapabilityBroker"));
}

#[test]
/// CM-14：统一算法输出证据/health，daemon memory tool 与 CLI memory search 把它带进产品输出；两个适配器
/// 只做只读派生，不得自称 `Verified`，也不得重新内联第二套排名公式。
fn product_adapters_surface_retrieval_evidence_and_health() {
    let domain = include_str!("../../kiana-domain/src/unified_retrieval.rs");
    let daemon = include_str!("../../kiana-daemon/src/memory_retrieval.rs");
    let harness = include_str!("../../kiana-daemon/src/harness_memory.rs");
    let commands = include_str!("../../kiana-commands/src/memory.rs");
    for marker in [
        "RetrievalItemProvenance",
        "RetrievalHealth",
        "result_health",
        "empty_reason",
        "RetrievalRetryPolicy",
    ] {
        assert!(
            domain.contains(marker),
            "CM-14 domain marker missing: {marker}"
        );
    }
    for marker in [
        "record_provenance",
        "RetrievalSnapshotContext",
        "RetrievalRetryPolicy::Deterministic",
        "retrieval_evidence",
        "retrieval_health",
    ] {
        assert!(
            daemon.contains(marker) || harness.contains(marker),
            "CM-14 daemon marker missing: {marker}"
        );
    }
    for marker in [
        "RetrievalItemProvenance",
        "RETRIEVAL_UNVERSIONED_GENERATION",
        "retrieval_evidence",
        "empty_reason",
    ] {
        assert!(
            commands.contains(marker),
            "CM-14 commands marker missing: {marker}"
        );
    }
    // 反向锚点：适配器只做只读派生——不把来源证据提升为 Verified，也不内联第二套 RRF/MMR 公式。
    for adapter in [daemon, commands] {
        assert!(!adapter.contains("EvidenceStatus::Verified"));
        assert!(!adapter.contains("1.0 / (61.0 + rank as f64)"));
        assert!(!adapter.contains("0.7 * rrf"));
    }
}

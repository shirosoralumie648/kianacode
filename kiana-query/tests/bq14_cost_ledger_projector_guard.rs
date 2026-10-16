//! BQ-14 Query 账本 projector 的静态源码边界守卫。
//!
//! `include_str!` 检查 projector、append correction 适配和 source cursor marker，并确认该文件未直接
//! 依赖若干写入/执行接口。它不执行账本重放，也不扫描整个 workspace 的副作用调用。

#[test]
fn query_projector_is_read_only_and_digest_bound() {
    let source = include_str!("../src/cost_ledger_projector.rs");
    // 这些 marker 仅说明预期文本留在 Query projector 源码中。
    assert!(source.contains("project_cost_ledger"));
    assert!(source.contains("append_correction"));
    assert!(source.contains("source_cursor"));
    assert!(!source.contains("EventStorePort"));
    assert!(!source.contains("CapabilityBrokerPort"));
    assert!(!source.contains("ModelOutput"));
}

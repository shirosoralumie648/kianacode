//! BQ-19 allocation projector 的源码边界静态守卫。
//!
//! `include_str!` 把 Query projector 读作文本，检查多维汇总及只读标记存在，并确认几个执行/写入
//! 词没有出现在该文件。它不运行事件投影，也不证明其他模块没有调用副作用接口。

#[test]
fn allocation_projector_is_read_only_and_does_not_merge_dimensions() {
    let source = include_str!("../src/allocation_projector.rs");
    // 所有 marker 必须保留在同一投影源文件中；这里检查名称存在，不检查每个 view 的数值。
    for marker in [
        "project_cost_allocations",
        "seen_usage_ids",
        "run_totals",
        "project_totals",
        "organization_totals",
        "workflow_totals",
        "cell_totals",
        "ALLOCATION_PROJECTOR_IS_READ_ONLY",
    ] {
        assert!(source.contains(marker), "BQ-19 marker missing: {marker}");
    }
    // 这些字符串只构成单文件的静态负向约束，不能替代 workspace-wide 权限或调用图审计。
    for forbidden in [
        "append_event",
        "EventStorePort",
        "CostAllocationAdmission",
        "fn authorize",
        "fn release",
        "fn reserve",
        "CapabilityBrokerPort",
        "tokio::spawn",
    ] {
        assert!(
            !source.contains(forbidden),
            "projection must not {forbidden}"
        );
    }
}

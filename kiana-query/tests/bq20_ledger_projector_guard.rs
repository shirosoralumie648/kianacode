//! BQ-20 投影器的单文件源码边界检查。
//!
//! 该测试把目标源文件作为普通文本读入，确认关键版本/来源/隔离/时间窗标记仍存在，并拒绝
//! 若干会扩大查询层权限边界的源码标记。它不会构造账单事件、执行 projector、连接事件存储
//! 或证明生产路由已经调用 BQ-20；行为由同目录的独立 fixture 覆盖，真实接线仍需沿调用图核实。

/// 检查 BQ-20 源文件仍声明只读投影所需的关键契约，且没有出现已禁止的写入/授权依赖标记。
///
/// 【输入】编译期通过 `include_str!` 嵌入的 `billing_ledger_projector.rs` 文本。
/// 【输出】任一必要 marker 缺失或禁止字符串出现时，断言会使此静态测试失败。
/// 【边界】这是文本包含检查：它不理解 Rust 语法、不验证运行时调用关系，也不检查其他文件。
#[test]
fn billing_projector_has_one_source_and_read_only_boundary() {
    let source = include_str!("../src/billing_ledger_projector.rs");
    // 这些 marker 用来防止重构时意外删掉 BQ-20 所依赖的来源游标、重放链、坏事实隔离、
    // 时间窗常量或成本账本/分摊事件类型。它们存在不代表所有不变量都已经被此 guard 验证。
    for marker in [
        "BillingProjectionCursor",
        "replay_fence",
        "BillingQuarantineRecord",
        "BILLING_WINDOW_MILLIS",
        "COST_LEDGER_ENTRY_EVENT",
        "COST_ALLOCATION_EVENT",
    ] {
        assert!(source.contains(marker), "BQ-20 marker missing: {marker}");
    }
    // 静态禁止词把投影器限定在读模型职责内：它不能依赖有执行权限的组件，也不能在这里
    // 扩展账单事实、释放预留或引入审批存储。将来若调整边界，必须先更新架构契约和对应证据，
    // 不能只删掉断言让文本检查变绿。
    for forbidden in [
        "EventStorePort",
        "CapabilityBroker",
        "commit_transition",
        "append(",
        "release_reservation",
        "ApprovalStore",
    ] {
        assert!(
            !source.contains(forbidden),
            "query boundary widened: {forbidden}"
        );
    }
}

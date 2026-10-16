//! DEP-15 Daemon adapter 的静态委托边界守卫。
//!
//! 本文件通过 `include_str!` 检查 adapter 源码中的输入/输出/错误映射标记，并排除一组已知 I/O 或执行关键字。
//! 它不实例化 DaemonHost、不调用路由，也不证明未来所有副作用都会包含在禁止词列表中。

/// 确认 daemon adapter 的源码继续把运维诊断请求转交 Core 并映射端口错误。
///
/// 这是源文本守卫而非端到端运行用例；真实事实采集、调用入口和 report 消费都不在该测试覆盖范围内。
#[test]
fn dep15_daemon_route_only_delegates_to_core() {
    let source = include_str!("../src/ops_diagnostics.rs");
    for marker in [
        "OpsDiagnosticsInput",
        "OpsDiagnosticsReport",
        "evaluate_ops_diagnostics",
        "evaluate_ops_mode",
        "PortError::Failed",
    ] {
        assert!(
            source.contains(marker),
            "DEP-15 daemon marker missing: {marker}"
        );
    }
    for forbidden in [
        "std::fs",
        "std::env",
        "std::process",
        "EventStore",
        "CapabilityBroker",
        "KianaHarness",
        "ProviderGateway",
        "tokio::",
        "http::StatusCode",
    ] {
        assert!(
            !source.contains(forbidden),
            "DEP-15 route crossed forbidden boundary: {forbidden}"
        );
    }
}

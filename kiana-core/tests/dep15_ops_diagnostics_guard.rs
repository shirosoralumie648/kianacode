//! DEP-15 Core/Domain 源码边界守卫。
//!
//! `include_str!` 在编译测试时把源码作为普通字符串嵌入；本用例搜索必要合同标记并排除若干副作用关键字。
//! 它不是运行时调用测试，不能证明 Core/DaemonHost 路由已被 CLI/UI 调用，也不能穷尽所有可能的间接副作用。

#[path = "support/source_guard.rs"]
mod source_guard;

/// 检查诊断源码仍包含关键 DTO/输出模式并维持声明的只读边界标记。
///
/// 标记命中只说明对应文本仍存在；新增副作用若不用这些禁止字符串表达，也可能绕过该守卫。
#[test]
fn dep15_diagnostics_is_redacted_read_only_and_mode_bound() {
    let domain = include_str!("../../kiana-domain/src/ops_diagnostics.rs");
    let core = include_str!("../src/ops_diagnostics.rs");
    for marker in [
        "OpsDiagnosticsInput",
        "OpsDiagnosticFact",
        "OpsDiagnosticsReport",
        "render_json",
        "render_human",
        "reproduction",
        "exit_code",
        "read_only_diagnostics_no_fact_mutation",
        "ops_diagnostics_mode_mismatch",
    ] {
        assert!(
            domain.contains(marker) || core.contains(marker),
            "DEP-15 marker missing: {marker}"
        );
    }
    let domain_code = source_guard::production_source(domain);
    let core_code = source_guard::production_source(core);
    for forbidden in [
        "std::fs",
        "std::env",
        "std::process",
        "EventStore::append",
        "CapabilityBroker",
        "KianaHarness",
        "ProviderGateway",
        "tokio::",
    ] {
        assert!(
            !domain_code.contains(forbidden) && !core_code.contains(forbidden),
            "DEP-15 diagnostics crossed effect boundary: {forbidden}"
        );
    }
}

//! P4-L3-01 的 Core 静态源码守卫。
//!
//! `include_str!` 在编译时把 Core/Domain/Prompt/Provider/baseline 文件作为文本读入；断言检查关键
//! 名称存在、Core 委托 Domain reducer，且源码没有模型/能力执行与自动切换标记。它不会调用
//! `handle_version_command()`，不构造 EventStore，不验证 actor/project 过滤的运行结果，也不证明
//! 事件标签对应真实 Provider 请求。Domain 内存 fixture 位于
//! `kiana-domain/tests/p4_l3_01_versioning.rs`。
#[test]
fn drift_report_is_bucketed_by_version() {
    // 静态守卫覆盖实现、相关类型和本切片范围说明；文本 marker 存在不能代替运行路径验收。
    let core = include_str!("../src/versioning.rs");
    let domain = include_str!("../../kiana-domain/src/versioning.rs");
    let prompts = include_str!("../../kiana-domain/src/prompts.rs");
    let provider = include_str!("../../kiana-services/src/api/provider.rs");
    let baseline = include_str!("../../docs/roadmap/p4-l3-01-versioning-baseline.md");

    // marker 列表让主要归因维度和固定只读语义显式进入源码审查范围。
    for marker in [
        "version.drift",
        "RouteDecision",
        "DriftBucket",
        "DriftReport",
        "ROUTE_DECISION_SCHEMA",
        "DRIFT_REPORT_SCHEMA",
        "ModelProfile",
        "PromptBundle",
        "model_profile",
        "prompt_hash",
        "route_digest",
        "configuration_revision",
        "budget_schema",
        "runtime_version",
        "BTreeMap",
        "version_key",
        "json_digest",
        "run.model_turn",
        "event_ids",
        "automatic_model_switch",
        "observed_turns",
        "cost",
        "drift_report_is_bucketed_by_version",
    ] {
        assert!(
            core.contains(marker)
                || domain.contains(marker)
                || prompts.contains(marker)
                || provider.contains(marker)
                || baseline.contains(marker),
            "versioning marker missing: {marker}"
        );
    }

    // 这些断言要求 Core 使用 Domain 统计合同，而不是自行复制一套版本桶算法。
    assert!(core.contains("kiana_domain::DriftReport::default()"));
    assert!(core.contains("kiana_domain::RouteDecision::from_model_turn"));
    assert!(core.contains("report.record"));
    assert!(core.contains("report.validate"));
    assert!(!core.contains("ModelClient"));
    assert!(!core.contains("CapabilityBroker"));
    assert!(!core.contains("\"automatic_model_switch\":true"));
    assert!(!core.contains("switch_model"));
}

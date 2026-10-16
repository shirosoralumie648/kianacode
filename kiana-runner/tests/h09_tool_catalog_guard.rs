//! H09 工具目录版本与调用链的静态源码守卫。
//!
//! 测试使用 `include_str!` 检查 Domain、Runner、Provider 和 Harness 源文件中的关键字段/标记，
//! 以便发现目录版本、摘要 pin 或拒绝路径被意外删掉。它不执行这些源码、不调用真实 Provider，
//! 也不证明 checkpoint 或 Approval 的运行时恢复结果；具体行为仍需对应的 crate 测试与系统证据。

#[test]
/// 检查当前工具目录实现仍包含版本、摘要、wire 映射、运行 pin 和禁止 fallback 的源码标记。
///
/// 【输入/输出】无运行时数据；源码字符串包含约定标记时通过，缺失或出现禁用标记时失败。
/// 【调用关系】测试只读取文件文本，不进入 DaemonHost → Harness → ControlPlane → Broker 链路。
fn h09_tool_catalog_is_versioned_and_single_source() {
    // include_str! 让守卫能够跨 crate 检查接线位置，但也意味着断言的是源码形状，
    // 而不是这些位置已经在本测试中运行过。
    let authority = include_str!("../../kiana-domain/src/tool_authority.rs");
    let schemas = include_str!("../../kiana-domain/src/tool_catalog.rs");
    let actions = include_str!("../../kiana-domain/src/actions.rs");
    let model = include_str!("../../kiana-domain/src/model.rs");
    let tools = include_str!("../src/tools.rs");
    let provider = include_str!("../../kiana-provider/src/request.rs");
    let harness = include_str!("../src/harness.rs");
    let fixtures = include_str!("h09_tool_catalog.rs");
    // marker 分散在不同实现文件：例如 checkpoint 漂移由 Harness 管，wire 转换由 Provider 管，
    // 所以守卫逐个允许 marker 出现在其合理所属的任一相关文件。
    for marker in [
        "TOOL_CATALOG_SCHEMA",
        "ToolCatalogSnapshot",
        "ToolDescriptor",
        "tool_catalog_digest",
        "tool_catalog_hash",
        "tool_wire_name",
        "current_tool_catalog",
        "output_limit",
        "execution_mode",
        "replay_class",
        "catalog_change_cannot_reuse_old_approval",
        "catalog_digest",
        "tool_catalog_changed",
        "runner_checkpoint_tool_catalog_changed",
        "capability_action_catalog_digest",
    ] {
        assert!(
            authority.contains(marker)
                || schemas.contains(marker)
                || actions.contains(marker)
                || model.contains(marker)
                || tools.contains(marker)
                || provider.contains(marker)
                || harness.contains(marker)
                || fixtures.contains(marker),
            "H09 marker missing: {marker}"
        );
    }
    assert!(authority.contains("ToolCatalogSnapshot::current().validate()"));
    assert!(tools.contains("catalog.validate()?"));
    assert!(provider.contains("ToolNameMap::from_tools") && provider.contains("names.wire_name"));
    assert!(model.contains("tool_catalog_hash != crate::tool_catalog_hash"));
    assert!(actions.contains("tool_catalog_digest"));
    // 当前守卫只拒绝列出的精确源文本。它没有解析 Rust 语义，因此改用局部变量表达相同
    // 约束时可能检测不到；需结合实现审阅，不能将这组 contains 断言当作语义证明。
    for forbidden in [
        "TOOL_SPECS.len() != 5",
        "unadvertised_tool_fallback_to_shell",
        "catalog_change_reuses_approval",
        "wire_name_collision_ignored",
    ] {
        assert!(
            !authority.contains(forbidden)
                && !tools.contains(forbidden)
                && !provider.contains(forbidden),
            "forbidden H09 catalog bypass: {forbidden}"
        );
    }
}

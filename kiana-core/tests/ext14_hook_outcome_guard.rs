//! EXT-14 HookOutcome 的源码 marker 静态检查。
//!
//! 它读取 Query parser 文件文本，确认闭合结果变体、未知字段拒绝、UpdateInput 重授权标志和
//! patch 大小错误仍存在；测试不喂入 JSON、不执行 parser，也不证明 fail-closed 的所有输入分支。

/// 检查 hook outcome parser 仍包含关键结果类型和拒绝路径 marker。
///
/// 【输入】通过 `include_str!` 编译进测试的源文件文本。
/// 【输出】任一 marker 缺失会触发断言失败。
/// 【边界】文本包含关系不等于行为覆盖，也不会找到生产调用者。
#[test]
fn hook_outcomes_are_closed_and_fail_closed() {
    let outcome = include_str!("../../kiana-query/src/hook_outcome.rs");
    // guard 只盯住这些关键分支的存在，不检查每个决策值的具体映射。
    for marker in [
        "HookOutcome",
        "AdditionalContext",
        "Timeout",
        "Cancelled",
        "Unknown",
        "hook_output_unknown_field",
        "hook_update_requires_reauthorization",
        "hook_patch_limit_exceeded",
    ] {
        assert!(outcome.contains(marker), "missing EXT-14 marker: {marker}");
    }
}

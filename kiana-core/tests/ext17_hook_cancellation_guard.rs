//! EXT-17 hook 取消源码 marker 的单文件静态 guard。
//!
//! 它通过 `include_str!` 检查 Query 源码仍包含幂等键、递归深度、访问集合、限制错误和
//! Observer/Unknown 术语；不会调用 helper、模拟取消信号或验证生产接线。

/// 检查 hook cancellation 源码保留递归和 observer 重试相关 marker。
///
/// 【输入】编译期嵌入的目标 Rust 文件文本。
/// 【输出】缺少任何 marker 时断言失败。
/// 【边界】这是子串存在性检查，不解析语法、不证明 marker 所在分支正确，也不检查其他模块。
#[test]
fn hook_cancellation_is_idempotent_recursive_and_observer_retry_only() {
    let source = include_str!("../../kiana-query/src/hook_cancellation.rs");
    for marker in [
        "idempotency_key",
        "recursion_depth",
        "visited",
        "hook_recursion_limit",
        "hook_recursion_visited",
        "HookRunTerminal::Unknown",
        "Observer",
    ] {
        assert!(source.contains(marker), "missing EXT-17 marker: {marker}");
    }
}

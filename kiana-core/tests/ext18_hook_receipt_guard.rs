//! EXT-18 HookReceipt 的源码 marker 静态检查。
//!
//! 它确认回执包含快照/patch 摘要、cleanup、控制面决策和固定未执行标记，同时排除两种进程启动
//! 依赖文本；不会构造 receipt、校验 digest 或尝试恢复执行。

/// 检查 hook 回放视图读取 receipt 并保持不执行边界。
///
/// 【输入】通过 `include_str!` 编译进来的 Query receipt 源文件。
/// 【输出】任一必需 marker 不存在，或出现被禁止的执行文本时断言失败。
/// 【边界】只看一个文件的子串，不验证字段关系、实际进程清理或生产调用链。
#[test]
fn hook_replay_is_receipt_only_and_fences_unknown_cleanup() {
    let receipt = include_str!("../../kiana-query/src/hook_receipt.rs");
    // 这些 marker 对应可审计摘要、未知清理的人工决策边界和 replay 不执行承诺。
    for marker in [
        "HookReceipt",
        "snapshot_digest",
        "patch_before_digest",
        "cleanup",
        "requires_control_plane_decision",
        "executed: false",
        "hook_receipt_invalid_or_not_replayable",
    ] {
        assert!(receipt.contains(marker), "missing EXT-18 marker: {marker}");
    }
    assert!(!receipt.contains("Command::new"));
    assert!(!receipt.contains("tokio::process"));
}

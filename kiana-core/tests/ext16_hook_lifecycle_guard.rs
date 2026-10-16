//! EXT-16 生命周期事件的源码边界静态检查。
//!
//! 测试把 Query DTO 文件读成文本，检查身份字段、提交前置条件和固定关闭的 capability 标志，
//! 并排除两个执行依赖标记。它不构造事件、不检查多次 dispatch 的顺序，也不证明生产接线。

/// 检查生命周期事件仍然携带调用位置身份，并且不会通过此 DTO 开启 capability dispatch。
///
/// 【输入】编译期嵌入的 `hook_lifecycle_dispatch.rs` 文本。
/// 【输出】缺少 marker 或出现被禁止依赖字符串时断言失败。
/// 【边界】仅做单文件子串检查，不解析 Rust 语法或验证实际运行结果。
#[test]
fn hook_lifecycle_dispatch_is_identity_bound_and_non_executing() {
    let dispatch = include_str!("../../kiana-query/src/hook_lifecycle_dispatch.rs");
    // 这些 marker 分别代表事件类型、作用范围、顺序与“工具结果先提交”的数据边界。
    for marker in [
        "HookLifecycleEventKind",
        "session_id",
        "run_id",
        "snapshot_id",
        "result_committed",
        "capability_dispatch_allowed: false",
        "hook_post_tool_result_not_committed",
    ] {
        assert!(dispatch.contains(marker), "missing EXT-16 marker: {marker}");
    }
    // DTO 只能描述待处理事件，不能引入能力执行器或直接启动外部进程。
    assert!(!dispatch.contains("CapabilityBroker"));
    assert!(!dispatch.contains("std::process::Command"));
}

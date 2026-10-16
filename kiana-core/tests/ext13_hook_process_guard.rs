//! EXT-13 的源码文本守卫：在编译时把 Query hook 适配器源码嵌入测试，再检查若干关键标记。
//!
//! 它用于防止未来改动意外删掉“清理子进程环境、固定工作目录、进程组、drop 时处理 child、
//! 输出截断和 timeout”等显式配置，也拒绝全量枚举父进程环境变量的写法。它不启动 hook
//! 子进程，不验证 timeout 是否覆盖 stdin 写入，也不证明输出采集有内存上限或后代进程已退出。
//! 因此这是单文件静态回归检查，不是 EXT-13 运行时行为或产品接线证明。
#[test]
fn hook_process_execution_has_supervised_environment_and_output_bounds() {
    let hooks = include_str!("../../kiana-query/src/stop_hooks.rs");
    for marker in [
        "env_clear",
        "current_dir(cwd)",
        "process_group(0)",
        "kill_on_drop(true)",
        "bounded_hook_output",
        "hook_output_budget",
        "timeout_duration",
    ] {
        assert!(hooks.contains(marker), "missing EXT-13 marker: {marker}");
    }
    assert!(!hooks.contains("std::env::vars()"));
}

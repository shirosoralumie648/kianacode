//! P1-H-01 的 Core 静态源码守卫。
//!
//! 该测试用 `include_str!` 把 Domain、Runner、Daemon 等源码读为编译期字符串，检查单一工具
//! 权威目录及其接线标记是否仍存在。它不会构造 `DaemonHost`、运行 `ControlPlane`、映射真实
//! 模型响应或调用 Broker，因此证明上限是源码约定守卫；行为合同由相应 crate 的运行时测试覆盖。

#[test]
/// 检查 Runner 和 Daemon 源码仍引用同一 Domain 工具规格，并保留关键映射/启动校验标记。
///
/// 【输入】无运行时输入；`include_str!` 在编译期载入仓库中的源码文本。
/// 【输出】所有 marker 存在则测试通过；缺失会在对应断言处失败。
/// 【边界】字符串存在不代表该分支在运行时一定会执行，也不证明策略、审批或 handler 有效。
fn runner_and_daemon_consume_the_single_tool_authority_registry() {
    // 分别观察定义目录、Runner 消费路径、Daemon 组合根和稳定合同 schema 的源文件。
    let authority = include_str!("../../kiana-domain/src/tool_authority.rs");
    let catalog = include_str!("../../kiana-domain/src/tool_catalog.rs");
    let runner = include_str!("../../kiana-runner/src/tools.rs");
    let daemon = include_str!("../../kiana-daemon/src/lib.rs");
    let contracts = include_str!("../../kiana-domain/src/contracts.rs");
    // 这些标记代表 authority 的定义和公共查找/验证入口；文本守卫用于防止接线被无声删除。
    for marker in [
        "pub struct ToolSpec",
        "pub const TOOL_SPECS",
        "pub fn tool_spec",
        "static TOOL_SPECS_CACHE",
        "impl std::ops::Deref for ToolSpecCollection",
        "impl AsRef<[ToolSpec]> for ToolSpecCollection",
        "validate_tool_authority",
        "side_effecting",
        "risk_policy",
    ] {
        assert!(
            authority.contains(marker),
            "authority marker missing: {marker}"
        );
    }
    assert!(catalog.contains("crate::tool_authority::tool_spec(name)"));
    // Runner 必须先从 descriptor 规范化名称，再通过固定分支构造请求；不能接受任意工具名
    // 后悄悄转成 shell。下面只核对相关映射结构仍在源文件中。
    for marker in [
        "let canonical = descriptor.name.as_str()",
        "match canonical",
        "TOOL_SHELL =>",
        "TOOL_APPLY_PATCH =>",
        "TOOL_MCP =>",
        "capability_action_minimum_risk(",
    ] {
        assert!(
            runner.contains(marker),
            "runner registry marker missing: {marker}"
        );
    }
    // 宿主启动检查与 Broker catalog binding 检查分别落在组合根和执行适配边界；这两条
    // 字符串断言并不模拟实际构造或 dispatch。
    assert!(daemon.contains("kiana_domain::validate_tool_authority().map_err(PortError::Failed)?;"));
    assert!(daemon.contains("capabilities.validate_catalog_bindings()?;"));
    assert!(contracts.contains("kiana.tool-authority.v1"));
}

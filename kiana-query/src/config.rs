//! 查询循环入口处冻结的配置快照。
//!
//! 配置在一次 query 调用开始时从环境变量读取一次，随后作为不可变数据传给 reducer 和
//! 外层执行器。这样状态变化不会反过来修改功能开关，也避免同一轮执行前后读取到不同
//! 环境值。这里仅承载运行时 gates；涉及编译裁剪或 tree-shaking 的 feature flag 仍必须
//! 保持在实际受保护的代码块附近，不能假装是运行时配置。

use kiana_types::ids::SessionId;
use serde::{Deserialize, Serialize};

/// 在查询开始时解析一次的运行时门控开关。
///
/// 字段值一旦写入快照，在本轮 query 中不会再次从环境读取。环境变量只是当前适配器的
/// 配置来源，不是安全授权来源；任何能力执行仍要经过 Kiana 控制面。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QueryGates {
    /// 模型是否支持流式工具执行；当前 Kiana Web 主路径仍可能明确关闭该能力。
    pub streaming_tool_execution: bool,
    /// 是否在对话中发出工具使用摘要消息。
    pub emit_tool_use_summaries: bool,
    /// 是否通过 `USER_TYPE=ant` 标记为特定内部用户环境。
    pub is_ant: bool,
    /// 是否启用 fast mode；`CLAUDE_CODE_DISABLE_FAST_MODE` 为真时关闭。
    pub fast_mode_enabled: bool,
}

/// 每次查询独立持有的不可变配置。
///
/// `config` 与可变的 `QueryState` 分离：同一配置可以被多个纯状态转移复用，测试也能用
/// 合成值构造它。该快照只描述“本轮应该采用哪些门控”，不包含授权、项目 trust 或工具
/// 执行结果；这些事实由控制面和外层适配器负责。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QueryConfig {
    /// 配置所属的会话 ID，用于把 reducer 状态绑定回调用方。
    pub session_id: SessionId,
    /// 本轮查询的运行时门控。
    pub gates: QueryGates,
}

impl QueryConfig {
    /// 从当前进程环境构造一次配置快照。
    ///
    /// 生产代码应在 query 开始处调用一次；不要在每个状态转移或工具调用中重复读取环境，
    /// 否则同一会话可能在中途改变行为。测试可以直接构造 [`QueryConfig`]，避免依赖全局
    /// 环境变量。
    pub fn from_env(session_id: SessionId) -> Self {
        let streaming_tool_execution = std::env::var("CLAUDE_STREAMING_TOOL_EXECUTION")
            .map(|v| is_env_truthy(&v))
            .unwrap_or(false);

        let emit_tool_use_summaries = std::env::var("CLAUDE_CODE_EMIT_TOOL_USE_SUMMARIES")
            .map(|v| is_env_truthy(&v))
            .unwrap_or(false);

        let is_ant = std::env::var("USER_TYPE")
            .map(|v| v == "ant")
            .unwrap_or(false);

        let fast_mode_enabled = !std::env::var("CLAUDE_CODE_DISABLE_FAST_MODE")
            .map(|v| is_env_truthy(&v))
            .unwrap_or(false);

        QueryConfig {
            session_id,
            gates: QueryGates {
                streaming_tool_execution,
                emit_tool_use_summaries,
                is_ant,
                fast_mode_enabled,
            },
        }
    }
}

/// 判断环境变量是否采用常见的真值拼写。
///
/// `1`、`true`、`yes`、`on`（不区分大小写）返回 `true`，其他字符串包括空字符串返回
/// `false`。函数不读取环境，也不对缺失变量作额外推断。
pub fn is_env_truthy(val: &str) -> bool {
    matches!(val.to_lowercase().as_str(), "1" | "true" | "yes" | "on")
}

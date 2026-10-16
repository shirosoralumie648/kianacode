//! `kiana-query` 状态转移所使用的会话身份和运行时配置快照。
//!
//! 【文件职责与系统位置】
//! 本文件把调用方给出的 `SessionId` 和进程环境变量整理成 `QueryConfig`。它位于 Query
//! 状态转移 API 的配置边界：`transitions::transition_query_state()` 接收该值，Start 事件
//! 会用其中的会话 ID 更新状态。配置本身只描述开关，不代表授权，不会启动模型或工具。
//!
//! 数据流：
//! 调用方提供 SessionId + 当前进程环境
//!     ↓ `QueryConfig::from_env()` 读取各环境变量一次
//! `QueryConfig { session_id, gates }`
//!     ↓
//! `transition_query_state()`（纯状态转移）
//!     ↓
//! 返回新的 QueryState 与待由外层解释的 QueryEffect
//!
//! 仓内调用核对：`QueryConfig` 被 `transitions` 模块的 reducer 和测试引用，并由 crate 根重导出；
//! 当前没有找到 `QueryConfig::from_env()` 的产品调用，reducer 也只读取 `session_id`，没有
//! 读取四个 gate 字段。因此不能根据配置名推断这些开关已经改变了实际执行行为。
//!
//! 【为什么在入口读取并保留为普通数据】
//! 同一轮状态转移应看到相同的配置值，不能在处理中途重新查询环境变量；不可变快照让输入
//! 可显式传递，也让测试可以构造配置而不改全局环境。这里承载的是运行时值；编译期 feature
//! 或 tree-shaking 决定的代码是否存在，仍需在真实受保护代码附近处理，不能由布尔字段替代。

use kiana_types::ids::SessionId;
use serde::{Deserialize, Serialize};

/// 查询配置快照中的四个运行时开关字段。
///
/// `Serialize` / `Deserialize` 来自 serde 的派生宏：它们分别让 Rust 值能转换到外部数据格式、
/// 以及从外部格式还原成 Rust 值。derive 只生成转换实现，不会自行读取环境或写文件。
///
/// 这些字段是普通 `bool`，不是授权凭证。`true` / `false` 表达配置意图；本仓当前 reducer
/// 没有读取这些 gate，所以“设为 true”本身不能证明某能力会运行。环境变量也不是权限来源，
/// 能力执行仍须遵循 Kiana 的授权控制面。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QueryGates {
    /// 配置层对流式工具执行的开关意图：true 表示开启，false 表示关闭。
    /// 当前仓内只有配置字段和测试构造，没有 reducer 消费点；它不验证模型或适配器是否支持。
    pub streaming_tool_execution: bool,
    /// 是否请求对话层发出工具使用摘要：true 表示应发出，false 表示不发出；当前 reducer 未读取。
    pub emit_tool_use_summaries: bool,
    /// 环境标签是否精确等于 `USER_TYPE=ant`。这是环境分类标记，不是身份认证或授权判断。
    pub is_ant: bool,
    /// fast mode 的配置意图：true 表示没有通过禁用变量关闭它，false 表示禁用变量为真。
    /// 未设置禁用变量时默认 true；当前 reducer 未读取这个值。
    pub fast_mode_enabled: bool,
}

/// 单次 Query 状态转移使用的配置快照。
///
/// `session_id` 由 `QueryState::new()` 和 `QueryEvent::Start` 用来把状态绑定到当前会话；
/// `gates` 是环境解析出的开关快照。它与可变 `QueryState` 分开，所以一次转移更新状态时不会
/// 顺带修改配置。测试也能直接构造合成配置，不必写进程级环境变量。
///
/// 这份 DTO 不含审批、ProjectTrust、工具执行结果或 EventLog 事实；这些责任属于控制面和
/// 外层 adapter。序列化能力也不表示项目当前已经把它持久化。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QueryConfig {
    /// 配置所属的会话 ID，用于把 reducer 状态绑定回调用方。
    pub session_id: SessionId,
    /// 本轮查询的运行时门控。
    pub gates: QueryGates,
}

impl QueryConfig {
    /// 从调用者给定的会话 ID 和当前进程环境构造一次配置快照。
    ///
    /// 【调用者】当前仓内未找到生产调用；该方法由 crate API 暴露，测试可以直接构造
    /// [`QueryConfig`]。若未来执行器使用它，应在一次 Query 开始时调用一次并继续传递该快照。
    /// 【输入】`session_id` 是调用方确定的会话身份；另外读取四个环境变量：
    /// `CLAUDE_STREAMING_TOOL_EXECUTION`、`CLAUDE_CODE_EMIT_TOOL_USE_SUMMARIES`、
    /// `USER_TYPE`、`CLAUDE_CODE_DISABLE_FAST_MODE`。
    /// 【输出】拥有 session ID 和四个 bool gate 的配置值。
    /// 【默认与解析】前两个变量仅在 `is_env_truthy()` 返回 true 时开启，否则关闭；`USER_TYPE`
    /// 只有精确等于 `ant` 才把 `is_ant` 设为 true；禁用 fast mode 的变量为真时将
    /// `fast_mode_enabled` 设为 false，未设置或不是真值时默认为 true。读取失败（包括缺失）
    /// 会按各字段的上述默认值继续，不返回错误。
    /// 【副作用】只读取当前进程环境，不修改环境或会话状态。
    /// 【为什么生成快照】后续 reducer 收到显式值，不必在每个事件转移时再次读取全局环境。
    /// 但四个 gate 当前未被 reducer 消费，调用本方法不等于打开任何实际能力。
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

/// 把环境变量文本转换为项目约定的真/假值。
///
/// 【输入】已经读取到的环境变量内容；函数不负责查找变量。
/// 【输出】精确匹配 `1`、`true`、`yes`、`on`（英文大小写不敏感）时返回 true，其他字符串
/// 包括空字符串都返回 false。实现会转成小写，但不会 trim，因此 ` true ` 仍为 false。
/// 【副作用】无；函数不读取或修改进程环境。
/// 【失败处理】此函数不返回 Result；环境变量缺失/读取失败由 `from_env()` 的 `unwrap_or`
/// 决定默认值。不要把这里的 false 一概理解为显式用户拒绝，它也可能表示未配置或拼写不受支持。
pub fn is_env_truthy(val: &str) -> bool {
    matches!(val.to_lowercase().as_str(), "1" | "true" | "yes" | "on")
}

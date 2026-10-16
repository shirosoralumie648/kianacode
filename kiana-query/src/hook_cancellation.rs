//! Hook 调用身份、递归防护和取消后的重试建议。
//!
//! 【文件职责与系统位置】
//! 本文件定义 hook 一次调用的身份 DTO，并提供三个纯 helper：为父子 hook 生成递归身份、
//! 把取消观察映射成终态、判断某类终态是否允许 observer 重试。它不启动或停止进程，不发出
//! cancellation signal，也不执行重试。
//!
//! 预期数据流：
//! 外层已经确定的 run/hook/invocation ID + 可选父身份
//!     ↓ `prepare_hook_invocation()` 校验输入、深度与 visited 集合
//! `HookInvocationIdentity`（含幂等键和摘要）
//!     ↓ 外层运行 hook，并观察取消是否已确认
//! `cancellation_terminal()` / `retry_allowed()` 只计算终态与布尔建议
//!     ↓ 外层生命周期执行器决定实际处理
//!
//! 调用关系核对：仓内只找到本文件定义、`kiana-query` 根重导出和 Core 的文本静态 guard，
//! 没有找到生产消费者。不能把这些 DTO/布尔函数当成已经生效的 hook 监督器或重试机制。
//! 设计意图与调用证据必须分开看。
//!
//! serde derive 提供 Rust 数据与外部格式间的转换；`deny_unknown_fields` 在反序列化时拒绝
//! 未声明字段。它们不会验证调用者身份，也不会自动计算或核验 `identity_digest`。

use kiana_domain::json_digest;
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

pub const HOOK_CANCELLATION_SCHEMA: &str = "kiana.hook-cancellation.v1";
/// 允许写入 identity 的最大递归深度；根调用深度为 0，因此最多接受深度值 8。
pub const MAX_HOOK_RECURSION_DEPTH: u8 = 8;

/// 本次 hook 执行承担的角色。
///
/// 当前 `retry_allowed()` 只允许 Observer 在 Unknown 终态重试；Guard 角色不获此建议。
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HookRunRole {
    /// 负责阻断/守卫类检查的角色，不能走 observer 的 Unknown 重试分支。
    Guard,
    /// 观察类 hook；在结果未知时可以由外层考虑重试。
    Observer,
}

/// 外层对一次 hook 执行最终观察到的终态分类。
///
/// 本文件的 `cancellation_terminal()` 只会返回 Cancelled 或 Unknown；Completed/Blocked 可由
/// 生命周期执行器在其他结果路径中使用。Unknown 表示不能确认已完成或已停止，不等同于失败。
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HookRunTerminal {
    /// 外层确认 hook 正常完成。
    Completed,
    /// hook 的策略结果要求阻断后续动作。
    Blocked,
    /// 外层确认取消已经停止 hook。
    Cancelled,
    /// 取消或执行结果尚未确认，不能假定操作已停止。
    Unknown,
}

/// 一次 hook 调用及其递归链的可传递身份数据。
///
/// 上游由外层分配 run、hook 和 invocation ID；子 hook 从 parent 继承递归深度与 visited 集合。
/// 该结构自身没有 `validate()`，公开字段可被直接构造，因此消费者不能只检查 digest 字符串
/// 就认为来源可信。
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HookInvocationIdentity {
    /// DTO 版本标识，用于调用边界识别此数据形状。
    pub schema: String,
    /// 所属 run 的文本 ID，由调用方提供。
    pub run_id: String,
    /// 当前 hook 的文本 ID，由调用方提供。
    pub hook_id: String,
    /// 同一 hook 在该 run 中这一次调用的文本 ID。
    pub invocation_id: String,
    /// 根据 run/hook/invocation 三个 ID 拼出的幂等键；格式见构造函数说明。
    pub idempotency_key: String,
    /// 从根调用 0 开始，每向下一层 hook 增加 1。
    pub recursion_depth: u8,
    /// 当前递归链已经访问的拼接身份集合，用于拒绝重复访问。
    pub visited: BTreeSet<String>,
    /// 对构造时 identity JSON 求出的摘要；它不是密钥签名，也没有配套校验函数。
    pub identity_digest: String,
}

/// 为本次 hook 构造身份，并在生成子调用前检查递归限制和重复访问。
///
/// 【调用者】设计上由 hook 生命周期 adapter 在启动一次 hook 前调用；仓内当前没有直接生产调用。
/// 【输入】调用方分配的 run ID、hook ID、invocation ID，以及可选父调用身份。
/// 【输出】带 schema、递归深度、继承后的 visited 集合、幂等键和摘要的新身份。
/// 【拒绝情况】三个输入中任一 trim 后为空、UTF-8 字节长度超过 256、包含 NUL 时返回
/// `hook_invocation_identity_invalid`；深度超过 8 返回 `hook_recursion_limit`；拼接键已在
/// parent.visited 中时返回 `hook_recursion_visited`。错误只返回字符串，没有自动日志、重试或
/// 生命周期状态变化。
/// 【核心流程】根调用深度为 0；子调用对 parent 深度饱和加一；复制父集合并插入新键；构造 DTO；
/// 最后对当时 `identity_digest` 仍为空字符串的 JSON 求摘要。
/// 【为什么记录 visited】最大深度限制只防止无限加深，visited 还能更早发现同一递归链的环回。
/// 【边界/易误改】长度按 UTF-8 字节数计算；冒号目前未禁止，键又通过冒号拼接，所以不同 ID
/// 组合可能形成相同文本键。函数也不验证 parent 的 digest/schema/depth/集合大小；外层必须
/// 先保证 parent 来自可信构造路径，且不能把摘要当作认证或完整性签名。
pub fn prepare_hook_invocation(
    run_id: &str,
    hook_id: &str,
    invocation_id: &str,
    parent: Option<&HookInvocationIdentity>,
) -> Result<HookInvocationIdentity, String> {
    if [run_id, hook_id, invocation_id]
        .iter()
        .any(|value| value.trim().is_empty() || value.len() > 256 || value.contains('\0'))
    {
        return Err("hook_invocation_identity_invalid".to_owned());
    }
    // 深度表示从根向下经过了多少层父子边；饱和加法可防止 u8 溢出绕回 0，再由上限拒绝。
    let recursion_depth = parent.map_or(0, |value| value.recursion_depth.saturating_add(1));
    if recursion_depth > MAX_HOOK_RECURSION_DEPTH {
        return Err("hook_recursion_limit".to_owned());
    }
    // 通过一个可读的扁平键同时做幂等关联和环回检测；分隔符没有转义，因此 ID 含冒号时
    // 不同三元组存在碰撞可能，调用方应视作当前格式的边界。
    let key = format!("{run_id}:{hook_id}:{invocation_id}");
    // 父集合必须复制，子身份只能在自己的集合副本上追加，不能反向修改父调用的数据。
    let mut visited = parent.map_or_else(BTreeSet::new, |value| value.visited.clone());
    if !visited.insert(key.clone()) {
        return Err("hook_recursion_visited".to_owned());
    }
    let mut identity = HookInvocationIdentity {
        schema: HOOK_CANCELLATION_SCHEMA.to_owned(),
        run_id: run_id.to_owned(),
        hook_id: hook_id.to_owned(),
        invocation_id: invocation_id.to_owned(),
        idempotency_key: format!("hook:{key}"),
        recursion_depth,
        visited,
        identity_digest: String::new(),
    };
    // 先把空 digest 字段包含在确定性 JSON 中求摘要，再写入结果字段。json_digest 不是带密钥签名。
    identity.identity_digest = json_digest(&serde_json::to_value(&identity).unwrap_or_default());
    Ok(identity)
}

/// 根据 hook 角色和取消确认情况，给出保守的终态分类。
///
/// 【输入】角色，以及 `stop_confirmed`：Some(true) 表示外层确认已停止；Some(false) 表示确认
/// 未停止；None 表示没有足够观察结果。
/// 【输出】只有 Some(true) 映射为 Cancelled，其余组合都映射为 Unknown。
/// 【重要】当前匹配中 Observer/Guard 不改变结果；`role` 暂时没有实际区分作用。函数不返回
/// Completed 或 Blocked，因为那需要其他执行结果路径提供证据。
/// 【副作用/失败】纯函数，不发信号、不等待进程结束、不返回错误。
pub fn cancellation_terminal(role: HookRunRole, stop_confirmed: Option<bool>) -> HookRunTerminal {
    match (role, stop_confirmed) {
        (_, Some(true)) => HookRunTerminal::Cancelled,
        (HookRunRole::Observer, None) => HookRunTerminal::Unknown,
        _ => HookRunTerminal::Unknown,
    }
}

/// 判断外层是否可以考虑重试某类 hook 终态。
///
/// 只有 Observer 且终态为 Unknown 返回 true；Guard、Cancelled、Completed、Blocked 都返回 false。
/// 该函数只给布尔建议，不执行重试、不生成新 invocation ID，也不验证重试是否复用原幂等键。
/// 调用方仍需结合实际执行契约避免把结果未确认的非幂等副作用重复执行。
pub fn retry_allowed(role: HookRunRole, terminal: HookRunTerminal) -> bool {
    role == HookRunRole::Observer && matches!(terminal, HookRunTerminal::Unknown)
}

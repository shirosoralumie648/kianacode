//! 将 hook 返回的 JSON 文本解析为受限的结果分类。
//!
//! 【文件职责与系统位置】
//! 本文件定义闭合的 `HookOutcome` enum，并提供 JSON stdout 文本到 enum 的解析和字段边界校验。
//! 预期上游是读取 hook 输出的外层 adapter；下游应把结果交给授权/生命周期逻辑，尤其 Ask 和
//! UpdateInput 不能在此处直接视为已批准或已执行。
//!
//! 仓内调用核对：当前只有定义、`kiana-query` 根重导出和 Core 的文本静态 guard；没有找到
//! `stop_hooks` 或其他产品代码调用 `parse_hook_outcome()`。因此这里描述的是独立 parser 的
//! 实际规则，不能推断现有 hook 进程输出已经经过它。
//!
//! serde tagged enum 会把变体名称写入 `kind` 字段，供反序列化端选择不同字段形状。JSON 大小、
//!文本长度上限用于限制进入后续流程的数据规模；解析与校验本身不运行 hook、不做审批或重试。

use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::BTreeSet;

/// HookOutcome 契约的版本名称；当前 enum 本身没有 schema 字段使用它。
pub const HOOK_OUTCOME_SCHEMA: &str = "kiana.hook-outcome.v1";
// patch 和 context 各自最多占 16 KiB；这是序列化 JSON 的 UTF-8 字节数限制，不是字符数。
const MAX_PATCH_BYTES: usize = 16 * 1024;
const MAX_CONTEXT_BYTES: usize = 16 * 1024;

/// hook 输出适配器识别的封闭结果集合。
///
/// 枚举把原始 JSON 的多种拼写收敛成稳定 Rust 变体；直接构造 enum 的调用者仍需调用
/// `validate()`，否则可以绕过 parser 的长度和重新授权标记检查。
#[derive(Debug, Clone, Eq, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum HookOutcome {
    /// hook 明确允许当前操作继续；这不是 Kiana 授权决定的替代品。
    Allow,
    /// hook 拒绝或阻断当前操作，并提供可展示原因。
    Block {
        /// 拒绝原因文本，校验时须非空且不超过 4,096 字节。
        reason: String,
    },
    /// hook 要求取得审批；仅带有引用标识，不代表该审批存在或已批准。
    Ask {
        /// 外部审批流程的引用，校验时须非空且不超过 256 字节。
        approval_ref: String,
        /// 请求审批的原因。
        reason: String,
    },
    /// hook 建议替换输入；调用方必须在重新授权后才可继续使用 patch。
    UpdateInput {
        /// 任意 JSON patch 值，序列化后最多 16 KiB；本 enum 不解释 patch 的业务结构。
        patch: Value,
        /// 必须为 true，表示新输入需要重新授权；标记 true 本身不会执行授权。
        reauthorize: bool,
    },
    /// hook 附加给后续上下文的文本项，不是权威状态或工具结果。
    AdditionalContext {
        /// 最多 32 项；每项非空且不超过 4,096 字节，合计不超过 16 KiB。
        items: Vec<String>,
    },
    /// hook 报告的超时结果。
    Timeout {
        /// 允许范围为 1 到 10,000 毫秒；0 和超过 10 秒均无效。
        timeout_ms: u64,
    },
    /// hook 报告已经取消；实际停止是否确认由外层执行器负责。
    Cancelled {
        /// 取消原因。
        reason: String,
    },
    /// 输出无法映射到已知决策时的保守表示；未知原因不会自动触发重试。
    Unknown {
        /// 未知/无法解释的原因。
        reason: String,
    },
}

impl HookOutcome {
    /// 校验直接构造或解析出的 outcome 字段边界。
    ///
    /// 【调用者】`parse_hook_outcome()` 在返回前调用；直接构造此 enum 的下游也必须自行调用。
    /// 【输入/输出】借用当前变体；所有字段合法返回 Ok，否则返回固定字符串错误。
    /// 【校验重点】文本非空、NUL/字节数边界、UpdateInput 必须要求重授权、JSON patch 大小、
    /// AdditionalContext 的项目数/单项/总字节上限，以及 timeout 的 1..=10,000ms 范围。
    /// 【失败处理】这里不记录日志、不批准、不重试；调用者收到 Err 后需按 hook 角色采取策略。
    /// 【为什么分开校验】serde 反序列化能构造数据结构，但不保证业务字段符合本项目边界；
    /// 将限制集中在此处，也让直接构造的 enum 有明确检查入口。
    pub fn validate(&self) -> Result<(), String> {
        match self {
            Self::Allow => Ok(()),
            Self::Block { reason } | Self::Cancelled { reason } | Self::Unknown { reason } => {
                bounded_text(reason, 4_096)
            }
            Self::Ask {
                approval_ref,
                reason,
            } => {
                bounded_text(approval_ref, 256)?;
                bounded_text(reason, 4_096)
            }
            Self::UpdateInput { patch, reauthorize } => {
                // 输入替换会改变先前授权覆盖的内容，因此没有明确的重新授权要求就不能接受。
                if !*reauthorize {
                    return Err("hook_update_requires_reauthorization".to_owned());
                }
                bounded_json(patch, MAX_PATCH_BYTES)
            }
            Self::AdditionalContext { items } => {
                // 项数和逐项长度先限制最坏遍历规模，再检查总字节，防止许多短项合计过大。
                if items.len() > 32
                    || items
                        .iter()
                        .any(|item| item.trim().is_empty() || item.len() > 4_096)
                    || items.iter().map(String::len).sum::<usize>() > MAX_CONTEXT_BYTES
                {
                    return Err("hook_context_limit_exceeded".to_owned());
                }
                Ok(())
            }
            // 10 秒是此 DTO 接受的最大超时值；超过范围可能与 hook 执行器的资源策略不一致。
            Self::Timeout { timeout_ms } if *timeout_ms == 0 || *timeout_ms > 10_000 => {
                Err("hook_timeout_invalid".to_owned())
            }
            Self::Timeout { .. } => Ok(()),
        }
    }
}

/// 把一次 hook 的 stdout JSON 文本转换成已校验的 `HookOutcome`。
///
/// 【调用者】设计上由 hook 输出 adapter 调用；当前仓内没有产品消费者，Core 测试只静态读取源码。
/// 【输入】一个完整 JSON 文本，顶层必须是对象。
/// 【输出】识别的 decision 映射到闭合 enum，并在返回前执行 `validate()`。
/// 【解析规则】先拒绝未知顶层键；`decision` 识别 allow/approve、block/deny、ask、update_input、
/// additional_context、timeout、cancelled；未识别 decision 映射为 Unknown。审批引用和 patch
/// 支持 snake_case/camelCase 两种字段别名。
/// 【失败情况】JSON 无效/不是对象、未知键、Ask 缺引用、输入 patch 缺失、context 缺失、timeout
/// 不可读或 outcome 字段越界时返回错误字符串；错误由调用方决定记录、阻断或转成 Unknown。
/// 【副作用】纯解析；不启动进程、不对 patch 重新授权、不执行 approval、不重试。
/// 【注意】当前实现会把缺失或非字符串的 `decision` 都转成 None，并在没有 `unknown` 键时默认
/// Allow；因此 `{}`、`{"decision":null}`，甚至只有 `{"cancelled":true}` 都会成为 Allow。
/// 若调用协议要求缺失/类型错误的 decision fail-closed，这是潜在 fail-open 边界，不能靠类型名
/// `Unknown` 推断这些输入会被拒绝。raw 字符串本身没有先行字节数上限，实际 adapter 需要限制 stdout。
pub fn parse_hook_outcome(raw: &str) -> Result<HookOutcome, String> {
    // from_str 解析单个完整 JSON 值；语法错误、额外尾随 JSON 或非对象根节点都会失败。
    let value: Value = serde_json::from_str(raw).map_err(|_| "hook_output_invalid".to_owned())?;
    let object = value
        .as_object()
        .ok_or_else(|| "hook_output_invalid".to_owned())?;
    let allowed = BTreeSet::from([
        "decision",
        "reason",
        "approval_ref",
        "approvalRef",
        "update_input",
        "updated_input",
        "additional_context",
        "additionalContext",
        "timeout_ms",
        "cancelled",
        "unknown",
    ]);
    // 只检查顶层键名。允许的兼容字段即使与当前 decision 无关也会被保留接受，不代表其值都会被使用。
    if object.keys().any(|key| !allowed.contains(key.as_str())) {
        return Err("hook_output_unknown_field".to_owned());
    }
    let reason = object
        .get("reason")
        .and_then(Value::as_str)
        .unwrap_or("hook outcome")
        .to_owned();
    let decision = object.get("decision").and_then(Value::as_str);
    // `unknown` 是一个存在性标记：无 decision 且没有该键时走默认 Allow；有该键时走下方 Unknown。
    // cancelled/timeout_ms 等虽在允许键表中，但没有 decision 时也不会单独决定变体。
    let outcome = match decision {
        Some("allow" | "approve") | None if object.get("unknown").is_none() => HookOutcome::Allow,
        Some("block" | "deny") => HookOutcome::Block { reason },
        Some("ask") => HookOutcome::Ask {
            approval_ref: object
                .get("approval_ref")
                .or_else(|| object.get("approvalRef"))
                .and_then(Value::as_str)
                .ok_or_else(|| "hook_approval_ref_missing".to_owned())?
                .to_owned(),
            reason,
        },
        Some("update_input") => HookOutcome::UpdateInput {
            patch: object
                .get("update_input")
                .or_else(|| object.get("updated_input"))
                .cloned()
                .ok_or_else(|| "hook_update_input_missing".to_owned())?,
            reauthorize: true,
        },
        Some("additional_context") => HookOutcome::AdditionalContext {
            items: object
                .get("additional_context")
                .or_else(|| object.get("additionalContext"))
                .and_then(Value::as_array)
                .ok_or_else(|| "hook_context_missing".to_owned())?
                .iter()
                // 非字符串数组项在解析时被静默跳过，之后只验证保留下来的字符串集合。
                .filter_map(Value::as_str)
                .map(str::to_owned)
                .collect(),
        },
        Some("timeout") => HookOutcome::Timeout {
            timeout_ms: object
                .get("timeout_ms")
                .and_then(Value::as_u64)
                .ok_or_else(|| "hook_timeout_missing".to_owned())?,
        },
        Some("cancelled") => HookOutcome::Cancelled { reason },
        _ => HookOutcome::Unknown { reason },
    };
    // 返回前总是再走统一字段校验，避免 parser 分支漏掉长度或范围限制。
    outcome.validate()?;
    Ok(outcome)
}

/// 校验非空、无 NUL 且按 UTF-8 字节数受限的文本。
/// `max` 是字节上限，不是 Unicode 字符个数；失败统一返回 hook outcome 文本错误。
fn bounded_text(value: &str, max: usize) -> Result<(), String> {
    if value.trim().is_empty() || value.len() > max || value.contains('\0') {
        Err("hook_outcome_text_invalid".to_owned())
    } else {
        Ok(())
    }
}

/// 将 JSON 值紧凑序列化后按实际 UTF-8 字节长度限制大小。
/// 该检查只限制体积，不验证 patch 的键、字段类型或调用方权限。
fn bounded_json(value: &Value, max_bytes: usize) -> Result<(), String> {
    if serde_json::to_vec(value)
        .map_err(|_| "hook_patch_invalid".to_owned())?
        .len()
        > max_bytes
    {
        Err("hook_patch_limit_exceeded".to_owned())
    } else {
        Ok(())
    }
}

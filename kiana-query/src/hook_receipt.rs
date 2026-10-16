//! Hook 结果回执的只读检查与 replay view 投影。
//!
//! 【文件职责与系统位置】
//! 本文件定义外层 hook 执行后可携带的摘要 DTO，并把一份 receipt 转成恢复/查询侧使用的
//! `HookReplayView`。调用方提供 receipt；本模块只检查少量头字段并标注未知结果是否需要 fence，
//! 不再运行 hook、脚本或任何外部资源。
//!
//! 仓内调用核对：当前只找到类型/函数定义、crate 根重导出和 Core 的文本静态 guard，没有找到
//! 生产 receipt 写入者或 replay 调用者。因此字段描述是该 DTO 的现有约定，不证明已有执行结果
//! 被真实记录或恢复。
//!
//! serde 派生只负责数据转换；`deny_unknown_fields` 在反序列化时拒绝未声明键。自摘要、digest
//! 前缀和 `executed: false` 都不能认证 receipt 来源或证明外部 hook 的真实效果。

use crate::hook_outcome::HookOutcome;
use serde::{Deserialize, Serialize};
use serde_json::Value;

pub const HOOK_RECEIPT_SCHEMA: &str = "kiana.hook-receipt.v1";

/// hook 进程资源清理情况。
///
/// Unknown 表示无法确认清理已经完成；replay helper 会因此把结果 fence 并要求控制面决定下一步。
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HookCleanupState {
    /// 外层已确认相关进程/资源完成清理。
    Confirmed,
    /// 清理是否完成无法确认；不得据此假设进程已经停止。
    Unknown,
    /// hook 尚未启动，因此没有需要清理的已启动资源。
    NotStarted,
}

/// 一次 hook 执行结果及其输入/输出摘要。
///
/// 预期由负责实际执行和资源回收的外层 adapter 填写。结构公开且没有本地 validate 方法；
/// `replay_hook_receipt()` 只检查 schema、hook ID、三个 digest 前缀和 replay_safe 标志，不会
/// 校验所有字段之间是否一致，也不会调用 `HookOutcome::validate()`。
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HookReceipt {
    /// receipt DTO 版本名称。
    pub schema: String,
    /// 产生该回执的 hook 标识。
    pub hook_id: String,
    /// hook 运行时读取的上下文快照摘要。
    pub snapshot_digest: String,
    /// 传给 hook 的输入摘要。
    pub input_digest: String,
    /// 从 hook 得到的输出摘要。
    pub output_digest: String,
    /// 解析后的结果分类；单独保存 receipt 不会证明外层已按该结果执行策略。
    pub outcome: HookOutcome,
    /// Ask 类结果关联的审批引用；None 表示无引用，回放 helper 不会验证其存在或状态。
    pub approval_ref: Option<String>,
    /// 若执行器有进程退出码则记录；回放 helper 不核对它与 outcome 的一致性。
    pub exit_code: Option<i32>,
    /// 外层声明执行是否超时；此字段不参与当前 replay fence 计算。
    pub timed_out: bool,
    /// 外层声明执行是否被取消；此字段不参与当前 replay fence 计算。
    pub cancelled: bool,
    /// 输入 patch 生效前的摘要，可为空。
    pub patch_before_digest: Option<String>,
    /// 输入 patch 生效后的摘要，可为空。
    pub patch_after_digest: Option<String>,
    /// 外层对资源清理情况的分类。
    pub cleanup: HookCleanupState,
    /// 当前 replay helper 遇到 true 会拒绝生成 replay view；不要仅凭字段名推断其为“允许 replay”。
    pub replay_safe: bool,
    /// receipt 的摘要字段；当前 replay helper 不校验此值。
    pub receipt_digest: String,
}

/// 只读 replay/恢复查询得到的展示数据，不包含再次执行 hook 的命令。
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HookReplayView {
    /// 来源回执中的 hook ID。
    pub hook_id: String,
    /// 来源回执的 outcome。
    pub outcome: HookOutcome,
    /// 来源回执的清理状态。
    pub cleanup: HookCleanupState,
    /// 当前结果是否被标记为不确定而需要停止自动推进。
    pub fenced: bool,
    /// 是否需要控制面做显式决定后才可继续。
    pub requires_control_plane_decision: bool,
    /// 固定为 false，表示生成 replay view 不执行 hook。
    pub executed: bool,
}

/// 从一份 receipt 生成只读的恢复视图。
///
/// 【调用者】设计上由恢复/查询 adapter 调用；当前仓内未找到生产消费者，只有 Core 静态 guard。
/// 【输入】外层提供的 HookReceipt。
/// 【输出】hook ID、outcome、cleanup 和是否 fence 的视图，`executed` 始终为 false。
/// 【拒绝情况】schema 不匹配、hook ID 空白、三个摘要没有 `sha256:` 前缀，或
/// `replay_safe == true` 时统一返回 `hook_receipt_invalid_or_not_replayable`。
/// 【副作用】无；不会重启进程，也不把 Unknown 变成成功。
/// 【fence 规则】只有 cleanup 为 Unknown 或 outcome 为 HookOutcome::Unknown 时，`fenced` 与
/// `requires_control_plane_decision` 才同时为 true；timeout/cancelled 字段以及 receipt_digest
/// 当前不参与该判断。
/// 【重要限制】摘要只检查前缀，不验证后面是否是 64 位十六进制；本函数不校验 receipt_digest、
/// outcome 字段约束、审批、退出码与状态字段的一致性，也不证明 receipt 来源可信。
pub fn replay_hook_receipt(receipt: &HookReceipt) -> Result<HookReplayView, String> {
    if receipt.schema != HOOK_RECEIPT_SCHEMA
        || receipt.hook_id.trim().is_empty()
        || !receipt.snapshot_digest.starts_with("sha256:")
        || !receipt.input_digest.starts_with("sha256:")
        || !receipt.output_digest.starts_with("sha256:")
        || receipt.replay_safe
    {
        return Err("hook_receipt_invalid_or_not_replayable".to_owned());
    }
    let unknown = receipt.cleanup == HookCleanupState::Unknown
        || matches!(receipt.outcome, HookOutcome::Unknown { .. });
    Ok(HookReplayView {
        hook_id: receipt.hook_id.clone(),
        outcome: receipt.outcome.clone(),
        cleanup: receipt.cleanup,
        fenced: unknown,
        requires_control_plane_decision: unknown,
        executed: false,
    })
}

/// 对整份序列化 receipt 求 JSON digest。
///
/// 此函数把当前 `receipt_digest` 字段也包含在序列化对象里，并在无法转换时以 JSON Null 求摘要；
/// 它不先清空 digest 字段，也不验证已有摘要是否正确。调用方必须按实际代码语义使用，不能
/// 假定这是自动验证或签名 API。
pub fn hook_receipt_digest(receipt: &HookReceipt) -> String {
    kiana_domain::json_digest(&serde_json::to_value(receipt).unwrap_or(Value::Null))
}

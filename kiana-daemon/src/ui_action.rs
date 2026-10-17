//! UI-04：把版本化界面意图翻译成 Domain UI action command 的 adapter。
//!
//! 入口（Workbench/Web/CLI）提交的是 [`UiActionV1`] 意图；它只描述"用户想做什么"，没有权限、
//! 没有 owner/scope/permit，也没有服务端摘要。本模块负责在**组合根**把它翻译成
//! [`UiActionCommand`]：owner 一律取自 daemon 已认证主体，scope 摘要与 permit 一律取自服务端
//! authority 快照；客户端的字段只作为 expected 值参与对照，不能自己声明成权威。
//!
//! ```text
//! UiActionV1（wire 意图，不可信）
//!     -> 本模块：校验形状 + 绑定服务端 owner/scope/permit + 重算 payload/command digest
//!     -> UiActionCommand（Domain 身份）
//!     -> DaemonHost::admit_ui_action -> ControlPlane -> EventStore CAS
//! ```
//!
//! 本模块是纯函数，不写事件、不发放 permit、不执行 effect；它拒绝时只返回稳定错误标签，
//! 不返回部分构造的命令。

use kiana_core::UiActionAuthoritySnapshot;
use kiana_domain::UiActionCommand;
use kiana_protocol::UiActionV1;

/// 把一次界面意图翻译成只引用服务端权威值的 Domain command。
///
/// 【调用者】`DaemonHost::ui_action_command()`；入口层不得绕过它自行拼接 owner/scope。
/// 【输入】版本化意图、daemon 已认证主体 ID、服务端 authority 快照。
/// 【输出】owner=已认证主体、scope/permit 来自快照、payload/command digest 由服务端重算的 command。
/// 【失败】意图或快照形状非法、`submitted_by` 与已认证主体不一致、快照 owner 不是该主体、
/// 快照缺少 permit、意图缺少 deadline 时返回稳定错误标签；不写任何事件、不执行 effect。
/// 【容易误改】owner 不能取自 `intent.submitted_by`：那是客户端自报值，用一个自报 owner 去撞库
/// 就会把"别人的记录"变成"我的重放"。必须用 daemon 侧已认证主体。
/// 【边界】这里不做 epoch/cursor/revision 新鲜度判定——那是 journal `accept()` 的职责，避免出现
/// 第二套过期规则；本函数只保证进入 journal 的命令身份不可被客户端伪造。
pub fn command_from_surface_intent(
    intent: &UiActionV1,
    principal_actor_id: &str,
    authority: &UiActionAuthoritySnapshot,
) -> Result<UiActionCommand, String> {
    intent.validate()?;
    authority.validate()?;
    if principal_actor_id.trim().is_empty() || principal_actor_id.len() > 256 {
        return Err("ui_action_principal_invalid".to_owned());
    }
    if intent.submitted_by != principal_actor_id {
        return Err("ui_action_submitter_mismatch".to_owned());
    }
    if authority.owner_id != principal_actor_id {
        return Err("ui_action_owner_mismatch".to_owned());
    }
    if authority.permit_digest.is_none() {
        return Err("ui_action_permit_required".to_owned());
    }
    let deadline_unix_ms = intent
        .deadline_unix_ms
        .ok_or_else(|| "ui_action_deadline_required".to_owned())?;
    UiActionCommand::new(
        intent.command_id,
        intent.idempotency_key.clone(),
        intent.target_id.clone(),
        principal_actor_id.to_owned(),
        authority.scope_digest.clone(),
        intent.expected_epoch.clone(),
        intent.expected_cursor,
        intent.expected_revision,
        deadline_unix_ms,
        intent.payload.clone(),
    )
}

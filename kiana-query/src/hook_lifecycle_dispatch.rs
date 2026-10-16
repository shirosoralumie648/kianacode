//! Hook 生命周期事件的只读数据构造器。
//!
//! 【文件职责与系统位置】
//! 本文件把一次 hook 生命周期触发所需的 kind、会话/run/snapshot 身份、顺序号和载荷摘要组合成
//! `HookLifecycleEvent`。预期上游是已经确认本轮生命周期位置的调用方；返回值可交给 Runner 或
//! ControlPlane 边界继续处理。本文件只构造 DTO，不向队列发送事件、不执行 hook，也不发放能力。
//!
//! 调用关系核对：仓内当前只找到定义、crate 根重导出和 Core 的源码文本 guard，没有生产消费者。
//! 因此“dispatch”是事件构造 API 名称，不能据此认定生命周期触发已接入产品运行流。
//!
//! 摘要由确定性 JSON digest 生成，可用于发现字段变化；它没有密钥，也没有配套验证入口，
//! 不能证明调用者身份、提交状态或事件的真实发生。

use kiana_domain::json_digest;
use serde::{Deserialize, Serialize};
use serde_json::Value;

pub const HOOK_LIFECYCLE_DISPATCH_SCHEMA: &str = "kiana.hook-lifecycle-dispatch.v1";

/// 可由调用方报告的 hook 生命周期触发点。
///
/// serde 的 `rename_all = "snake_case"` 会把这些 Rust 变体编码成稳定的小写下划线字符串；
/// 枚举只给事件分类，不会在相应时机自动调用 hook。
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HookLifecycleEventKind {
    /// 新会话初始化时的 hook 阶段。
    SessionStart,
    /// 用户提交新提示时的 hook 阶段。
    UserPromptSubmit,
    /// 请求模型之前的 hook 阶段。
    BeforeModel,
    /// 工具执行之前的 hook 阶段。
    PreToolUse,
    /// 工具成功结果提交之后的 hook 阶段。
    PostToolUse,
    /// 工具失败结果提交之后的 hook 阶段。
    PostToolFailure,
    /// 上下文压缩前后的生命周期阶段。
    Compaction,
    /// 一轮或会话停止时的 hook 阶段。
    Stop,
    /// 会话结束阶段。
    SessionEnd,
    /// 终态收尾阶段。
    Terminal,
}

/// 已绑定触发位置和载荷摘要的 hook 生命周期事件。
///
/// serde 反序列化时拒绝结构中未声明的字段，避免不同版本调用方悄悄传入当前实现不理解的数据；
/// 这不校验字段值是否来自可信 EventLog，也不校验 `event_digest`。
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HookLifecycleEvent {
    /// 当前事件 DTO 的版本标识。
    pub schema: String,
    /// 生命周期阶段分类。
    pub kind: HookLifecycleEventKind,
    /// 产生该生命周期触发的会话 ID。
    pub session_id: String,
    /// 该会话中的执行 run ID。
    pub run_id: String,
    /// 调用方使用的状态快照 ID，用于说明事件对应哪个上下文版本。
    pub snapshot_id: String,
    /// 调用方提供的正数顺序位置；本无状态类型不会与前一个事件比较。
    pub sequence: u64,
    /// 输入 payload 的确定性摘要；原始 payload 不保存在此 DTO 中。
    pub payload_digest: String,
    /// 调用方声明相关结果已经提交；PostTool 事件必须为 true，但此 helper 不验证提交事实。
    pub result_committed: bool,
    /// 固定为 false，表示此事件不能授权或直接触发 capability dispatch。
    pub capability_dispatch_allowed: bool,
    /// 对本事件其余字段求出的确定性摘要；不包含自身字段。
    pub event_digest: String,
}

/// 无内部状态的事件构造器。
///
/// 它不保存上次 sequence，因此不能保证不同调用之间序号递增。
#[derive(Default)]
pub struct HookLifecycleDispatcher;

impl HookLifecycleDispatcher {
    /// 校验基本身份后，构造一条 hook 生命周期事件。
    ///
    /// 【调用者】设计上由 Runner/ControlPlane adapter 在准备交付某阶段事件时调用；仓内未找到
    /// 生产调用者。
    /// 【输入】触发类型、session/run/snapshot ID、调用方给出的 sequence、原始 JSON payload、
    /// 以及调用方声明结果是否已提交。
    /// 【输出】包含 payload 摘要、固定 false capability 标记和 event digest 的事件 DTO。
    /// 【拒绝情况】三个身份去除首尾空白后为空或 sequence 为 0 时返回
    /// `hook_lifecycle_identity_invalid`；PostToolUse/PostToolFailure 的 result_committed 为 false
    /// 时返回 `hook_post_tool_result_not_committed`。
    /// 【副作用】无 I/O；不发送事件、不检查真实提交记录、不同调用的顺序也不做关联校验。
    /// 【容易误改】PostTool 必须先有已提交结果才能构造事件；`capability_dispatch_allowed` 始终
    /// 为 false。schema/digest 只是数据摘要，不能作为权限授权。
    pub fn dispatch(
        &self,
        kind: HookLifecycleEventKind,
        session_id: impl Into<String>,
        run_id: impl Into<String>,
        snapshot_id: impl Into<String>,
        sequence: u64,
        payload: &Value,
        result_committed: bool,
    ) -> Result<HookLifecycleEvent, String> {
        let session_id = session_id.into();
        let run_id = run_id.into();
        let snapshot_id = snapshot_id.into();
        if session_id.trim().is_empty()
            || run_id.trim().is_empty()
            || snapshot_id.trim().is_empty()
            || sequence == 0
        {
            return Err("hook_lifecycle_identity_invalid".to_owned());
        }
        if matches!(
            kind,
            HookLifecycleEventKind::PostToolUse | HookLifecycleEventKind::PostToolFailure
        ) && !result_committed
        {
            return Err("hook_post_tool_result_not_committed".to_owned());
        }
        // 只保存 payload 摘要，避免事件 DTO 再复制任意大小的完整载荷；这里没有载荷字节数上限，
        // 调用方仍需负责控制传入 Value 的规模。
        let mut event = HookLifecycleEvent {
            schema: HOOK_LIFECYCLE_DISPATCH_SCHEMA.to_owned(),
            kind,
            session_id,
            run_id,
            snapshot_id,
            sequence,
            payload_digest: json_digest(payload),
            result_committed,
            capability_dispatch_allowed: false,
            event_digest: String::new(),
        };
        event.event_digest = event.digest();
        Ok(event)
    }
}

impl HookLifecycleEvent {
    /// 计算事件字段摘要，供构造器填充 event_digest 或外层比较字段是否变化。
    ///
    /// 摘要不包含 `event_digest` 自身以避免循环依赖；本方法不验证 schema、sequence 或提交事实，
    /// 也不是带密钥的签名。
    pub fn digest(&self) -> String {
        json_digest(&serde_json::json!({
            "schema": self.schema,
            "kind": self.kind,
            "session_id": self.session_id,
            "run_id": self.run_id,
            "snapshot_id": self.snapshot_id,
            "sequence": self.sequence,
            "payload_digest": self.payload_digest,
            "result_committed": self.result_committed,
            "capability_dispatch_allowed": self.capability_dispatch_allowed,
        }))
    }
}

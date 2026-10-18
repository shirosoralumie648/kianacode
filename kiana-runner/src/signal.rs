//! AUT-18 runner 侧的 pause/resume 边界：把已提交 signal 事实折叠成模型循环可以执行的一步动作。
//!
//! # 本文件负责什么
//!
//! [`RunnerSignalGate::fold`] 是一个**纯函数**：输入是 runner 自己相信的执行身份
//! （[`RunnerSignalIdentity`]）与 Core 已提交的 signal 事实历史，输出是模型循环在边界处唯一允许的
//! 动作（[`RunnerSignalDirective`]）。它把“暂停要真的停、恢复必须绑定同一个 run 的 checkpoint、
//! 终态不可复活”变成可测试的判定，而不是让 harness 自己解释 signal 文本。
//!
//! # 拒绝面（先拒绝）
//!
//! * 身份不完整或摘要格式非法 → `runner_signal_identity_invalid`；
//! * 事实不属于本 execution → `runner_signal_execution_drift`；
//! * owner/definition/action/path 与 runner 身份不一致 → `runner_signal_*_drift`
//!   （重启后只有这些字段全部匹配才允许恢复）；
//! * 单条事实不自洽 → `runner_signal_fact_invalid`；
//! * 重复 `signal_id`：同摘要稳定去重跳过，不同摘要 → `runner_signal_conflict`；
//! * signal 序号不从 1 连续递增 → `runner_signal_sequence_invalid`；
//! * 状态转移不在 pause→resume→consume 白名单（含同状态新事实）→ `runner_signal_transition_refused`；
//! * authority epoch 回退 → `runner_signal_authority_epoch_stale`；epoch 前进只允许出现在 `Resumed`
//!   事实（重启恢复），其余 → `runner_signal_authority_epoch_ahead`；
//! * checkpoint 序号与 runner 实际持有的 checkpoint 不一致 → `runner_signal_checkpoint_unknown`。
//!
//! # 边界
//!
//! 本模块不调用 harness、不发起模型请求、不执行工具、不写 EventStore、不消费审批、不创建
//! checkpoint，也不授予任何授权：`Hold`/`Closed` 时 `model_calls_permitted()` 为 false，工具调用仍
//! 必须回到既有 ControlPlane/Broker 脊柱。它只是把已提交事实读成“下一步该不该继续模型循环”。

use kiana_domain::{
    automation_signal_transition_allowed, AutomationSignalFact, AutomationSignalState,
};
use std::collections::BTreeMap;

/// runner 自己相信的执行身份；重启后用它判断已提交 signal 是否仍然属于同一个 run。
///
/// 这些字段来自 runner 实际持有的 run/checkpoint，而不是调用方在 signal 载荷里自带的文本：
/// `fold` 只把它与已提交事实逐项比较，任何一项不一致都拒绝。
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RunnerSignalIdentity {
    /// 执行 ID；已提交事实必须属于同一个 execution。
    pub execution_id: String,
    /// 执行方 owner。
    pub owner_id: String,
    /// 工作流定义摘要（重启后必须匹配）。
    pub definition_digest: String,
    /// action 内容摘要（参数漂移在这里被拒绝）。
    pub action_digest: String,
    /// 路径范围摘要。
    pub path_scope_digest: String,
    /// runner 当前相信的 authority epoch；更高的 epoch 只允许出现在带匹配 checkpoint 的恢复上。
    pub authority_epoch: u64,
    /// runner 实际持有的 checkpoint 序号；signal 事实必须绑定同一个 checkpoint。
    pub checkpoint_sequence: u64,
}

impl RunnerSignalIdentity {
    /// 构造一个 runner 身份；字段原样保存，校验在 [`RunnerSignalGate::fold`] 中完成。
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        execution_id: impl Into<String>,
        owner_id: impl Into<String>,
        definition_digest: impl Into<String>,
        action_digest: impl Into<String>,
        path_scope_digest: impl Into<String>,
        authority_epoch: u64,
        checkpoint_sequence: u64,
    ) -> Self {
        Self {
            execution_id: execution_id.into(),
            owner_id: owner_id.into(),
            definition_digest: definition_digest.into(),
            action_digest: action_digest.into(),
            path_scope_digest: path_scope_digest.into(),
            authority_epoch,
            checkpoint_sequence,
        }
    }
}

/// 模型循环在 signal 边界处唯一允许的动作。
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RunnerSignalDirective {
    /// 没有任何 signal 事实生效：按当前授权继续模型循环。
    Continue,
    /// 已提交 `Paused` 事实：挂起模型循环，不发起模型请求、不提议工具调用。
    Hold,
    /// 已提交 `Resumed` 事实：从绑定的 checkpoint 继续模型循环（authority epoch 可能已前进）。
    Resume {
        /// 恢复事实绑定的 authority epoch。
        authority_epoch: u64,
        /// 恢复事实绑定的 checkpoint 序号。
        checkpoint_sequence: u64,
    },
    /// signal 生命周期到达 `Consumed` 终态：run 不可重启，也不可再次恢复。
    Closed,
}

impl RunnerSignalDirective {
    /// 该动作是否允许模型循环继续发起请求；`Hold`/`Closed` 一律为 false。
    pub fn model_calls_permitted(self) -> bool {
        matches!(self, Self::Continue | Self::Resume { .. })
    }

    /// 该动作是否是终态关闭；终态之后任何“继续”都需要新的 Run 身份与授权。
    pub fn is_closed(self) -> bool {
        matches!(self, Self::Closed)
    }
}

/// 纯 signal 折叠器：没有状态、没有 I/O，可重复调用并得到字节等价的判定。
pub struct RunnerSignalGate;

impl RunnerSignalGate {
    /// 把已提交 signal 事实折叠成 runner 允许执行的一步动作。
    ///
    /// 【输入】runner 身份 + 某个 execution 的已提交事实（按提交顺序）。
    /// 【输出】`Continue` / `Hold` / `Resume` / `Closed`；没有任何事实时是 `Continue`。
    /// 【失败】任一拒绝条件返回稳定静态错误码，且不产生任何副作用；调用方必须把失败当成“不放行”，
    /// 不能回落到 Continue。
    /// 【副作用】无：不调用模型/工具/Broker/EventStore，不消费审批，不创建 checkpoint。
    pub fn fold(
        identity: &RunnerSignalIdentity,
        facts: &[AutomationSignalFact],
    ) -> Result<RunnerSignalDirective, &'static str> {
        if !valid_identity(identity) {
            return Err("runner_signal_identity_invalid");
        }
        let mut seen: BTreeMap<&str, &str> = BTreeMap::new();
        let mut expected_sequence: u64 = 1;
        let mut previous: Option<&AutomationSignalFact> = None;
        let mut highest_epoch = identity.authority_epoch;
        for fact in facts {
            if fact.validate().is_err() {
                return Err("runner_signal_fact_invalid");
            }
            if fact.execution_id != identity.execution_id {
                return Err("runner_signal_execution_drift");
            }
            if fact.owner_id != identity.owner_id {
                return Err("runner_signal_owner_drift");
            }
            if fact.definition_digest != identity.definition_digest {
                return Err("runner_signal_definition_drift");
            }
            if fact.action_digest != identity.action_digest {
                return Err("runner_signal_action_drift");
            }
            if fact.path_scope_digest != identity.path_scope_digest {
                return Err("runner_signal_path_scope_drift");
            }
            if let Some(recorded) = seen.get(fact.signal_id.as_str()) {
                // 重复 signal_id：同摘要稳定去重跳过，不同摘要冲突。
                if *recorded != fact.signal_digest.as_str() {
                    return Err("runner_signal_conflict");
                }
                continue;
            }
            seen.insert(fact.signal_id.as_str(), fact.signal_digest.as_str());
            if fact.signal_sequence != expected_sequence {
                return Err("runner_signal_sequence_invalid");
            }
            expected_sequence += 1;
            match previous {
                None => {
                    if fact.state != AutomationSignalState::Paused {
                        return Err("runner_signal_transition_refused");
                    }
                }
                Some(previous) => {
                    if previous.state == fact.state
                        || !automation_signal_transition_allowed(previous.state, fact.state)
                    {
                        return Err("runner_signal_transition_refused");
                    }
                }
            }
            if fact.authority_epoch < highest_epoch {
                return Err("runner_signal_authority_epoch_stale");
            }
            if fact.authority_epoch > highest_epoch && fact.state != AutomationSignalState::Resumed
            {
                return Err("runner_signal_authority_epoch_ahead");
            }
            if fact.checkpoint_sequence != identity.checkpoint_sequence {
                return Err("runner_signal_checkpoint_unknown");
            }
            highest_epoch = fact.authority_epoch;
            previous = Some(fact);
        }
        Ok(match previous {
            None => RunnerSignalDirective::Continue,
            Some(fact) => match fact.state {
                AutomationSignalState::Paused => RunnerSignalDirective::Hold,
                AutomationSignalState::Resumed => RunnerSignalDirective::Resume {
                    authority_epoch: fact.authority_epoch,
                    checkpoint_sequence: fact.checkpoint_sequence,
                },
                AutomationSignalState::Consumed => RunnerSignalDirective::Closed,
            },
        })
    }
}

/// runner 身份必须完整可用：非空 ID、合法摘要、非零 epoch/checkpoint。
fn valid_identity(identity: &RunnerSignalIdentity) -> bool {
    valid_text(&identity.execution_id)
        && valid_text(&identity.owner_id)
        && valid_digest(&identity.definition_digest)
        && valid_digest(&identity.action_digest)
        && valid_digest(&identity.path_scope_digest)
        && identity.authority_epoch != 0
        && identity.checkpoint_sequence != 0
}

/// 检查 ID 类字段是否为非空、无空白、无 NUL/换行且不超过 512 UTF-8 字节的文本。
fn valid_text(value: &str) -> bool {
    !value.trim().is_empty()
        && value.len() <= 512
        && !value.contains(['\0', '\n', '\r'])
        && !value.chars().any(char::is_whitespace)
}

/// 检查摘要是否为 `sha256:` 前缀加 64 个十六进制字符；不验证摘要来源。
fn valid_digest(value: &str) -> bool {
    value
        .strip_prefix("sha256:")
        .is_some_and(|hex| hex.len() == 64 && hex.bytes().all(|byte| byte.is_ascii_hexdigit()))
}

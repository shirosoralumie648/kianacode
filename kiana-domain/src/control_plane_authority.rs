//! CP-29 确定性 authority / 状态机证据合同。
//!
//! # 文件职责与数据流
//!
//! 本文件把 ControlPlane 的命令、重放和崩溃边界整理成可验证的 Domain 数据结构。它不是
//! 运行时控制器，而是对已经提交的事实/fixture 做 fail-closed 校验：
//!
//! ```text
//! ControlPlane
//!   -> TransitionBatch / Journal / EventLog 写入事实
//!   -> 证据投影收集 Cp29CommandFact 与 Cp29CrashObservation
//!   -> Cp29AuthorityScenario 聚合 online/replay fold digest
//!   -> 【当前文件】校验状态迁移、权限/预算单调性、幂等和 crash matrix
//!   -> Core 只读门面 / CI guard 消费 Ok 或稳定错误码
//! ```
//!
//! 校验成功只说明输入证据满足 CP-29 合同，不会启动进程、杀死子进程、调用 Broker/Runner/
//! provider，也不会把模拟崩溃提升成 runtime、durable、live 或 physical 证明。真正的授权和
//! 事实权威仍是现有 `ControlPlane`、`TransitionBatch` 与 EventLog；不要把这里的 `digest`
//! 或 `ResultUnknown` 字段当作签名、自动恢复或外部动作确认。
//!
//! # 读者应先掌握的三个层次
//!
//! 1. `Cp29CommandFact` 描述一个命令从 `state_before` 到 `state_after` 的单步事实，并约束
//!    authority epoch、权限等级、预算、effect 数量和 replay 行为；
//! 2. `Cp29CrashObservation` 描述某个固定 crash point 看见的 effect/终态关系，尤其要求
//!    已开始但未确认的 effect 进入 `ResultUnknown` 并保持 resource fence；
//! 3. `Cp29AuthorityScenario` 把连续 command facts、七个 crash point 以及 online/replay
//!    fold 聚合起来，拒绝缺点、重复、回滚、payload drift 和摘要漂移。
//!
//! ⚠ 不要在本文件新增第二条执行循环或 fake crash runner。该合同只消费事实；如果它开始
//! 自己执行副作用，CI fixture 就会变成第二个事实源，无法证明真正的 ControlPlane 行为。

use crate::json_digest;
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::collections::{BTreeMap, BTreeSet};

pub const CONTROL_PLANE_AUTHORITY_SCHEMA: &str = "kiana.control-plane-authority.v1";
pub const CONTROL_PLANE_COMMAND_FACT_SCHEMA: &str = "kiana.control-plane-command-fact.v1";
pub const CONTROL_PLANE_CRASH_OBSERVATION_SCHEMA: &str = "kiana.control-plane-crash-observation.v1";

/// 检查需要非空、长度受限且不含控制字符的 ID/引用文本。
///
/// 这是字段形状检查，不是身份认证或权限检查。返回的 `field` 会作为稳定错误码的一部分
/// 向 Core/CI 传播；调用者不能把通过这个 helper 误解为“来源可信”。
fn required(value: &str, field: &'static str) -> Result<(), String> {
    if value.trim().is_empty() || value.len() > 16_384 || value.contains(['\0', '\r', '\n']) {
        Err(field.to_owned())
    } else {
        Ok(())
    }
}

/// 检查未加密钥 `sha256:<64 位十六进制>` 摘要的字符串形状。
///
/// 真实一致性由各结构的 `canonical_digest()` 重算比较完成；本 helper 不提供签名、来源
/// 证明、授权或 replay protection。
fn digest(value: &str, field: &'static str) -> Result<(), String> {
    let Some(hex) = value.strip_prefix("sha256:") else {
        return Err(field.to_owned());
    };
    if hex.len() == 64 && hex.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        Ok(())
    } else {
        Err(field.to_owned())
    }
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
/// ControlPlane 命令在证据中的生命周期状态。
///
/// `ResultUnknown` 有意不属于 `terminal()`：它表示 effect 结果不能安全确认，必须先通过
/// fencing、恢复或对账重新得到确定结论。终态（Completed/Failed/Cancelled）不能被普通新
/// 命令复活；只有显式 replay 事实才能在场景中再次出现同一命令。
pub enum Cp29LifecycleState {
    /// 命令已排队，尚未开始执行。
    Queued,
    /// 命令正在执行。
    Running,
    /// 命令暂时暂停，可由合法迁移恢复。
    Paused,
    /// 已请求取消，等待取消结果确认。
    CancelRequested,
    /// 已成功完成的终态。
    Completed,
    /// 已失败的终态。
    Failed,
    /// 已确认取消的终态。
    Cancelled,
    /// effect 可能发生但结果未知的保护状态。
    ResultUnknown,
}

impl Cp29LifecycleState {
    /// 判断状态是否为“不应被普通新命令复活”的终态。
    ///
    /// `ResultUnknown` 不在这里，因为它仍需要通过恢复/对账继续推进；把它当作终态会让
    /// 调用方错误地丢弃未确认 effect 的 fencing 信息。
    pub fn terminal(self) -> bool {
        matches!(self, Self::Completed | Self::Failed | Self::Cancelled)
    }
}

/// 判断一条非 replay 的生命周期迁移是否在 CP-29 合同中允许。
///
/// 这个列表是状态机的白名单，而不是建议值。校验器拒绝列表外迁移，防止例如从终态直接回到
/// Running、从 Queued 直接伪造 Completed，或绕过 `CancelRequested` 声称已取消。新增状态时
/// 必须同步这里、`terminal()`、command fixture 和 scenario 校验。
fn transition_allowed(from: Cp29LifecycleState, to: Cp29LifecycleState) -> bool {
    use Cp29LifecycleState::*;
    matches!(
        (from, to),
        (Queued, Running)
            | (Queued, Paused)
            | (Queued, CancelRequested)
            | (Queued, Failed)
            | (Running, Paused)
            | (Running, CancelRequested)
            | (Running, Completed)
            | (Running, Failed)
            | (Running, ResultUnknown)
            | (Paused, Running)
            | (Paused, CancelRequested)
            | (Paused, Failed)
            | (Paused, ResultUnknown)
            | (CancelRequested, Cancelled)
            | (CancelRequested, ResultUnknown)
            | (ResultUnknown, Paused)
    )
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
/// CP-29 规定必须覆盖的七个崩溃/中断观察点。
///
/// 它们按“写帧、同步、审批、准备、分发、结果、重放”覆盖一次命令从事实落盘到 effect
/// 结果确认的关键边界。`Cp29AuthorityScenario::validate` 要求七个值恰好各出现一次；缺少
/// 某点或重复某点都不能被报告为完整 crash matrix。
pub enum Cp29CrashPoint {
    /// 写入事件帧之后、同步之前的边界。
    WriteFrame,
    /// 持久化同步边界。
    Sync,
    /// 审批消费边界。
    Approval,
    /// effect 准备边界。
    Prepare,
    /// effect 分发边界。
    Dispatch,
    /// 结果确认边界。
    Result,
    /// 重放/恢复边界。
    Replay,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
/// 一条 ControlPlane 命令事实（command fact）。
///
/// 它描述一个命令在连续事实流中的位置和前后状态，并把授权代次、权限等级、预算、effect
/// 次数及 replay 标记一起固定下来。`validate` 使用这些字段检查“权限只能收紧、预算不能
/// 超支、终态不能复活、重放不能产生第二个 effect”。该结构是证据，不会自行执行命令。
pub struct Cp29CommandFact {
    /// 固定的 command-fact schema 版本。
    pub schema: String,
    /// 在当前 scenario 中从 1 开始连续递增的事实序号。
    pub sequence: u64,
    /// 业务命令 ID；重复出现时必须明确标记为 replay。
    pub command_id: String,
    /// 命令 payload 的内容摘要，用于检测同 ID 的 payload drift。
    pub command_digest: String,
    /// 本事实应用前的生命周期状态。
    pub state_before: Cp29LifecycleState,
    /// 本事实应用后的生命周期状态。
    pub state_after: Cp29LifecycleState,
    /// 授权规则的代次；scenario 中不能倒退。
    pub authority_epoch: u64,
    /// 应用前权限等级；数字越大/越小的业务含义由上层约定，但本合同只允许 after 不变大。
    pub permission_rank_before: u32,
    /// 应用后权限等级；只能保持或收紧，不能借命令提升权限。
    pub permission_rank_after: u32,
    /// 当前命令允许使用的预算上限。
    pub budget_limit: u64,
    /// 已预留预算；不能超过 `budget_limit`，scenario 中不能回退。
    pub budget_reserved: u64,
    /// 已结算预算；不能超过预留预算，scenario 中不能回退。
    pub budget_settled: u64,
    /// 本事实产生的外部 effect 数量；replay 和同状态事实必须为零。
    pub effect_count: u32,
    /// 是否为对已经出现过的 command ID 的幂等重放。
    pub replayed: bool,
    /// 是否明确表示结果未知；必须与 `state_after == ResultUnknown` 同步。
    pub result_unknown: bool,
    /// 对上述字段（不含自身 digest）重算的内容摘要。
    pub digest: String,
}

impl Cp29CommandFact {
    /// 构造一条 command fact，并计算 canonical digest。
    ///
    /// 【调用者】
    /// EventLog/TransitionBatch 的证据投影、Domain fixture 或 replay 适配层在已经知道一条
    /// 命令前后事实后调用。构造不会验证状态迁移、预算和序列，也不会写入 EventLog；完整
    /// 合同由本事实的 `validate` 和外层 scenario `validate` 共同完成。
    ///
    /// 【为什么参数较多】
    /// authority epoch、permission、budget、effect、replay 和 unknown 是不同的安全维度。
    /// 把它们合成一个“成功”布尔值会隐藏权限扩大、预算回滚或重放产生副作用等错误。
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        sequence: u64,
        command_id: impl Into<String>,
        command_digest: impl Into<String>,
        state_before: Cp29LifecycleState,
        state_after: Cp29LifecycleState,
        authority_epoch: u64,
        permission_rank_before: u32,
        permission_rank_after: u32,
        budget_limit: u64,
        budget_reserved: u64,
        budget_settled: u64,
        effect_count: u32,
        replayed: bool,
        result_unknown: bool,
    ) -> Self {
        let mut fact = Self {
            schema: CONTROL_PLANE_COMMAND_FACT_SCHEMA.to_owned(),
            sequence,
            command_id: command_id.into(),
            command_digest: command_digest.into(),
            state_before,
            state_after,
            authority_epoch,
            permission_rank_before,
            permission_rank_after,
            budget_limit,
            budget_reserved,
            budget_settled,
            effect_count,
            replayed,
            result_unknown,
            digest: String::new(),
        };
        fact.digest = fact.canonical_digest();
        fact
    }

    /// 校验一条 command fact 的形状、digest、状态迁移和单步安全不变量。
    ///
    /// 【检查内容】
    /// - schema/sequence/authority epoch 非零；权限等级只能不增加，预留/结算预算不能越界；
    /// - `ResultUnknown` 与 `result_unknown` 必须双向一致；
    /// - 普通命令不能从 terminal 复活，replay 不能有 effect；
    /// - 前后状态变化必须在 `transition_allowed` 白名单中，同状态不能伪造 effect。
    ///
    /// 【失败处理】
    /// 返回稳定错误码，不会回滚事实、重试命令、发送取消信号或自动修复 digest。scenario
    /// 会在更高层继续检查序列连续性、跨事实单调性和 command ID 幂等性。
    pub fn validate(&self) -> Result<(), String> {
        if self.schema != CONTROL_PLANE_COMMAND_FACT_SCHEMA
            || self.sequence == 0
            || self.authority_epoch == 0
            || self.permission_rank_after > self.permission_rank_before
            || self.budget_reserved > self.budget_limit
            || self.budget_settled > self.budget_reserved
        {
            return Err("cp29_command_fact_header_or_monotonicity_invalid".to_owned());
        }
        required(&self.command_id, "cp29_command_id_required")?;
        digest(&self.command_digest, "cp29_command_digest_invalid")?;
        digest(&self.digest, "cp29_command_fact_digest_invalid")?;
        if self.digest != self.canonical_digest() {
            return Err("cp29_command_fact_digest_mismatch".to_owned());
        }
        if self.state_after == Cp29LifecycleState::ResultUnknown && !self.result_unknown {
            return Err("cp29_unknown_state_requires_unknown_fact".to_owned());
        }
        if self.result_unknown && self.state_after != Cp29LifecycleState::ResultUnknown {
            return Err("cp29_unknown_fact_state_mismatch".to_owned());
        }
        if self.state_before.terminal() && !self.replayed {
            return Err("cp29_terminal_cannot_revive".to_owned());
        }
        if self.replayed && self.effect_count != 0 {
            return Err("cp29_replay_effect_count_nonzero".to_owned());
        }
        if self.state_before != self.state_after
            && !transition_allowed(self.state_before, self.state_after)
        {
            return Err("cp29_illegal_state_transition".to_owned());
        }
        if self.state_before == self.state_after && self.effect_count > 0 {
            return Err("cp29_same_state_effect_without_transition".to_owned());
        }
        Ok(())
    }

    /// 计算不包含自身 `digest` 字段的 command fact canonical digest。
    ///
    /// 摘要用于检测内容被修改或同 command ID 的 payload drift；它是未加密钥的完整性指纹，
    /// 不是授权签名。修改任意参与字段后，调用方必须重新计算，否则 `validate` 会拒绝。
    pub fn canonical_digest(&self) -> String {
        json_digest(&json!({
            "schema": self.schema,
            "sequence": self.sequence,
            "command_id": self.command_id,
            "command_digest": self.command_digest,
            "state_before": self.state_before,
            "state_after": self.state_after,
            "authority_epoch": self.authority_epoch,
            "permission_rank_before": self.permission_rank_before,
            "permission_rank_after": self.permission_rank_after,
            "budget_limit": self.budget_limit,
            "budget_reserved": self.budget_reserved,
            "budget_settled": self.budget_settled,
            "effect_count": self.effect_count,
            "replayed": self.replayed,
            "result_unknown": self.result_unknown,
        }))
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
/// 某个固定 crash point 的 effect/终态观察。
///
/// 这条记录回答“崩溃发生在流程哪一步、effect 是否已经开始/确认、恢复后应该相信什么”。
/// 最关键的保护是：effect 已开始但未确认时，最终状态必须是 `ResultUnknown` 且资源必须
/// fenced；否则恢复器可能把不确定的外部动作误判为成功，或重复发送 effect。
pub struct Cp29CrashObservation {
    /// 固定的 crash-observation schema 版本。
    pub schema: String,
    /// 观察所属的七个标准 crash point 之一。
    pub point: Cp29CrashPoint,
    /// 崩溃前 effect 是否已经开始。
    pub effect_started: bool,
    /// 外部 effect 是否已有可确认的结果。
    pub effect_confirmed: bool,
    /// 在该观察点之后记录到的生命周期状态。
    pub final_state: Cp29LifecycleState,
    /// 是否已对可能重复执行的资源进行 fencing。
    pub resource_fenced: bool,
    /// 是否观察到重复 effect；只要为 true，观察就拒绝。
    pub duplicate_effect: bool,
    /// 产生该观察的事实/fixture 引用，不能为空。
    pub source_ref: String,
    /// 对本观察字段重算出的内容摘要。
    pub digest: String,
}

impl Cp29CrashObservation {
    /// 构造一条 crash observation，并默认把 `duplicate_effect` 设为 false。
    ///
    /// 构造只记录调用方提供的观察值，不推断 fencing，也不把 `effect_started` 自动转换为
    /// Unknown；这些安全关系必须由 `validate` 显式检查。它不会真的触发 crash、杀进程或
    /// 读取外部资源。
    pub fn new(
        point: Cp29CrashPoint,
        effect_started: bool,
        effect_confirmed: bool,
        final_state: Cp29LifecycleState,
        resource_fenced: bool,
        source_ref: impl Into<String>,
    ) -> Self {
        let mut observation = Self {
            schema: CONTROL_PLANE_CRASH_OBSERVATION_SCHEMA.to_owned(),
            point,
            effect_started,
            effect_confirmed,
            final_state,
            resource_fenced,
            duplicate_effect: false,
            source_ref: source_ref.into(),
            digest: String::new(),
        };
        observation.digest = observation.canonical_digest();
        observation
    }

    /// 校验 crash observation 的 schema、摘要和 effect/状态/fencing 关系。
    ///
    /// 【关键规则】
    /// 1. 已开始但未确认的 effect 必须是 `ResultUnknown` 且 `resource_fenced=true`；
    /// 2. 已确认的 effect 不能仍然声称 Unknown；
    /// 3. effect 根本未开始时不能声称 Completed；
    /// 4. `duplicate_effect` 永远拒绝，避免把第二次外部动作当成正常恢复。
    ///
    /// 失败只返回错误码，不会自动 fence 资源或发起补偿；真正的恢复动作属于 Runner/
    /// ControlPlane 上层，并且必须依据 EventLog 事实。
    pub fn validate(&self) -> Result<(), String> {
        if self.schema != CONTROL_PLANE_CRASH_OBSERVATION_SCHEMA || self.duplicate_effect {
            return Err("cp29_crash_observation_header_or_duplicate_invalid".to_owned());
        }
        required(&self.source_ref, "cp29_crash_source_ref_required")?;
        digest(&self.digest, "cp29_crash_digest_invalid")?;
        if self.digest != self.canonical_digest() {
            return Err("cp29_crash_digest_mismatch".to_owned());
        }
        if self.effect_started && !self.effect_confirmed {
            if self.final_state != Cp29LifecycleState::ResultUnknown || !self.resource_fenced {
                return Err("cp29_crash_started_effect_requires_unknown_fence".to_owned());
            }
        }
        if self.effect_confirmed && self.final_state == Cp29LifecycleState::ResultUnknown {
            return Err("cp29_crash_confirmed_effect_cannot_remain_unknown".to_owned());
        }
        if !self.effect_started && self.final_state == Cp29LifecycleState::Completed {
            return Err("cp29_crash_not_started_cannot_complete".to_owned());
        }
        Ok(())
    }

    /// 计算不包含自身 `digest` 字段的 crash observation canonical digest。
    ///
    /// 该摘要帮助检测观察字段漂移，但不是 crash 证明、签名或进程存活证明。
    pub fn canonical_digest(&self) -> String {
        json_digest(&json!({
            "schema": self.schema,
            "point": self.point,
            "effect_started": self.effect_started,
            "effect_confirmed": self.effect_confirmed,
            "final_state": self.final_state,
            "resource_fenced": self.resource_fenced,
            "duplicate_effect": self.duplicate_effect,
            "source_ref": self.source_ref,
        }))
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
/// CP-29 authority 场景的完整证据集合。
///
/// scenario 把一段连续的 command facts、七个 crash observations，以及在线投影和确定性
/// replay 的 fold digest 放在一起。它负责跨记录检查：序列是否连续、authority/budget 是否
/// 单调、command ID 重放是否保持同一 payload、terminal 是否复活、crash point 是否完整，
/// 以及 online/replay 是否收敛到同一状态摘要。
pub struct Cp29AuthorityScenario {
    /// 固定的场景 schema 版本。
    pub schema: String,
    /// 场景 ID，用于把一组事实和 fixture 归在一起。
    pub scenario_id: String,
    /// 按 sequence 排列的命令事实，数量上限为 4096。
    pub command_facts: Vec<Cp29CommandFact>,
    /// 七个 crash point 的观察，数量上限为 64。
    pub crash_observations: Vec<Cp29CrashObservation>,
    /// 在线状态投影计算出的 fold digest。
    pub online_digest: String,
    /// 从同一 command facts 确定性重放得到的 fold digest。
    pub replay_digest: String,
    /// 对场景字段（不含自身 digest）重算出的内容摘要。
    pub digest: String,
}

impl Cp29AuthorityScenario {
    /// 构造 authority scenario，并计算场景 canonical digest。
    ///
    /// `online_digest` 与 `replay_digest` 由调用方提供，因为本合同要比较两条已有投影；构造
    /// 阶段不会替调用方执行 replay，也不会自动填充七个 crash point。空事实、重复 point、
    /// digest 漂移和跨记录回滚都留给 `validate` 返回明确错误。
    pub fn new(
        scenario_id: impl Into<String>,
        command_facts: Vec<Cp29CommandFact>,
        crash_observations: Vec<Cp29CrashObservation>,
        online_digest: impl Into<String>,
        replay_digest: impl Into<String>,
    ) -> Self {
        let mut scenario = Self {
            schema: CONTROL_PLANE_AUTHORITY_SCHEMA.to_owned(),
            scenario_id: scenario_id.into(),
            command_facts,
            crash_observations,
            online_digest: online_digest.into(),
            replay_digest: replay_digest.into(),
            digest: String::new(),
        };
        scenario.digest = scenario.canonical_digest();
        scenario
    }

    /// 验证整个 authority scenario 的跨记录不变量。
    ///
    /// 【处理顺序】
    /// 1. 检查 schema、非空/数量上限和 scenario ID；
    /// 2. 检查 online/replay digest 都是合法摘要、彼此相等，并等于 command facts 的 fold；
    /// 3. 检查 scenario canonical digest；
    /// 4. 按 sequence 遍历 facts，复用单事实校验，并拒绝序列间隙、authority/budget 回滚、
    ///    terminal 复活、同 ID payload drift、首次 command 伪造 replay；
    /// 5. 校验每条 crash observation，拒绝重复 point，并要求七个标准 point 恰好完整覆盖。
    ///
    /// 【失败与副作用】
    /// 任一步失败都返回稳定错误码，不会重放命令、启动恢复、fence 资源、写事件或修改输入。
    /// “在线和重放 digest 相等”只说明 supplied facts 的确定性投影相同，不能证明生产投影器
    /// 已被完整运行，更不能把 source fixture 提升为 durable/live/physical 证据。
    pub fn validate(&self) -> Result<(), String> {
        if self.schema != CONTROL_PLANE_AUTHORITY_SCHEMA
            || self.command_facts.is_empty()
            || self.command_facts.len() > 4_096
            || self.crash_observations.is_empty()
            || self.crash_observations.len() > 64
        {
            return Err("cp29_scenario_header_invalid".to_owned());
        }
        required(&self.scenario_id, "cp29_scenario_id_required")?;
        digest(&self.online_digest, "cp29_online_digest_invalid")?;
        digest(&self.replay_digest, "cp29_replay_digest_invalid")?;
        if self.online_digest != self.replay_digest
            || self.online_digest != Self::fold_digest(&self.command_facts)
        {
            return Err("cp29_online_replay_digest_mismatch".to_owned());
        }
        digest(&self.digest, "cp29_scenario_digest_invalid")?;
        if self.digest != self.canonical_digest() {
            return Err("cp29_scenario_digest_mismatch".to_owned());
        }

        let mut command_digests = BTreeMap::new();
        let mut last_epoch = 0;
        let mut previous_reserved = 0;
        let mut previous_settled = 0;
        let mut previous_state: Option<Cp29LifecycleState> = None;
        for (index, fact) in self.command_facts.iter().enumerate() {
            fact.validate()?;
            if fact.sequence != (index as u64).saturating_add(1) {
                return Err("cp29_command_sequence_gap".to_owned());
            }
            if fact.authority_epoch < last_epoch
                || fact.budget_reserved < previous_reserved
                || fact.budget_settled < previous_settled
            {
                return Err("cp29_authority_or_budget_rollback".to_owned());
            }
            if let Some(previous) = previous_state {
                if previous.terminal() && !fact.replayed {
                    return Err("cp29_terminal_cannot_revive".to_owned());
                }
            }
            if let Some(original_digest) = command_digests.get(&fact.command_id) {
                if original_digest != &fact.command_digest || !fact.replayed {
                    return Err("cp29_command_idempotency_or_payload_drift".to_owned());
                }
            } else if fact.replayed {
                return Err("cp29_first_command_cannot_be_replay".to_owned());
            } else {
                command_digests.insert(fact.command_id.clone(), fact.command_digest.clone());
            }
            last_epoch = fact.authority_epoch;
            previous_reserved = fact.budget_reserved;
            previous_settled = fact.budget_settled;
            previous_state = Some(fact.state_after);
        }

        let mut points = BTreeSet::new();
        for observation in &self.crash_observations {
            observation.validate()?;
            if !points.insert(observation.point) {
                return Err("cp29_crash_point_duplicate".to_owned());
            }
        }
        let expected_points = [
            Cp29CrashPoint::WriteFrame,
            Cp29CrashPoint::Sync,
            Cp29CrashPoint::Approval,
            Cp29CrashPoint::Prepare,
            Cp29CrashPoint::Dispatch,
            Cp29CrashPoint::Result,
            Cp29CrashPoint::Replay,
        ]
        .into_iter()
        .collect::<BTreeSet<_>>();
        if points != expected_points {
            return Err("cp29_crash_matrix_incomplete".to_owned());
        }
        Ok(())
    }

    /// 从 command facts 的最终状态和逐条 fact digest 计算确定性 fold digest。
    ///
    /// 它把最后序号、终态、authority/permission/budget、累计 effect、是否出现 Unknown 以及
    /// 所有 fact digest 放入一个 canonical JSON。空切片也有确定输出，便于调用方比较；但
    /// `validate()` 会先拒绝空 command_facts，因此空 fold 不能单独构成有效 scenario。
    /// 该摘要是内容一致性比较，不是签名或在线状态真实性证明。
    pub fn fold_digest(command_facts: &[Cp29CommandFact]) -> String {
        json_digest(&json!({
            "last_sequence": command_facts.last().map(|fact| fact.sequence).unwrap_or(0),
            "final_state": command_facts.last().map(|fact| fact.state_after),
            "authority_epoch": command_facts.last().map(|fact| fact.authority_epoch).unwrap_or(0),
            "permission_rank": command_facts.last().map(|fact| fact.permission_rank_after).unwrap_or(0),
            "budget_limit": command_facts.last().map(|fact| fact.budget_limit).unwrap_or(0),
            "budget_reserved": command_facts.last().map(|fact| fact.budget_reserved).unwrap_or(0),
            "budget_settled": command_facts.last().map(|fact| fact.budget_settled).unwrap_or(0),
            "effect_count": command_facts.iter().map(|fact| u64::from(fact.effect_count)).sum::<u64>(),
            "unknown_fence": command_facts.iter().any(|fact| fact.result_unknown),
            "command_facts": command_facts.iter().map(|fact| &fact.digest).collect::<Vec<_>>(),
        }))
    }

    /// 计算不包含自身 `digest` 字段的 scenario canonical digest。
    ///
    /// command facts、crash observations 和 online/replay digest 都参与摘要。任何事实、限制
    /// 或投影摘要发生变化后必须重算；复用旧摘要会被 `validate()` 拒绝。
    pub fn canonical_digest(&self) -> String {
        json_digest(&json!({
            "schema": self.schema,
            "scenario_id": self.scenario_id,
            "command_facts": self.command_facts,
            "crash_observations": self.crash_observations,
            "online_digest": self.online_digest,
            "replay_digest": self.replay_digest,
        }))
    }
}

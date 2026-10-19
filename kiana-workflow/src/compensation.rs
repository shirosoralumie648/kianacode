//! AUT-20 补偿准入规划：把“补偿计划 DTO”接到既有 durable planner，而不是另起执行引擎。
//!
//! # 在系统中的位置
//!
//! ```text
//! 调用方（Core / 受控服务）准备 AutomationCompensationPlan（原实例/执行/attempt/动作摘要、
//! 独立补偿实例/执行/动作、fresh authorization epoch）
//!     -> plan_compensation_admission(state, plan, authority, proof, live_authority_epoch)
//!        1. plan.validate()                       // 领域静态合同（先拒绝优先）
//!        2. 对照 AutomationState 证明原实例存在且为 Failed/Cancelled
//!        3. 证明原 execution/attempt 确实是该实例上的一次节点尝试
//!        4. 拒绝重复补偿、拒绝没有声明补偿节点
//!        5. 要求补偿执行身份 == 本次 authority 的 execution（不是原执行）
//!        6. 要求补偿 authority epoch == 调用方从权威账本读到的 live epoch
//!        7. 复用 kiana_workflow::plan_command(Compensate) 生成独立补偿实例
//!     -> (next_state, AutomationCompensationAdmission, effect)
//! ```
//!
//! # 为什么复用 `plan_command`
//!
//! 补偿实例的创建、`parent_instance_id` 引用、`selected_nodes` 选择和 `Compensating` 状态都由
//! durable planner 唯一实现；本模块只做准入判定，不复制状态机、不创建第二条执行循环。
//!
//! # 边界与诚实口径
//!
//! * 本模块不派发任何 effect：`Compensate` 分支返回 `None`，补偿节点仍需后续 `Advance` 走
//!   Core → Broker/Harness 的正常授权路径；本切片没有自动推进该实例。
//! * `live_authority_epoch` 是调用方声明；本模块不读取 AuthorityLedger。Core 侧应传入
//!   `ControlPlane::observed_authority_epoch()` 的结果，本函数只保证“计划声明的代次必须等于
//!   调用方提供的当前代次，且严格晚于原授权代次”，无法证明该 epoch 由权威系统签发。
//! * `fresh_authorization_digest` 只是调用方提供的摘要；本模块不验证审批事件、scope 或预算。
//! * 原执行存在性只对照内存状态投影，不查 EventStore/执行账本；状态本身须来自已提交事实。
//! * 补偿实例的预算/路径/审批仍必须由后续节点 dispatch 重新走 ControlPlane；
//!   `create_instance` 不为补偿实例扣减源实例的剩余 step 预算（补偿是独立实例，不是嵌套
//!   subworkflow），这一点在本切片中修正，见 `create_instance(..., charge_parent_budget)`。

use crate::durable::{definition_key, plan_command};
use kiana_domain::{
    json_digest, AutomationAuthority, AutomationCommand, AutomationCompensationPlan,
    AutomationProof, AutomationState, WorkflowEffect, WorkflowInstanceStatus, WorkflowNodeStatus,
};
use serde::{Deserialize, Serialize};

/// 补偿准入事实的固定 wire schema 名称。
pub const AUTOMATION_COMPENSATION_ADMISSION_SCHEMA: &str =
    "kiana.automation-compensation-admission.v1";

/// 源实例在状态中不存在时的稳定拒绝码。
pub const AUTOMATION_COMPENSATION_ORIGINAL_INSTANCE_MISSING: &str =
    "automation_compensation_original_instance_missing";
/// 源实例不是已知的 Failed/Cancelled 终态（包括 Succeeded/Unknown/运行中）时的稳定拒绝码。
pub const AUTOMATION_COMPENSATION_ORIGINAL_NOT_TERMINAL: &str =
    "automation_compensation_original_not_terminal";
/// 计划引用的原执行在源实例的节点尝试中不存在时的稳定拒绝码。
pub const AUTOMATION_COMPENSATION_ORIGINAL_ATTEMPT_MISSING: &str =
    "automation_compensation_original_attempt_missing";
/// 原执行存在但 attempt 序号不一致时的稳定拒绝码。
pub const AUTOMATION_COMPENSATION_ORIGINAL_ATTEMPT_MISMATCH: &str =
    "automation_compensation_original_attempt_mismatch";
/// 源实例已经有补偿子实例时的稳定拒绝码。
pub const AUTOMATION_COMPENSATION_ALREADY_REQUESTED: &str =
    "automation_compensation_already_requested";
/// 已成功节点没有声明任何补偿节点时的稳定拒绝码。
pub const AUTOMATION_COMPENSATION_NOT_DECLARED: &str = "automation_compensation_not_declared";
/// 补偿执行身份不是本次 authority 提供的 fresh execution 时的稳定拒绝码。
pub const AUTOMATION_COMPENSATION_EXECUTION_IDENTITY_MISMATCH: &str =
    "automation_compensation_execution_identity_mismatch";
/// 计划声明的 authority epoch 与调用方提供的当前授权代次不一致时的稳定拒绝码。
pub const AUTOMATION_COMPENSATION_AUTHORIZATION_STALE: &str =
    "automation_compensation_authorization_stale";
/// 规划后源实例被意外改写时的稳定拒绝码（防御性断言）。
pub const AUTOMATION_COMPENSATION_ORIGINAL_MUTATED: &str =
    "automation_compensation_original_mutated";

/// 一次补偿准入的事实记录；它不执行补偿，只说明基于哪个快照接受了哪份计划。
///
/// `decision_digest` 绑定计划摘要、源实例 revision、原执行/attempt、补偿实例/执行、授权代次
/// 和选中的补偿节点。它是无密钥内容摘要，用于审计关联与重放身份，不是签名或执行回执。
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AutomationCompensationAdmission {
    /// 必须等于 `AUTOMATION_COMPENSATION_ADMISSION_SCHEMA`。
    pub schema: String,
    /// 被接受的计划自摘要；与 `AutomationCompensationPlan::plan_digest` 相同。
    pub plan_digest: String,
    /// 源 workflow instance ID。
    pub original_instance_id: String,
    /// 源节点执行 ID。
    pub original_execution_id: String,
    /// 源节点 attempt 序号。
    pub original_attempt: u32,
    /// 原授权 epoch（来自计划）。
    pub original_authorization_epoch: u64,
    /// 新建的补偿实例 ID。
    pub compensation_instance_id: String,
    /// 补偿执行身份；等于本次 authority 的 execution ID。
    pub compensation_execution_id: String,
    /// 补偿授权 epoch；等于调用方提供的 live epoch。
    pub authority_epoch: u64,
    /// 补偿计划声明的 fresh authorization 摘要。
    pub fresh_authorization_digest: String,
    /// 由源实例已成功节点派生、按字典序排列的补偿节点。
    pub selected_nodes: Vec<String>,
    /// 规划前的源聚合 revision。
    pub source_revision: u64,
    /// 对上方字段的 SHA-256 内容摘要。
    pub decision_digest: String,
}

impl AutomationCompensationAdmission {
    /// 校验 schema、身份文本、选中节点顺序与自摘要。
    ///
    /// 只检查准入事实内部一致性；不证明补偿已执行、授权已签发或原实例仍可补偿。
    pub fn validate(&self) -> Result<(), &'static str> {
        if self.schema != AUTOMATION_COMPENSATION_ADMISSION_SCHEMA
            || self.plan_digest.is_empty()
            || self.original_instance_id.trim().is_empty()
            || self.original_execution_id.trim().is_empty()
            || self.original_attempt == 0
            || self.compensation_instance_id.trim().is_empty()
            || self.compensation_execution_id.trim().is_empty()
            || self.authority_epoch == 0
            || self.selected_nodes.is_empty()
            || self
                .selected_nodes
                .windows(2)
                .any(|pair| pair[0] >= pair[1])
            || self.decision_digest != self.digest()
        {
            return Err("automation_compensation_admission_invalid");
        }
        Ok(())
    }

    fn digest(&self) -> String {
        json_digest(&serde_json::json!({
            "schema": self.schema,
            "plan_digest": self.plan_digest,
            "original_instance_id": self.original_instance_id,
            "original_execution_id": self.original_execution_id,
            "original_attempt": self.original_attempt,
            "original_authorization_epoch": self.original_authorization_epoch,
            "compensation_instance_id": self.compensation_instance_id,
            "compensation_execution_id": self.compensation_execution_id,
            "authority_epoch": self.authority_epoch,
            "fresh_authorization_digest": self.fresh_authorization_digest,
            "selected_nodes": self.selected_nodes,
            "source_revision": self.source_revision,
        }))
    }
}

/// 对照当前状态与 live 授权代次，决定是否允许把这份补偿计划登记成独立补偿实例。
///
/// 【调用者】Core 只读门面 `admit_compensation_plan()` 转发；当前没有生产调用者。
///
/// 【输入】`state` 是 EventStore 折叠出的当前状态（调用方负责真实性）；`plan` 是调用方封口的
/// 补偿计划；`authority` 是本次命令的 actor/role/session/时间/execution 快照；`proof` 原样交给
/// planner；`live_authority_epoch` 必须是调用方从权威授权账本读取的当前代次（> 0）。
///
/// 【拒绝顺序】计划结构 → 源实例存在 → 源实例 Failed/Cancelled → 补偿执行身份 →
/// 原执行/attempt 匹配 → 已有补偿 → 已声明补偿节点 → 授权代次新鲜 → planner 拒绝。
/// 每个拒绝面都有稳定错误码；planner 的补偿拒绝码映射为同一稳定集合。
///
/// 【输出】成功时返回新状态、补偿准入事实和 planner 的 effect（`Compensate` 恒为 `None`）。
/// 源实例的节点/状态/depth 不被修改；补偿实例通过 `parent_instance_id` 引用原实例。
///
/// 【副作用】无 EventStore、Broker、runner、文件、时钟或网络操作；纯函数。
pub fn plan_compensation_admission(
    state: &AutomationState,
    plan: &AutomationCompensationPlan,
    authority: &AutomationAuthority,
    proof: &AutomationProof,
    live_authority_epoch: u64,
) -> Result<
    (
        AutomationState,
        AutomationCompensationAdmission,
        Option<WorkflowEffect>,
    ),
    &'static str,
> {
    // 1. 领域静态合同：schema/摘要/身份碰撞/permit 复用/动作未变化/代次单调。
    plan.validate()?;
    // 2. 原实例必须在当前投影中；计划不能凭空引用一个不存在的执行。
    let source = state
        .instances
        .get(&plan.original_instance_id)
        .ok_or(AUTOMATION_COMPENSATION_ORIGINAL_INSTANCE_MISSING)?;
    // 3. 只有已知失败/取消的实例可补偿；Succeeded 无需补偿，ResultUnknown 无法确认 effect 是否发生。
    if !matches!(
        source.status,
        WorkflowInstanceStatus::Failed | WorkflowInstanceStatus::Cancelled
    ) {
        return Err(AUTOMATION_COMPENSATION_ORIGINAL_NOT_TERMINAL);
    }
    // 4. 补偿必须使用本次 fresh authority 的 execution 身份，不能把原执行 ID 当补偿执行。
    if plan.compensation_execution_id != authority.execution_id.to_string() {
        return Err(AUTOMATION_COMPENSATION_EXECUTION_IDENTITY_MISMATCH);
    }
    // 5. 原执行/attempt 必须是源实例上真实存在的节点尝试，防止把任意 UUID 当成“原执行”。
    let original_attempt = source
        .nodes
        .values()
        .find(|execution| execution.execution_id.to_string() == plan.original_execution_id)
        .ok_or(AUTOMATION_COMPENSATION_ORIGINAL_ATTEMPT_MISSING)?;
    if original_attempt.attempt != plan.original_attempt {
        return Err(AUTOMATION_COMPENSATION_ORIGINAL_ATTEMPT_MISMATCH);
    }
    // 6. 同一源实例只允许一次补偿请求（与 planner 的 already_requested 检查语义一致）。
    if state.instances.values().any(|instance| {
        instance.parent_instance_id.as_deref() == Some(plan.original_instance_id.as_str())
            && instance.selected_nodes.is_some()
    }) {
        return Err(AUTOMATION_COMPENSATION_ALREADY_REQUESTED);
    }
    // 7. 必须先有已成功节点且该节点声明了补偿节点；否则补偿实例无事可做。
    let definition = state
        .definitions
        .get(&definition_key(
            &source.definition_id,
            source.definition_version,
        ))
        .ok_or("workflow_definition_not_found")?;
    let mut selected_nodes = source
        .nodes
        .iter()
        .filter(|(_, node)| node.status == WorkflowNodeStatus::Succeeded)
        .filter_map(|(id, _)| definition.nodes.get(id)?.compensation.clone())
        .collect::<Vec<_>>();
    selected_nodes.sort();
    selected_nodes.dedup();
    if selected_nodes.is_empty() {
        return Err(AUTOMATION_COMPENSATION_NOT_DECLARED);
    }
    // 8. 授权必须新鲜：计划代次等于调用方读到的 live 代次，且严格晚于原授权代次
    //    （后者已由领域 validator 保证，这里再显式对照 live 值，防止计划使用旧代次）。
    if live_authority_epoch == 0
        || plan.authority_epoch != live_authority_epoch
        || plan.authority_epoch <= plan.original_authorization_epoch
    {
        return Err(AUTOMATION_COMPENSATION_AUTHORIZATION_STALE);
    }
    // 9. 复用唯一 durable planner 创建独立补偿实例；失败原因映射为稳定补偿码。
    let command = AutomationCommand::Compensate {
        instance_id: plan.original_instance_id.clone(),
        compensation_instance_id: plan.compensation_instance_id.clone(),
    };
    let (next, effect) =
        plan_command(state, &command, authority, proof).map_err(|reason| match reason {
            "workflow_compensation_requires_known_terminal" => {
                AUTOMATION_COMPENSATION_ORIGINAL_NOT_TERMINAL
            }
            "workflow_compensation_already_requested" => AUTOMATION_COMPENSATION_ALREADY_REQUESTED,
            "workflow_compensation_not_defined" => AUTOMATION_COMPENSATION_NOT_DECLARED,
            other => other,
        })?;
    // 10. 源实例只能被引用，不能被补偿规划改写（原事实保留是 AUT-20 的硬约束）。
    if next.instances.get(&plan.original_instance_id) != Some(source) {
        return Err(AUTOMATION_COMPENSATION_ORIGINAL_MUTATED);
    }
    let compensation = next
        .instances
        .get(&plan.compensation_instance_id)
        .ok_or("workflow_compensation_instance_missing")?;
    let planned_selected = compensation
        .selected_nodes
        .clone()
        .ok_or("workflow_compensation_not_defined")?;
    if planned_selected != selected_nodes {
        // planner 由同一源事实派生；不一致说明状态在两次读取间漂移，整体拒绝。
        return Err("automation_compensation_selected_nodes_drift");
    }
    let mut admission = AutomationCompensationAdmission {
        schema: AUTOMATION_COMPENSATION_ADMISSION_SCHEMA.to_owned(),
        plan_digest: plan.plan_digest.clone(),
        original_instance_id: plan.original_instance_id.clone(),
        original_execution_id: plan.original_execution_id.clone(),
        original_attempt: plan.original_attempt,
        original_authorization_epoch: plan.original_authorization_epoch,
        compensation_instance_id: plan.compensation_instance_id.clone(),
        compensation_execution_id: plan.compensation_execution_id.clone(),
        authority_epoch: plan.authority_epoch,
        fresh_authorization_digest: plan.fresh_authorization_digest.clone(),
        selected_nodes,
        source_revision: state.revision,
        decision_digest: String::new(),
    };
    admission.decision_digest = json_digest(&serde_json::json!({
        "schema": admission.schema,
        "plan_digest": admission.plan_digest,
        "original_instance_id": admission.original_instance_id,
        "original_execution_id": admission.original_execution_id,
        "original_attempt": admission.original_attempt,
        "original_authorization_epoch": admission.original_authorization_epoch,
        "compensation_instance_id": admission.compensation_instance_id,
        "compensation_execution_id": admission.compensation_execution_id,
        "authority_epoch": admission.authority_epoch,
        "fresh_authorization_digest": admission.fresh_authorization_digest,
        "selected_nodes": admission.selected_nodes,
        "source_revision": admission.source_revision,
    }));
    admission.validate()?;
    Ok((next, admission, effect))
}

//! 本文件实现持久化 workflow 的纯规划逻辑：校验版本化定义、从当前状态和命令计算下一状态，
//! 并把需要外部执行的动作作为 `WorkflowEffect` 返回给 Core。
//!
//! # 在系统中的位置
//!
//! ```text
//! Core Automation / EventStore 重放
//!     -> 当前 AutomationState + AutomationCommand + Authority + Proof
//!     -> 本文件：校验不变量，生成 next state 和可选 effect
//!     -> Core 先用 CAS 写 workflow.command_applied
//!     -> Core 才把 effect 交给 Company / Capability / cancellation 路径
//! ```
//!
//! `kiana-domain` 定义本文件读写的 workflow DTO 和状态；`kiana-core/src/automation.rs` 是当前
//! 产品路径中的主要生产调用者，负责从 EventStore 读取 state、提供服务端 authority/proof、
//! 提交规划结果并执行 effect。Domain/Workflow 测试也会直接调用 planner。此 crate 不负责
//! EventStore、权限适配、网络、运行时调度或后台定时器。
//!
//! # 纯规划边界
//!
//! `plan_command()` 先克隆输入状态，然后只在副本上计算。因此失败时调用者手里的原 state
//! 不会被部分修改；成功则返回新 revision 和一个可选 effect。相同的状态、命令、authority
//! 快照（特别是 `now_ms`）和 proof 会得到相同规划结果。
//!
//! effect 只是“提交 workflow 事实后 Core 应尝试的动作”，不是许可，也不会在本文件里运行。
//! EventStore CAS 成功与 Company/Capability 副作用分属不同步骤；未知结果不能在 planner 内
//! 自动重试可能已发生的外部动作，Core 会尝试通过 `Reconcile` 读取事实。当前 reducer 把
//! `ResultUnknown` 视为终态，而 `observe()` 拒绝更新终态节点，因此“Unknown 后再由 Reconcile
//! 改成已确认结果”存在状态机阻断，见 `observe()` 的风险说明。
//!
//! Interval trigger 也只是由 `Tick` 命令推进的确定性日程计算。本文件不创建 Tokio task、timer
//! 或 worker；是否有产品 host 自动提交 Tick，要沿 Core/Daemon 的实际装配路径另行确认。
use kiana_domain::*;
use std::collections::{BTreeMap, BTreeSet};

/// planner 内部错误统一使用静态 reason，便于 Core 将其转换成稳定错误码。
type Result<T> = std::result::Result<T, &'static str>;

/// 一次 FanOut 最多允许的直接分支数；限制单次定义扩展的执行面大小。
const MAX_WORKFLOW_FAN_OUT: usize = 32;

/// 嵌套 subworkflow 的最大父子层数，避免递归定义/执行链无限增长。
const MAX_WORKFLOW_DEPTH: u32 = 8;

/// 把一个条件映射为 planner 的稳定错误原因；不记录日志、不修改状态。
///
/// 调用者通过 `?` 将失败原因传回 `plan_command()` 的入口，由 Core 决定是否形成拒绝事件。
fn require(ok: bool, reason: &'static str) -> Result<()> {
    if ok {
        Ok(())
    } else {
        Err(reason)
    }
}

/// 沿实例的 `parent_instance_id` 链判断它是否属于指定祖先实例的 subworkflow 后代。
///
/// 此关系不是节点依赖图：它只回答实例嵌套关系，用于祖先取消传播和运行时 guard。
/// 遇到缺失实例、无父项、父链循环或 `selected_nodes` 补偿实例时返回 false。补偿实例虽然
/// 记录原实例为 parent，但不是原 workflow 的嵌套子流程，因此不会被普通后代遍历当成 child。
pub fn workflow_descends_from(state: &AutomationState, instance_id: &str, ancestor: &str) -> bool {
    let mut cursor = state.instances.get(instance_id);
    let mut seen = BTreeSet::new();
    while let Some(instance) = cursor {
        if !seen.insert(instance.instance_id.clone()) {
            return false;
        }
        let Some(parent) = &instance.parent_instance_id else {
            return false;
        };
        if instance.selected_nodes.is_some() {
            return false;
        }
        if parent == ancestor {
            return true;
        }
        cursor = state.instances.get(parent);
    }
    false
}

/// 检查某个实例的真实 subworkflow 祖先链上，是否没有终态、暂停或取消请求。
///
/// 【调用者】Core 的 workflow 能力守卫与 planner 命令准入会用它阻止已失效父实例下的子流程
/// 继续推进。该判断仅检查祖先实例状态，不检查父实例里某个具体节点是否成功，也不证明外部
/// side effect 已停止。
pub fn workflow_ancestors_active(state: &AutomationState, instance_id: &str) -> bool {
    !state.instances.values().any(|parent| {
        workflow_descends_from(state, instance_id, &parent.instance_id)
            && (parent.status.terminal()
                || matches!(
                    parent.status,
                    WorkflowInstanceStatus::CancelRequested | WorkflowInstanceStatus::Paused
                ))
    })
}

/// 生成 workflow definition 在 `AutomationState.definitions` 中使用的版本键。
///
/// ID 与版本都参与键，因此升级应增加版本并保留旧定义，不能原地替换旧版本内容。
pub fn definition_key(id: &str, version: u64) -> String {
    format!("{id}@{version}")
}

/// 在登记定义或规划引用它之前，检查 workflow DAG、节点策略、输入输出和 artifact 依赖。
///
/// 【调用者】`plan_command(RegisterDefinition)` 在把定义加入状态前调用；实例创建依赖此前已
/// 登记且通过校验的 `(definition_id, version)`。
/// 【输入/输出】读取一个不可变 `WorkflowDefinition`，成功只表示静态结构满足本文件检查；返回
/// 的 `&'static str` 是首个失败原因。这里不会运行节点、访问项目文件或验证实际 packet、权限、
/// capability handler 是否存在。
///
/// 【主要检查】定义/节点标识和时间/步数预算、角色与 schema key、补偿引用、各 node kind 的
/// 必要字段与 authority 注入限制、节点 DAG、FanOut/FanIn 形状、artifact 生成者和 artifact DAG。
/// 所有长度上限使用 Rust `str::len()` 时单位是 UTF-8 字节；定义允许的最长总时长为 86,400,000
/// 毫秒（24 小时），单节点 retry_limit 不大于 3。
///
/// ⚠ 通过这里不等于运行授权。特别是 Capability 节点只要求调用方不能预填 Cell、budget lease
/// 或 grant ID；这些字段仍须由 Core 的实际执行路径按当前 authority 解析和准入。
pub fn validate_definition(d: &WorkflowDefinition) -> Result<()> {
    // 先限制单个定义及其工作量，避免后面的图遍历处理无界输入。
    require(
        !d.definition_id.trim().is_empty()
            && d.definition_id.len() <= 128
            && !d.definition_id.contains(['\0', '\r', '\n'])
            && d.version > 0,
        "workflow_definition_identity_invalid",
    )?;
    require(
        !d.nodes.is_empty() && d.nodes.len() <= 128 && (1..=1024).contains(&d.max_steps),
        "workflow_definition_budget_invalid",
    )?;
    require(
        d.max_duration_ms > 0 && d.max_duration_ms <= 86_400_000,
        "workflow_definition_timeout_invalid",
    )?;
    require(
        !d.allowed_roles.is_empty()
            && d.allowed_roles
                .iter()
                .all(|r| RoleSpec::lookup(r).is_some()),
        "workflow_definition_role_invalid",
    )?;
    require(
        d.input_keys.iter().all(|k| !k.trim().is_empty())
            && d.output_keys.iter().all(|k| !k.trim().is_empty()),
        "workflow_schema_key_invalid",
    )?;
    require(
        d.input_keys.len() <= 256
            && d.output_keys.len() <= 256
            && d.input_keys.windows(2).all(|pair| pair[0] < pair[1])
            && d.output_keys.windows(2).all(|pair| pair[0] < pair[1]),
        "workflow_schema_key_noncanonical",
    )?;

    // 复用 WorkPacket 的 DAG 校验器只借用“节点 ID + dependencies”图形，不会创建或派发任务。
    // 这样 workflow 节点环路和缺失依赖沿用已经存在的领域图约束。
    let mut graph = BTreeMap::new();
    for (id, node) in &d.nodes {
        require(
            !id.trim().is_empty() && id.len() <= 128 && !id.contains(['\0', '\r', '\n']),
            "workflow_node_id_invalid",
        )?;
        require(
            node.timeout_ms > 0 && node.timeout_ms <= d.max_duration_ms && node.retry_limit <= 3,
            "workflow_node_policy_invalid",
        )?;
        if let Some(compensation) = &node.compensation {
            require(
                d.nodes.contains_key(compensation)
                    && compensation != id
                    && d.nodes[compensation].dependencies.is_empty(),
                "workflow_compensation_invalid",
            )?;
        }
        match &node.kind {
            WorkflowNodeKind::AgentTask {
                project_id,
                packet_id,
                ..
            } => require(
                !project_id.trim().is_empty()
                    && project_id.len() <= 256
                    && !project_id.contains(['\0', '\r', '\n'])
                    && !packet_id.trim().is_empty()
                    && packet_id.len() <= 256
                    && !packet_id.contains(['\0', '\r', '\n']),
                "workflow_packet_required",
            )?,
            WorkflowNodeKind::Capability { request } => require(
                // Definition 不能携带一份可重放的能力 grant/Cell/预算 lease。Core 后续必须从
                // 当前请求上下文重新计算权限；operation 不能为空，但具体能否执行由 Broker 决定。
                request.cell_id.is_none()
                    && request.budget_lease_id.is_none()
                    && request.capability_grant_id.is_none()
                    && !request.operation.is_empty(),
                "workflow_capability_authority_injected",
            )?,
            WorkflowNodeKind::Approval { role_id } => require(
                RoleSpec::lookup(role_id).is_some(),
                "workflow_approval_role_invalid",
            )?,
            WorkflowNodeKind::WaitSignal { signal } => {
                require(!signal.trim().is_empty(), "workflow_signal_required")?
            }
            WorkflowNodeKind::SubWorkflow {
                definition_id,
                version,
            } => require(
                definition_id != &d.definition_id
                    && !definition_id.trim().is_empty()
                    && definition_id.len() <= 128
                    && !definition_id.contains(['\0', '\r', '\n'])
                    && *version > 0,
                "workflow_subworkflow_recursive",
            )?,
            _ => {}
        }
        let mut packet = WorkPacket::builder_task(id, id);
        packet.dependencies = node.dependencies.clone();
        graph.insert(id.clone(), packet);
    }
    validate_dependency_dag(&graph).map_err(|_| "workflow_dependency_graph_invalid")?;

    // fan-out 分支是依赖此 FanOut 节点的直接子节点；FanIn 则必须至少依赖一个结果节点。
    // 有界分支数让单次 Advance 不会隐式申请任意大的并行执行面。
    for (id, node) in &d.nodes {
        match &node.kind {
            WorkflowNodeKind::FanOut => {
                let branches = d
                    .nodes
                    .iter()
                    .filter(|(_, candidate)| candidate.dependencies.contains(id))
                    .count();
                require(branches > 0, "workflow_fanout_without_branches")?;
                require(
                    branches <= MAX_WORKFLOW_FAN_OUT,
                    "workflow_fanout_limit_exceeded",
                )?;
            }
            WorkflowNodeKind::FanIn => {
                require(
                    !node.dependencies.is_empty(),
                    "workflow_fanin_dependencies_required",
                )?;
            }
            _ => {}
        }
    }

    // Artifact 图描述产物间的先后依赖；此外每个输入 artifact 的 generator 还必须是消费
    // artifact generator 的 workflow 节点祖先，不能仅凭声明一条 artifact 边越过节点 DAG。
    let mut artifacts = BTreeMap::new();
    for (id, artifact) in &d.artifacts {
        require(
            !id.trim().is_empty()
                && id.len() <= 128
                && !id.contains(['\0', '\r', '\n'])
                && d.nodes.contains_key(&artifact.generated_by)
                && !artifact.output_key.is_empty()
                && artifact.output_key.len() <= 256
                && !artifact.output_key.contains(['\0', '\r', '\n']),
            "workflow_artifact_generator_invalid",
        )?;
        let mut packet = WorkPacket::builder_task(id, id);
        packet.dependencies = artifact.requires.clone();
        artifacts.insert(id.clone(), packet);
    }
    for artifact in d.artifacts.values() {
        for input in &artifact.requires {
            let source_artifact = d
                .artifacts
                .get(input)
                .ok_or("workflow_artifact_dependency_missing")?;
            let source = &source_artifact.generated_by;
            let mut ancestors = BTreeSet::new();
            let mut pending = d.nodes[&artifact.generated_by].dependencies.clone();
            while let Some(next) = pending.pop() {
                if ancestors.insert(next.clone()) {
                    pending.extend(d.nodes[&next].dependencies.clone());
                }
            }
            require(
                ancestors.contains(source),
                "workflow_artifact_missing_dependency_edge",
            )?;
        }
    }
    validate_dependency_dag(&artifacts).map_err(|_| "workflow_artifact_graph_invalid")?;
    Ok(())
}

/// 运行纯命令 planner，并同时返回一个供下游观察的摘要 intent。
///
/// 【调用者】Core `automation.rs` 在提交外部 workflow 命令前调用；AUT-06 等领域测试也直接
/// 使用它。当前 Core 命令路径会计算并接收 intent，但真正提交的是命令事实，执行权仍归 Core。
/// 【输入】状态、命令、Core 提供的 authority 时间/身份快照，以及本命令所需的 proof。
/// 【输出】新状态、摘要型 `WorkflowPlanIntent`、可选 `WorkflowEffect`。任一步校验失败都会返回
/// reason，不会返回部分状态。
/// 【副作用】无 I/O、无持久化、无时钟读取；时钟已由调用方放在 `AutomationAuthority.now_ms`。
///
/// intent 和 effect 都不是许可。Core 仍须用 expected revision 提交状态事实，再依既有权限链
/// 消费 effect；不能把这个函数接成第二条执行循环。
pub fn plan_command_intent(
    state: &AutomationState,
    command: &AutomationCommand,
    authority: &AutomationAuthority,
    proof: &AutomationProof,
) -> Result<(AutomationState, WorkflowPlanIntent, Option<WorkflowEffect>)> {
    let (next, effect) = plan_command(state, command, authority, proof)?;
    let intent = derive_plan_intent(state, &next, command, effect.as_ref(), authority)?;
    Ok((next, intent, effect))
}

/// 从状态转换结果提取有限、可摘要的计划描述，供 queue/诊断边界消费。
///
/// `kind` 依据已返回的 effect 或新实例状态确定；定义与输入摘要用于说明本次计划针对的内容，
/// `authority_digest` 绑定 actor/session/role/project/time 快照，`intent_digest` 再绑定整份 intent。
/// 这些 SHA-256 摘要用于内容一致性/重放身份，不是签名，也不证明权限有效。
///
/// 对没有 effect 的命令，本函数通过“新状态里此前不存在的第一个 node key”猜出一个 node ID，
/// 并据状态归类为 Reserve/Wait/Terminal/Noop。生成摘要失败或 intent revision/摘要不变量不符时
/// 返回 `workflow_plan_intent_invalid`；函数不修改 state，也不向队列写入任务。
///
/// ⚠ `Start` 会在 `next` 中创建新实例，但 `AutomationCommand::instance_id()` 不为 `Start` 提供
/// 实例 ID；因此当前实现生成的 Start intent 缺少 `instance_id`，通常归类为 `Noop`。现有 Core
/// 主路径会丢弃 intent，所以这里没有改变当前 dispatch；若后续 queue/诊断消费者依赖 intent，
/// 需要先处理这个身份缺口，不能把它当作 Start 已被准确描述的证明。
pub fn derive_plan_intent(
    state: &AutomationState,
    next: &AutomationState,
    command: &AutomationCommand,
    effect: Option<&WorkflowEffect>,
    authority: &AutomationAuthority,
) -> Result<WorkflowPlanIntent> {
    let (kind, instance_id, node_id) = match effect {
        Some(WorkflowEffect::Dispatch {
            instance_id,
            node_id,
            ..
        }) => (
            WorkflowIntentKind::Dispatch,
            Some(instance_id.clone()),
            Some(node_id.clone()),
        ),
        Some(WorkflowEffect::Cancel { instance_id }) => {
            (WorkflowIntentKind::Cancel, Some(instance_id.clone()), None)
        }
        None => {
            let instance_id = command.instance_id().map(str::to_owned);
            let node_id = instance_id.as_deref().and_then(|instance_id| {
                let before = state.instances.get(instance_id);
                let after = next.instances.get(instance_id)?;
                after
                    .nodes
                    .keys()
                    .find(|node_id| {
                        before.is_none_or(|before| !before.nodes.contains_key(*node_id))
                    })
                    .cloned()
            });
            let next_status = instance_id
                .as_deref()
                .and_then(|id| next.instances.get(id))
                .map(|instance| instance.status);
            let kind = match next_status {
                Some(status) if status.terminal() => WorkflowIntentKind::Terminal,
                Some(
                    WorkflowInstanceStatus::WaitingApproval | WorkflowInstanceStatus::WaitingSignal,
                ) => WorkflowIntentKind::Wait,
                Some(WorkflowInstanceStatus::Ready | WorkflowInstanceStatus::Running) => {
                    if node_id.is_some() {
                        WorkflowIntentKind::Reserve
                    } else {
                        WorkflowIntentKind::Noop
                    }
                }
                _ => WorkflowIntentKind::Noop,
            };
            (kind, instance_id, node_id)
        }
    };
    let definition_digest = instance_id.as_deref().and_then(|id| {
        next.instances.get(id).and_then(|instance| {
            next.definitions
                .get(&definition_key(
                    &instance.definition_id,
                    instance.definition_version,
                ))
                .map(WorkflowDefinition::digest)
        })
    });
    let input_digest = instance_id.as_deref().and_then(|instance_id| {
        node_id.as_deref().and_then(|node_id| {
            next.instances
                .get(instance_id)
                .and_then(|instance| instance.nodes.get(node_id))
                .map(|node| node.input_digest.clone())
        })
    });
    let queue_key = instance_id.as_ref().map(|instance_id| {
        format!(
            "workflow:{instance_id}:{}:{}",
            node_id.as_deref().unwrap_or("none"),
            next.revision
        )
    });
    let authority_digest = json_digest(&serde_json::json!({
        "actor_id": authority.context.actor_id,
        "session_id": authority.context.session_id,
        "role_id": authority.context.role_id,
        "project_root": authority.context.project_root,
        "now_ms": authority.now_ms,
    }));
    let mut intent = WorkflowPlanIntent {
        schema: WORKFLOW_PLAN_INTENT_SCHEMA.to_owned(),
        kind,
        instance_id,
        node_id,
        expected_revision: state.revision,
        next_revision: next.revision,
        definition_digest,
        input_digest,
        queue_key,
        authority_digest,
        intent_digest: String::new(),
    };
    intent.intent_digest = intent.digest();
    intent
        .validate()
        .map_err(|_| "workflow_plan_intent_invalid")?;
    Ok(intent)
}

/// 按定义的稳定 key 顺序列出当前可执行的节点 ID。
///
/// 一个节点必须属于当前实例选定的节点范围、尚未有执行记录、所有 dependency 节点都存在，且
/// 每个 dependency 状态均为 Succeeded。结果顺序来自 `WorkflowDefinition.nodes` 的 BTreeMap 顺序。
/// `Advance` 当前只消费第一个 ID 并为它创建一个 execution ID；因此即使图中有多个就绪节点，
/// 也要经过多次已提交的 Advance 转换，不会由一次 effect 偷偷并行派发。
/// 缺失依赖返回稳定错误；没有 ready 节点返回空列表，由 `Advance` 调用者决定是否拒绝。
pub fn ready_node_ids(
    instance: &WorkflowInstance,
    definition: &WorkflowDefinition,
) -> Result<Vec<String>> {
    let mut ready = Vec::new();
    for (id, node) in &definition.nodes {
        if !selected(instance, definition, id) || instance.nodes.contains_key(id) {
            continue;
        }
        for dependency in &node.dependencies {
            require(
                definition.nodes.contains_key(dependency),
                "workflow_dependency_missing",
            )?;
        }
        if node.dependencies.iter().all(|dependency| {
            instance
                .nodes
                .get(dependency)
                .is_some_and(|execution| execution.status == WorkflowNodeStatus::Succeeded)
        }) {
            ready.push(id.clone());
        }
    }
    Ok(ready)
}

/// 读取 FanIn 节点的依赖输出，并按依赖 ID 字典序返回。
///
/// 【输入】实例状态、固定版本的 workflow definition 和目标 node ID。
/// 【验证】目标必须确实是 FanIn；每个 dependency 都必须已有 Succeeded 执行，且输出已标记记录、
/// 当前输出的摘要与保存摘要一致。任一结果缺失/漂移都会整体失败，不用空值填补缺口。
/// 【调用者】`Advance` 对 FanIn 节点做纯状态规划时调用。这里仅读取快照，不修改节点或持久化。
pub fn stable_fan_in_outputs(
    instance: &WorkflowInstance,
    definition: &WorkflowDefinition,
    node_id: &str,
) -> Result<Vec<WorkflowValue>> {
    let node = definition
        .nodes
        .get(node_id)
        .ok_or("workflow_node_not_found")?;
    require(
        matches!(&node.kind, WorkflowNodeKind::FanIn),
        "workflow_fanin_node_required",
    )?;
    let mut dependencies = node.dependencies.clone();
    dependencies.sort();
    let mut values = Vec::with_capacity(dependencies.len());
    for dependency in dependencies {
        let execution = instance
            .nodes
            .get(&dependency)
            .ok_or("workflow_fanin_result_missing")?;
        require(
            execution.status == WorkflowNodeStatus::Succeeded
                && execution.output_recorded
                && execution.output_digest == json_digest(&execution.output),
            "workflow_fanin_result_missing",
        )?;
        values.push(execution.output.clone());
    }
    Ok(values)
}

/// 把已终结的子 workflow 状态折叠到父实例对应的 SubWorkflow 节点上。
///
/// 【调用者】父 workflow 执行 `Advance` 之前调用，确保父节点依赖判断读取到最新 child 状态。
/// 【数据流】父节点保存 child instance ID → 校验 child 定义版本和 parent link → 将 child 成功
/// 输出/失败/Unknown 映射到父节点 → 若有变化则重新计算父实例状态。
/// child 仍在运行/等待时保持父节点现状；父节点已终结时不会被 child 后续状态覆盖。
///
/// child 丢失、定义漂移、节点类型不符或父定义缺失都会返回错误，阻止父 workflow 基于错误
/// 归属继续执行。该函数只改 planner 正在构造的内存状态副本，是否提交由 Core 决定。
fn reconcile_subworkflow_children(state: &mut AutomationState, parent_id: &str) -> Result<()> {
    let parent = state
        .instances
        .get(parent_id)
        .ok_or("workflow_instance_not_found")?;
    let definition_key = definition_key(&parent.definition_id, parent.definition_version);
    let definition = state
        .definitions
        .get(&definition_key)
        .cloned()
        .ok_or("workflow_definition_not_found")?;
    let children = parent
        .nodes
        .iter()
        .filter_map(|(node_id, node)| {
            let child_id = node.child_instance_id.as_ref()?;
            let WorkflowNodeKind::SubWorkflow {
                definition_id,
                version,
            } = &definition.nodes.get(node_id)?.kind
            else {
                return Some(Err("workflow_child_node_kind_mismatch"));
            };
            Some(Ok((
                node_id.clone(),
                child_id.clone(),
                definition_id.clone(),
                *version,
            )))
        })
        .collect::<Result<Vec<_>>>()?;
    let mut changed = false;
    for (node_id, child_id, expected_definition_id, expected_version) in children {
        let Some(child) = state.instances.get(&child_id).cloned() else {
            return Err("workflow_child_instance_missing");
        };
        require(
            child.definition_id == expected_definition_id
                && child.definition_version == expected_version
                && child.parent_instance_id.as_deref() == Some(parent_id),
            "workflow_child_definition_drift",
        )?;
        let parent = state
            .instances
            .get_mut(parent_id)
            .ok_or("workflow_instance_not_found")?;
        let node = parent
            .nodes
            .get_mut(&node_id)
            .ok_or("workflow_node_not_started")?;
        if node.status.terminal() {
            continue;
        }
        match child.status {
            WorkflowInstanceStatus::Succeeded => {
                node.status = WorkflowNodeStatus::Succeeded;
                node.output = WorkflowValue::Object(child.outputs.into_iter().collect());
                node.output_digest = json_digest(&node.output);
                node.output_recorded = true;
                // 当前源代码以父子实例的创建时间较大值填写节点 ended_at；此字段的实际含义
                // 和这两个时间来源是否符合预期，需结合数据契约核查，planner 本身没有 child 完成时间。
                node.ended_at = Some(child.created_at.max(parent.created_at));
                // ⚠ 这里更新的是 WorkflowNodeExecution.output；当前代码没有把 child.outputs 放进
                // parent.outputs。后续节点 Advance 的 CopyInput/Gate 输入来自实例 inputs/outputs，
                // 且 refresh 会检查定义 output_keys/artifact output_key，因此不要假定 child 结果已
                // 对父流程下游节点可见。该数据流缺口已单独记录，注释不代替修复。
                changed = true;
            }
            WorkflowInstanceStatus::Failed | WorkflowInstanceStatus::Cancelled => {
                node.status = WorkflowNodeStatus::Failed;
                node.error_code = Some("workflow_child_failed".to_owned());
                node.ended_at = Some(child.created_at.max(parent.created_at));
                changed = true;
            }
            WorkflowInstanceStatus::ResultUnknown => {
                node.status = WorkflowNodeStatus::ResultUnknown;
                node.error_code = Some("workflow_child_result_unknown".to_owned());
                node.ended_at = Some(child.created_at.max(parent.created_at));
                changed = true;
            }
            _ => {}
        }
    }
    if changed {
        let parent = state
            .instances
            .get_mut(parent_id)
            .ok_or("workflow_instance_not_found")?;
        refresh(parent, &definition)?;
    }
    Ok(())
}

/// 在纯状态中创建一个 workflow 实例；不启动任何 node 或外部运行。
///
/// 【调用者】`Start`、trigger firing 和 `SubWorkflow`/`Compensate` 规划分支使用。
/// 【校验】实例 ID 唯一且有界，全局实例数少于 4096；定义版本已登记；actor 属于定义允许角色；
/// 必需 input key 均存在。当前实现只要求声明的 keys 都在 inputs 中，不拒绝额外 key。
/// 有 parent 时另校验 owner 相同，并把 depth 限制在 `MAX_WORKFLOW_DEPTH`（8）。
/// 只有真正嵌套执行的调用方（`SubWorkflow`/trigger child）才要求子定义 `max_steps` 不超过父实例
/// 剩余预算；补偿实例是独立的原实例引用而不是嵌套 child，`charge_parent_budget=false`，否则一个
/// 已消耗 step 的失败源实例永远无法补偿（其剩余预算必然小于完整定义）。
/// 【状态变化】把 owner/role/创建时间/deadline/输入/可选 trigger 和 parent 记入新实例，初始为
/// Ready；deadline 是创建时刻加定义最大持续时间。所有失败只丢弃调用方的 planner 副本。
fn create_instance(
    state: &mut AutomationState,
    id: &str,
    definition_id: &str,
    version: u64,
    inputs: BTreeMap<String, WorkflowValue>,
    a: &AutomationAuthority,
    trigger_id: Option<String>,
    parent_instance_id: Option<String>,
    charge_parent_budget: bool,
) -> Result<()> {
    require(
        !id.trim().is_empty() && id.len() <= 256 && !state.instances.contains_key(id),
        "workflow_instance_exists_or_invalid",
    )?;
    require(
        state.instances.len() < 4096,
        "workflow_instance_limit_exceeded",
    )?;
    let d = state
        .definitions
        .get(&definition_key(definition_id, version))
        .ok_or("workflow_definition_not_found")?;
    let depth = if let Some(parent_id) = parent_instance_id.as_deref() {
        let parent = state
            .instances
            .get(parent_id)
            .ok_or("workflow_parent_instance_missing")?;
        require(
            parent.owner_id == a.context.actor_id.clone().unwrap_or_default(),
            "workflow_child_owner_mismatch",
        )?;
        let parent_definition = state
            .definitions
            .get(&definition_key(
                &parent.definition_id,
                parent.definition_version,
            ))
            .ok_or("workflow_definition_not_found")?;
        if charge_parent_budget {
            require(
                d.max_steps
                    <= parent_definition
                        .max_steps
                        .saturating_sub(parent.steps_used),
                "workflow_child_budget_exceeded",
            )?;
        }
        parent
            .depth
            .checked_add(1)
            .ok_or("workflow_child_depth_overflow")?
    } else {
        0
    };
    require(depth <= MAX_WORKFLOW_DEPTH, "workflow_child_depth_exceeded")?;
    require(
        d.allowed_roles.contains(&a.context.role_id),
        "workflow_role_denied",
    )?;
    require(
        d.input_keys.iter().all(|key| inputs.contains_key(key)),
        "workflow_input_schema_invalid",
    )?;
    state.instances.insert(
        id.into(),
        WorkflowInstance {
            instance_id: id.into(),
            definition_id: definition_id.into(),
            definition_version: version,
            owner_id: a
                .context
                .actor_id
                .clone()
                .ok_or("workflow_actor_required")?,
            role_id: a.context.role_id.clone(),
            created_at: a.now_ms,
            deadline: a.now_ms.saturating_add(d.max_duration_ms),
            inputs,
            outputs: BTreeMap::new(),
            status: WorkflowInstanceStatus::Ready,
            steps_used: 0,
            error_code: None,
            nodes: BTreeMap::new(),
            signals: BTreeMap::new(),
            trigger_id,
            parent_instance_id,
            depth,
            selected_nodes: None,
            retry_counts: BTreeMap::new(),
        },
    );
    Ok(())
}

/// 判断某个 definition node 是否包含在当前实例要运行的节点集合里。
///
/// 普通实例运行所有非 compensation 节点；补偿实例通过 `selected_nodes` 显式挑出一组节点。
/// 这只过滤执行集合，不改变 definition 本身的 DAG 或节点内容。
fn selected(instance: &WorkflowInstance, definition: &WorkflowDefinition, id: &str) -> bool {
    if let Some(nodes) = &instance.selected_nodes {
        return nodes.iter().any(|node| node == id);
    }
    !definition
        .nodes
        .values()
        .any(|node| node.compensation.as_deref() == Some(id))
}

/// 按节点状态重新计算实例状态，并在普通 workflow 全部成功时检查输出契约。
///
/// 状态优先级有意固定：ResultUnknown 最高，其次是 CancelRequested 的收尾、失败/取消节点、
/// 全部选中节点成功、Reserved、WaitingApproval、WaitingSignal，最后 Ready。Unknown 不能被
/// 后续“还有别的节点运行中”覆盖；CancelRequested 只有全部节点终结才变成 Cancelled。
///
/// 所有选中节点成功后，普通实例还必须提供 definition 声明的全部 output key 和 artifact
/// output key；缺一项会变成 Failed。补偿实例只执行 selected_nodes，不要求满足原 definition
/// 的整体输出/产物 schema。若调用前实例为 Paused，重算后任何非终态都会恢复为 Paused。
/// 因而调用时仍为 Paused 的 Resume 分支也会被末尾逻辑设回 Paused；这是当前 Resume 无法使
/// 非终态实例离开 Paused 的源码问题，需在业务修复任务中处理。
/// 本函数只更新内存投影；调用者仍需把状态转换连同命令一起持久化。
fn refresh(instance: &mut WorkflowInstance, definition: &WorkflowDefinition) -> Result<()> {
    let paused = instance.status == WorkflowInstanceStatus::Paused;
    if instance
        .nodes
        .values()
        .any(|n| n.status == WorkflowNodeStatus::ResultUnknown)
    {
        instance.status = WorkflowInstanceStatus::ResultUnknown;
    } else if instance.status == WorkflowInstanceStatus::CancelRequested {
        if instance.nodes.values().all(|n| n.status.terminal()) {
            instance.status = WorkflowInstanceStatus::Cancelled;
        }
    } else if instance.nodes.values().any(|n| {
        matches!(
            n.status,
            WorkflowNodeStatus::Failed | WorkflowNodeStatus::Cancelled
        )
    }) {
        instance.status = WorkflowInstanceStatus::Failed;
    } else if definition
        .nodes
        .keys()
        .filter(|id| selected(instance, definition, id))
        .all(|id| {
            instance
                .nodes
                .get(id)
                .is_some_and(|n| n.status == WorkflowNodeStatus::Succeeded)
        })
    {
        if instance.selected_nodes.is_none() {
            if !definition
                .output_keys
                .iter()
                .all(|k| instance.outputs.contains_key(k))
            {
                instance.status = WorkflowInstanceStatus::Failed;
                instance.error_code = Some("workflow_output_schema_invalid".into());
                return Ok(());
            }
            if !definition
                .artifacts
                .values()
                .all(|artifact| instance.outputs.contains_key(&artifact.output_key))
            {
                instance.status = WorkflowInstanceStatus::Failed;
                instance.error_code = Some("workflow_artifact_output_missing".into());
                return Ok(());
            }
        }
        instance.status = WorkflowInstanceStatus::Succeeded;
    } else if instance
        .nodes
        .values()
        .any(|n| n.status == WorkflowNodeStatus::Reserved)
    {
        instance.status = WorkflowInstanceStatus::Running;
    } else if instance
        .nodes
        .values()
        .any(|n| n.status == WorkflowNodeStatus::WaitingApproval)
    {
        instance.status = WorkflowInstanceStatus::WaitingApproval;
    } else if instance
        .nodes
        .values()
        .any(|n| n.status == WorkflowNodeStatus::WaitingSignal)
    {
        instance.status = WorkflowInstanceStatus::WaitingSignal;
    } else {
        instance.status = WorkflowInstanceStatus::Ready;
    }
    if paused && !instance.status.terminal() {
        instance.status = WorkflowInstanceStatus::Paused;
    }
    Ok(())
}

/// 根据 ControlPlane 提供的运行响应和 proof，将一个未决节点更新为可重放的观测状态。
///
/// 【调用者】`Reconcile` 和 Core 私有的 `RecordObservation` 在 planner 中共用此 reducer。
/// 【身份/证据】必须提供 CoreResponse，目标节点必须已启动且未终结，response request ID 必须
/// 与节点 execution ID 相同；成功/失败等需要至少一个 evidence ref。只有 `ResultUnknown` 可在
/// 没有 event ref 时继续，但必须同时提供绑定同一 instance/node/request/evidence 的 incident。
/// 任一终态响应必须有字段一致的 runtime receipt；非终态不能携带终态 receipt。
/// 【状态映射】Completed → Succeeded；Failed/Blocked/Denied → Failed；Cancelled → Cancelled；
/// ResultUnknown 保持 Unknown；AwaitingApproval → WaitingApproval；其余未终态仍为 Reserved。
/// 保存 response output/error、evidence、receipt、incident 引用；终态写入 `ended_at`，成功输出
/// 按 node ID 更新 instance.outputs，最后调用 `refresh()`。
///
/// proof 缺失、身份不匹配、receipt/incident 绑定错误或重复 incident 内容冲突都会拒绝整个
/// 状态转换。函数本身不查询 EventStore，也不签名证明；proof 的来源可信度由 Core 负责。
///
/// ⚠ `WorkflowNodeStatus::ResultUnknown` 当前也属于 `terminal()`；上面的通用条件会拒绝再次
/// observe 这个节点。Core 的 Reconcile 可以重新读取到已确认的 Company/Capability 事实，但
/// 传回这里后仍无法把原 Unknown 节点改成 Succeeded/Failed。因此 Unknown incident 目前可能
/// 永久留在 workflow state；不要把“支持 Reconcile 命令”误写成“Unknown 可以被终态证据修正”。
fn observe(
    instance: &mut WorkflowInstance,
    definition: &WorkflowDefinition,
    node_id: &str,
    a: &AutomationAuthority,
    p: &AutomationProof,
    incidents: &mut BTreeMap<String, WorkflowIncident>,
) -> Result<()> {
    let response = p
        .response
        .as_ref()
        .ok_or("workflow_runtime_evidence_required")?;
    let node = instance
        .nodes
        .get_mut(node_id)
        .ok_or("workflow_node_not_started")?;
    require(
        !node.status.terminal() && response.request_id == node.execution_id,
        "workflow_observation_identity_conflict",
    )?;
    require(
        !p.evidence_refs.is_empty() || response.status == ExecutionStatus::ResultUnknown,
        "workflow_evidence_required",
    )?;
    if response.status.is_terminal() {
        let receipt = p
            .runtime_receipt
            .as_ref()
            .ok_or("workflow_runtime_receipt_required")?;
        receipt
            .validate()
            .map_err(|_| "workflow_runtime_receipt_invalid")?;
        require(
            receipt.request_id == response.request_id
                && receipt.status == response.status
                && receipt.event_refs == p.evidence_refs,
            "workflow_runtime_receipt_binding_mismatch",
        )?;
    } else {
        require(
            p.runtime_receipt.is_none(),
            "workflow_runtime_receipt_terminal_required",
        )?;
    }
    if response.status == ExecutionStatus::ResultUnknown {
        let incident = p.incident.as_ref().ok_or("workflow_incident_required")?;
        incident
            .validate()
            .map_err(|_| "workflow_incident_invalid")?;
        require(
            incident.instance_id == instance.instance_id
                && incident.node_id == node_id
                && incident.request_id == response.request_id
                && incident.evidence_refs == p.evidence_refs,
            "workflow_incident_binding_mismatch",
        )?;
        if let Some(existing) = incidents.get(&incident.incident_id) {
            require(existing == incident, "workflow_incident_conflict")?;
        } else {
            incidents.insert(incident.incident_id.clone(), incident.clone());
        }
    } else {
        require(p.incident.is_none(), "workflow_incident_unknown_only")?;
    }
    node.status = match response.status {
        ExecutionStatus::Completed => WorkflowNodeStatus::Succeeded,
        ExecutionStatus::Failed | ExecutionStatus::Blocked | ExecutionStatus::Denied => {
            WorkflowNodeStatus::Failed
        }
        ExecutionStatus::Cancelled => WorkflowNodeStatus::Cancelled,
        ExecutionStatus::ResultUnknown => WorkflowNodeStatus::ResultUnknown,
        ExecutionStatus::AwaitingApproval => WorkflowNodeStatus::WaitingApproval,
        _ => WorkflowNodeStatus::Reserved,
    };
    node.output = response.output.clone();
    node.output_digest = json_digest(&node.output);
    node.output_recorded = true;
    node.error_code = response.error.clone();
    node.evidence_refs = p.evidence_refs.clone();
    node.runtime_receipt = p.runtime_receipt.clone();
    node.incident_id = p
        .incident
        .as_ref()
        .map(|incident| incident.incident_id.clone());
    if node.status.terminal() {
        node.ended_at = Some(a.now_ms);
    }
    if node.status == WorkflowNodeStatus::Succeeded {
        instance.outputs.insert(node_id.into(), node.output.clone());
    }
    refresh(instance, definition)
}
/// 对一个 workflow 命令执行纯状态转换，并返回待 Core 消费的可选外部 effect。
///
/// 【调用者】Core `automation.rs` 用它重放 `workflow.command_applied` 事实；新命令主路径通过
/// `plan_command_intent()` 间接调用；Workflow 测试也用固定 state/authority/proof 检查转换。
/// 【输入】`state` 是 EventStore 事件折叠出的当前状态；`command` 指定一次注册、推进、审批、
/// 取消、trigger 等转换；`a` 是 Core 提供的 actor/role/session/时间和执行 ID 快照；`p` 携带
/// EventStore/Company 结果证据或运行响应。
/// 【输出】成功返回一份新 `AutomationState` 和至多一个 `WorkflowEffect`。原 state 永不修改；
/// effect 仅用于 Core 在事件 CAS 成功后调用已有 Company/Capability/取消入口。
/// 【失败】不变量、owner/role、deadline、lease、证据或预算不匹配时返回稳定的静态 reason，整份
/// next state 被丢弃；本函数没有日志、网络、EventStore 写入、定时器或 retry。
///
/// 【核心阶段】先验证 actor 和实例归属/父链/角色，再按命令 enum 执行唯一状态分支，最后统一
/// 增加一个 state revision。Cancel 还会标记真实 subworkflow descendants；不确定外部结果不会
/// 被转换成自动 retry。
///
/// ⚠ 所有变更先发生在 `state.clone()` 上，而非就地修改调用者状态。这使 Core 能在任一检查失败时
/// 拒绝整条命令而不留下半更新状态；不要为了减少 clone 将部分分支改为写入共享状态。
pub fn plan_command(
    state: &AutomationState,
    command: &AutomationCommand,
    a: &AutomationAuthority,
    p: &AutomationProof,
) -> Result<(AutomationState, Option<WorkflowEffect>)> {
    // 用副本承载整次规划，确保 `?` 返回错误时调用方的快照完全不变。
    let mut next = state.clone();
    let mut effect = None;
    // actor 是所有 workflow 实例/trigger 的所有权基础。该值由 Core 身份上下文提供；外部
    // AutomationCommand 不能自行选择“以哪个 actor 执行”。
    require(
        a.context.actor_id.as_ref().is_some_and(|id| !id.is_empty()),
        "workflow_actor_required",
    )?;
    if let Some(id) = command.instance_id() {
        let instance = next
            .instances
            .get(id)
            .ok_or("workflow_instance_not_found")?;
        require(
            Some(instance.owner_id.as_str()) == a.context.actor_id.as_deref(),
            "workflow_owner_mismatch",
        )?;
        // Reconcile/内部观测必须能收尾已暂停、已取消或父实例已结束的运行；Cancel 也必须能
        // 穿过 inactive parent 来停止后代。其他推进命令要求整条 subworkflow 祖先链仍有效。
        if !matches!(
            command,
            AutomationCommand::Reconcile { .. }
                | AutomationCommand::RecordObservation { .. }
                | AutomationCommand::Cancel { .. }
        ) {
            require(
                workflow_ancestors_active(state, id),
                "workflow_parent_inactive",
            )?;
        }
        // Workflow owner/role 是实例创建时冻结的上下文。审批由 definition 指定 role 决定，且
        // 要求独立 session；观测与 Reconcile 使用 Core 生成的运行 proof，因此这三类命令不使用
        // 普通推进角色比较，但仍受上面的 actor owner 检查约束。
        if !matches!(
            command,
            AutomationCommand::Decide { .. }
                | AutomationCommand::RecordObservation { .. }
                | AutomationCommand::Reconcile { .. }
        ) {
            require(
                instance.role_id == a.context.role_id,
                "workflow_role_denied",
            )?;
        }
    }

    // 一个 enum 分支只计算一种事实转换。此处不调用 IO：例如 Advance 只产出 Dispatch intent，
    // RegisterTrigger 只改变 trigger state；Core 负责在 CAS 后执行 effect。
    match command {
        AutomationCommand::RegisterDefinition { definition } => {
            require(
                matches!(a.context.role_id.as_str(), "pm" | "sponsor" | "architect"),
                "workflow_definition_role_denied",
            )?;
            validate_definition(definition)?;
            let key = definition_key(&definition.definition_id, definition.version);
            // 定义版本是历史运行解释的一部分，已用过的 `(id, version)` 不可覆盖；新版本才是
            // 升级入口。先要求 subdefinition 已经登记且冻结，避免未来新定义形成间接递归引用。
            require(
                !next.definitions.contains_key(&key),
                "workflow_definition_version_immutable",
            )?;
            for node in definition.nodes.values() {
                if let WorkflowNodeKind::SubWorkflow {
                    definition_id,
                    version,
                } = &node.kind
                {
                    require(
                        next.definitions
                            .contains_key(&definition_key(definition_id, *version)),
                        "workflow_subdefinition_not_found",
                    )?;
                }
            }
            next.definitions.insert(key, definition.clone());
        }
        AutomationCommand::Start {
            instance_id,
            definition_id,
            version,
            inputs,
        } => create_instance(
            &mut next,
            instance_id,
            definition_id,
            *version,
            inputs.clone(),
            a,
            None,
            None,
            true,
        )?,
        AutomationCommand::Advance { instance_id } => {
            // 先把已结束的子实例状态带回父节点，之后才计算 ready nodes；不读取或启动子流程。
            reconcile_subworkflow_children(&mut next, instance_id)?;
            let instance = next.instances.get_mut(instance_id).unwrap();
            require(
                !instance.status.terminal()
                    && !matches!(
                        instance.status,
                        WorkflowInstanceStatus::Paused | WorkflowInstanceStatus::CancelRequested
                    ),
                "workflow_not_advanceable",
            )?;
            require(a.now_ms < instance.deadline, "workflow_deadline_expired")?;
            let definition = &next.definitions
                [&definition_key(&instance.definition_id, instance.definition_version)];
            require(
                instance.steps_used < definition.max_steps,
                "workflow_step_budget_exhausted",
            )?;
            // 同一实例一次最多保留一个 Reserved 节点。图上的 fan-out 表示多个依赖分支，
            // 不是授权 Core 一次并行派发；并发要由另一个明确受控路径实现。
            require(
                !instance
                    .nodes
                    .values()
                    .any(|n| n.status == WorkflowNodeStatus::Reserved),
                "workflow_execution_in_flight",
            )?;
            let ready = ready_node_ids(instance, definition)?;
            // ready_node_ids 来自 BTreeMap 顺序；每个 Advance 只取第一个，使重放结果稳定，并
            // 让每个执行 ID 都对应一条单独提交的 reservation 事实。
            let id = ready.first().ok_or("workflow_no_ready_node")?;
            let node = definition.nodes.get(id).ok_or("workflow_node_not_found")?;
            let (id, node) = (id.clone(), node.clone());
            let mut inputs = instance.inputs.clone();
            inputs.extend(instance.outputs.clone());
            // 本次 node 输入指纹覆盖实例原始输入与此前已提交的输出，用于后续 plan/receipt 关联。
            // 它是领域里的 FNV 内容指纹，不是密码学签名或授权令牌。
            let mut execution = WorkflowNodeExecution {
                execution_id: a.execution_id,
                session_id: a.session_id.clone(),
                attempt: instance.retry_counts.get(&id).copied().unwrap_or(0) + 1,
                status: WorkflowNodeStatus::Succeeded,
                started_at: a.now_ms,
                lease_expires_at: a
                    .now_ms
                    .saturating_add(node.timeout_ms)
                    .min(instance.deadline),
                ended_at: Some(a.now_ms),
                input_digest: workflow_input_digest(&inputs),
                output: WorkflowValue::Null,
                output_digest: json_digest(&WorkflowValue::Null),
                output_recorded: false,
                error_code: None,
                evidence_refs: Vec::new(),
                // 这里只创建 reservation。运行 receipt/Unknown incident 必须等 ControlPlane
                // 真正观测到 effect 后再写，不能在派发之前制造结果证明。
                runtime_receipt: None,
                incident_id: None,
                child_instance_id: None,
            };
            // 本地节点在规划时即可确定输出；外部节点只记 Reserved 并把 effect 交回 Core。
            // 这个分界保证纯 planner 不假装已经调用过 AgentTask 或 Capability。
            match &node.kind {
                WorkflowNodeKind::Literal { values } => {
                    instance.outputs.extend(values.clone());
                    execution.output = WorkflowValue::Object(values.clone().into_iter().collect());
                    execution.output_recorded = true;
                }
                WorkflowNodeKind::CopyInput { mapping } => {
                    let mut copied = BTreeMap::new();
                    for (output, input) in mapping {
                        let value = inputs.get(input).ok_or("workflow_input_missing")?;
                        instance.outputs.insert(output.clone(), value.clone());
                        copied.insert(output.clone(), value.clone());
                    }
                    execution.output = WorkflowValue::Object(copied.into_iter().collect());
                    execution.output_recorded = true;
                }
                WorkflowNodeKind::Gate { key, equals } => {
                    if inputs.get(key) != Some(equals) {
                        execution.status = WorkflowNodeStatus::Failed;
                        execution.error_code = Some("workflow_gate_denied".into());
                    }
                    execution.output = serde_json::json!({"key": key, "matched": execution.status == WorkflowNodeStatus::Succeeded});
                    execution.output_recorded = true;
                }
                WorkflowNodeKind::FanOut => {
                    // FanOut 只产生有序分支 ID 列表；真正执行每个分支仍要通过后续 Advance/CAS。
                    let mut branches = definition
                        .nodes
                        .iter()
                        .filter(|(_, candidate)| candidate.dependencies.contains(&id))
                        .map(|(branch_id, _)| branch_id.clone())
                        .collect::<Vec<_>>();
                    branches.sort();
                    require(!branches.is_empty(), "workflow_fanout_without_branches")?;
                    require(
                        branches.len() <= MAX_WORKFLOW_FAN_OUT,
                        "workflow_fanout_limit_exceeded",
                    )?;
                    execution.output = WorkflowValue::Array(
                        branches.into_iter().map(WorkflowValue::String).collect(),
                    );
                    execution.output_recorded = true;
                }
                WorkflowNodeKind::FanIn => {
                    // FanIn 只汇总通过成功状态和输出摘要校验的已提交依赖结果。
                    let values = stable_fan_in_outputs(instance, definition, &id)?;
                    execution.output = WorkflowValue::Array(values);
                    execution.output_recorded = true;
                    // ⚠ 当前 FanIn 分支只设置节点 execution.output，没有写入 instance.outputs；而
                    // 后继 Advance 的输入映射由 instance.inputs + instance.outputs 组成。下游节点和
                    // 完成时 output_keys/artifact 检查可能因此读不到本次 FanIn 结果。
                }
                WorkflowNodeKind::Approval { .. } => {
                    execution.status = WorkflowNodeStatus::WaitingApproval;
                    execution.ended_at = None;
                }
                WorkflowNodeKind::WaitSignal { signal } => {
                    if let Some(value) = instance.signals.get(signal) {
                        execution.output = value.clone();
                        instance.outputs.insert(id.clone(), value.clone());
                        execution.output_recorded = true;
                    } else {
                        execution.status = WorkflowNodeStatus::WaitingSignal;
                        execution.ended_at = None;
                    }
                }
                WorkflowNodeKind::AgentTask { .. } | WorkflowNodeKind::Capability { .. } => {
                    // 先把节点记成 Reservation 并向 Core 返回描述；没有在此处运行工作内容。
                    execution.status = WorkflowNodeStatus::Reserved;
                    execution.ended_at = None;
                    effect = Some(WorkflowEffect::Dispatch {
                        instance_id: instance_id.clone(),
                        node_id: id.clone(),
                        kind: node.kind.clone(),
                        execution_id: a.execution_id,
                        session_id: a.session_id.clone(),
                    });
                }
                WorkflowNodeKind::SubWorkflow { .. } => {
                    // 子 workflow 由同一状态转换创建自己的实例，不产生第二个执行循环 effect。
                    // 父节点保留 child ID，后续 Advance/Reconcile 通过它读取子实例终态。
                    execution.status = WorkflowNodeStatus::Reserved;
                    execution.ended_at = None;
                    execution.child_instance_id =
                        Some(format!("{}:{}:{}", instance_id, id, a.execution_id));
                }
            }
            if execution.output_recorded {
                execution.output_digest = json_digest(&execution.output);
            }
            instance.steps_used += 1;
            instance.nodes.insert(id.clone(), execution.clone());
            refresh(instance, definition)?;
            if let WorkflowNodeKind::SubWorkflow {
                definition_id,
                version,
            } = &node.kind
            {
                let child_id = execution.child_instance_id.unwrap();
                create_instance(
                    &mut next,
                    &child_id,
                    definition_id,
                    *version,
                    inputs,
                    a,
                    None,
                    Some(instance_id.clone()),
                    true,
                )?;
            }
        }
        AutomationCommand::Reconcile {
            instance_id,
            node_id,
        }
        | AutomationCommand::RecordObservation {
            instance_id,
            node_id,
        } => {
            // 两种命令都用 observe() 更新节点，但来源不同：Reconcile 的 proof 由 Core 根据当前
            // EventStore/Company 事实新算；RecordObservation 只能由 Core 内部在一次 dispatch 后构造。
            let instance = next.instances.get_mut(instance_id).unwrap();
            let definition = next
                .definitions
                .get(&definition_key(
                    &instance.definition_id,
                    instance.definition_version,
                ))
                .ok_or("workflow_definition_not_found")?
                .clone();
            observe(instance, &definition, node_id, a, p, &mut next.incidents)?;
        }
        AutomationCommand::Decide {
            instance_id,
            node_id,
            approve,
            evidence_ref,
        } => {
            // 引用必须先由 Core workflow_proof 验证为当前调用者可见的持久事件；planner 只确认
            // proof 与 command 内指定的 event ref 一致，不自行读取 EventStore。
            require(
                p.evidence_refs.contains(evidence_ref),
                "workflow_approval_evidence_required",
            )?;
            let instance = next.instances.get_mut(instance_id).unwrap();
            require(
                !instance.status.terminal() && a.now_ms < instance.deadline,
                "workflow_approval_expired",
            )?;
            let definition = &next.definitions
                [&definition_key(&instance.definition_id, instance.definition_version)];
            let WorkflowNodeKind::Approval { role_id } = &definition
                .nodes
                .get(node_id)
                .ok_or("workflow_node_not_found")?
                .kind
            else {
                return Err("workflow_approval_node_required");
            };
            require(
                role_id == &a.context.role_id,
                "workflow_approval_role_denied",
            )?;
            let node = instance
                .nodes
                .get_mut(node_id)
                .ok_or("workflow_node_not_started")?;
            require(
                node.status == WorkflowNodeStatus::WaitingApproval
                    && a.now_ms < node.lease_expires_at
                    && node.session_id != a.context.session_id,
                "workflow_approval_identity_or_expiry_invalid",
            )?;
            // lease 到期时这里只拒绝 Decide，不把等待节点自动变成 Failed/Cancelled；如果没有
            // 其他取消或状态收尾命令，节点与实例可能继续停在等待状态。
            // Approval 节点只有在 owner actor 以定义要求的角色、并从不同于提交/创建节点的
            // session 回答时才能结束。approve=false 是已知拒绝，进入 Failed，不是 Unknown。
            node.status = if *approve {
                WorkflowNodeStatus::Succeeded
            } else {
                WorkflowNodeStatus::Failed
            };
            node.ended_at = Some(a.now_ms);
            node.evidence_refs.push(evidence_ref.clone());
            refresh(instance, definition)?;
        }
        AutomationCommand::Signal {
            instance_id,
            signal,
            value,
            evidence_ref,
        } => {
            // signal 同样必须带 Core 验证过的事件引用；同名 signal 只可消费一次，但可以唤醒
            // 当前实例里所有等待这个 signal 的节点。信号也可以先存入 instance.signals，供之后
            // Advance 创建的 WaitSignal 节点读取；节点 lease 到期本身不会触发自动超时转换。
            require(
                p.evidence_refs.contains(evidence_ref),
                "workflow_signal_evidence_required",
            )?;
            let instance = next.instances.get_mut(instance_id).unwrap();
            require(
                !instance.status.terminal() && a.now_ms < instance.deadline,
                "workflow_signal_expired",
            )?;
            require(
                !instance.signals.contains_key(signal),
                "workflow_signal_already_consumed",
            )?;
            let definition = &next.definitions
                [&definition_key(&instance.definition_id, instance.definition_version)];
            require(definition.nodes.values().any(|n| matches!(&n.kind,WorkflowNodeKind::WaitSignal { signal:key } if key==signal)),"workflow_signal_unknown")?;
            instance.signals.insert(signal.clone(), value.clone());
            for (id, node) in &mut instance.nodes {
                if node.status == WorkflowNodeStatus::WaitingSignal
                    && matches!(&definition.nodes[id].kind,WorkflowNodeKind::WaitSignal { signal:key } if key==signal)
                {
                    require(a.now_ms < node.lease_expires_at, "workflow_signal_expired")?;
                    node.status = WorkflowNodeStatus::Succeeded;
                    node.output = value.clone();
                    node.output_digest = json_digest(&node.output);
                    node.output_recorded = true;
                    node.ended_at = Some(a.now_ms);
                    node.evidence_refs.push(evidence_ref.clone());
                    instance.outputs.insert(id.clone(), value.clone());
                }
            }
            refresh(instance, definition)?;
        }
        AutomationCommand::Pause { instance_id } => {
            // 暂停只阻止后续 Advance；它不会取消已 Reserved 的外部运行，也不会抹掉节点状态。
            let i = next.instances.get_mut(instance_id).unwrap();
            require(!i.status.terminal(), "workflow_terminal")?;
            i.status = WorkflowInstanceStatus::Paused;
        }
        AutomationCommand::Resume { instance_id } => {
            // 只有未过总 deadline 的 Paused 实例才进入恢复分支。⚠ 目前这里没有先将 status
            // 改离 Paused 就调用 refresh；refresh 会捕获 paused=true，并在末尾把非终态重新设为
            // Paused。因此普通未终态实例执行 Resume 后仍保持 Paused。这是当前状态机问题，本任务
            // 只如实记录，不改变转换行为。
            let i = next.instances.get_mut(instance_id).unwrap();
            require(
                i.status == WorkflowInstanceStatus::Paused && a.now_ms < i.deadline,
                "workflow_not_resumable",
            )?;
            refresh(
                i,
                &next.definitions[&definition_key(&i.definition_id, i.definition_version)],
            )?;
        }
        AutomationCommand::Cancel {
            instance_id,
            reason,
        } => {
            // reason 必须非空，command 事件历史保存原始理由；这里不立即把运行中外部节点标成
            // Cancelled，因为是否停止只能由 Core 发出取消并在后续 Reconcile 证实。
            require(!reason.trim().is_empty(), "workflow_cancel_reason_required")?;
            let i = next.instances.get_mut(instance_id).unwrap();
            require(!i.status.terminal(), "workflow_terminal")?;
            i.status = WorkflowInstanceStatus::CancelRequested;
            for (id, node) in &mut i.nodes {
                if matches!(
                    next.definitions[&definition_key(&i.definition_id, i.definition_version)].nodes
                        [id]
                        .kind,
                    WorkflowNodeKind::Approval { .. } | WorkflowNodeKind::WaitSignal { .. }
                ) && !node.status.terminal()
                {
                    node.status = WorkflowNodeStatus::Cancelled;
                    node.ended_at = Some(a.now_ms);
                }
            }
            refresh(
                i,
                &next.definitions[&definition_key(&i.definition_id, i.definition_version)],
            )?;
            // effect 只请求 Core 沿 Company/approval 边界传播取消。节点级 reducer 对 Approval 和
            // WaitSignal 可以直接确认已取消；AgentTask/Capability 保持未决直到收到事实。
            effect = Some(WorkflowEffect::Cancel {
                instance_id: instance_id.clone(),
            });
        }
        AutomationCommand::Retry {
            instance_id,
            node_id,
        } => {
            // retry 是创建下一次纯计算尝试，不是通用网络重试。前面要求节点确认为 Failed；未知
            // 结果不满足该条件，外部 effect 也被限制为必须重新审查的新 contract。
            let i = next.instances.get_mut(instance_id).unwrap();
            let d = &next.definitions[&definition_key(&i.definition_id, i.definition_version)];
            let n = i.nodes.get(node_id).ok_or("workflow_node_not_started")?;
            require(
                n.status == WorkflowNodeStatus::Failed
                    && n.attempt <= d.nodes[node_id].retry_limit
                    && a.now_ms < i.deadline,
                "workflow_retry_denied",
            )?;
            // 只允许重做 Literal、CopyInput、Gate 这些无外部副作用的节点；外部副作用必须先有
            // 新的 reviewed work packet/contract，不能因超时或 Unknown 自动重发。
            require(
                matches!(
                    d.nodes[node_id].kind,
                    WorkflowNodeKind::Literal { .. }
                        | WorkflowNodeKind::CopyInput { .. }
                        | WorkflowNodeKind::Gate { .. }
                ),
                "workflow_effect_retry_requires_new_contract",
            )?;
            let retries = i.retry_counts.get(node_id).copied().unwrap_or(0);
            require(
                retries < d.nodes[node_id].retry_limit,
                "workflow_retry_budget_exhausted",
            )?;
            i.retry_counts.insert(node_id.clone(), retries + 1);
            i.nodes.remove(node_id);
            i.status = WorkflowInstanceStatus::Retrying;
        }
        AutomationCommand::Compensate {
            instance_id,
            compensation_instance_id,
        } => {
            // 补偿不是把原实例改写成成功，而是创建一个有独立 ID/历史的新实例，只挑出已成功节点
            // 声明的 compensation node。源实例必须是已知 Failed/Cancelled；Unknown 不可补偿，
            // 因为无法确认要撤销的 effect 是否实际发生。
            let source = next.instances.get(instance_id).unwrap().clone();
            require(
                matches!(
                    source.status,
                    WorkflowInstanceStatus::Failed | WorkflowInstanceStatus::Cancelled
                ),
                "workflow_compensation_requires_known_terminal",
            )?;
            require(
                !next.instances.values().any(|i| {
                    i.parent_instance_id.as_deref() == Some(instance_id)
                        && i.selected_nodes.is_some()
                }),
                "workflow_compensation_already_requested",
            )?;
            let d = &next.definitions
                [&definition_key(&source.definition_id, source.definition_version)];
            let mut nodes = source
                .nodes
                .iter()
                .filter(|(_, n)| n.status == WorkflowNodeStatus::Succeeded)
                .filter_map(|(id, _)| d.nodes[id].compensation.clone())
                .collect::<Vec<_>>();
            nodes.sort();
            nodes.dedup();
            require(!nodes.is_empty(), "workflow_compensation_not_defined")?;
            let mut inputs = source.inputs;
            inputs.extend(source.outputs);
            create_instance(
                &mut next,
                compensation_instance_id,
                &source.definition_id,
                source.definition_version,
                inputs,
                a,
                None,
                Some(instance_id.clone()),
                false,
            )?;
            let compensation = next.instances.get_mut(compensation_instance_id).unwrap();
            // `selected_nodes` 让 ready_node_ids 仅运行已选中的补偿节点；definition 仍保持原样。
            compensation.selected_nodes = Some(nodes);
            compensation.status = WorkflowInstanceStatus::Compensating;
        }
        AutomationCommand::RegisterTrigger { trigger } => {
            // Trigger 是经批准的自动创建入口配置，不包含 Provider/Capability 的执行权。这里
            // 绑定当前 owner 和 approval proof、已冻结 definition/role/input，并在 state 中初始化
            // occurrence 去重表和有限 firing budget。
            trigger
                .validate_shape(a.now_ms)
                .map_err(|_| "trigger_definition_shape_invalid")?;
            require(
                matches!(a.context.role_id.as_str(), "sponsor" | "pm")
                    && trigger.owner_id == a.context.actor_id.clone().unwrap_or_default(),
                "trigger_owner_or_role_denied",
            )?;
            require(
                p.evidence_refs.contains(&trigger.approval_ref),
                "trigger_approval_required",
            )?;
            require(
                !next.triggers.contains_key(&trigger.trigger_id)
                    && !trigger.trigger_id.is_empty()
                    && trigger.trigger_id.len() <= 128,
                "trigger_identity_invalid",
            )?;
            require(
                trigger.expires_at > a.now_ms
                    && trigger.expires_at - a.now_ms <= 31_536_000_000
                    && (1..=1024).contains(&trigger.max_firings),
                "trigger_budget_or_expiry_invalid",
            )?;
            let d = next
                .definitions
                .get(&definition_key(
                    &trigger.definition_id,
                    trigger.definition_version,
                ))
                .ok_or("workflow_definition_not_found")?;
            require(
                d.allowed_roles.contains(&trigger.role_id)
                    && d.input_keys.iter().all(|k| trigger.inputs.contains_key(k)),
                "trigger_template_invalid",
            )?;
            let next_at = match trigger.schedule {
                TriggerSchedule::Interval { every_ms, first_at } => {
                    require(every_ms >= 1000 && first_at > 0, "trigger_schedule_invalid")?;
                    Some(first_at)
                }
                _ => None,
            };
            next.triggers.insert(
                trigger.trigger_id.clone(),
                DurableTrigger {
                    definition: trigger.clone(),
                    enabled: true,
                    next_at,
                    firings_used: 0,
                    pending_keys: Vec::new(),
                    pending_digests: BTreeMap::new(),
                    fired: BTreeMap::new(),
                    fired_digests: BTreeMap::new(),
                },
            );
        }
        AutomationCommand::DisableTrigger { trigger_id } => {
            // 禁用会丢弃尚未启动的 pending occurrences，但不会取消已经创建的 workflow instances；
            // 对已运行实例的停止必须使用独立 Cancel/Reconcile 路径。
            let t = next
                .triggers
                .get_mut(trigger_id)
                .ok_or("trigger_not_found")?;
            require(
                matches!(a.context.role_id.as_str(), "sponsor" | "pm")
                    && Some(t.definition.owner_id.as_str()) == a.context.actor_id.as_deref(),
                "trigger_owner_or_role_denied",
            )?;
            t.enabled = false;
            t.pending_keys.clear();
            t.pending_digests.clear();
        }
        AutomationCommand::Tick { trigger_id } | AutomationCommand::Fire { trigger_id, .. } => {
            // 定时/事件来源都被折叠为明确命令输入；planner 不自己轮询时钟或监听 EventStore。
            fire_trigger(&mut next, trigger_id, command, a, p)?
        }
    }
    if let AutomationCommand::Cancel { instance_id, .. } = command {
        // Cancel 的工作不止根实例：把真实嵌套 descendants 标成 CancelRequested，并仅本地终结
        // 纯等待中的 Approval/Signal 节点。Core 随后负责对子实例运行与待审批能力发送取消请求。
        let descendants = next
            .instances
            .keys()
            .filter(|id| workflow_descends_from(&next, id, instance_id))
            .cloned()
            .collect::<Vec<_>>();
        for id in descendants {
            let i = next.instances.get_mut(&id).unwrap();
            if i.status.terminal() {
                continue;
            }
            i.status = WorkflowInstanceStatus::CancelRequested;
            let d = &next.definitions[&definition_key(&i.definition_id, i.definition_version)];
            for (node_id, node) in &mut i.nodes {
                if !node.status.terminal()
                    && matches!(
                        d.nodes[node_id].kind,
                        WorkflowNodeKind::Approval { .. } | WorkflowNodeKind::WaitSignal { .. }
                    )
                {
                    node.status = WorkflowNodeStatus::Cancelled;
                    node.ended_at = Some(a.now_ms);
                }
            }
            refresh(i, d)?;
        }
    }
    // 一个通过所有分支校验的命令只递增一次 revision；溢出时整次规划失败，调用方仍持有旧 state。
    next.revision = next
        .revision
        .checked_add(1)
        .ok_or("workflow_revision_exhausted")?;
    Ok((next, effect))
}

/// 根据显式 `Tick` 或 `Fire` 命令，把到期/外部事件转换为实例创建或待处理 occurrence。
///
/// 【调用者】`plan_command()` 的两个 trigger 命令分支调用；上游 Core 先校验 trigger owner/role、
/// 时间和事件 proof，再由这里执行纯状态转换。该函数不监听事件、不启动 timer，也不派发节点。
/// 【输入】当前 planner 状态、trigger ID、命令、Core authority 时间/身份快照和事件证据 proof。
/// 【数据流】Fire 提供单个 firing key；Tick 重放 pending key 并按 `next_at/every_ms` 计算到期项；
/// 之后先做摘要去重，再按并发策略创建实例或排队，最后更新游标和 pending 队列。
/// 【失败】owner/role 不符、过期/禁用、证据不匹配、摘要冲突、队列满或 firing budget 用尽时返回
/// 稳定 reason；外围 `plan_command()` 在副本上工作，所以失败不会部分改动调用方状态。
///
/// ⚠ `Replace` 目前只把旧实例标成 `CancelRequested` 并排入新 occurrence，没有返回
/// `WorkflowEffect::Cancel`。Core 只会对 Cancel effect 调用外部取消辅助流程，因此该分支本身不会
/// 向旧 AgentTask/Capability 发送取消；旧实例也可能因 Approval/WaitSignal 未结束而一直非终态。
/// 此外 active 筛选排除所有 terminal 状态，包含“副作用结果未知”的 `ResultUnknown`；如果上游
/// effect 仍可能运行，并发策略可能把它当作没有活动实例。这里只记录源码边界，不改变逻辑。
fn fire_trigger(
    state: &mut AutomationState,
    id: &str,
    command: &AutomationCommand,
    a: &AutomationAuthority,
    p: &AutomationProof,
) -> Result<()> {
    let t = state.triggers.get(id).ok_or("trigger_not_found")?.clone();
    require(
        t.enabled
            && a.now_ms < t.definition.expires_at
            && Some(t.definition.owner_id.as_str()) == a.context.actor_id.as_deref(),
        "trigger_disabled_expired_or_owner_mismatch",
    )?;
    require(
        t.definition.role_id == a.context.role_id,
        "trigger_execution_role_mismatch",
    )?;
    let mut occurrences = Vec::new();
    let mut next_at = t.next_at;
    match command {
        AutomationCommand::Fire {
            firing_key,
            event_ref,
            ..
        } => {
            require(
                !firing_key.is_empty() && firing_key.len() <= 256,
                "trigger_firing_key_invalid",
            )?;
            require(
                !firing_key.contains(['\0', '\r', '\n']),
                "trigger_firing_key_invalid",
            )?;
            match &t.definition.schedule {
                TriggerSchedule::Manual => {
                    require(event_ref.is_none(), "trigger_manual_event_invalid")?
                }
                TriggerSchedule::Event { kind } => require(
                    event_ref
                        .as_ref()
                        .is_some_and(|r| p.evidence_refs.contains(r))
                        && p.event_kind.as_ref() == Some(kind),
                    "trigger_event_evidence_invalid",
                )?,
                _ => return Err("trigger_interval_requires_tick"),
            }
            let digest = trigger_occurrence_digest(
                id,
                firing_key,
                event_ref.as_deref(),
                p.event_kind.as_deref(),
            );
            push_occurrence(&t, &mut occurrences, firing_key.clone(), digest)?;
        }
        AutomationCommand::Tick { .. } => {
            for key in &t.pending_keys {
                let digest = t
                    .pending_digests
                    .get(key)
                    .cloned()
                    .unwrap_or_else(|| trigger_occurrence_digest(id, key, None, None));
                push_occurrence(&t, &mut occurrences, key.clone(), digest)?;
            }
            if let TriggerSchedule::Interval { every_ms, .. } = t.definition.schedule {
                let batch = plan_interval_due(
                    t.next_at.ok_or("trigger_schedule_invalid")?,
                    every_ms,
                    a.now_ms,
                    t.definition.missed_schedule,
                )
                .map_err(|_| "trigger_interval_cursor_invalid")?;
                for key in batch.occurrence_keys {
                    let digest = trigger_occurrence_digest(id, &key, None, Some("interval"));
                    push_occurrence(&t, &mut occurrences, key, digest)?;
                }
                next_at = Some(batch.next_at);
            }
        }
        _ => return Err("trigger_command_invalid"),
    }
    let active: Vec<_> = state
        .instances
        .values()
        .filter(|i| i.trigger_id.as_deref() == Some(id) && !i.status.terminal())
        .map(|i| i.instance_id.clone())
        .collect();
    let mut pending = Vec::new();
    let mut started = !active.is_empty();
    for (key, digest) in occurrences {
        if started {
            match t.definition.concurrency {
                TriggerConcurrency::Reject => return Err("trigger_already_running"),
                TriggerConcurrency::Queue => push_pending(&mut pending, key, digest)?,
                TriggerConcurrency::Coalesce => {
                    pending.clear();
                    push_pending(&mut pending, key, digest)?;
                }
                TriggerConcurrency::Replace => {
                    // Replacement never assumes an old side effect stopped. Queue it and require
                    // the existing cancellation/reconciliation path before starting a successor.
                    for active_id in &active {
                        state.instances.get_mut(active_id).unwrap().status =
                            WorkflowInstanceStatus::CancelRequested;
                    }
                    pending.clear();
                    push_pending(&mut pending, key, digest)?;
                }
            }
            continue;
        }
        let count = state.triggers[id].firings_used;
        require(count < t.definition.max_firings, "trigger_budget_exhausted")?;
        let instance_id = format!("trigger:{id}:{}", count + 1);
        create_instance(
            state,
            &instance_id,
            &t.definition.definition_id,
            t.definition.definition_version,
            t.definition.inputs.clone(),
            a,
            Some(id.into()),
            None,
            true,
        )?;
        let current = state.triggers.get_mut(id).unwrap();
        current.fired.insert(key.clone(), instance_id);
        current.fired_digests.insert(key, digest);
        current.firings_used += 1;
        started = true;
    }
    let current = state.triggers.get_mut(id).unwrap();
    current.next_at = next_at;
    current.pending_keys = pending.iter().map(|(key, _)| key.clone()).collect();
    current.pending_digests = pending.into_iter().collect();
    Ok(())
}

/// 计算单次触发输入的稳定摘要，用来识别相同 key 是否被不同事件内容复用。
///
/// `event_ref`/`event_kind` 为空通常表示手动或 interval occurrence；摘要是完整性/去重用的
/// 内容指纹，不是签名，不能证明事件来源可信。可信事件引用由调用者提供的 `AutomationProof` 校验。
fn trigger_occurrence_digest(
    trigger_id: &str,
    firing_key: &str,
    event_ref: Option<&str>,
    event_kind: Option<&str>,
) -> String {
    json_digest(&serde_json::json!({
        "trigger_id": trigger_id,
        "firing_key": firing_key,
        "event_ref": event_ref,
        "event_kind": event_kind,
    }))
}

/// 把本次 Fire/Tick 得到的 occurrence 加入本次待处理列表，并检查历史与本批次 key 冲突。
///
/// 已经 fire 过且摘要相同的 key 被忽略，实现重放幂等；相同 key 对应不同摘要则拒绝，避免把
/// 新事件伪装成已处理 occurrence。该函数只构造 planner 内存列表，不创建实例或写 EventStore。
fn push_occurrence(
    trigger: &DurableTrigger,
    occurrences: &mut Vec<(String, String)>,
    key: String,
    digest: String,
) -> Result<()> {
    if let Some(previous) = trigger.fired_digests.get(&key) {
        if previous != &digest {
            return Err("trigger_occurrence_digest_conflict");
        }
        return Ok(());
    }
    if trigger.fired.contains_key(&key) {
        return Ok(());
    }
    if let Some(previous) = trigger.pending_digests.get(&key) {
        if previous != &digest {
            return Err("trigger_occurrence_digest_conflict");
        }
    }
    if let Some((_, previous)) = occurrences.iter().find(|(candidate, _)| candidate == &key) {
        if previous != &digest {
            return Err("trigger_occurrence_digest_conflict");
        }
        return Ok(());
    }
    occurrences.push((key, digest));
    Ok(())
}

/// 在 trigger 并发策略要求排队时保存 occurrence；相同 key/摘要不会重复排队。
///
/// `pending` 上限为 128，超过后整条 trigger 命令返回 `trigger_queue_full`，而不是静默丢事件。
/// 相同 key 携带不同 digest 也会报冲突；队列状态最终随 trigger 状态一起由 Core CAS 持久化。
fn push_pending(pending: &mut Vec<(String, String)>, key: String, digest: String) -> Result<()> {
    if let Some((_, previous)) = pending.iter().find(|(candidate, _)| candidate == &key) {
        if previous != &digest {
            return Err("trigger_occurrence_digest_conflict");
        }
        return Ok(());
    }
    if pending.len() >= 128 {
        return Err("trigger_queue_full");
    }
    pending.push((key, digest));
    Ok(())
}

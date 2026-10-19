//! AUT-20 补偿准入先拒绝优先夹具：把计划 DTO 接到既有 durable planner，证明原实例终态、原
//! attempt 存在、补偿执行身份、重复补偿、授权代次与“原实例不被改写”都先于成功路径被拒绝。

use kiana_domain::{
    AutomationAuthority, AutomationCommand, AutomationCompensationPlan,
    AutomationCompensationState, AutomationProof, AutomationState, PermissionProfile,
    RequestContext, RequestId, RoleSpec, SessionId, WorkflowDefinition, WorkflowInstanceStatus,
    WorkflowNode, WorkflowNodeKind, AUTOMATION_COMPENSATION_ACTION_UNCHANGED,
    AUTOMATION_COMPENSATION_SCHEMA,
};
use kiana_workflow::{
    plan_compensation_admission, AutomationCompensationAdmission,
    AUTOMATION_COMPENSATION_ALREADY_REQUESTED, AUTOMATION_COMPENSATION_AUTHORIZATION_STALE,
    AUTOMATION_COMPENSATION_EXECUTION_IDENTITY_MISMATCH, AUTOMATION_COMPENSATION_NOT_DECLARED,
    AUTOMATION_COMPENSATION_ORIGINAL_ATTEMPT_MISMATCH,
    AUTOMATION_COMPENSATION_ORIGINAL_ATTEMPT_MISSING,
    AUTOMATION_COMPENSATION_ORIGINAL_INSTANCE_MISSING,
    AUTOMATION_COMPENSATION_ORIGINAL_NOT_TERMINAL,
};
use serde_json::json;
use std::collections::BTreeMap;

const A: &str = "sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
const B: &str = "sha256:bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";
const C: &str = "sha256:cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc";

fn authority(execution: RequestId, now_ms: u64) -> AutomationAuthority {
    authority_as("builder", execution, now_ms)
}

fn authority_as(role: &str, execution: RequestId, now_ms: u64) -> AutomationAuthority {
    let mut context = RequestContext::local("aut20-session", "/aut20-project");
    context.project_trusted = true;
    context.permission_profile = PermissionProfile::Balanced;
    context.assign_role(&RoleSpec::lookup(role).expect("role fixture"));
    AutomationAuthority {
        context,
        now_ms,
        execution_id: execution,
        session_id: SessionId::new("aut20-runtime"),
    }
}

/// 定义登记要求 pm/sponsor/architect；实例运行使用定义声明的 builder 角色。
fn register_authority() -> AutomationAuthority {
    authority_as("pm", RequestId::new(), 100)
}

fn literal(values: &[(&str, serde_json::Value)], compensation: Option<&str>) -> WorkflowNode {
    WorkflowNode {
        dependencies: Vec::new(),
        kind: WorkflowNodeKind::Literal {
            values: values
                .iter()
                .map(|(key, value)| ((*key).to_owned(), value.clone()))
                .collect(),
        },
        timeout_ms: 1_000,
        retry_limit: 0,
        compensation: compensation.map(str::to_owned),
    }
}

/// `a_work`(成功，声明补偿 `undo`) -> `b_fail`(Gate 失败)；失败后源实例为 Failed，且已成功节点
/// 声明了补偿节点。节点按字典序推进，故顺序是 a_work、b_fail。
fn definition() -> WorkflowDefinition {
    let mut nodes = BTreeMap::new();
    nodes.insert(
        "a_work".to_owned(),
        literal(&[("done", json!(true))], Some("undo")),
    );
    nodes.insert(
        "b_fail".to_owned(),
        WorkflowNode {
            dependencies: vec!["a_work".to_owned()],
            kind: WorkflowNodeKind::Gate {
                key: "ok".to_owned(),
                equals: json!(true),
            },
            timeout_ms: 1_000,
            retry_limit: 0,
            compensation: None,
        },
    );
    // `undo` 是补偿节点：无依赖、不参与普通实例执行（`selected()` 会把它排除）。
    nodes.insert("undo".to_owned(), literal(&[("undone", json!(true))], None));
    WorkflowDefinition {
        definition_id: "aut20-definition".to_owned(),
        version: 1,
        input_keys: Vec::new(),
        output_keys: Vec::new(),
        allowed_roles: vec!["builder".to_owned()],
        max_duration_ms: 60_000,
        max_steps: 8,
        nodes,
        artifacts: BTreeMap::new(),
    }
}

/// 失败实例 + 原执行 attempt 1 的 execution ID。
fn failed_state() -> (AutomationState, RequestId) {
    let a = authority(RequestId::new(), 100);
    let mut state = AutomationState::default();
    let (next, _) = kiana_workflow::plan_command(
        &state,
        &AutomationCommand::RegisterDefinition {
            definition: definition(),
        },
        &register_authority(),
        &AutomationProof::default(),
    )
    .expect("definition registers");
    state = next;
    let (next, _) = kiana_workflow::plan_command(
        &state,
        &AutomationCommand::Start {
            instance_id: "instance-original".to_owned(),
            definition_id: "aut20-definition".to_owned(),
            version: 1,
            inputs: BTreeMap::new(),
        },
        &a,
        &AutomationProof::default(),
    )
    .expect("instance starts");
    state = next;
    // 第一次 Advance 运行 a_work 并成功；它的 execution 就是“原执行”。
    let work_authority = authority(RequestId::new(), 110);
    let (next, _) = kiana_workflow::plan_command(
        &state,
        &AutomationCommand::Advance {
            instance_id: "instance-original".to_owned(),
        },
        &work_authority,
        &AutomationProof::default(),
    )
    .expect("a_work succeeds");
    state = next;
    let original_execution = state.instances["instance-original"].nodes["a_work"].execution_id;
    // 第二次 Advance 运行 b_fail；缺少 `ok` 输入使 Gate 失败，实例进入 Failed。
    let (next, _) = kiana_workflow::plan_command(
        &state,
        &AutomationCommand::Advance {
            instance_id: "instance-original".to_owned(),
        },
        &authority(RequestId::new(), 120),
        &AutomationProof::default(),
    )
    .expect("gate failure is a planned fact");
    state = next;
    assert_eq!(
        state.instances["instance-original"].status,
        WorkflowInstanceStatus::Failed
    );
    (state, original_execution)
}

fn plan_for(
    original_execution: &RequestId,
    compensation_execution: &RequestId,
) -> AutomationCompensationPlan {
    AutomationCompensationPlan {
        schema: AUTOMATION_COMPENSATION_SCHEMA.to_owned(),
        original_instance_id: "instance-original".to_owned(),
        original_execution_id: original_execution.to_string(),
        original_attempt: 1,
        original_authorization_epoch: 4,
        original_action_digest: A.to_owned(),
        original_authorization_digest: B.to_owned(),
        compensation_instance_id: "instance-compensation".to_owned(),
        compensation_execution_id: compensation_execution.to_string(),
        compensation_action_digest: C.to_owned(),
        authority_epoch: 5,
        fresh_authorization_digest: C.to_owned(),
        reused_original_permit: false,
        state: AutomationCompensationState::Planned,
        plan_digest: String::new(),
    }
    .seal()
    .expect("fixture plan is valid")
}

/// 用当前字段重算 `plan_digest` 但不做语义校验，用于构造“结构自洽但策略非法”的拒绝夹具。
/// 领域 `seal()` 会先拒绝，所以不能拿它造这些载荷。
fn resigned(mut plan: AutomationCompensationPlan) -> AutomationCompensationPlan {
    let mut value = serde_json::to_value(&plan).expect("plan serializes");
    value
        .as_object_mut()
        .expect("plan is an object")
        .remove("plan_digest");
    plan.plan_digest = kiana_domain::json_digest(&value);
    plan
}

fn live_epoch() -> u64 {
    5
}

#[test]
fn source_instance_must_exist_and_be_known_failed_or_cancelled() {
    let (state, original_execution) = failed_state();
    let compensation_execution = RequestId::new();
    let a = authority(compensation_execution, 200);

    // 1. 源实例不存在：计划不能凭空引用一个执行。
    let mut missing = plan_for(&original_execution, &compensation_execution);
    missing.original_instance_id = "instance-unknown".to_owned();
    let missing = missing.seal().expect("shape is valid");
    assert_eq!(
        plan_compensation_admission(
            &state,
            &missing,
            &a,
            &AutomationProof::default(),
            live_epoch()
        ),
        Err(AUTOMATION_COMPENSATION_ORIGINAL_INSTANCE_MISSING)
    );

    // 2. 源实例只是 Ready（尚未失败）：不能补偿一个还没结束的运行。
    let mut ready = AutomationState::default();
    let eager = authority(RequestId::new(), 100);
    let (next, _) = kiana_workflow::plan_command(
        &ready,
        &AutomationCommand::RegisterDefinition {
            definition: definition(),
        },
        &register_authority(),
        &AutomationProof::default(),
    )
    .unwrap();
    ready = next;
    let (next, _) = kiana_workflow::plan_command(
        &ready,
        &AutomationCommand::Start {
            instance_id: "instance-original".to_owned(),
            definition_id: "aut20-definition".to_owned(),
            version: 1,
            inputs: BTreeMap::new(),
        },
        &eager,
        &AutomationProof::default(),
    )
    .unwrap();
    ready = next;
    assert_eq!(
        plan_compensation_admission(
            &ready,
            &plan_for(&original_execution, &compensation_execution),
            &a,
            &AutomationProof::default(),
            live_epoch()
        ),
        Err(AUTOMATION_COMPENSATION_ORIGINAL_NOT_TERMINAL)
    );

    // 3. 源实例已成功：成功不需要补偿，且改写成补偿会污染原事实。
    let mut succeeded = AutomationState::default();
    let (next, _) = kiana_workflow::plan_command(
        &succeeded,
        &AutomationCommand::RegisterDefinition {
            definition: definition(),
        },
        &register_authority(),
        &AutomationProof::default(),
    )
    .unwrap();
    succeeded = next;
    let mut inputs = BTreeMap::new();
    inputs.insert("ok".to_owned(), json!(true));
    let (next, _) = kiana_workflow::plan_command(
        &succeeded,
        &AutomationCommand::Start {
            instance_id: "instance-original".to_owned(),
            definition_id: "aut20-definition".to_owned(),
            version: 1,
            inputs,
        },
        &eager,
        &AutomationProof::default(),
    )
    .unwrap();
    succeeded = next;
    for now in [110, 120] {
        let (next, _) = kiana_workflow::plan_command(
            &succeeded,
            &AutomationCommand::Advance {
                instance_id: "instance-original".to_owned(),
            },
            &authority(RequestId::new(), now),
            &AutomationProof::default(),
        )
        .unwrap();
        succeeded = next;
    }
    assert_eq!(
        succeeded.instances["instance-original"].status,
        WorkflowInstanceStatus::Succeeded
    );
    assert_eq!(
        plan_compensation_admission(
            &succeeded,
            &plan_for(&original_execution, &compensation_execution),
            &a,
            &AutomationProof::default(),
            live_epoch()
        ),
        Err(AUTOMATION_COMPENSATION_ORIGINAL_NOT_TERMINAL)
    );
}

#[test]
fn original_execution_and_attempt_must_match_a_real_node_attempt() {
    let (state, original_execution) = failed_state();
    let compensation_execution = RequestId::new();
    let a = authority(compensation_execution, 200);

    // 原执行 ID 不在源实例的节点尝试中。
    let mut unknown = plan_for(&RequestId::new(), &compensation_execution);
    unknown.original_execution_id = RequestId::new().to_string();
    let unknown = unknown.seal().expect("shape is valid");
    assert_eq!(
        plan_compensation_admission(
            &state,
            &unknown,
            &a,
            &AutomationProof::default(),
            live_epoch()
        ),
        Err(AUTOMATION_COMPENSATION_ORIGINAL_ATTEMPT_MISSING)
    );

    // 执行 ID 存在但 attempt 序号不符。
    let mut mismatch = plan_for(&original_execution, &compensation_execution);
    mismatch.original_attempt = 2;
    let mismatch = mismatch.seal().expect("shape is valid");
    assert_eq!(
        plan_compensation_admission(
            &state,
            &mismatch,
            &a,
            &AutomationProof::default(),
            live_epoch()
        ),
        Err(AUTOMATION_COMPENSATION_ORIGINAL_ATTEMPT_MISMATCH)
    );
}

#[test]
fn compensation_must_use_fresh_execution_and_live_authorization_epoch() {
    let (state, original_execution) = failed_state();

    // 1. 补偿执行身份必须等于本次 authority 的 execution，而不是原执行或其他 UUID。
    let compensation_execution = RequestId::new();
    let a = authority(compensation_execution, 200);
    let mut other_execution = plan_for(&original_execution, &RequestId::new());
    other_execution.compensation_execution_id = RequestId::new().to_string();
    let other_execution = other_execution.seal().expect("shape is valid");
    assert_eq!(
        plan_compensation_admission(
            &state,
            &other_execution,
            &a,
            &AutomationProof::default(),
            live_epoch()
        ),
        Err(AUTOMATION_COMPENSATION_EXECUTION_IDENTITY_MISMATCH)
    );

    // 2. 计划的 epoch 必须等于调用方读到的 live 代次：旧代次、超前代次、零都拒绝。
    let plan = plan_for(&original_execution, &compensation_execution);
    for live in [4, 6, 0] {
        assert_eq!(
            plan_compensation_admission(&state, &plan, &a, &AutomationProof::default(), live),
            Err(AUTOMATION_COMPENSATION_AUTHORIZATION_STALE)
        );
    }

    // 3. 计划自身的代次不严格晚于原授权（领域层拒绝先于准入）。
    let mut inherited = plan_for(&original_execution, &compensation_execution);
    inherited.authority_epoch = 4;
    let inherited = resigned(inherited);
    assert_eq!(
        plan_compensation_admission(&state, &inherited, &a, &AutomationProof::default(), 4),
        Err(kiana_domain::AUTOMATION_COMPENSATION_AUTHORIZATION_NOT_FRESH)
    );
}

#[test]
fn compensation_action_must_differ_from_the_original_action() {
    let (state, original_execution) = failed_state();
    let compensation_execution = RequestId::new();
    let a = authority(compensation_execution, 200);
    let mut same_action = plan_for(&original_execution, &compensation_execution);
    same_action.compensation_action_digest = same_action.original_action_digest.clone();
    let same_action = resigned(same_action);
    assert_eq!(
        plan_compensation_admission(
            &state,
            &same_action,
            &a,
            &AutomationProof::default(),
            live_epoch()
        ),
        Err(AUTOMATION_COMPENSATION_ACTION_UNCHANGED)
    );
}

#[test]
fn second_compensation_for_one_source_is_refused() {
    let (state, original_execution) = failed_state();
    let compensation_execution = RequestId::new();
    let a = authority(compensation_execution, 200);
    let plan = plan_for(&original_execution, &compensation_execution);
    let (next, admission, effect) =
        plan_compensation_admission(&state, &plan, &a, &AutomationProof::default(), live_epoch())
            .expect("first compensation is admitted");
    assert!(effect.is_none(), "compensation planning dispatches nothing");
    assert_eq!(admission.compensation_instance_id, "instance-compensation");

    // 同一源实例再次请求补偿：拒绝，而不是再建一个补偿实例。
    assert_eq!(
        plan_compensation_admission(&next, &plan, &a, &AutomationProof::default(), live_epoch()),
        Err(AUTOMATION_COMPENSATION_ALREADY_REQUESTED)
    );
    assert_eq!(
        next.instances
            .values()
            .filter(|instance| instance.selected_nodes.is_some())
            .count(),
        1
    );
}

#[test]
fn source_without_declared_compensation_nodes_is_refused() {
    let mut definition = definition();
    // 移除 a_work 声明的补偿节点：已成功节点没有可执行的补偿动作。
    definition.nodes.get_mut("a_work").unwrap().compensation = None;
    definition.nodes.remove("undo");
    let a = authority(RequestId::new(), 100);
    let mut state = AutomationState::default();
    let (next, _) = kiana_workflow::plan_command(
        &state,
        &AutomationCommand::RegisterDefinition { definition },
        &register_authority(),
        &AutomationProof::default(),
    )
    .unwrap();
    state = next;
    let (next, _) = kiana_workflow::plan_command(
        &state,
        &AutomationCommand::Start {
            instance_id: "instance-original".to_owned(),
            definition_id: "aut20-definition".to_owned(),
            version: 1,
            inputs: BTreeMap::new(),
        },
        &a,
        &AutomationProof::default(),
    )
    .unwrap();
    state = next;
    let (next, _) = kiana_workflow::plan_command(
        &state,
        &AutomationCommand::Advance {
            instance_id: "instance-original".to_owned(),
        },
        &authority(RequestId::new(), 110),
        &AutomationProof::default(),
    )
    .unwrap();
    state = next;
    let (next, _) = kiana_workflow::plan_command(
        &state,
        &AutomationCommand::Advance {
            instance_id: "instance-original".to_owned(),
        },
        &authority(RequestId::new(), 120),
        &AutomationProof::default(),
    )
    .unwrap();
    state = next;
    assert_eq!(
        state.instances["instance-original"].status,
        WorkflowInstanceStatus::Failed
    );
    // 原执行 attempt 仍真实存在（a_work 成功），因此拒绝点落在“未声明补偿节点”。
    let original_execution = state.instances["instance-original"].nodes["a_work"].execution_id;
    let compensation_execution = RequestId::new();
    let plan = plan_for(&original_execution, &compensation_execution);
    assert_eq!(
        plan_compensation_admission(
            &state,
            &plan,
            &authority(compensation_execution, 200),
            &AutomationProof::default(),
            live_epoch()
        ),
        Err(AUTOMATION_COMPENSATION_NOT_DECLARED)
    );
}

#[test]
fn admitted_compensation_references_the_original_and_leaves_it_unchanged() {
    let (state, original_execution) = failed_state();
    let compensation_execution = RequestId::new();
    let a = authority(compensation_execution, 200);
    let plan = plan_for(&original_execution, &compensation_execution);
    let original_before = state.instances["instance-original"].clone();

    let (next, admission, effect) =
        plan_compensation_admission(&state, &plan, &a, &AutomationProof::default(), live_epoch())
            .expect("valid pair is admitted");

    // 1. planner 的 Compensate 分支不产生外部 effect，补偿节点仍需后续 Advance 走 ControlPlane。
    assert!(effect.is_none());
    // 2. 源实例事实完全不变：补偿是引用，不是改写。
    assert_eq!(
        next.instances["instance-original"], original_before,
        "compensation must not rewrite the original instance"
    );
    // 3. 补偿实例独立存在、引用原实例、只运行声明过的补偿节点。
    let compensation = &next.instances["instance-compensation"];
    assert_eq!(
        compensation.parent_instance_id.as_deref(),
        Some("instance-original")
    );
    assert_eq!(
        compensation.selected_nodes.as_deref(),
        Some(&["undo".to_owned()][..])
    );
    assert_eq!(compensation.status, WorkflowInstanceStatus::Compensating);
    // 4. 准入事实绑定计划、原 attempt、补偿执行与 live epoch，并自校验通过。
    let expected: AutomationCompensationAdmission = admission.clone();
    assert_eq!(expected.validate(), Ok(()));
    assert_eq!(admission.plan_digest, plan.plan_digest);
    assert_eq!(
        admission.original_execution_id,
        original_execution.to_string()
    );
    assert_eq!(admission.original_attempt, 1);
    assert_eq!(
        admission.compensation_execution_id,
        compensation_execution.to_string()
    );
    assert_eq!(admission.authority_epoch, 5);
    assert_eq!(admission.selected_nodes, vec!["undo".to_owned()]);
    // 5. 补偿运行有自己的一次尝试：规划没有为它创建任何节点执行。
    assert!(compensation.nodes.is_empty());
}

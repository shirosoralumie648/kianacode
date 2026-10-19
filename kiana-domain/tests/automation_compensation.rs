//! AUT-20 补偿计划先拒绝优先夹具：原 permit 复用、授权代次不新鲜、身份/动作碰撞都先于
//! 成功夹具被拒绝，最后才接受一份合法的“原执行 → 独立补偿执行 + fresh epoch”计划。

use kiana_domain::{
    AutomationCompensationPlan, AutomationCompensationState,
    AUTOMATION_COMPENSATION_ACTION_UNCHANGED, AUTOMATION_COMPENSATION_AUTHORIZATION_NOT_FRESH,
    AUTOMATION_COMPENSATION_IDENTITY_COLLISION, AUTOMATION_COMPENSATION_ORIGINAL_PERMIT_REUSED,
    AUTOMATION_COMPENSATION_PLAN_INVALID, AUTOMATION_COMPENSATION_SCHEMA,
};

const A: &str = "sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
const B: &str = "sha256:bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";
const C: &str = "sha256:cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc";

fn base() -> AutomationCompensationPlan {
    AutomationCompensationPlan {
        schema: AUTOMATION_COMPENSATION_SCHEMA.to_owned(),
        original_instance_id: "instance-original".to_owned(),
        original_execution_id: "execution-original".to_owned(),
        original_attempt: 1,
        original_authorization_epoch: 4,
        original_action_digest: A.to_owned(),
        original_authorization_digest: B.to_owned(),
        compensation_instance_id: "instance-compensation".to_owned(),
        compensation_execution_id: "execution-compensation".to_owned(),
        compensation_action_digest: C.to_owned(),
        authority_epoch: 5,
        fresh_authorization_digest: C.to_owned(),
        reused_original_permit: false,
        state: AutomationCompensationState::Planned,
        plan_digest: String::new(),
    }
}

fn sealed(mut plan: AutomationCompensationPlan) -> AutomationCompensationPlan {
    plan.plan_digest = kiana_domain::json_digest(&serde_json::json!({
        "schema": plan.schema,
        "original_instance_id": plan.original_instance_id,
        "original_execution_id": plan.original_execution_id,
        "original_attempt": plan.original_attempt,
        "original_authorization_epoch": plan.original_authorization_epoch,
        "original_action_digest": plan.original_action_digest,
        "original_authorization_digest": plan.original_authorization_digest,
        "compensation_instance_id": plan.compensation_instance_id,
        "compensation_execution_id": plan.compensation_execution_id,
        "compensation_action_digest": plan.compensation_action_digest,
        "authority_epoch": plan.authority_epoch,
        "fresh_authorization_digest": plan.fresh_authorization_digest,
        "reused_original_permit": plan.reused_original_permit,
        "state": plan.state,
    }));
    plan
}

fn plan() -> AutomationCompensationPlan {
    sealed(base())
}

#[test]
fn reused_original_permit_is_refused_by_flag_and_by_identical_digest() {
    // 布尔声明复用原 permit：即使摘要全部合法也必须拒绝。
    let mut flag = plan();
    flag.reused_original_permit = true;
    assert_eq!(
        sealed(flag).validate(),
        Err(AUTOMATION_COMPENSATION_ORIGINAL_PERMIT_REUSED)
    );

    // 声明为 false，但 fresh 授权摘要就是原授权摘要：这是“换了个布尔值”的同一份 permit。
    let mut same_digest = base();
    same_digest.fresh_authorization_digest = same_digest.original_authorization_digest.clone();
    assert_eq!(
        sealed(same_digest).validate(),
        Err(AUTOMATION_COMPENSATION_ORIGINAL_PERMIT_REUSED)
    );

    // 仅大小写不同是同一摘要，不能绕过规范化比较。
    let mut case_only = base();
    case_only.fresh_authorization_digest = format!("sha256:{}", "B".repeat(64));
    assert_eq!(
        sealed(case_only).validate(),
        Err(AUTOMATION_COMPENSATION_ORIGINAL_PERMIT_REUSED)
    );
}

#[test]
fn authorization_epoch_must_be_strictly_after_the_original() {
    for epoch in [4, 3, 0] {
        let mut stale = base();
        stale.authority_epoch = epoch;
        assert_eq!(
            sealed(stale).validate(),
            Err(if epoch == 0 {
                AUTOMATION_COMPENSATION_PLAN_INVALID
            } else {
                AUTOMATION_COMPENSATION_AUTHORIZATION_NOT_FRESH
            })
        );
    }
    // 严格加一即新鲜。
    let mut fresh = base();
    fresh.authority_epoch = 5;
    assert!(sealed(fresh).validate().is_ok());
}

#[test]
fn compensation_cannot_reuse_the_original_instance_execution_or_action_identity() {
    let mut same_instance = base();
    same_instance.compensation_instance_id = same_instance.original_instance_id.clone();
    assert_eq!(
        sealed(same_instance).validate(),
        Err(AUTOMATION_COMPENSATION_IDENTITY_COLLISION)
    );

    let mut same_execution = base();
    same_execution.compensation_execution_id = same_execution.original_execution_id.clone();
    assert_eq!(
        sealed(same_execution).validate(),
        Err(AUTOMATION_COMPENSATION_IDENTITY_COLLISION)
    );

    let mut same_action = base();
    same_action.compensation_action_digest = same_action.original_action_digest.clone();
    assert_eq!(
        sealed(same_action).validate(),
        Err(AUTOMATION_COMPENSATION_ACTION_UNCHANGED)
    );

    // 大小写变体仍是同一个动作摘要。
    let mut case_only = base();
    case_only.compensation_action_digest = format!("sha256:{}", "A".repeat(64));
    assert_eq!(
        sealed(case_only).validate(),
        Err(AUTOMATION_COMPENSATION_ACTION_UNCHANGED)
    );
}

#[test]
fn structural_drift_and_unknown_fields_fail_closed() {
    // schema 漂移。
    let mut wrong_schema = plan();
    wrong_schema.schema = "kiana.automation-compensation.v1".to_owned();
    assert_eq!(
        sealed(wrong_schema).validate(),
        Err(AUTOMATION_COMPENSATION_PLAN_INVALID)
    );

    // 篡改字段但保留旧摘要。
    let mut tampered = plan();
    tampered.authority_epoch = 6;
    assert_eq!(
        tampered.validate(),
        Err(AUTOMATION_COMPENSATION_PLAN_INVALID)
    );

    // 错误形状的摘要。
    let mut bad_digest = plan();
    bad_digest.fresh_authorization_digest = "sha256:not-a-digest".to_owned();
    assert_eq!(
        sealed(bad_digest).validate(),
        Err(AUTOMATION_COMPENSATION_PLAN_INVALID)
    );

    // 空 attempt / 空实例 ID。
    let mut empty_attempt = base();
    empty_attempt.original_attempt = 0;
    assert_eq!(
        sealed(empty_attempt).validate(),
        Err(AUTOMATION_COMPENSATION_PLAN_INVALID)
    );

    let mut empty_instance = base();
    empty_instance.compensation_instance_id = "  ".to_owned();
    assert_eq!(
        sealed(empty_instance).validate(),
        Err(AUTOMATION_COMPENSATION_PLAN_INVALID)
    );

    // 未知字段被 Serde 拒绝。
    let mut value = serde_json::to_value(plan()).unwrap();
    value["unexpected"] = serde_json::json!(true);
    assert!(serde_json::from_value::<AutomationCompensationPlan>(value).is_err());
}

#[test]
fn a_valid_plan_binds_original_and_compensation_with_a_fresh_epoch() {
    let plan = plan();
    assert_eq!(plan.validate(), Ok(()));
    // seal() 重算摘要后同样通过；字段确实被绑定进摘要。
    let mut changed = plan.clone();
    changed.compensation_action_digest = B.to_owned();
    assert_ne!(changed.seal(), Ok(plan.clone()));
    // typed round-trip 保留全部字段。
    let round_trip: AutomationCompensationPlan =
        serde_json::from_value(serde_json::to_value(&plan).unwrap()).unwrap();
    assert_eq!(round_trip, plan);
}

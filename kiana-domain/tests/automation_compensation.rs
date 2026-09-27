use kiana_domain::{
    AutomationCompensationPlan, AutomationCompensationState, AUTOMATION_COMPENSATION_SCHEMA,
};
use serde_json::json;

const A: &str = "sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
const B: &str = "sha256:bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";

fn plan() -> AutomationCompensationPlan {
    let mut value = AutomationCompensationPlan {
        schema: AUTOMATION_COMPENSATION_SCHEMA.to_owned(),
        original_execution_id: "original".to_owned(),
        original_attempt: 1,
        original_action_digest: A.to_owned(),
        compensation_action_digest: B.to_owned(),
        compensation_execution_id: "compensation".to_owned(),
        authority_epoch: 5,
        fresh_authorization_digest: A.to_owned(),
        reused_original_permit: false,
        state: AutomationCompensationState::Planned,
        plan_digest: String::new(),
    };
    value.plan_digest = kiana_domain::json_digest(&json!({
        "schema": value.schema,
        "original_execution_id": value.original_execution_id,
        "original_attempt": value.original_attempt,
        "original_action_digest": value.original_action_digest,
        "compensation_action_digest": value.compensation_action_digest,
        "compensation_execution_id": value.compensation_execution_id,
        "authority_epoch": value.authority_epoch,
        "fresh_authorization_digest": value.fresh_authorization_digest,
        "reused_original_permit": value.reused_original_permit,
        "state": value.state,
    }));
    value
}

#[test]
fn compensation_requires_new_execution_action_and_authorization() {
    assert!(plan().validate().is_ok());
    let mut reused = plan();
    reused.reused_original_permit = true;
    reused.plan_digest = kiana_domain::json_digest(&json!({
        "schema": reused.schema,
        "original_execution_id": reused.original_execution_id,
        "original_attempt": reused.original_attempt,
        "original_action_digest": reused.original_action_digest,
        "compensation_action_digest": reused.compensation_action_digest,
        "compensation_execution_id": reused.compensation_execution_id,
        "authority_epoch": reused.authority_epoch,
        "fresh_authorization_digest": reused.fresh_authorization_digest,
        "reused_original_permit": reused.reused_original_permit,
        "state": reused.state,
    }));
    assert_eq!(
        reused.validate(),
        Err("automation_compensation_plan_invalid")
    );
}

#[test]
fn unknown_fields_fail_closed() {
    let mut value = serde_json::to_value(plan()).unwrap();
    value["unexpected"] = json!(true);
    assert!(serde_json::from_value::<AutomationCompensationPlan>(value).is_err());
}

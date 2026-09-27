use kiana_domain::{
    SwarmPlanId, SwarmRetirementFact, SwarmRetirementState, SWARM_RETIREMENT_SCHEMA,
};
use serde_json::json;

const D1: &str = "sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
const D2: &str = "sha256:bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";

fn digest(fact: &SwarmRetirementFact) -> String {
    kiana_domain::json_digest(&json!({
        "schema": fact.schema,
        "swarm_plan_id": fact.swarm_plan_id,
        "source_cursor": fact.source_cursor,
        "child_count": fact.child_count,
        "terminal_known_count": fact.terminal_known_count,
        "unknown_count": fact.unknown_count,
        "residual_budget_digest": fact.residual_budget_digest,
        "path_lock_release_digest": fact.path_lock_release_digest,
        "release_receipt_digest": fact.release_receipt_digest,
        "release_attempt": fact.release_attempt,
        "state": fact.state,
        "facts_retained": fact.facts_retained,
    }))
}

fn fact(state: SwarmRetirementState) -> SwarmRetirementFact {
    let mut fact = SwarmRetirementFact {
        schema: SWARM_RETIREMENT_SCHEMA.to_owned(),
        swarm_plan_id: SwarmPlanId::new(),
        source_cursor: 10,
        child_count: 2,
        terminal_known_count: if matches!(state, SwarmRetirementState::BlockedUnknown) {
            1
        } else {
            2
        },
        unknown_count: u32::from(matches!(state, SwarmRetirementState::BlockedUnknown)),
        residual_budget_digest: D1.to_owned(),
        path_lock_release_digest: D2.to_owned(),
        release_receipt_digest: D1.to_owned(),
        release_attempt: 1,
        state,
        facts_retained: true,
        retirement_digest: String::new(),
    };
    fact.retirement_digest = digest(&fact);
    fact
}

#[test]
fn release_and_retire_require_known_children_and_one_release() {
    assert!(fact(SwarmRetirementState::Pending).validate().is_ok());
    assert!(fact(SwarmRetirementState::Released).validate().is_ok());
    assert!(fact(SwarmRetirementState::Retired).validate().is_ok());
    assert!(fact(SwarmRetirementState::BlockedUnknown)
        .validate()
        .is_ok());
}

#[test]
fn unknown_or_repeat_release_and_unknown_fields_fail_closed() {
    let mut repeated = fact(SwarmRetirementState::Released);
    repeated.release_attempt = 2;
    repeated.retirement_digest = digest(&repeated);
    assert_eq!(
        repeated.validate(),
        Err("swarm_retirement_release_not_exactly_once")
    );

    let mut value = serde_json::to_value(fact(SwarmRetirementState::Pending)).unwrap();
    value["unexpected"] = json!(true);
    assert!(serde_json::from_value::<SwarmRetirementFact>(value).is_err());
}

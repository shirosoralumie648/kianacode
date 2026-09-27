use kiana_domain::{AutomationFanoutPlan, AUTOMATION_FANOUT_SCHEMA};
use serde_json::json;

fn plan() -> AutomationFanoutPlan {
    let mut value = AutomationFanoutPlan {
        schema: AUTOMATION_FANOUT_SCHEMA.to_owned(),
        parent_execution_id: "parent".to_owned(),
        child_execution_ids: vec!["child-a".to_owned(), "child-b".to_owned()],
        max_depth: 3,
        depth: 1,
        max_concurrency: 2,
        requested_concurrency: 2,
        parent_budget_units: 10,
        child_budget_units: 4,
        ttl_ms: 100,
        parent_ttl_remaining_ms: 200,
        child_scope_is_subset: true,
        parent_id_in_children: false,
        fail_fast: true,
        fan_in_required: true,
        plan_digest: String::new(),
    };
    value.plan_digest = kiana_domain::json_digest(&json!({
        "schema": value.schema,
        "parent_execution_id": value.parent_execution_id,
        "child_execution_ids": value.child_execution_ids,
        "max_depth": value.max_depth,
        "depth": value.depth,
        "max_concurrency": value.max_concurrency,
        "requested_concurrency": value.requested_concurrency,
        "parent_budget_units": value.parent_budget_units,
        "child_budget_units": value.child_budget_units,
        "ttl_ms": value.ttl_ms,
        "parent_ttl_remaining_ms": value.parent_ttl_remaining_ms,
        "child_scope_is_subset": value.child_scope_is_subset,
        "parent_id_in_children": value.parent_id_in_children,
        "fail_fast": value.fail_fast,
        "fan_in_required": value.fan_in_required,
    }));
    value
}

#[test]
fn bounded_fanout_and_fanin_plan_is_valid() {
    assert!(plan().validate().is_ok());
}

#[test]
fn scope_budget_depth_ttl_and_cycle_fences_fail_closed() {
    let mut invalid = plan();
    invalid.child_scope_is_subset = false;
    invalid.plan_digest = kiana_domain::json_digest(&json!({
        "schema": invalid.schema,
        "parent_execution_id": invalid.parent_execution_id,
        "child_execution_ids": invalid.child_execution_ids,
        "max_depth": invalid.max_depth,
        "depth": invalid.depth,
        "max_concurrency": invalid.max_concurrency,
        "requested_concurrency": invalid.requested_concurrency,
        "parent_budget_units": invalid.parent_budget_units,
        "child_budget_units": invalid.child_budget_units,
        "ttl_ms": invalid.ttl_ms,
        "parent_ttl_remaining_ms": invalid.parent_ttl_remaining_ms,
        "child_scope_is_subset": invalid.child_scope_is_subset,
        "parent_id_in_children": invalid.parent_id_in_children,
        "fail_fast": invalid.fail_fast,
        "fan_in_required": invalid.fan_in_required,
    }));
    assert_eq!(invalid.validate(), Err("automation_fanout_plan_invalid"));

    let mut cycle = plan();
    cycle.parent_id_in_children = true;
    cycle.plan_digest = kiana_domain::json_digest(&json!({
        "schema": cycle.schema,
        "parent_execution_id": cycle.parent_execution_id,
        "child_execution_ids": cycle.child_execution_ids,
        "max_depth": cycle.max_depth,
        "depth": cycle.depth,
        "max_concurrency": cycle.max_concurrency,
        "requested_concurrency": cycle.requested_concurrency,
        "parent_budget_units": cycle.parent_budget_units,
        "child_budget_units": cycle.child_budget_units,
        "ttl_ms": cycle.ttl_ms,
        "parent_ttl_remaining_ms": cycle.parent_ttl_remaining_ms,
        "child_scope_is_subset": cycle.child_scope_is_subset,
        "parent_id_in_children": cycle.parent_id_in_children,
        "fail_fast": cycle.fail_fast,
        "fan_in_required": cycle.fan_in_required,
    }));
    assert_eq!(cycle.validate(), Err("automation_fanout_plan_invalid"));
}

#[test]
fn unknown_fields_fail_closed() {
    let mut value = serde_json::to_value(plan()).unwrap();
    value["unexpected"] = json!(true);
    assert!(serde_json::from_value::<AutomationFanoutPlan>(value).is_err());
}

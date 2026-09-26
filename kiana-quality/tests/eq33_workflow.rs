use kiana_quality::{DeterministicEvaluator, WorkflowSwarmEvaluator, WORKFLOW_SWARM_INPUT_SCHEMA};
use serde_json::{json, Value};

const DIGEST: &str = "sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
const OTHER_DIGEST: &str =
    "sha256:bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";

fn node(
    node_id: &str,
    parent_ids: Value,
    depth: u64,
    expected_children: u64,
    observed_children: u64,
) -> Value {
    json!({
        "schema": "kiana.quality-workflow-node-evidence.v1",
        "node_id": node_id,
        "parent_ids": parent_ids,
        "depth": depth,
        "attempts": [1],
        "parent_scope": ["project:alpha"],
        "child_scope": ["project:alpha"],
        "expected_children": expected_children,
        "observed_children": observed_children,
        "required_parents": 0,
        "observed_parents": 0,
        "merge_verified": false,
        "merge_expected_digest": null,
        "merge_observed_digest": null,
        "compensation_required": false,
        "compensation_recorded": false,
        "status": "succeeded",
    })
}

fn valid_input() -> Value {
    let mut root = node("root", json!([]), 0, 1, 1);
    root["required_parents"] = json!(0);
    let mut child = node("child", json!(["root"]), 1, 0, 0);
    child["required_parents"] = json!(1);
    child["observed_parents"] = json!(1);
    json!({
        "schema": WORKFLOW_SWARM_INPUT_SCHEMA,
        "max_fan_out": 4,
        "max_depth": 4,
        "nodes": [root, child],
    })
}

#[test]
fn bounded_dag_attempts_and_scopes_have_no_findings() {
    let findings = WorkflowSwarmEvaluator.evaluate(&valid_input()).unwrap();
    assert!(findings.is_empty(), "unexpected findings: {findings:?}");
}

#[test]
fn child_scope_expansion_fails_quality_gate() {
    let mut input = valid_input();
    input["nodes"][1]["child_scope"] = json!(["project:alpha", "network:external"]);
    let findings = WorkflowSwarmEvaluator.evaluate(&input).unwrap();
    assert!(findings
        .iter()
        .any(|finding| finding.code == "workflow.child_scope_expanded"));
}

#[test]
fn dag_attempt_fanout_merge_and_compensation_fail_closed() {
    let mut input = valid_input();
    input["nodes"][0]["parent_ids"] = json!(["child"]);
    input["nodes"][0]["expected_children"] = json!(5);
    input["nodes"][0]["observed_children"] = json!(5);
    input["nodes"][1]["attempts"] = json!([2, 1]);
    input["nodes"][1]["expected_children"] = json!(1);
    input["nodes"][1]["observed_children"] = json!(2);
    input["nodes"][1]["required_parents"] = json!(2);
    input["nodes"][1]["observed_parents"] = json!(1);
    input["nodes"][1]["merge_verified"] = json!(false);
    input["nodes"][1]["merge_expected_digest"] = json!(DIGEST);
    input["nodes"][1]["merge_observed_digest"] = json!(OTHER_DIGEST);
    input["nodes"][1]["compensation_required"] = json!(true);
    input["nodes"][1]["compensation_recorded"] = json!(false);

    let findings = WorkflowSwarmEvaluator.evaluate(&input).unwrap();
    let codes: Vec<_> = findings
        .iter()
        .map(|finding| finding.code.as_str())
        .collect();
    assert!(codes.contains(&"workflow.dag_cycle"));
    assert!(codes.contains(&"workflow.attempt_invalid"));
    assert!(codes.contains(&"workflow.fanout_exceeded"));
    assert!(codes.contains(&"workflow.fanin_mismatch"));
    assert!(codes.contains(&"workflow.merge_missing"));
    assert!(codes.contains(&"workflow.merge_mismatch"));
    assert!(codes.contains(&"workflow.compensation_missing"));
}

#[test]
fn unknown_fields_fail_closed() {
    let mut input = valid_input();
    input["unexpected"] = json!(true);
    assert!(WorkflowSwarmEvaluator.evaluate(&input).is_err());
}

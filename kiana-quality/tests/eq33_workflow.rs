use kiana_quality::{
    canonical_findings, digest_findings, evaluate_workflow_swarm, DeterministicEvaluator,
    EvaluatorRegistry, Finding, WorkflowNodeEvidence, WorkflowNodeStatus, WorkflowSwarmEvaluator,
    WorkflowSwarmInput, EVALUATION_RESULT_SCHEMA, WORKFLOW_SWARM_EVALUATOR_ID,
    WORKFLOW_SWARM_INPUT_SCHEMA,
};
use serde_json::{json, Value};
use std::collections::BTreeSet;

const NODE_SCHEMA: &str = "kiana.quality-workflow-node-evidence.v1";
const DIGEST: &str = "sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
const OTHER_DIGEST: &str =
    "sha256:bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";

fn node(node_id: &str) -> Value {
    json!({
        "schema": NODE_SCHEMA,
        "node_id": node_id,
        "parent_ids": [],
        "depth": 0,
        "attempts": [1],
        "parent_scope": ["project:alpha"],
        "child_scope": ["project:alpha"],
        "expected_children": 0,
        "observed_children": 0,
        "required_parents": 0,
        "observed_parents": 0,
        "merge_verified": false,
        "merge_expected_digest": null,
        "merge_observed_digest": null,
        "compensation_required": false,
        "compensation_recorded": false,
        "compensation_receipt_ref": "",
        "status": "succeeded",
    })
}

fn with_parents(node_id: &str, parents: &[&str], depth: u64) -> Value {
    let mut value = node(node_id);
    value["parent_ids"] = json!(parents);
    value["depth"] = json!(depth);
    value["required_parents"] = json!(parents.len() as u64);
    value["observed_parents"] = json!(parents.len() as u64);
    value
}

fn input(nodes: Vec<Value>) -> Value {
    json!({
        "schema": WORKFLOW_SWARM_INPUT_SCHEMA,
        "max_fan_out": 4,
        "max_depth": 4,
        "nodes": nodes,
    })
}

fn valid_input() -> Value {
    let mut root = node("root");
    root["expected_children"] = json!(1);
    root["observed_children"] = json!(1);
    let child = with_parents("child", &["root"], 1);
    input(vec![root, child])
}

fn fan_in_input() -> Value {
    let mut root = node("root");
    root["expected_children"] = json!(2);
    root["observed_children"] = json!(2);
    let mut left = with_parents("left", &["root"], 1);
    left["expected_children"] = json!(1);
    left["observed_children"] = json!(1);
    let mut right = with_parents("right", &["root"], 1);
    right["expected_children"] = json!(1);
    right["observed_children"] = json!(1);
    let mut join = with_parents("join", &["left", "right"], 2);
    join["merge_verified"] = json!(true);
    join["merge_expected_digest"] = json!(DIGEST);
    join["merge_observed_digest"] = json!(DIGEST);
    input(vec![root, left, right, join])
}

fn evaluate(input: &Value) -> Vec<Finding> {
    WorkflowSwarmEvaluator
        .evaluate(input)
        .expect("workflow fixture decodes")
}

fn codes(findings: &[Finding]) -> BTreeSet<String> {
    findings
        .iter()
        .map(|finding| finding.code.clone())
        .collect()
}

fn set(items: &[&str]) -> BTreeSet<String> {
    items.iter().map(|item| (*item).to_owned()).collect()
}

fn expected_literals(findings: &[Finding], code: &str) -> Vec<String> {
    findings
        .iter()
        .filter(|finding| finding.code == code)
        .filter_map(|finding| finding.expected.as_str().map(|value| value.to_owned()))
        .collect()
}

fn node_ref(input: &Value, node_id: &str) -> String {
    let mut ids: Vec<&str> = input["nodes"]
        .as_array()
        .expect("nodes array")
        .iter()
        .map(|node| node["node_id"].as_str().expect("node id"))
        .collect();
    ids.sort_unstable();
    let index = ids
        .iter()
        .position(|id| *id == node_id)
        .expect("node present");
    format!("workflow:node:{index}")
}

#[test]
fn bounded_dag_attempts_and_scopes_have_no_findings() {
    assert!(evaluate(&valid_input()).is_empty());
    assert!(evaluate(&fan_in_input()).is_empty());
}

#[test]
fn child_scope_expansion_fails_quality_gate() {
    let mut fixture = valid_input();
    fixture["nodes"][1]["child_scope"] = json!(["project:alpha", "network:external"]);
    let findings = evaluate(&fixture);
    assert_eq!(findings.len(), 1, "unexpected findings: {findings:?}");
    assert_eq!(findings[0].code, "workflow.child_scope_expanded");
    assert_eq!(findings[0].evidence_ref, node_ref(&fixture, "child"));
}

#[test]
fn dag_attempt_fanout_merge_and_compensation_fail_closed() {
    let mut root = node("root");
    root["parent_ids"] = json!(["child"]);
    root["required_parents"] = json!(1);
    root["observed_parents"] = json!(1);
    root["expected_children"] = json!(5);
    root["observed_children"] = json!(5);
    let parent = node("parent");
    let mut child = with_parents("child", &["root", "parent"], 1);
    child["attempts"] = json!([2, 1]);
    child["expected_children"] = json!(1);
    child["observed_children"] = json!(2);
    child["observed_parents"] = json!(1);
    child["merge_verified"] = json!(true);
    child["merge_expected_digest"] = json!(DIGEST);
    child["merge_observed_digest"] = json!(OTHER_DIGEST);
    child["compensation_required"] = json!(true);
    child["compensation_recorded"] = json!(false);

    let fixture = input(vec![root, parent, child]);
    let findings = evaluate(&fixture);
    let found = codes(&findings);
    for expected in [
        "workflow.dag_cycle",
        "workflow.depth_inconsistent",
        "workflow.attempt_invalid",
        "workflow.fanout_exceeded",
        "workflow.fanin_mismatch",
        "workflow.merge_mismatch",
        "workflow.compensation_missing",
    ] {
        assert!(
            found.contains(expected),
            "missing {expected} in {findings:?}"
        );
    }
}

#[test]
fn unknown_fields_fail_closed() {
    let mut top_level = valid_input();
    top_level["unexpected"] = json!(true);
    assert!(WorkflowSwarmEvaluator.evaluate(&top_level).is_err());

    let mut nested = valid_input();
    nested["nodes"][0]["unexpected"] = json!(true);
    assert!(WorkflowSwarmEvaluator.evaluate(&nested).is_err());

    let mut wrong_type = valid_input();
    wrong_type["max_fan_out"] = Value::Null;
    assert!(WorkflowSwarmEvaluator.evaluate(&wrong_type).is_err());

    let mut missing_field = valid_input();
    missing_field["nodes"][0]
        .as_object_mut()
        .expect("node object")
        .remove("attempts");
    assert!(WorkflowSwarmEvaluator.evaluate(&missing_field).is_err());

    for malformed in [json!([]), json!("input"), Value::Null] {
        assert!(WorkflowSwarmEvaluator.evaluate(&malformed).is_err());
    }
}

#[test]
fn dag_cycles_missing_parents_and_self_parents_are_rejected() {
    let cycle_input = input(vec![
        with_parents("a", &["b"], 1),
        with_parents("b", &["a"], 2),
    ]);
    let cycle_findings = evaluate(&cycle_input);
    assert_eq!(
        codes(&cycle_findings),
        set(&["workflow.dag_cycle", "workflow.depth_inconsistent"])
    );
    let cycle_entries: Vec<&Finding> = cycle_findings
        .iter()
        .filter(|finding| finding.code == "workflow.dag_cycle")
        .collect();
    assert_eq!(cycle_entries.len(), 1);
    assert_eq!(cycle_entries[0].evidence_ref, "workflow:dag");

    let orphan_input = input(vec![with_parents("orphan", &["ghost"], 1)]);
    let orphan_findings = evaluate(&orphan_input);
    assert_eq!(codes(&orphan_findings), set(&["workflow.parent_missing"]));
    assert_eq!(
        orphan_findings[0].evidence_ref,
        node_ref(&orphan_input, "orphan")
    );

    let self_input = input(vec![with_parents("self", &["self"], 0)]);
    let self_findings = evaluate(&self_input);
    assert_eq!(
        codes(&self_findings),
        set(&["workflow.parent_self", "workflow.dag_cycle"])
    );
}

#[test]
fn node_identity_parent_identity_and_bounded_references_are_rejected() {
    let invalid_id = evaluate(&input(vec![node("bad node")]));
    assert_eq!(invalid_id.len(), 1, "unexpected findings: {invalid_id:?}");
    assert_eq!(invalid_id[0].code, "workflow.node_identity_invalid");

    let duplicate = evaluate(&input(vec![node("dup"), node("dup")]));
    assert_eq!(
        duplicate.len(),
        1,
        "duplicate evidence is visible once per extra occurrence"
    );
    assert_eq!(duplicate[0].code, "workflow.node_identity_invalid");

    let bad_parent = evaluate(&input(vec![with_parents("orphan", &["bad parent"], 1)]));
    assert_eq!(
        codes(&bad_parent),
        set(&[
            "workflow.parent_identity_invalid",
            "workflow.parent_missing"
        ])
    );

    let mut duplicate_parent = with_parents("join", &["left", "left"], 1);
    duplicate_parent["merge_verified"] = json!(true);
    duplicate_parent["merge_expected_digest"] = json!(DIGEST);
    duplicate_parent["merge_observed_digest"] = json!(DIGEST);
    let duplicate_edges = evaluate(&input(vec![node("left"), duplicate_parent]));
    assert_eq!(
        codes(&duplicate_edges),
        set(&["workflow.parent_identity_invalid"])
    );
}

#[test]
fn depth_must_increase_along_every_parent_edge() {
    let mut parent = node("parent");
    parent["depth"] = json!(1);
    let equal_depth = with_parents("child", &["parent"], 1);
    let findings = evaluate(&input(vec![parent.clone(), equal_depth]));
    assert_eq!(codes(&findings), set(&["workflow.depth_inconsistent"]));

    let shallower = with_parents("deep", &["parent"], 0);
    let findings = evaluate(&input(vec![parent, shallower]));
    assert_eq!(codes(&findings), set(&["workflow.depth_inconsistent"]));

    let mut too_deep = node("too-deep");
    too_deep["depth"] = json!(5);
    let findings = evaluate(&input(vec![too_deep]));
    assert_eq!(codes(&findings), set(&["workflow.depth_exceeded"]));
}

#[test]
fn attempts_must_be_positive_strictly_increasing_and_bounded() {
    for attempts in [json!([]), json!([0]), json!([1, 1]), json!([2, 1])] {
        let mut candidate = node("attempt-node");
        candidate["attempts"] = attempts;
        let findings = evaluate(&input(vec![candidate]));
        assert_eq!(codes(&findings), set(&["workflow.attempt_invalid"]));
    }

    let mut bounded = node("attempt-node");
    let attempts: Vec<u64> = (1..=64).collect();
    bounded["attempts"] = json!(attempts);
    assert!(evaluate(&input(vec![bounded])).is_empty());

    let mut unbounded = node("attempt-node");
    let attempts: Vec<u64> = (1..=65).collect();
    unbounded["attempts"] = json!(attempts);
    let findings = evaluate(&input(vec![unbounded]));
    assert_eq!(codes(&findings), set(&["workflow.attempt_unbounded"]));
}

#[test]
fn fan_out_bounds_and_expected_observed_children_must_agree() {
    let mut exceeded = node("parent");
    exceeded["expected_children"] = json!(5);
    exceeded["observed_children"] = json!(5);
    let findings = evaluate(&input(vec![exceeded]));
    assert_eq!(findings.len(), 2, "unexpected findings: {findings:?}");
    let mut literals = expected_literals(&findings, "workflow.fanout_exceeded");
    literals.sort();
    assert_eq!(
        literals,
        vec![
            "expected_children_within_max_fan_out".to_owned(),
            "observed_children_within_max_fan_out".to_owned(),
        ]
    );

    let mut under = node("parent");
    under["expected_children"] = json!(2);
    under["observed_children"] = json!(1);
    let findings = evaluate(&input(vec![under]));
    assert_eq!(findings.len(), 1);
    assert_eq!(findings[0].code, "workflow.fanout_mismatch");

    let mut expected_only = node("parent");
    expected_only["expected_children"] = json!(5);
    expected_only["observed_children"] = json!(3);
    let findings = evaluate(&input(vec![expected_only]));
    let mut literals = expected_literals(&findings, "workflow.fanout_exceeded");
    literals.sort();
    assert_eq!(
        literals,
        vec!["expected_children_within_max_fan_out".to_owned()]
    );
    assert_eq!(findings.len(), 2);
}

#[test]
fn fan_in_counts_must_be_coherent_with_declared_parents() {
    let mut required_mismatch = with_parents("child", &["root"], 1);
    required_mismatch["required_parents"] = json!(0);
    required_mismatch["observed_parents"] = json!(0);
    let findings = evaluate(&input(vec![node("root"), required_mismatch]));
    assert_eq!(findings.len(), 1, "unexpected findings: {findings:?}");
    assert_eq!(findings[0].code, "workflow.fanin_mismatch");
    assert_eq!(
        findings[0].expected,
        json!("required_parents_equal_declared_parents")
    );

    let mut observed_mismatch = with_parents("child", &["root"], 1);
    observed_mismatch["observed_parents"] = json!(0);
    let findings = evaluate(&input(vec![node("root"), observed_mismatch]));
    assert_eq!(findings.len(), 1, "unexpected findings: {findings:?}");
    assert_eq!(
        findings[0].expected,
        json!("observed_parents_equal_required_parents")
    );

    let mut nodes = Vec::new();
    let mut parents = Vec::new();
    for index in 0..65 {
        let id = format!("p{index:02}");
        nodes.push(node(&id));
        parents.push(id);
    }
    let parent_refs: Vec<&str> = parents.iter().map(String::as_str).collect();
    let mut join = with_parents("join", &parent_refs, 1);
    join["merge_verified"] = json!(true);
    join["merge_expected_digest"] = json!(DIGEST);
    join["merge_observed_digest"] = json!(DIGEST);
    nodes.push(join);
    let findings = evaluate(&input(nodes));
    assert_eq!(codes(&findings), set(&["workflow.fanin_unbounded"]));
}

#[test]
fn merge_evidence_is_required_for_fan_in_and_unexpected_below_it() {
    let mut join = with_parents("join", &["left", "right"], 1);
    let findings = evaluate(&input(vec![node("left"), node("right"), join.clone()]));
    assert_eq!(codes(&findings), set(&["workflow.merge_missing"]));

    join["merge_verified"] = json!(true);
    join["merge_expected_digest"] = json!(DIGEST);
    join["merge_observed_digest"] = json!(DIGEST);
    assert!(evaluate(&input(vec![node("left"), node("right"), join])).is_empty());

    let mut single = with_parents("child", &["root"], 1);
    single["merge_verified"] = json!(true);
    single["merge_expected_digest"] = json!(DIGEST);
    single["merge_observed_digest"] = json!(DIGEST);
    let findings = evaluate(&input(vec![node("root"), single]));
    assert_eq!(codes(&findings), set(&["workflow.merge_unexpected"]));

    let mut dangling = with_parents("child", &["root"], 1);
    dangling["merge_expected_digest"] = json!(DIGEST);
    let findings = evaluate(&input(vec![node("root"), dangling]));
    assert_eq!(codes(&findings), set(&["workflow.merge_unexpected"]));

    let mut root_merge = node("root");
    root_merge["merge_verified"] = json!(true);
    root_merge["merge_expected_digest"] = json!(DIGEST);
    root_merge["merge_observed_digest"] = json!(OTHER_DIGEST);
    let findings = evaluate(&input(vec![root_merge]));
    assert_eq!(codes(&findings), set(&["workflow.merge_unexpected"]));
}

#[test]
fn merge_digests_must_be_strict_sha256_and_equal() {
    let short = &DIGEST[..DIGEST.len() - 1];

    let mut join = with_parents("join", &["left", "right"], 1);
    join["merge_verified"] = json!(true);
    join["merge_expected_digest"] = json!(short);
    join["merge_observed_digest"] = json!(DIGEST);
    let findings = evaluate(&input(vec![node("left"), node("right"), join]));
    assert_eq!(codes(&findings), set(&["workflow.merge_invalid"]));

    let mut join = with_parents("join", &["left", "right"], 1);
    join["merge_verified"] = json!(true);
    join["merge_expected_digest"] = json!(DIGEST);
    let findings = evaluate(&input(vec![node("left"), node("right"), join]));
    assert_eq!(codes(&findings), set(&["workflow.merge_invalid"]));

    let mut join = with_parents("join", &["left", "right"], 1);
    join["merge_verified"] = json!(true);
    join["merge_expected_digest"] = json!(DIGEST);
    join["merge_observed_digest"] = json!(OTHER_DIGEST);
    let findings = evaluate(&input(vec![node("left"), node("right"), join]));
    assert_eq!(codes(&findings), set(&["workflow.merge_mismatch"]));

    let mut join = with_parents("join", &["left", "right"], 1);
    join["merge_expected_digest"] = json!(DIGEST);
    join["merge_observed_digest"] = json!(DIGEST);
    let findings = evaluate(&input(vec![node("left"), node("right"), join]));
    assert_eq!(codes(&findings), set(&["workflow.merge_missing"]));
}

#[test]
fn parent_scope_cannot_exceed_parent_child_scope() {
    let mut child = with_parents("child", &["root"], 1);
    child["parent_scope"] = json!(["project:alpha", "network:external"]);
    let fixture = input(vec![node("root"), child]);
    let findings = evaluate(&fixture);
    assert_eq!(findings.len(), 1, "unexpected findings: {findings:?}");
    assert_eq!(findings[0].code, "workflow.parent_scope_expanded");
    assert_eq!(findings[0].evidence_ref, node_ref(&fixture, "child"));

    let mut root = node("root");
    root["parent_scope"] = json!(["network:external"]);
    root["child_scope"] = json!(["project:alpha"]);
    assert!(evaluate(&input(vec![root])).is_empty());

    let mut join = with_parents("join", &["left", "right"], 1);
    join["parent_scope"] = json!(["project:alpha", "network:external"]);
    join["merge_verified"] = json!(true);
    join["merge_expected_digest"] = json!(DIGEST);
    join["merge_observed_digest"] = json!(DIGEST);
    let findings = evaluate(&input(vec![node("left"), node("right"), join]));
    assert_eq!(findings.len(), 2, "unexpected findings: {findings:?}");
    assert_eq!(codes(&findings), set(&["workflow.parent_scope_expanded"]));
}

#[test]
fn scope_entries_are_bounded_and_deduplicated() {
    let mut invalid = node("node");
    invalid["parent_scope"] = json!(["project alpha"]);
    invalid["child_scope"] = json!(["project alpha"]);
    let findings = evaluate(&input(vec![invalid]));
    assert_eq!(codes(&findings), set(&["workflow.scope_invalid"]));

    let oversized = "a".repeat(513);
    let mut invalid = node("node");
    invalid["parent_scope"] = json!([oversized]);
    invalid["child_scope"] = json!([oversized]);
    let findings = evaluate(&input(vec![invalid]));
    assert_eq!(codes(&findings), set(&["workflow.scope_invalid"]));

    let entries: Vec<String> = (0..65).map(|index| format!("scope:{index}")).collect();
    let mut oversized_set = node("node");
    oversized_set["parent_scope"] = json!(entries);
    oversized_set["child_scope"] = json!([]);
    let findings = evaluate(&input(vec![oversized_set]));
    assert_eq!(codes(&findings), set(&["workflow.scope_invalid"]));

    let mut duplicated = node("node");
    duplicated["parent_scope"] = json!(["project:alpha", "project:alpha"]);
    duplicated["child_scope"] = json!(["project:alpha"]);
    assert!(evaluate(&input(vec![duplicated])).is_empty());
}

#[test]
fn compensation_requires_recorded_receipt_evidence() {
    let mut missing = node("node");
    missing["compensation_required"] = json!(true);
    let findings = evaluate(&input(vec![missing]));
    assert_eq!(codes(&findings), set(&["workflow.compensation_missing"]));

    let mut recorded = node("node");
    recorded["compensation_required"] = json!(true);
    recorded["compensation_recorded"] = json!(true);
    recorded["compensation_receipt_ref"] = json!("receipt:1");
    assert!(evaluate(&input(vec![recorded])).is_empty());

    let mut bad_receipt = node("node");
    bad_receipt["compensation_required"] = json!(true);
    bad_receipt["compensation_recorded"] = json!(true);
    bad_receipt["compensation_receipt_ref"] = json!("bad receipt");
    let findings = evaluate(&input(vec![bad_receipt]));
    assert_eq!(
        codes(&findings),
        set(&["workflow.compensation_receipt_missing"])
    );

    let mut unrequested = node("node");
    unrequested["compensation_recorded"] = json!(true);
    unrequested["compensation_receipt_ref"] = json!("receipt:1");
    let findings = evaluate(&input(vec![unrequested]));
    assert_eq!(codes(&findings), set(&["workflow.compensation_unexpected"]));

    let mut dangling_receipt = node("node");
    dangling_receipt["compensation_receipt_ref"] = json!("receipt:stale");
    let findings = evaluate(&input(vec![dangling_receipt]));
    assert_eq!(
        codes(&findings),
        set(&["workflow.compensation_receipt_unexpected"])
    );
}

#[test]
fn compensating_and_compensated_statuses_are_coherent() {
    let mut compensated = node("node");
    compensated["status"] = json!("compensated");
    let findings = evaluate(&input(vec![compensated]));
    assert_eq!(findings.len(), 1, "unexpected findings: {findings:?}");
    assert_eq!(findings[0].code, "workflow.compensation_invalid");
    assert_eq!(
        findings[0].expected,
        json!("compensated_requires_required_and_recorded_compensation")
    );

    let mut good = node("node");
    good["status"] = json!("compensated");
    good["compensation_required"] = json!(true);
    good["compensation_recorded"] = json!(true);
    good["compensation_receipt_ref"] = json!("receipt:1");
    assert!(evaluate(&input(vec![good])).is_empty());

    let mut in_flight = node("node");
    in_flight["status"] = json!("compensating");
    in_flight["compensation_required"] = json!(true);
    assert!(evaluate(&input(vec![in_flight])).is_empty());

    let mut in_flight_unrequired = node("node");
    in_flight_unrequired["status"] = json!("compensating");
    let findings = evaluate(&input(vec![in_flight_unrequired]));
    assert_eq!(findings.len(), 1, "unexpected findings: {findings:?}");
    assert_eq!(
        findings[0].expected,
        json!("compensating_requires_a_compensation_requirement")
    );
}

#[test]
fn schema_drift_on_envelope_and_nodes_is_rejected() {
    let mut drifted = valid_input();
    drifted["nodes"][0]["schema"] = json!("kiana.quality-workflow-node-evidence.v0");
    let findings = evaluate(&drifted);
    assert_eq!(findings.len(), 1, "unexpected findings: {findings:?}");
    assert_eq!(findings[0].code, "workflow.node_schema_invalid");
    assert_eq!(findings[0].expected, json!(NODE_SCHEMA));

    let mut envelope = valid_input();
    envelope["schema"] = json!("kiana.quality-workflow-swarm-input.v0");
    assert!(WorkflowSwarmEvaluator.evaluate(&envelope).is_err());
    let decoded: WorkflowSwarmInput =
        serde_json::from_value(envelope).expect("decodes structurally");
    assert_eq!(
        decoded.validate(),
        Err("workflow_swarm_input_schema_invalid")
    );
}

#[test]
fn oversized_node_inputs_fail_closed_before_evaluation() {
    let nodes: Vec<Value> = (0..513)
        .map(|index| node(&format!("n{index:04}")))
        .collect();
    let oversized = input(nodes);
    assert!(WorkflowSwarmEvaluator.evaluate(&oversized).is_err());
    let decoded: WorkflowSwarmInput =
        serde_json::from_value(oversized).expect("decodes structurally");
    assert_eq!(
        decoded.validate(),
        Err("workflow_swarm_node_limit_exceeded")
    );

    let nodes: Vec<Value> = (0..512)
        .map(|index| node(&format!("n{index:04}")))
        .collect();
    let bounded = input(nodes);
    let decoded: WorkflowSwarmInput =
        serde_json::from_value(bounded.clone()).expect("decodes structurally");
    assert!(decoded.validate().is_ok());
    assert!(evaluate(&bounded).is_empty());
}

#[test]
fn envelope_bounds_are_positive_evidence() {
    let mut zero_fan_out = input(vec![node("root")]);
    zero_fan_out["max_fan_out"] = json!(0);
    let findings = evaluate(&zero_fan_out);
    assert_eq!(findings.len(), 1, "unexpected findings: {findings:?}");
    assert_eq!(findings[0].code, "workflow.bounds_invalid");
    assert_eq!(findings[0].expected, json!("positive_max_fan_out"));
    assert_eq!(findings[0].evidence_ref, "workflow:input");

    let mut zero_depth = input(vec![node("root")]);
    zero_depth["max_depth"] = json!(0);
    let findings = evaluate(&zero_depth);
    assert_eq!(findings.len(), 1, "unexpected findings: {findings:?}");
    assert_eq!(findings[0].expected, json!("positive_max_depth"));

    let mut both = input(vec![node("root")]);
    both["max_fan_out"] = json!(0);
    both["max_depth"] = json!(0);
    assert_eq!(evaluate(&both).len(), 2);
}

#[test]
fn node_order_does_not_change_the_findings() {
    let mut root = node("root");
    root["expected_children"] = json!(1);
    root["observed_children"] = json!(1);
    let mut child = with_parents("child", &["root"], 1);
    child["child_scope"] = json!(["network:external"]);
    let orphan = with_parents("orphan", &["ghost"], 1);

    let forward = input(vec![root.clone(), child.clone(), orphan.clone()]);
    let reversed = input(vec![orphan, child, root]);
    let forward_findings = evaluate(&forward);
    let reversed_findings = evaluate(&reversed);
    assert!(!forward_findings.is_empty());
    assert_eq!(
        canonical_findings(forward_findings.clone()).unwrap(),
        canonical_findings(reversed_findings).unwrap()
    );
    let scope_finding = forward_findings
        .iter()
        .find(|finding| finding.code == "workflow.child_scope_expanded")
        .expect("scope expansion finding");
    assert_eq!(scope_finding.evidence_ref, node_ref(&forward, "child"));

    let mut second_dup = node("dup");
    second_dup["depth"] = json!(1);
    let duplicates = input(vec![node("dup"), second_dup.clone()]);
    let swapped = input(vec![second_dup, node("dup")]);
    let first = evaluate(&duplicates);
    let second = evaluate(&swapped);
    assert_eq!(first.len(), 1, "unexpected findings: {first:?}");
    assert_eq!(codes(&first), set(&["workflow.node_identity_invalid"]));
    assert_eq!(
        canonical_findings(first).unwrap(),
        canonical_findings(second).unwrap()
    );
}

#[test]
fn redaction_boundary_covers_caller_supplied_values() {
    let mut secret_node = node("token=supersecret");
    secret_node["compensation_required"] = json!(true);
    secret_node["compensation_recorded"] = json!(true);
    secret_node["compensation_receipt_ref"] = json!("token=supersecret");
    let findings = evaluate(&input(vec![secret_node]));
    assert_eq!(
        codes(&findings),
        set(&[
            "workflow.node_identity_invalid",
            "workflow.compensation_receipt_missing",
        ])
    );
    let serialized = serde_json::to_string(&findings).unwrap();
    assert!(
        !serialized.contains("supersecret"),
        "secret leaked into findings: {serialized}"
    );
    assert!(serialized.contains("[REDACTED]"));
}

#[test]
fn workflow_contracts_round_trip_through_typed_input() {
    fn typed_node(node_id: &str, parents: &[&str], depth: u32) -> WorkflowNodeEvidence {
        WorkflowNodeEvidence {
            schema: NODE_SCHEMA.to_owned(),
            node_id: node_id.to_owned(),
            parent_ids: parents.iter().map(|parent| (*parent).to_owned()).collect(),
            depth,
            attempts: vec![1],
            parent_scope: ["project:alpha".to_owned()].into_iter().collect(),
            child_scope: ["project:alpha".to_owned()].into_iter().collect(),
            expected_children: 0,
            observed_children: 0,
            required_parents: parents.len() as u32,
            observed_parents: parents.len() as u32,
            merge_verified: false,
            merge_expected_digest: None,
            merge_observed_digest: None,
            compensation_required: false,
            compensation_recorded: false,
            compensation_receipt_ref: String::new(),
            status: WorkflowNodeStatus::Succeeded,
        }
    }

    let typed = WorkflowSwarmInput {
        schema: WORKFLOW_SWARM_INPUT_SCHEMA.to_owned(),
        max_fan_out: 4,
        max_depth: 4,
        nodes: vec![
            typed_node("root", &[], 0),
            typed_node("child", &["root"], 1),
        ],
    };
    assert!(typed.validate().is_ok());
    let value = typed.as_value();
    let decoded: WorkflowSwarmInput = serde_json::from_value(value.clone()).expect("round-trips");
    assert_eq!(decoded, typed);
    assert!(evaluate_workflow_swarm(&decoded).unwrap().is_empty());

    let mut tampered = value;
    tampered["schema"] = json!("kiana.quality-workflow-swarm-input.v0");
    assert!(WorkflowSwarmEvaluator.evaluate(&tampered).is_err());
}

#[test]
fn workflow_output_is_deterministic_evidence_bound_and_registry_wired() {
    let mut fixture = valid_input();
    fixture["nodes"][1]["child_scope"] = json!(["project:alpha", "network:external"]);
    fixture["nodes"][1]["compensation_required"] = json!(true);

    let first = evaluate(&fixture);
    let second = evaluate(&fixture);
    assert_eq!(first, second);
    assert_eq!(
        digest_findings(&first).unwrap(),
        digest_findings(&second).unwrap()
    );
    assert_eq!(
        codes(&first),
        set(&[
            "workflow.child_scope_expanded",
            "workflow.compensation_missing",
        ])
    );
    for finding in &first {
        assert!(
            finding.code.starts_with("workflow."),
            "unstable finding code: {}",
            finding.code
        );
        assert!(
            finding.evidence_ref.starts_with("workflow:"),
            "missing evidence reference: {}",
            finding.evidence_ref
        );
    }

    let mut registry = EvaluatorRegistry::new();
    registry.register(WorkflowSwarmEvaluator).unwrap();
    assert_eq!(registry.evaluator_ids(), vec![WORKFLOW_SWARM_EVALUATOR_ID]);
    let canonical = canonical_findings(first.clone()).unwrap();
    assert_eq!(registry.evaluate(&fixture).unwrap(), canonical);

    let result = registry.evaluate_with_digest(&fixture).unwrap();
    assert_eq!(result.schema, EVALUATION_RESULT_SCHEMA);
    assert_eq!(result.findings, canonical);
    assert_eq!(result.findings_digest, digest_findings(&canonical).unwrap());
    assert!(registry
        .evaluate(&json!({"schema": "kiana.quality-workflow-swarm-input.v0"}))
        .is_err());
}

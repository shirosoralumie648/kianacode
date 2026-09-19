use kiana_domain::{
    json_digest, RequestId, SessionId, WorkflowDefinition, WorkflowInstance,
    WorkflowInstanceStatus, WorkflowNode, WorkflowNodeExecution, WorkflowNodeKind,
    WorkflowNodeStatus,
};
use kiana_workflow::{ready_node_ids, stable_fan_in_outputs};
use serde_json::json;
use std::collections::BTreeMap;

fn definition() -> WorkflowDefinition {
    let mut nodes = BTreeMap::new();
    nodes.insert(
        "branch_a".to_owned(),
        WorkflowNode {
            dependencies: Vec::new(),
            kind: WorkflowNodeKind::Literal {
                values: BTreeMap::from([("value".to_owned(), json!("a"))]),
            },
            timeout_ms: 1_000,
            retry_limit: 0,
            compensation: None,
        },
    );
    nodes.insert(
        "branch_b".to_owned(),
        WorkflowNode {
            dependencies: Vec::new(),
            kind: WorkflowNodeKind::Literal {
                values: BTreeMap::from([("value".to_owned(), json!("b"))]),
            },
            timeout_ms: 1_000,
            retry_limit: 0,
            compensation: None,
        },
    );
    nodes.insert(
        "join".to_owned(),
        WorkflowNode {
            dependencies: vec!["branch_b".to_owned(), "branch_a".to_owned()],
            kind: WorkflowNodeKind::FanIn,
            timeout_ms: 1_000,
            retry_limit: 0,
            compensation: None,
        },
    );
    WorkflowDefinition {
        definition_id: "definition-1".to_owned(),
        version: 1,
        input_keys: Vec::new(),
        output_keys: Vec::new(),
        allowed_roles: vec!["builder".to_owned()],
        max_duration_ms: 10_000,
        max_steps: 10,
        nodes,
        artifacts: BTreeMap::new(),
    }
}

fn output(value: serde_json::Value) -> WorkflowNodeExecution {
    WorkflowNodeExecution {
        execution_id: RequestId::new(),
        session_id: SessionId::new("session-1"),
        attempt: 1,
        status: WorkflowNodeStatus::Succeeded,
        started_at: 1,
        lease_expires_at: 2_000,
        ended_at: Some(2),
        input_digest: json_digest(&json!({"input":1})),
        output_digest: json_digest(&value),
        output_recorded: true,
        output: value,
        error_code: None,
        evidence_refs: vec!["event:result".to_owned()],
        child_instance_id: None,
    }
}

fn instance(nodes: BTreeMap<String, WorkflowNodeExecution>) -> WorkflowInstance {
    WorkflowInstance {
        instance_id: "instance-1".to_owned(),
        definition_id: "definition-1".to_owned(),
        definition_version: 1,
        owner_id: "owner-1".to_owned(),
        role_id: "builder".to_owned(),
        created_at: 1,
        deadline: 10_000,
        inputs: BTreeMap::new(),
        outputs: BTreeMap::new(),
        status: WorkflowInstanceStatus::Ready,
        steps_used: 2,
        error_code: None,
        nodes,
        signals: BTreeMap::new(),
        trigger_id: None,
        parent_instance_id: None,
        depth: 0,
        selected_nodes: None,
        retry_counts: BTreeMap::new(),
    }
}

#[test]
fn ready_nodes_are_stable_and_fan_in_outputs_are_dependency_sorted() {
    let definition = definition();
    let mut nodes = BTreeMap::new();
    nodes.insert("branch_a".to_owned(), output(json!({"value":"a"})));
    nodes.insert("branch_b".to_owned(), output(json!({"value":"b"})));
    let instance = instance(nodes);
    assert_eq!(
        ready_node_ids(&instance, &definition).expect("ready nodes"),
        vec!["join"]
    );
    assert_eq!(
        stable_fan_in_outputs(&instance, &definition, "join").expect("fan-in"),
        vec![json!({"value":"a"}), json!({"value":"b"})]
    );
}

#[test]
fn fan_in_missing_or_tampered_result_is_not_an_empty_input() {
    let definition = definition();
    let mut missing = output(json!({"value":"a"}));
    missing.output_recorded = false;
    let missing_instance = instance(BTreeMap::from([
        ("branch_a".to_owned(), missing),
        ("branch_b".to_owned(), output(json!({"value":"b"}))),
    ]));
    assert_eq!(
        stable_fan_in_outputs(&missing_instance, &definition, "join").expect_err("missing result"),
        "workflow_fanin_result_missing"
    );

    let mut tampered = output(json!({"value":"a"}));
    tampered.output_digest = json_digest(&json!({"value":"different"}));
    let tampered_instance = instance(BTreeMap::from([
        ("branch_a".to_owned(), tampered),
        ("branch_b".to_owned(), output(json!({"value":"b"}))),
    ]));
    assert_eq!(
        stable_fan_in_outputs(&tampered_instance, &definition, "join")
            .expect_err("tampered result"),
        "workflow_fanin_result_missing"
    );
}

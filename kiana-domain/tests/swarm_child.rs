use kiana_domain::{
    materialize_swarm_child, AttemptId, ChildCellId, RunId, SessionId,
    SwarmChildMaterializationRequest, SWARM_CHILD_INPUT_SCHEMA,
};
use serde_json::json;

const PARENT_SCOPE: &str =
    "sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
const CHILD_SCOPE: &str = "sha256:bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";

fn request() -> SwarmChildMaterializationRequest {
    SwarmChildMaterializationRequest {
        schema: SWARM_CHILD_INPUT_SCHEMA.to_owned(),
        parent_session_id: SessionId::new("parent-session"),
        parent_run_id: RunId::new(),
        child_session_id: SessionId::new("child-session"),
        child_run_id: RunId::new(),
        child_attempt_id: AttemptId::new(),
        child_cell_id: ChildCellId::new(),
        partition_key: "partition-a".to_owned(),
        input_refs: vec!["artifact:input-a".to_owned()],
        authorized_input_refs: vec!["artifact:input-a".to_owned()],
        parent_scope_digest: PARENT_SCOPE.to_owned(),
        child_scope_digest: CHILD_SCOPE.to_owned(),
        child_scope_is_subset: true,
        parent_private_history_included: false,
        authority_epoch: 4,
        template_revision: "template-v1".to_owned(),
        policy_revision: "policy-v1".to_owned(),
    }
}

#[test]
fn child_materialization_requires_fresh_context_and_authorized_inputs() {
    let receipt = materialize_swarm_child(&request()).unwrap();
    assert!(receipt.fresh_context);
    assert!(receipt.private_history_excluded);
    assert!(receipt.validate().is_ok());
}

#[test]
fn parent_session_reuse_history_and_scope_expansion_fail_closed() {
    let mut reused = request();
    reused.child_session_id = reused.parent_session_id.clone();
    assert_eq!(
        materialize_swarm_child(&reused),
        Err("swarm_child_materialization_invalid")
    );

    let mut history = request();
    history.parent_private_history_included = true;
    assert_eq!(
        materialize_swarm_child(&history),
        Err("swarm_child_materialization_invalid")
    );

    let mut scope = request();
    scope.child_scope_is_subset = false;
    assert_eq!(
        materialize_swarm_child(&scope),
        Err("swarm_child_materialization_invalid")
    );
}

#[test]
fn child_input_and_unknown_fields_are_strict() {
    let mut unauthorized = request();
    unauthorized.authorized_input_refs = vec!["artifact:other".to_owned()];
    assert_eq!(
        materialize_swarm_child(&unauthorized),
        Err("swarm_child_materialization_invalid")
    );

    let mut value = serde_json::to_value(request()).unwrap();
    value["unexpected"] = json!(true);
    assert!(serde_json::from_value::<SwarmChildMaterializationRequest>(value).is_err());
}

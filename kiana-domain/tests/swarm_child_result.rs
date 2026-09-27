use kiana_domain::{
    AttemptId, RunId, TypedChildResult, TypedChildResultState, SWARM_CHILD_RESULT_SCHEMA,
};
use serde_json::json;

const OUT: &str = "sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";

fn result(state: TypedChildResultState) -> TypedChildResult {
    let mut value = TypedChildResult {
        schema: SWARM_CHILD_RESULT_SCHEMA.to_owned(),
        parent_run_id: RunId::new(),
        child_run_id: RunId::new(),
        attempt_id: AttemptId::new(),
        partition_key: "partition-a".to_owned(),
        state,
        output_digest: None,
        artifact_refs: vec!["artifact:one".to_owned()],
        failure_reason: None,
        source_cursor: 8,
        independent_review_required: true,
        result_digest: String::new(),
    };
    value.result_digest = kiana_domain::json_digest(&json!({
        "schema": value.schema,
        "parent_run_id": value.parent_run_id,
        "child_run_id": value.child_run_id,
        "attempt_id": value.attempt_id,
        "partition_key": value.partition_key,
        "state": value.state,
        "output_digest": value.output_digest,
        "artifact_refs": value.artifact_refs,
        "failure_reason": value.failure_reason,
        "source_cursor": value.source_cursor,
        "independent_review_required": value.independent_review_required,
    }));
    value
}

#[test]
fn typed_success_failure_and_unknown_have_distinct_contracts() {
    let mut success = result(TypedChildResultState::Succeeded);
    success.output_digest = Some(OUT.to_owned());
    success.independent_review_required = true;
    success.result_digest = kiana_domain::json_digest(&json!({
        "schema": success.schema,
        "parent_run_id": success.parent_run_id,
        "child_run_id": success.child_run_id,
        "attempt_id": success.attempt_id,
        "partition_key": success.partition_key,
        "state": success.state,
        "output_digest": success.output_digest,
        "artifact_refs": success.artifact_refs,
        "failure_reason": success.failure_reason,
        "source_cursor": success.source_cursor,
        "independent_review_required": success.independent_review_required,
    }));
    assert!(success.validate().is_ok());

    let mut failed = result(TypedChildResultState::Failed);
    failed.failure_reason = Some("child_failed".to_owned());
    failed.result_digest = kiana_domain::json_digest(&json!({
        "schema": failed.schema,
        "parent_run_id": failed.parent_run_id,
        "child_run_id": failed.child_run_id,
        "attempt_id": failed.attempt_id,
        "partition_key": failed.partition_key,
        "state": failed.state,
        "output_digest": failed.output_digest,
        "artifact_refs": failed.artifact_refs,
        "failure_reason": failed.failure_reason,
        "source_cursor": failed.source_cursor,
        "independent_review_required": failed.independent_review_required,
    }));
    assert!(failed.validate().is_ok());
    assert!(result(TypedChildResultState::ResultUnknown)
        .validate()
        .is_ok());
}

#[test]
fn success_without_review_or_failure_without_reason_fails_closed() {
    let mut success = result(TypedChildResultState::Succeeded);
    success.output_digest = Some(OUT.to_owned());
    success.independent_review_required = false;
    success.result_digest = kiana_domain::json_digest(&json!({
        "schema": success.schema,
        "parent_run_id": success.parent_run_id,
        "child_run_id": success.child_run_id,
        "attempt_id": success.attempt_id,
        "partition_key": success.partition_key,
        "state": success.state,
        "output_digest": success.output_digest,
        "artifact_refs": success.artifact_refs,
        "failure_reason": success.failure_reason,
        "source_cursor": success.source_cursor,
        "independent_review_required": success.independent_review_required,
    }));
    assert_eq!(
        success.validate(),
        Err("swarm_child_result_success_contract_invalid")
    );
}

#[test]
fn unknown_fields_fail_closed() {
    let mut value = serde_json::to_value(result(TypedChildResultState::ResultUnknown)).unwrap();
    value["unexpected"] = json!(true);
    assert!(serde_json::from_value::<TypedChildResult>(value).is_err());
}

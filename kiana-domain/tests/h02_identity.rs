use kiana_domain::{
    ModelAttemptId, ModelAttemptIdentity, RequestId, RunId, StepId, StepIdentity, TurnId,
    MODEL_ATTEMPT_IDENTITY_SCHEMA, STEP_IDENTITY_SCHEMA,
};

#[test]
fn step_identity_binds_session_run_turn_scope() {
    let run_id = RunId::new();
    let turn_id = TurnId::new();
    let identity = StepIdentity::new(run_id, turn_id, StepId::new(), 1).unwrap();
    assert_eq!(identity.schema, STEP_IDENTITY_SCHEMA);
    identity.validate().unwrap();

    let mut tampered = serde_json::to_value(&identity).unwrap();
    tampered["step_number"] = serde_json::json!(2);
    assert!(serde_json::from_value::<StepIdentity>(tampered)
        .unwrap()
        .validate()
        .is_err());
}

#[test]
fn model_attempt_identity_is_unique_per_step_and_attempt() {
    let run_id = RunId::new();
    let turn_id = TurnId::new();
    let step_id = StepId::new();
    let first = ModelAttemptIdentity::new(
        run_id,
        turn_id,
        step_id,
        ModelAttemptId::new(),
        RequestId::new(),
        1,
    )
    .unwrap();
    let second = ModelAttemptIdentity::new(
        run_id,
        turn_id,
        step_id,
        ModelAttemptId::new(),
        first.model_call_id,
        2,
    )
    .unwrap();
    assert_eq!(first.schema, MODEL_ATTEMPT_IDENTITY_SCHEMA);
    assert_ne!(first.model_attempt_id, second.model_attempt_id);
    assert_ne!(first.identity_digest, second.identity_digest);
    first.validate().unwrap();
    second.validate().unwrap();
    assert!(ModelAttemptIdentity::new(
        run_id,
        turn_id,
        step_id,
        ModelAttemptId::new(),
        RequestId::new(),
        0,
    )
    .is_err());
}

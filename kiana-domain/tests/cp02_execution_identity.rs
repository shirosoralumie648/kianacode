use kiana_domain::{
    ExecutionId, InvocationId, InvocationIdentity, RunId, TurnId, TurnIdentity, TurnSemantics,
    INVOCATION_IDENTITY_SCHEMA, TURN_IDENTITY_SCHEMA,
};

#[test]
fn turn_identity_distinguishes_start_new_turn_legacy_and_resume() {
    let session = "session-cp02";
    let run = RunId::new();
    let previous = RunId::new();
    let turn = TurnId::new();

    let start = TurnIdentity::new(session, run, turn, None, TurnSemantics::Start, 1).unwrap();
    assert_eq!(start.schema, TURN_IDENTITY_SCHEMA);
    start.validate().unwrap();

    let new_turn = TurnIdentity::new(
        session,
        run,
        TurnId::new(),
        Some(previous),
        TurnSemantics::NewTurn,
        1,
    )
    .unwrap();
    new_turn.validate().unwrap();
    assert!(
        TurnIdentity::new(session, run, TurnId::new(), None, TurnSemantics::NewTurn, 1,).is_err()
    );

    let legacy = TurnIdentity::new(
        session,
        run,
        TurnId::new(),
        None,
        TurnSemantics::LegacyContinue,
        1,
    )
    .unwrap();
    let encoded = serde_json::to_value(&legacy).unwrap();
    let decoded: TurnIdentity = serde_json::from_value(encoded).unwrap();
    decoded.validate().unwrap();

    let resume =
        TurnIdentity::new(session, run, TurnId::new(), None, TurnSemantics::Resume, 2).unwrap();
    resume.validate().unwrap();
}

#[test]
fn invocation_identity_keeps_call_id_as_correlation_only() {
    let run = RunId::new();
    let turn = TurnId::new();
    let first = InvocationIdentity::new(
        run,
        turn,
        InvocationId::new(),
        ExecutionId::new(),
        Some("call-reused".to_owned()),
        1,
    )
    .unwrap();
    let second = InvocationIdentity::new(
        run,
        turn,
        InvocationId::new(),
        ExecutionId::new(),
        Some("call-reused".to_owned()),
        1,
    )
    .unwrap();
    assert_eq!(first.schema, INVOCATION_IDENTITY_SCHEMA);
    assert_ne!(first.invocation_id, second.invocation_id);
    assert_ne!(first.execution_id, second.execution_id);
    assert_ne!(first.identity_digest, second.identity_digest);
    first.validate().unwrap();
    second.validate().unwrap();
    assert!(InvocationIdentity::new(
        run,
        turn,
        InvocationId::new(),
        ExecutionId::new(),
        Some("".to_owned()),
        1,
    )
    .is_err());
    assert!(
        InvocationIdentity::new(run, turn, InvocationId::new(), ExecutionId::new(), None, 0,)
            .is_err()
    );

    let mut unknown = serde_json::to_value(&first).unwrap();
    unknown["unexpected"] = serde_json::json!(true);
    assert!(serde_json::from_value::<InvocationIdentity>(unknown).is_err());
}

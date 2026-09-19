use kiana_domain::{
    ExecutionRevisionPin, RevisionDrain, RevisionDrainStatus, REVISION_DRAIN_SCHEMA,
};

const DIGEST_A: &str = "sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
const DIGEST_B: &str = "sha256:bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";
const DIGEST_C: &str = "sha256:cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc";
const DIGEST_D: &str = "sha256:dddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddd";

fn pin() -> ExecutionRevisionPin {
    ExecutionRevisionPin::new(
        "revision-1",
        DIGEST_A,
        DIGEST_B,
        DIGEST_C,
        DIGEST_D,
        DIGEST_A,
        "kiana.replay.v1",
    )
    .unwrap()
}

#[test]
fn revision_pin_replay_requires_workflow_provider_extension_and_trust_parity() {
    let expected = pin();
    let observed = pin();
    expected.compatible_replay_with(&observed).unwrap();

    let mut workflow = observed.clone();
    workflow.workflow_digest = DIGEST_A.to_owned();
    workflow.pin_digest = workflow.digest();
    assert_eq!(
        expected.compatible_replay_with(&workflow).unwrap_err(),
        "revision_workflow_digest_drift"
    );

    let mut replay = observed.clone();
    replay.replay_schema = "kiana.replay.v9".to_owned();
    replay.pin_digest = replay.digest();
    assert_eq!(
        expected.compatible_replay_with(&replay).unwrap_err(),
        "revision_replay_schema_unknown"
    );
}

#[test]
fn old_revision_drain_never_retires_with_runs_or_writers() {
    let mut drain = RevisionDrain::new(pin()).unwrap();
    drain.begin(100, 200, true).unwrap();
    assert_eq!(drain.status, RevisionDrainStatus::Draining);
    drain.observe(1, 0).unwrap();
    assert_eq!(drain.retire(150).unwrap_err(), "revision_drain_not_empty");
    drain.observe(0, 1).unwrap();
    assert_eq!(drain.retire(150).unwrap_err(), "revision_drain_not_empty");
    drain.observe(0, 0).unwrap();
    drain.retire(150).unwrap();
    assert_eq!(drain.status, RevisionDrainStatus::Retired);
    assert_eq!(drain.schema, REVISION_DRAIN_SCHEMA);
}

#[test]
fn drain_deadline_and_unknown_fields_fail_closed() {
    let mut drain = RevisionDrain::new(pin()).unwrap();
    drain.begin(100, 200, true).unwrap();
    drain.observe(0, 0).unwrap();
    assert_eq!(
        drain.retire(201).unwrap_err(),
        "revision_drain_deadline_exceeded"
    );
    let mut value = serde_json::to_value(drain).unwrap();
    value["unexpected"] = serde_json::json!(true);
    assert!(serde_json::from_value::<RevisionDrain>(value).is_err());
}

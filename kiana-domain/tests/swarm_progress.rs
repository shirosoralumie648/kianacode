use kiana_domain::{
    AttemptId, RunId, SwarmProgressBudget, SwarmProgressLedger, SwarmProgressObservation,
    SwarmProgressStatus, SWARM_PROGRESS_SCHEMA,
};
use serde_json::json;

fn observation(
    heartbeat: u64,
    turns: u64,
    status: SwarmProgressStatus,
) -> SwarmProgressObservation {
    let mut value = SwarmProgressObservation {
        schema: SWARM_PROGRESS_SCHEMA.to_owned(),
        run_id: RunId::new(),
        attempt_id: AttemptId::new(),
        heartbeat_sequence: heartbeat,
        checkpoint_sequence: heartbeat,
        turns,
        tokens: turns * 10,
        effects: turns,
        wall_time_ms: turns * 100,
        stalls: 0,
        budget: SwarmProgressBudget {
            max_turns: 5,
            max_tokens: 50,
            max_effects: 5,
            max_wall_time_ms: 500,
            max_stalls: 2,
        },
        status,
        observation_digest: String::new(),
    };
    value.observation_digest = value.digest_for_test();
    value
}

trait DigestForTest {
    fn digest_for_test(&self) -> String;
}

impl DigestForTest for SwarmProgressObservation {
    fn digest_for_test(&self) -> String {
        kiana_domain::json_digest(&serde_json::json!({
            "schema": self.schema,
            "run_id": self.run_id,
            "attempt_id": self.attempt_id,
            "heartbeat_sequence": self.heartbeat_sequence,
            "checkpoint_sequence": self.checkpoint_sequence,
            "turns": self.turns,
            "tokens": self.tokens,
            "effects": self.effects,
            "wall_time_ms": self.wall_time_ms,
            "stalls": self.stalls,
            "budget": self.budget,
            "status": self.status,
        }))
    }
}

#[test]
fn progress_is_monotonic_and_budget_bounded() {
    let mut ledger = SwarmProgressLedger::new();
    let first = observation(1, 1, SwarmProgressStatus::Running);
    let run = first.run_id;
    let attempt = first.attempt_id;
    ledger.record(first).unwrap();
    let mut next = observation(2, 2, SwarmProgressStatus::Running);
    next.run_id = run;
    next.attempt_id = attempt;
    next.observation_digest = next.digest_for_test();
    ledger.record(next).unwrap();
}

#[test]
fn heartbeat_rollback_and_stall_without_escalation_fail_closed() {
    let mut ledger = SwarmProgressLedger::new();
    let first = observation(2, 1, SwarmProgressStatus::Running);
    let run = first.run_id;
    let attempt = first.attempt_id;
    ledger.record(first).unwrap();
    let mut rollback = observation(1, 1, SwarmProgressStatus::Running);
    rollback.run_id = run;
    rollback.attempt_id = attempt;
    rollback.observation_digest = rollback.digest_for_test();
    assert_eq!(ledger.record(rollback), Err("swarm_progress_non_monotonic"));

    let mut stalled = observation(3, 1, SwarmProgressStatus::Running);
    stalled.run_id = run;
    stalled.attempt_id = attempt;
    stalled.stalls = 2;
    stalled.observation_digest = stalled.digest_for_test();
    assert_eq!(
        stalled.validate(),
        Err("swarm_progress_stall_not_escalated")
    );
}

#[test]
fn unknown_fields_and_digest_drift_fail_closed() {
    let mut value = serde_json::to_value(observation(1, 1, SwarmProgressStatus::Running)).unwrap();
    value["unexpected"] = json!(true);
    assert!(serde_json::from_value::<SwarmProgressObservation>(value).is_err());

    let mut invalid = observation(1, 1, SwarmProgressStatus::Running);
    invalid.observation_digest =
        "sha256:ffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff".to_owned();
    assert_eq!(
        invalid.validate(),
        Err("swarm_progress_observation_invalid")
    );
}

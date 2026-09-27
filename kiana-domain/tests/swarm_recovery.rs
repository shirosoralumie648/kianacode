use kiana_domain::{
    AttemptId, EventId, RunId, SwarmRecoveryFact, SwarmRecoveryState, SWARM_RECOVERY_SCHEMA,
};
use serde_json::json;

const DIGEST: &str = "sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";

fn fact(state: SwarmRecoveryState) -> SwarmRecoveryFact {
    let mut value = SwarmRecoveryFact {
        schema: SWARM_RECOVERY_SCHEMA.to_owned(),
        run_id: RunId::new(),
        attempt_id: AttemptId::new(),
        source_cursor: 10,
        last_committed_cursor: 9,
        pending_write_digest: DIGEST.to_owned(),
        source_event_ids: vec![EventId::new()],
        authority_epoch: 3,
        state,
        effect_known: false,
        re_admission_authorized: false,
        recovery_digest: String::new(),
    };
    value.recovery_digest = kiana_domain::json_digest(&json!({
        "schema": value.schema,
        "run_id": value.run_id,
        "attempt_id": value.attempt_id,
        "source_cursor": value.source_cursor,
        "last_committed_cursor": value.last_committed_cursor,
        "pending_write_digest": value.pending_write_digest,
        "source_event_ids": value.source_event_ids,
        "authority_epoch": value.authority_epoch,
        "state": value.state,
        "effect_known": value.effect_known,
        "re_admission_authorized": value.re_admission_authorized,
    }));
    value
}

#[test]
fn pending_and_unknown_recovery_are_visible_without_auto_success() {
    assert!(fact(SwarmRecoveryState::PendingWrite).validate().is_ok());
    assert!(fact(SwarmRecoveryState::ResultUnknown).validate().is_ok());
}

#[test]
fn recovered_requires_known_effect_and_explicit_re_admission() {
    let mut recovered = fact(SwarmRecoveryState::Recovered);
    recovered.effect_known = true;
    recovered.re_admission_authorized = true;
    recovered.recovery_digest = kiana_domain::json_digest(&json!({
        "schema": recovered.schema,
        "run_id": recovered.run_id,
        "attempt_id": recovered.attempt_id,
        "source_cursor": recovered.source_cursor,
        "last_committed_cursor": recovered.last_committed_cursor,
        "pending_write_digest": recovered.pending_write_digest,
        "source_event_ids": recovered.source_event_ids,
        "authority_epoch": recovered.authority_epoch,
        "state": recovered.state,
        "effect_known": recovered.effect_known,
        "re_admission_authorized": recovered.re_admission_authorized,
    }));
    assert!(recovered.validate().is_ok());

    let invalid = fact(SwarmRecoveryState::Recovered);
    assert_eq!(invalid.validate(), Err("swarm_recovery_fact_invalid"));
}

#[test]
fn cursor_and_unknown_fields_fail_closed() {
    let mut value = serde_json::to_value(fact(SwarmRecoveryState::Replaying)).unwrap();
    value["unexpected"] = json!(true);
    assert!(serde_json::from_value::<SwarmRecoveryFact>(value).is_err());

    let mut invalid = fact(SwarmRecoveryState::PendingWrite);
    invalid.last_committed_cursor = invalid.source_cursor;
    invalid.recovery_digest = kiana_domain::json_digest(&json!({
        "schema": invalid.schema,
        "run_id": invalid.run_id,
        "attempt_id": invalid.attempt_id,
        "source_cursor": invalid.source_cursor,
        "last_committed_cursor": invalid.last_committed_cursor,
        "pending_write_digest": invalid.pending_write_digest,
        "source_event_ids": invalid.source_event_ids,
        "authority_epoch": invalid.authority_epoch,
        "state": invalid.state,
        "effect_known": invalid.effect_known,
        "re_admission_authorized": invalid.re_admission_authorized,
    }));
    assert_eq!(invalid.validate(), Err("swarm_recovery_fact_invalid"));
}

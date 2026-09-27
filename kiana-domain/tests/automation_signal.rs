use kiana_domain::{AutomationSignalFact, AutomationSignalState, AUTOMATION_SIGNAL_SCHEMA};
use serde_json::json;

const D: &str = "sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";

fn fact(state: AutomationSignalState) -> AutomationSignalFact {
    let mut value = AutomationSignalFact {
        schema: AUTOMATION_SIGNAL_SCHEMA.to_owned(),
        execution_id: "execution-1".to_owned(),
        signal_id: "signal-1".to_owned(),
        owner_id: "owner-1".to_owned(),
        action_digest: D.to_owned(),
        path_scope_digest: D.to_owned(),
        authority_epoch: 4,
        checkpoint_sequence: 2,
        state,
        consumed: matches!(state, AutomationSignalState::Consumed),
        signal_digest: String::new(),
    };
    value.signal_digest = kiana_domain::json_digest(&json!({
        "schema": value.schema,
        "execution_id": value.execution_id,
        "signal_id": value.signal_id,
        "owner_id": value.owner_id,
        "action_digest": value.action_digest,
        "path_scope_digest": value.path_scope_digest,
        "authority_epoch": value.authority_epoch,
        "checkpoint_sequence": value.checkpoint_sequence,
        "state": value.state,
        "consumed": value.consumed,
    }));
    value
}

#[test]
fn pause_resume_and_single_consume_are_explicit() {
    assert!(fact(AutomationSignalState::Paused).validate().is_ok());
    assert!(fact(AutomationSignalState::Resumed).validate().is_ok());
    assert!(fact(AutomationSignalState::Consumed).validate().is_ok());
}

#[test]
fn consumed_flag_and_unknown_fields_fail_closed() {
    let mut invalid = fact(AutomationSignalState::Paused);
    invalid.consumed = true;
    invalid.signal_digest = kiana_domain::json_digest(&json!({
        "schema": invalid.schema,
        "execution_id": invalid.execution_id,
        "signal_id": invalid.signal_id,
        "owner_id": invalid.owner_id,
        "action_digest": invalid.action_digest,
        "path_scope_digest": invalid.path_scope_digest,
        "authority_epoch": invalid.authority_epoch,
        "checkpoint_sequence": invalid.checkpoint_sequence,
        "state": invalid.state,
        "consumed": invalid.consumed,
    }));
    assert_eq!(invalid.validate(), Err("automation_signal_fact_invalid"));

    let mut value = serde_json::to_value(fact(AutomationSignalState::Paused)).unwrap();
    value["unexpected"] = json!(true);
    assert!(serde_json::from_value::<AutomationSignalFact>(value).is_err());
}

use kiana_domain::{
    SwarmPlanId, SwarmProjectionEvent, SwarmProjectionEventKind, SwarmProjectionState,
    SWARM_PROJECTION_SCHEMA,
};
use serde_json::json;

const D: &str = "sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";

fn event(
    epoch: &str,
    sequence: u64,
    cursor: u64,
    kind: SwarmProjectionEventKind,
) -> SwarmProjectionEvent {
    let mut value = SwarmProjectionEvent {
        schema: SWARM_PROJECTION_SCHEMA.to_owned(),
        epoch: epoch.to_owned(),
        sequence,
        source_cursor: cursor,
        swarm_plan_id: SwarmPlanId::new(),
        parent_ref: "parent".to_owned(),
        child_ref: Some("child".to_owned()),
        kind,
        terminal: kind == SwarmProjectionEventKind::Terminal,
        payload_digest: D.to_owned(),
        event_digest: String::new(),
    };
    value.event_digest = kiana_domain::json_digest(&json!({
        "schema": value.schema,
        "epoch": value.epoch,
        "sequence": value.sequence,
        "source_cursor": value.source_cursor,
        "swarm_plan_id": value.swarm_plan_id,
        "parent_ref": value.parent_ref,
        "child_ref": value.child_ref,
        "kind": value.kind,
        "terminal": value.terminal,
        "payload_digest": value.payload_digest,
    }));
    value
}

#[test]
fn projection_applies_ordered_events_and_exact_replay_without_duplicate_effect() {
    let mut state = SwarmProjectionState::new("epoch-1").unwrap();
    let first = event("epoch-1", 1, 10, SwarmProjectionEventKind::Progress);
    state.apply(&first).unwrap();
    state.apply(&first).unwrap();
    let terminal = event("epoch-1", 2, 11, SwarmProjectionEventKind::Terminal);
    state.apply(&terminal).unwrap();
    assert!(state.terminal);
    assert_eq!(state.last_sequence, 2);
}

#[test]
fn gaps_epoch_drift_terminal_resurrection_and_unknown_fields_fail_closed() {
    let mut state = SwarmProjectionState::new("epoch-1").unwrap();
    let gap = event("epoch-1", 2, 11, SwarmProjectionEventKind::Progress);
    assert_eq!(
        state.apply(&gap).unwrap_err(),
        "swarm_projection_sequence_gap"
    );
    let drift = event("epoch-2", 1, 10, SwarmProjectionEventKind::Progress);
    assert_eq!(
        state.apply(&drift).unwrap_err(),
        "swarm_projection_epoch_mismatch"
    );
    let terminal = event("epoch-1", 1, 10, SwarmProjectionEventKind::Terminal);
    state.apply(&terminal).unwrap();
    let late = event("epoch-1", 2, 11, SwarmProjectionEventKind::Progress);
    assert_eq!(
        state.apply(&late).unwrap_err(),
        "swarm_projection_terminal_resurrection"
    );

    let mut value = serde_json::to_value(terminal).unwrap();
    value["unexpected"] = json!(true);
    assert!(serde_json::from_value::<SwarmProjectionEvent>(value).is_err());
}

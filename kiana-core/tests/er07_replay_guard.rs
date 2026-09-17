use kiana_core::ReplayProjection;
use kiana_domain::{ProjectionCheckpoint, RequestId, RuntimeEvent};
use serde_json::json;

fn event(request_id: RequestId, sequence: u64, delta: i64) -> RuntimeEvent {
    RuntimeEvent::new(
        request_id,
        sequence,
        "projection.delta",
        json!({"delta":delta}),
    )
    .unwrap()
}

fn fold(state: serde_json::Value, event: &RuntimeEvent) -> Result<serde_json::Value, String> {
    let mut state = state;
    let count = state["count"].as_i64().unwrap_or_default();
    let delta = event.data["delta"].as_i64().unwrap_or_default();
    state["count"] = json!(count + delta);
    Ok(state)
}

#[test]
fn replay_from_checkpoint_matches_rebuild_and_skips_duplicate_event_ids() {
    let request_id = RequestId::new();
    let first = event(request_id, 1, 2);
    let second = event(request_id, 2, 3);
    let third = event(request_id, 3, 5);
    let zero = ReplayProjection::from_zero(
        "counter",
        json!({"count":0}),
        &[first.clone(), second.clone()],
        2,
        fold,
    )
    .unwrap();
    let checkpoint = zero.checkpoint().unwrap();
    let resumed = ReplayProjection::from_checkpoint(
        "counter",
        &checkpoint,
        &[second.clone(), third.clone()],
        3,
        fold,
    )
    .unwrap();
    let rebuilt = ReplayProjection::from_zero(
        "counter",
        json!({"count":0}),
        &[first, second, third],
        3,
        fold,
    )
    .unwrap();
    assert_eq!(resumed.state(), rebuilt.state());
    assert_eq!(resumed.state_digest(), rebuilt.state_digest());
    assert_eq!(
        resumed.checkpoint().unwrap().checkpoint_digest,
        rebuilt.checkpoint().unwrap().checkpoint_digest
    );
}

#[test]
fn replay_checkpoint_failure_is_fail_closed_and_cannot_authorize() {
    let request_id = RequestId::new();
    let event = event(request_id, 1, 1);
    let checkpoint =
        ProjectionCheckpoint::new("counter", 0, Vec::new(), json!({"count":0})).unwrap();
    assert!(
        ReplayProjection::from_checkpoint("different", &checkpoint, &[event.clone()], 1, fold,)
            .is_err()
    );
    assert!(ReplayProjection::from_checkpoint(
        "counter",
        &checkpoint,
        &[event],
        0,
        |_state, _event| Err("source_failure".to_owned()),
    )
    .is_err());
}

#[test]
fn er07_replay_helper_is_read_only_and_checkpoint_driven() {
    let helper = include_str!("../src/projection_checkpoint.rs");
    let domain = include_str!("../../kiana-domain/src/projection_contracts.rs");
    for marker in [
        "from_zero",
        "from_checkpoint",
        "ProjectionCheckpoint",
        "source_cursor",
        "source_event_ids",
        "projection_checkpoint_digest_mismatch",
        "projection_fold_failed",
        "BTreeSet",
        "MAX_SOURCE_EVENT_IDS",
    ] {
        assert!(
            helper.contains(marker) || domain.contains(marker),
            "ER-07 marker missing: {marker}"
        );
    }
    for forbidden in [
        "CapabilityBrokerPort",
        "ExecutionPermit",
        "commit_transition",
        "append_event",
        "signal_cancel",
    ] {
        assert!(
            !helper.contains(forbidden),
            "projection helper must not {forbidden}"
        );
    }
}

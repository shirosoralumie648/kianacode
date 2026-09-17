use kiana_domain::{
    EventId, ProjectionCheckpoint, RequestId, RunId, RuntimeEvent, SchemaVersion,
    PROJECTION_CHECKPOINT_SCHEMA, PROJECTION_CHECKPOINT_VERSION,
};
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

#[test]
fn projection_checkpoint_is_strict_and_digest_bound() {
    let event_id = EventId::new();
    let checkpoint =
        ProjectionCheckpoint::new("counter", 4, vec![event_id], json!({"count":4})).unwrap();
    assert_eq!(checkpoint.schema, PROJECTION_CHECKPOINT_SCHEMA);
    assert_eq!(checkpoint.version, PROJECTION_CHECKPOINT_VERSION);
    assert!(checkpoint.validate().is_ok());
    assert_eq!(
        ProjectionCheckpoint::from_json(&checkpoint.to_json().unwrap()).unwrap(),
        checkpoint
    );

    let mut unknown = checkpoint.to_json().unwrap();
    unknown["secret_state"] = json!("must-not-be-accepted");
    assert_eq!(
        ProjectionCheckpoint::from_json(&unknown).unwrap_err(),
        "projection_checkpoint_decode_failed"
    );

    let mut tampered = checkpoint.clone();
    tampered.state["count"] = json!(5);
    assert_eq!(
        tampered.validate().unwrap_err(),
        "projection_checkpoint_state_digest_mismatch"
    );

    let mut wrong_version = checkpoint;
    wrong_version.version = SchemaVersion::new(2, 0);
    assert_eq!(
        wrong_version.validate().unwrap_err(),
        "projection_checkpoint_header_invalid"
    );
}

#[test]
fn projection_checkpoint_rejects_invalid_source_identity() {
    let run_id = RunId::new();
    let mut checkpoint =
        ProjectionCheckpoint::new("run", 1, Vec::new(), json!({"run":run_id})).unwrap();
    let duplicate = EventId::new();
    checkpoint.source_event_ids = vec![duplicate, duplicate];
    checkpoint.checkpoint_digest = checkpoint.digest();
    assert_eq!(
        checkpoint.validate().unwrap_err(),
        "projection_checkpoint_header_invalid"
    );
}

#[test]
fn projection_checkpoint_allows_empty_rebuild_anchor() {
    let checkpoint = ProjectionCheckpoint::new("empty", 0, Vec::new(), json!({})).unwrap();
    assert_eq!(checkpoint.source_cursor, 0);
    assert!(checkpoint.source_event_ids.is_empty());
    assert!(checkpoint.validate().is_ok());
}

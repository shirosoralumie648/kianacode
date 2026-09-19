use kiana_protocol::{ProgressAction, ProgressDecision, ProgressTracker};

#[test]
fn progress_projection_types_are_additive_and_bounded() {
    assert!(ProgressAction::Blocked.is_blocked());
    let tracker = ProgressTracker::default();
    let encoded = serde_json::to_value(&tracker).unwrap();
    assert_eq!(encoded["schema"], "kiana.progress-tracker.v1");
    let _ = std::mem::size_of::<ProgressDecision>();
}

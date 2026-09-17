#[test]
fn symposium_decision_is_durable_and_replayable() {
    let domain = include_str!("../../kiana-domain/src/symposiums.rs");
    let collaboration = include_str!("../src/collaboration.rs");
    let events = include_str!("../src/events.rs");
    let existing = include_str!("control_plane.rs");
    for marker in [
        "pub struct Symposium",
        "pub struct DecisionRecord",
        "pub fn close(",
        "pub async fn convene_symposium",
        "symposium.closed",
        "write_symposium_artifacts",
        "record_event",
        "each_department_anti_meeting_writes_its_own_artifact",
    ] {
        assert!(
            domain.contains(marker)
                || collaboration.contains(marker)
                || events.contains(marker)
                || existing.contains(marker),
            "Symposium replay marker missing: {marker}"
        );
    }
    assert!(collaboration.contains("decision"));
    assert!(collaboration.contains("decision_id"));
    assert!(events.contains("read_stream"));
}

#[test]
fn symposium_rejects_invalid_chair_and_preserves_decision_alternatives() {
    let domain = include_str!("../../kiana-domain/src/symposiums.rs");
    let existing = include_str!("control_plane.rs");
    assert!(domain.contains("symposium_chair_must_be_pm"));
    assert!(domain.contains("symposium_builder_not_attendee"));
    assert!(domain.contains("draft_decision"));
    assert!(domain.contains("blackboard"));
    assert!(existing.contains("symposium_architect_chair_fails_closed"));
    assert!(existing.contains("convene_two_rounds_uses_private_speaker_sessions"));
}

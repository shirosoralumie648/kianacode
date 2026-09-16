#[test]
fn swarm_status_reducer_is_typed_and_replay_uses_the_existing_event_path() {
    let domain = include_str!("../../kiana-domain/src/swarm_reducer.rs");
    let swarm = include_str!("../../kiana-domain/src/swarm.rs");
    let core = include_str!("../src/swarm.rs");
    for marker in [
        "pub enum SwarmTransitionEntity",
        "pub enum AttemptStatus",
        "pub struct SwarmTransitionEvent",
        "pub struct SwarmTransitionReducer",
        "swarm_transition_illegal",
        "swarm_partition_transition_illegal",
        "swarm_attempt_transition_illegal",
        "swarm_transition_epoch_regression",
        "pub fn replay",
        "review_complete",
    ] {
        assert!(domain.contains(marker), "reducer marker missing: {marker}");
    }
    assert!(swarm.contains("pub fn validate_transitions"));
    assert!(swarm.contains("transitions: Vec<SwarmTransitionEvent>"));
    assert!(core.contains("validate_transitions"));
    assert!(core.contains("commit_swarm"));
    assert!(core.contains("handle_company_command"));
}

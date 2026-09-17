#[test]
fn steer_cannot_change_sandbox_or_model_profile() {
    let lifecycle = include_str!("../src/lifecycle.rs");
    let protocol = include_str!("../../kiana-protocol/src/lib.rs");
    assert!(lifecycle.contains("pub async fn steer_run"));
    assert!(lifecycle.contains("expected_turn_id"));
    assert!(lifecycle.contains("RunnerCommand::Inject"));
    assert!(protocol.contains("pub struct SteerRequest"));
    assert!(protocol.contains("pub struct InjectRequest"));
    assert!(!lifecycle.contains("steer_run(\n        mut context"));
    assert!(!lifecycle.contains("sandbox: String"));
    assert!(!lifecycle.contains("model_profile: String"));
}

#[test]
fn stale_turn_steer_does_not_target_next_turn() {
    let lifecycle = include_str!("../src/lifecycle.rs");
    let steer_start = lifecycle.find("pub async fn steer_run").unwrap();
    let steer = &lifecycle[steer_start..];
    assert!(steer.contains("\"next-step\""));
    assert!(steer.contains("Some(expected_turn_id)"));
    assert!(lifecycle.contains("expected_turn_id != current_turn"));
    assert!(lifecycle.contains("\"stale_turn_steer\""));
    assert!(!steer.contains("\"next-turn\""));
}

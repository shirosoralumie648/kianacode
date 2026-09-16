use kiana_domain::{RunId, TurnId};
use kiana_runner_protocol::RunnerCommand;

#[test]
fn native_start_carries_optional_server_turn_and_legacy_start_stays_readable() {
    let run_id = RunId::new();
    let turn_id = TurnId::new();
    let command = RunnerCommand::start_in_with_history_and_turn(
        run_id,
        "hello",
        Vec::new(),
        "/tmp/project",
        "read-only",
        "",
        true,
        8,
        Some(turn_id),
    );
    let value = serde_json::to_value(&command).unwrap();
    assert_eq!(value["turn_id"], turn_id.to_string());
    assert_eq!(
        serde_json::from_value::<RunnerCommand>(value).unwrap(),
        command
    );

    let legacy = serde_json::json!({
        "command": "start",
        "run_id": run_id,
        "prompt": "legacy"
    });
    let decoded = serde_json::from_value::<RunnerCommand>(legacy).unwrap();
    assert!(matches!(
        decoded,
        RunnerCommand::Start { turn_id: None, .. }
    ));
}

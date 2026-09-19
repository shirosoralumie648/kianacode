use kiana_protocol::{LegacyCheckpointDecision, RunId, TurnId};

#[test]
fn unknown_checkpoint_wire_material_is_explicitly_not_resumable() {
    let decision = LegacyCheckpointDecision::assess(
        "kiana.harness-checkpoint.v1",
        77,
        Some(RunId::new()),
        Some(TurnId::new()),
    )
    .unwrap();
    let encoded = serde_json::to_value(decision).unwrap();
    assert_eq!(encoded["disposition"], "not_supported");
    assert_eq!(encoded["resume_allowed"], false);
    assert_eq!(encoded["schema"], "kiana.legacy-checkpoint-decision.v1");
}

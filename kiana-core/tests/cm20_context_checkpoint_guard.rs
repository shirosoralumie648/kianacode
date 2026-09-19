#[test]
fn context_checkpoint_is_artifact_first_and_inbox_cas_bound() {
    let source = include_str!("../../kiana-domain/src/context_checkpoint.rs");
    for marker in [
        "ContextCheckpoint",
        "CompactionArtifact",
        "CompactionCommit",
        "context_checkpoint_source_binding_changed",
        "context_checkpoint_inbox_changed",
        "context_checkpoint_inbox_regressed",
        "ContextCheckpointState::Committed",
        "current_inbox_sequence",
        "checkpoint_digest",
    ] {
        assert!(
            source.contains(marker),
            "CM-20 source marker missing: {marker}"
        );
    }
    assert!(!source.contains("ModelClient"));
    assert!(!source.contains("CapabilityBroker"));
}

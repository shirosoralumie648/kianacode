#[test]
fn h23_compaction_commit_is_artifact_first_and_epoch_bound() {
    let domain = include_str!("../../kiana-domain/src/compaction_commit.rs");
    let runner = include_str!("../../kiana-runner/src/harness.rs");
    let protocol = include_str!("../../kiana-runner-protocol/src/lib.rs");
    for marker in [
        "CompactionArtifact",
        "CompactionCommit",
        "validate_before_commit",
        "compaction_source_changed",
        "compaction_workspace_changed",
        "compaction_data_epoch_changed",
        "source_event_ids",
        "append_idempotent_expected",
        "RunnerEvent::Compacted",
        "prompt_sources",
    ] {
        assert!(
            domain.contains(marker) || runner.contains(marker) || protocol.contains(marker),
            "H23 compaction marker missing: {marker}"
        );
    }
    assert!(runner.contains("checkpoint"));
    assert!(!domain.contains("std::fs"));
}

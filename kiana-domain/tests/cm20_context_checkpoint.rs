use kiana_domain::{
    json_digest, CompactionArtifact, CompactionCommit, ContextCheckpoint, ContextCheckpointState,
    EventId, RunId,
};

fn digest(value: &str) -> String {
    json_digest(&serde_json::json!({"value": value}))
}

fn artifact() -> CompactionArtifact {
    CompactionArtifact::new(
        RunId::new(),
        digest("summary"),
        digest("artifact"),
        10,
        12,
        256,
    )
    .unwrap()
}

fn prepared() -> (ContextCheckpoint, CompactionArtifact, CompactionCommit) {
    let checkpoint = ContextCheckpoint::new(RunId::new(), 4, 10, digest("inbox"), 1).unwrap();
    let original = artifact();
    let artifact = CompactionArtifact::new(
        checkpoint.run_id,
        original.summary_digest,
        original.artifact_digest,
        original.source_cursor_start,
        original.source_cursor_end,
        original.content_bytes,
    )
    .unwrap();
    let commit = CompactionCommit::new(
        &artifact,
        4,
        5,
        vec![EventId::new()],
        digest("prompt"),
        "builder-default",
        digest("budget"),
        8,
        3,
    )
    .unwrap();
    (checkpoint, artifact, commit)
}

#[test]
fn crash_before_compaction_commit_keeps_old_context() {
    let (checkpoint, _, _) = prepared();
    checkpoint.validate().unwrap();
    assert_eq!(checkpoint.state, ContextCheckpointState::Active);
    assert_eq!(checkpoint.active_context_revision, 4);
    assert!(checkpoint.artifact_digest.is_none());
}

#[test]
fn stale_summary_cannot_overwrite_new_steering() {
    let (checkpoint, artifact, commit) = prepared();
    assert_eq!(
        checkpoint
            .commit(&artifact, &commit, 5, 12, 8, 3, digest("inbox"), 1,)
            .unwrap_err(),
        "compaction_source_changed"
    );
}

#[test]
fn compaction_commit_preserves_new_inbox_input() {
    let (checkpoint, artifact, commit) = prepared();
    let committed = checkpoint
        .commit(
            &artifact,
            &commit,
            4,
            12,
            8,
            3,
            digest("inbox-after-steering"),
            2,
        )
        .unwrap();
    committed.validate().unwrap();
    assert_eq!(committed.state, ContextCheckpointState::Committed);
    assert_eq!(committed.active_context_revision, 5);
    assert_eq!(committed.inbox_sequence, 2);
    assert_eq!(committed.inbox_digest, digest("inbox-after-steering"));
}

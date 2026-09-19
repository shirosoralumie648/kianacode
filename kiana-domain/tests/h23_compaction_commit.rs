use kiana_domain::{json_digest, CompactionArtifact, CompactionCommit, EventId, RunId};

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

fn commit() -> CompactionCommit {
    let artifact = artifact();
    CompactionCommit::new(
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
    .unwrap()
}

#[test]
fn artifact_is_validated_before_compaction_view_commit() {
    let commit = commit();
    commit.validate().unwrap();
    commit.validate_before_commit(4, 12, 8, 3).unwrap();
    assert_eq!(
        commit.validate_before_commit(4, 13, 8, 3).unwrap_err(),
        "compaction_source_changed"
    );
}

#[test]
fn stale_steering_workspace_or_revoked_data_cannot_overwrite_summary() {
    let commit = commit();
    assert_eq!(
        commit.validate_before_commit(5, 12, 8, 3).unwrap_err(),
        "compaction_source_changed"
    );
    assert_eq!(
        commit.validate_before_commit(4, 12, 9, 3).unwrap_err(),
        "compaction_workspace_changed"
    );
    assert_eq!(
        commit.validate_before_commit(4, 12, 8, 4).unwrap_err(),
        "compaction_data_epoch_changed"
    );
}

#[test]
fn forged_artifact_or_duplicate_source_event_is_rejected() {
    let mut artifact = artifact();
    artifact.artifact_digest = digest("forged");
    assert_eq!(
        artifact.validate().unwrap_err(),
        "compaction_artifact_digest_mismatch"
    );
    let artifact = artifact();
    let event_id = EventId::new();
    assert_eq!(
        CompactionCommit::new(
            &artifact,
            4,
            5,
            vec![event_id, event_id],
            digest("prompt"),
            "builder-default",
            digest("budget"),
            8,
            3,
        )
        .unwrap_err(),
        "compaction_commit_source_duplicate"
    );
}

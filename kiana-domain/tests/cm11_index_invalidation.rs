use kiana_domain::*;

fn digest(value: char) -> String {
    format!("sha256:{}", value.to_string().repeat(64))
}

fn limits() -> WorkspaceSnapshotLimits {
    WorkspaceSnapshotLimits {
        max_files: 8,
        max_total_bytes: 8_192,
        max_file_bytes: 1_024,
        max_depth: 4,
        max_elapsed_ms: 100,
    }
}

fn file(path: &str, content: char, inode: u64, modified: u64) -> WorkspaceFileSnapshot {
    let content_digest = digest(content);
    WorkspaceFileSnapshot {
        schema: WORKSPACE_FILE_SNAPSHOT_SCHEMA.to_owned(),
        path: path.to_owned(),
        kind: WorkspaceEntryKind::File,
        trust: WorkspaceTrust::Trusted,
        identity_before: WorkspaceFileIdentity {
            device: Some(1),
            inode: Some(inode),
            size_bytes: 1,
            modified_unix_ms: Some(modified),
            content_digest: None,
            hard_link_count: Some(1),
        },
        identity_after: Some(WorkspaceFileIdentity {
            device: Some(1),
            inode: Some(inode),
            size_bytes: 1,
            modified_unix_ms: Some(modified),
            content_digest: Some(content_digest.clone()),
            hard_link_count: Some(1),
        }),
        bytes_read: 1,
        disposition: WorkspaceReadDisposition::Indexed,
        instruction_safe: true,
        reason: None,
        file_digest: content_digest,
    }
}

fn snapshot(files: Vec<WorkspaceFileSnapshot>) -> WorkspaceSnapshot {
    WorkspaceSnapshot::new(
        "/workspace",
        WorkspaceTrust::Trusted,
        limits(),
        files,
        false,
        Vec::new(),
    )
    .unwrap()
}

#[test]
fn changed_file_invalidates_only_affected_chunks() {
    let previous = snapshot(vec![file("a.rs", 'a', 1, 1), file("b.rs", 'b', 2, 1)]);
    let current = snapshot(vec![file("a.rs", 'a', 1, 2), file("b.rs", 'c', 2, 2)]);
    let plan = IndexInvalidationPlan::from_snapshots(Some(&previous), &current, 1, 2).unwrap();
    assert_eq!(plan.changes.len(), 1);
    assert_eq!(plan.changes[0].kind, WorkspaceChangeKind::Changed);
    assert_eq!(plan.invalidated_paths, vec!["b.rs"]);
    assert!(plan.tombstones.is_empty());
}

#[test]
fn rename_and_delete_emit_tombstones_and_do_not_use_mtime_alone() {
    let previous = snapshot(vec![file("old.rs", 'a', 1, 1), file("same.rs", 'b', 2, 1)]);
    let current = snapshot(vec![file("new.rs", 'a', 9, 9), file("same.rs", 'b', 2, 99)]);
    let plan = IndexInvalidationPlan::from_snapshots(Some(&previous), &current, 3, 4).unwrap();
    assert_eq!(plan.changes.len(), 1);
    assert_eq!(plan.changes[0].kind, WorkspaceChangeKind::Renamed);
    assert_eq!(plan.changes[0].previous_path.as_deref(), Some("old.rs"));
    assert_eq!(plan.tombstones, vec!["old.rs"]);
    assert!(plan.invalidated_paths.contains(&"new.rs".to_owned()));
    assert!(plan.invalidated_paths.contains(&"old.rs".to_owned()));
}

#[test]
fn cache_key_binds_all_source_and_algorithm_inputs() {
    let key = IndexCacheKey::new(
        digest('a'),
        digest('b'),
        "main",
        digest('c'),
        digest('d'),
        digest('e'),
        digest('f'),
    )
    .unwrap();
    let changed = IndexCacheKey::new(
        digest('a'),
        digest('b'),
        "feature",
        digest('c'),
        digest('d'),
        digest('e'),
        digest('f'),
    )
    .unwrap();
    assert_ne!(key.key_digest, changed.key_digest);
    key.validate().unwrap();
}

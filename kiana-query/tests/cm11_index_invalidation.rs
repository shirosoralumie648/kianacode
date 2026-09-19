use kiana_domain::*;
use kiana_query::plan_index_invalidation;

fn digest(value: char) -> String {
    format!("sha256:{}", value.to_string().repeat(64))
}

fn snapshot(path: &str) -> WorkspaceSnapshot {
    let file = WorkspaceFileSnapshot {
        schema: WORKSPACE_FILE_SNAPSHOT_SCHEMA.to_owned(),
        path: path.to_owned(),
        kind: WorkspaceEntryKind::File,
        trust: WorkspaceTrust::Trusted,
        identity_before: WorkspaceFileIdentity {
            device: Some(1),
            inode: Some(1),
            size_bytes: 1,
            modified_unix_ms: Some(1),
            content_digest: None,
            hard_link_count: Some(1),
        },
        identity_after: Some(WorkspaceFileIdentity {
            device: Some(1),
            inode: Some(1),
            size_bytes: 1,
            modified_unix_ms: Some(1),
            content_digest: Some(digest('a')),
            hard_link_count: Some(1),
        }),
        bytes_read: 1,
        disposition: WorkspaceReadDisposition::Indexed,
        instruction_safe: true,
        reason: None,
        file_digest: digest('a'),
    };
    WorkspaceSnapshot::new(
        "/workspace",
        WorkspaceTrust::Trusted,
        WorkspaceSnapshotLimits {
            max_files: 4,
            max_total_bytes: 4_096,
            max_file_bytes: 1_024,
            max_depth: 4,
            max_elapsed_ms: 100,
        },
        vec![file],
        false,
        Vec::new(),
    )
    .unwrap()
}

#[test]
fn query_adapter_returns_typed_invalidation_plan() {
    let previous = snapshot("before.rs");
    let current = snapshot("after.rs");
    let plan = plan_index_invalidation(Some(&previous), &current, 1, 2).unwrap();
    assert_eq!(plan.changes[0].kind, WorkspaceChangeKind::Renamed);
    assert!(plan.tombstones.contains(&"before.rs".to_owned()));
}

use kiana_domain::*;

fn digest(value: char) -> String {
    format!("sha256:{}", value.to_string().repeat(64))
}

fn identity(size: u64, digest_value: Option<String>) -> WorkspaceFileIdentity {
    WorkspaceFileIdentity {
        device: Some(1),
        inode: Some(2),
        size_bytes: size,
        modified_unix_ms: Some(3),
        content_digest: digest_value,
        hard_link_count: Some(1),
    }
}

fn limits() -> WorkspaceSnapshotLimits {
    WorkspaceSnapshotLimits {
        max_files: 4,
        max_total_bytes: 4_096,
        max_file_bytes: 1_024,
        max_depth: 4,
        max_elapsed_ms: 100,
    }
}

#[test]
fn trusted_read_requires_stable_before_after_identity() {
    let file = WorkspaceFileSnapshot {
        schema: WORKSPACE_FILE_SNAPSHOT_SCHEMA.to_owned(),
        path: "src/lib.rs".to_owned(),
        kind: WorkspaceEntryKind::File,
        trust: WorkspaceTrust::Trusted,
        identity_before: identity(3, None),
        identity_after: Some(identity(3, Some(digest('a')))),
        bytes_read: 3,
        disposition: WorkspaceReadDisposition::Indexed,
        instruction_safe: true,
        reason: None,
        file_digest: digest('a'),
    };
    file.validate().unwrap();
    let snapshot = WorkspaceSnapshot::new(
        "/workspace",
        WorkspaceTrust::Trusted,
        limits(),
        vec![file],
        false,
        Vec::new(),
    )
    .unwrap();
    snapshot.validate().unwrap();
    assert_eq!(
        schema_contract(WORKSPACE_SNAPSHOT_SCHEMA)
            .unwrap()
            .owner_crate,
        "kiana-domain"
    );
}

#[test]
fn untrusted_material_is_metadata_only_and_fenced_change_is_not_safe() {
    let metadata = WorkspaceFileSnapshot {
        schema: WORKSPACE_FILE_SNAPSHOT_SCHEMA.to_owned(),
        path: "README.md".to_owned(),
        kind: WorkspaceEntryKind::File,
        trust: WorkspaceTrust::Untrusted,
        identity_before: identity(3, None),
        identity_after: Some(identity(3, None)),
        bytes_read: 0,
        disposition: WorkspaceReadDisposition::MetadataOnly,
        instruction_safe: false,
        reason: Some("untrusted_metadata_only".to_owned()),
        file_digest: digest('b'),
    };
    metadata.validate().unwrap();

    let mut changed_after = identity(4, None);
    changed_after.inode = Some(9);
    let fenced = WorkspaceFileSnapshot {
        schema: WORKSPACE_FILE_SNAPSHOT_SCHEMA.to_owned(),
        path: "src/main.rs".to_owned(),
        kind: WorkspaceEntryKind::File,
        trust: WorkspaceTrust::Trusted,
        identity_before: identity(3, None),
        identity_after: Some(changed_after),
        bytes_read: 0,
        disposition: WorkspaceReadDisposition::Fenced,
        instruction_safe: false,
        reason: Some("workspace_change_between_scan_and_read".to_owned()),
        file_digest: digest('c'),
    };
    fenced.validate().unwrap();
}

#[test]
fn snapshot_rejects_duplicate_paths_and_invalid_limits() {
    let invalid = WorkspaceSnapshotLimits {
        max_files: 0,
        ..limits()
    };
    assert_eq!(
        invalid.validate().unwrap_err(),
        "workspace_snapshot_limits_invalid"
    );

    let file = WorkspaceFileSnapshot {
        schema: WORKSPACE_FILE_SNAPSHOT_SCHEMA.to_owned(),
        path: "same.txt".to_owned(),
        kind: WorkspaceEntryKind::File,
        trust: WorkspaceTrust::Untrusted,
        identity_before: identity(0, None),
        identity_after: Some(identity(0, None)),
        bytes_read: 0,
        disposition: WorkspaceReadDisposition::MetadataOnly,
        instruction_safe: false,
        reason: Some("untrusted_metadata_only".to_owned()),
        file_digest: digest('d'),
    };
    assert_eq!(
        WorkspaceSnapshot::new(
            "/workspace",
            WorkspaceTrust::Untrusted,
            limits(),
            vec![file.clone(), file],
            false,
            Vec::new(),
        )
        .unwrap_err(),
        "workspace_snapshot_duplicate_path"
    );
}

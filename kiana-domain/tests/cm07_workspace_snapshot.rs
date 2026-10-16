//! CM-07 Domain workspace snapshot 合同测试。
//!
//! 本文件手工构造身份、trust、disposition 和摘要，不访问真实目录，也不调用 Query scanner。
//! 它验证 Domain DTO 对 supplied evidence 的内部一致性与稳定拒绝错误；不能证明字段来自真实文件系统。

use kiana_domain::*;

/// 用固定重复字符生成形状正确的 SHA-256 fixture，不对应真实内容。
fn digest(value: char) -> String {
    format!("sha256:{}", value.to_string().repeat(64))
}

/// 构造可控的文件身份 fixture；调用者可分别改变大小和可选内容摘要。
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

/// 提供非零且互相兼容的小预算，避免测试依赖生产默认值。
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
/// 稳定的 Trusted 普通文件证据可以进入 Indexed，并能组成结构有效的 Snapshot。
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
/// Untrusted 正文只能是 metadata-only；读取前后身份变化时 Trusted 文件必须标为 Fenced。
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
/// 文件数上限必须有效，且聚合快照拒绝重复相对路径。
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

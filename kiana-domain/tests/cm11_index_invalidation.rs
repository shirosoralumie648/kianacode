//! CM-11 Domain snapshot diff、tombstone 和 cache identity 合同测试。
//!
//! 这里的 WorkspaceSnapshot/FileIdentity 全部是合成值；测试证明 Domain 对这些声明字段的分类规则，
//! 不证明真实 scanner、磁盘变更或索引缓存已接入。

use kiana_domain::*;

/// 生成用于文件内容、cache 输入和 snapshot 摘要的合成 SHA-256 字符串。
fn digest(value: char) -> String {
    format!("sha256:{}", value.to_string().repeat(64))
}

/// 提供边界明确的测试扫描限制；这些数值是 fixture，不是生产默认配置。
fn limits() -> WorkspaceSnapshotLimits {
    WorkspaceSnapshotLimits {
        max_files: 8,
        max_total_bytes: 8_192,
        max_file_bytes: 1_024,
        max_depth: 4,
        max_elapsed_ms: 100,
    }
}

/// 构造可信、可索引的单文件 snapshot。
///
/// `identity_after` 携带读取后 content digest；modified 时间可独立变化，以验证 mtime 本身不制造内容变更。
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

/// 用合成文件列表生成并立即验证 WorkspaceSnapshot。
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

/// 验证同路径文件只有内容摘要变化时被失效，未变化文件不进入 affected path 集合。
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

/// 验证相同内容迁移到新路径可形成 rename+tombstone，且 mtime-only 变化不会被报告。
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

/// 验证 branch 改变会改变整体 cache key，即使其他 root/parser/chunker/config 输入不变。
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

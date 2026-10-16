//! CM-11 Query adapter 对 typed Domain invalidation plan 的转发测试。
//!
//! fixture 使用两个经过校验的合成 WorkspaceSnapshot，确认 Query 暴露旧/新路径和 tombstone；
//! 它不删除 cache、不运行索引 rebuild，也不检查生产调用链。

use kiana_domain::*;
use kiana_query::plan_index_invalidation;

/// 生成测试用的格式化摘要字符串。
fn digest(value: char) -> String {
    format!("sha256:{}", value.to_string().repeat(64))
}

/// 创建单文件合成 snapshot，供 adapter fixture 比较不同相对路径。
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

/// 验证 Query adapter 返回 Domain rename 计划，并把旧路径作为 tombstone 带回。
#[test]
fn query_adapter_returns_typed_invalidation_plan() {
    let previous = snapshot("before.rs");
    let current = snapshot("after.rs");
    let plan = plan_index_invalidation(Some(&previous), &current, 1, 2).unwrap();
    assert_eq!(plan.changes[0].kind, WorkspaceChangeKind::Renamed);
    assert!(plan.tombstones.contains(&"before.rs".to_owned()));
}

//! SnapshotManifest 的内存合同 fixture。
//!
//! 这些测试直接构造文件元数据和根路径字符串，再调用 Domain 的 manifest constructor；它们
//! 覆盖字段组合、排序后封存和边界拒绝，不创建目录、不读取文件，也不证明实际备份可恢复。
//! 文件哈希由 `hash()` 生成固定形状的假值，测试只关心格式和摘要绑定。

use kiana_domain::{SnapshotFileSeal, SnapshotManifest, SnapshotMode};

/// 生成固定长度的合成 SHA-256 文本，不对任何文件内容做哈希。
fn hash(ch: char) -> String {
    format!("sha256:{}", ch.to_string().repeat(64))
}

#[test]
/// 完整快照在有效源游标、epoch、封存声明和单文件清单下可以构造并通过复验。
fn sealed_full_snapshot_binds_cursor_epoch_and_hashes() {
    let manifest = SnapshotManifest::new(
        "snapshot-1",
        "project:/repo",
        "store-1",
        "instance-1",
        "/repo/.kiana",
        "/backup/repo-1",
        SnapshotMode::Full,
        12,
        4,
        None,
        true,
        true,
        vec![SnapshotFileSeal {
            relative_path: "events.jsonl".to_owned(),
            size_bytes: 10,
            content_hash: hash('a'),
        }],
    )
    .unwrap();
    manifest.validate().unwrap();
}

#[test]
/// 相同活动/备份根与缺少前序快照的增量清单都必须在构造时被拒绝。
fn unsafe_backup_boundaries_and_unsealed_material_fail_closed() {
    let same_root = SnapshotManifest::new(
        "snapshot-1",
        "project:/repo",
        "store-1",
        "instance-1",
        "/repo/.kiana",
        "/repo/.kiana",
        SnapshotMode::Full,
        1,
        1,
        None,
        true,
        true,
        vec![SnapshotFileSeal {
            relative_path: "events.jsonl".to_owned(),
            size_bytes: 1,
            content_hash: hash('a'),
        }],
    );
    assert_eq!(
        same_root.unwrap_err(),
        "snapshot_backup_root_overlaps_active_root"
    );

    let incremental = SnapshotManifest::new(
        "snapshot-2",
        "project:/repo",
        "store-1",
        "instance-1",
        "/repo/.kiana",
        "/backup/repo-2",
        SnapshotMode::Incremental,
        2,
        1,
        None,
        true,
        true,
        vec![SnapshotFileSeal {
            relative_path: "events.jsonl".to_owned(),
            size_bytes: 1,
            content_hash: hash('a'),
        }],
    );
    assert_eq!(
        incremental.unwrap_err(),
        "snapshot_incremental_previous_required"
    );
}

use kiana_domain::{SnapshotFileSeal, SnapshotManifest, SnapshotMode};

fn hash(ch: char) -> String {
    format!("sha256:{}", ch.to_string().repeat(64))
}

#[test]
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

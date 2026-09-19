use kiana_domain::{IndexComponentKind, IndexGenerationState};
use kiana_query::{read_index_manifest, write_index_manifest_atomic};
use std::collections::BTreeMap;
use std::fs;
use std::path::PathBuf;

fn digest(value: char) -> String {
    format!("sha256:{}", value.to_string().repeat(64))
}

fn components(value: char) -> BTreeMap<String, String> {
    IndexComponentKind::ALL
        .into_iter()
        .map(|kind| (kind.as_str().to_owned(), digest(value)))
        .collect()
}

fn root() -> PathBuf {
    let root = std::env::temp_dir().join(format!("kiana-cm10-{}", std::process::id()));
    let _ = fs::remove_dir_all(&root);
    fs::create_dir_all(&root).unwrap();
    root
}

#[test]
fn ready_manifest_publishes_by_temp_sync_and_atomic_rename() {
    let root = root();
    let path = root.join("index/manifest.json");
    let mut state = IndexGenerationState::new();
    let building = state.begin(digest('a'), components('b')).unwrap();
    let ready = state.commit(building).unwrap();
    write_index_manifest_atomic(&path, &ready).unwrap();
    let loaded = read_index_manifest(&path).unwrap();
    assert_eq!(loaded, ready);
    let _ = fs::remove_dir_all(root);
}

#[test]
fn building_manifest_cannot_be_published_as_ready() {
    let root = root();
    let path = root.join("manifest.json");
    let mut state = IndexGenerationState::new();
    let building = state.begin(digest('a'), components('b')).unwrap();
    assert!(write_index_manifest_atomic(&path, &building).is_err());
    assert!(!path.exists());
    let _ = fs::remove_dir_all(root);
}

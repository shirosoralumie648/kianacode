use kiana_query::{
    read_context_material_manifest, write_context_material_manifest_atomic, ContextArtifactOptions,
    ContextMaterialManifest, RepoMapOptions,
};
use std::fs;
use std::path::PathBuf;

fn root() -> PathBuf {
    let root = std::env::temp_dir().join(format!("kiana-pd20-{}", std::process::id()));
    let _ = fs::remove_dir_all(&root);
    fs::create_dir_all(root.join("src")).unwrap();
    fs::write(root.join("src/lib.rs"), "pub fn one() {}\n").unwrap();
    fs::write(root.join("README.md"), "# PD-20\n").unwrap();
    root
}

#[test]
fn material_manifest_binds_repo_map_artifacts_and_dependency_graph() {
    let root = root();
    let manifest = ContextMaterialManifest::build(
        &root,
        RepoMapOptions::default(),
        ContextArtifactOptions::default(),
        7,
        "kiana-query-pd20",
    )
    .unwrap();
    assert_eq!(manifest.source_cursor, 7);
    assert_eq!(manifest.repo_map.root, manifest.root);
    assert_eq!(manifest.artifact_store.root, manifest.root);
    assert_eq!(manifest.artifact_store.dependency_graph.root, manifest.root);

    let path = root.join(".kiana/context-material-manifest.json");
    write_context_material_manifest_atomic(&path, &manifest).unwrap();
    let loaded = read_context_material_manifest(&path).unwrap();
    assert_eq!(loaded.manifest_digest, manifest.manifest_digest);
    let _ = fs::remove_dir_all(root);
}

#[test]
fn tampered_root_or_content_hash_is_rejected() {
    let root = root();
    let manifest = ContextMaterialManifest::build(
        &root,
        RepoMapOptions::default(),
        ContextArtifactOptions::default(),
        1,
        "kiana-query-pd20",
    )
    .unwrap();
    let mut forged = manifest.clone();
    forged.root = "/foreign".to_owned();
    assert_eq!(
        forged.validate().unwrap_err(),
        "context_material_root_mismatch"
    );
    let mut forged_hash = manifest;
    forged_hash.repo_map.files[0].content_hash = "not-a-digest".to_owned();
    forged_hash.manifest_digest = forged_hash.digest();
    assert_eq!(
        forged_hash.validate().unwrap_err(),
        "context_material_repo_content_hash_invalid"
    );
    let _ = fs::remove_dir_all(root);
}

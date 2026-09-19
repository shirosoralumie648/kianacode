//! PD-14 source guard for immutable artifact manifest/hash boundaries.

#[test]
fn artifact_store_is_bounded_and_does_not_follow_workspace_paths() {
    let source = include_str!("../src/artifact_store.rs");
    for marker in [
        "stage_artifact",
        "commit_artifact",
        "artifact_content_hash_mismatch",
        "artifact_reference_manifest_mismatch",
        "artifact_version_already_staged",
        "artifact_version_not_committed",
    ] {
        assert!(source.contains(marker), "PD-14 marker missing: {marker}");
    }
    assert!(!source.contains("PathBuf"));
    assert!(!source.contains("std::fs"));
    assert!(!source.contains("CapabilityBrokerPort"));
}

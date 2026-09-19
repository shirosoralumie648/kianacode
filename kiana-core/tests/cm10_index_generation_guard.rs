#[test]
fn index_generation_has_one_manifest_and_atomic_publication_boundary() {
    let domain = include_str!("../../kiana-domain/src/index_generation.rs");
    let query = include_str!("../../kiana-query/src/index_generation.rs");
    for marker in [
        "IndexManifest",
        "IndexGenerationState",
        "IndexComponentKind",
        "source_manifest_digest",
        "IndexGenerationStatus::Ready",
        "reader",
        "index_generation_commit_fence_mismatch",
        "last_failure",
    ] {
        assert!(
            domain.contains(marker) || query.contains(marker),
            "CM-10 marker missing: {marker}"
        );
    }
    for marker in ["create_new", "sync_all", "rename", "read_index_manifest"] {
        assert!(
            query.contains(marker),
            "atomic publication marker missing: {marker}"
        );
    }
    assert!(!query.contains("ModelClient"));
    assert!(!query.contains("CapabilityBroker"));
}

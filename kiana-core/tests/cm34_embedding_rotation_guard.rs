#[test]
fn embedding_manifest_and_rotation_are_offline_and_generation_bound() {
    let domain = include_str!("../../kiana-domain/src/embedding_manifest.rs");
    let index = include_str!("../../kiana-domain/src/index_generation.rs");
    let query = include_str!("../../kiana-query/src/index.rs");

    for marker in [
        "EmbeddingManifest",
        "EMBEDDING_MANIFEST_SCHEMA",
        "package_digest",
        "weights_digest",
        "tokenizer_digest",
        "config_digest",
        "dimensions",
        "EmbeddingPooling",
        "EmbeddingProvider",
        "EmbeddingDevice",
        "network_allowed",
        "EmbeddingIndexBinding",
        "embedding_manifest_digest",
        "EmbeddingRotationPlan",
        "old_read_only",
        "reader_allowed",
        "IndexGenerationState",
        "DETERMINISTIC_VECTOR_MODEL",
        "manifest",
    ] {
        assert!(
            domain.contains(marker) || index.contains(marker) || query.contains(marker),
            "CM-34 source marker missing: {marker}"
        );
    }
    assert!(domain.contains("network_allowed"));
    assert!(domain.contains("new_generation <= self.old_generation"));
    assert!(!domain.contains("reqwest"));
    assert!(!domain.contains("download"));
}

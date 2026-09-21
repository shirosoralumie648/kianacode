#[test]
fn pd19_source_guard_keeps_index_reads_generation_bound() {
    let generation = include_str!("../src/index_generation.rs");
    let index = include_str!("../src/index.rs");
    for marker in [
        "ContextIndexSourceManifest",
        "source_fingerprint",
        "Freshness::Stale",
        "IndexGenerationState",
        "write_context_index_generation_atomic",
        "build_context_index_generation",
    ] {
        assert!(
            generation.contains(marker) || index.contains(marker),
            "missing marker: {marker}"
        );
    }
    assert!(index.contains("build_context_index"));
    assert!(!generation.contains("ProviderClient"));
    assert!(!generation.contains("CapabilityBroker"));
}

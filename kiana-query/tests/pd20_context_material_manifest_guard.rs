#[test]
fn pd20_manifest_guard_keeps_context_material_source_bound() {
    let manifest = include_str!("../src/context_manifest.rs");
    let query = include_str!("../src/index.rs");
    for marker in [
        "ContextMaterialManifest",
        "source_cursor",
        "content_hash",
        "dependency_graph",
        "context_material_root_mismatch",
        "atomic_write_json",
    ] {
        assert!(
            manifest.contains(marker) || query.contains(marker),
            "missing marker: {marker}"
        );
    }
    assert!(!manifest.contains("CapabilityBroker"));
    assert!(!manifest.contains("ProviderClient"));
}

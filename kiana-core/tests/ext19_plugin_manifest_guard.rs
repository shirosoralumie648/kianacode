#[test]
fn plugin_manifest_v2_has_server_namespace_and_bounded_components() {
    let manifest = include_str!("../../kiana-skills/src/manifest.rs");
    for marker in [
        "NORMALIZED_PLUGIN_MANIFEST_V2_SCHEMA",
        "plugin_id",
        "required_dependencies",
        "optional_dependencies",
        "configuration_schema",
        "state_schema",
        "migration_refs",
        "plugin:{}:{}",
    ] {
        assert!(manifest.contains(marker), "missing EXT-19 marker: {marker}");
    }
    assert!(manifest.contains("component identity invalid"));
}

use kiana_skills::{
    parse_plugin_manifest_v2, NormalizedPluginManifestV2, PluginComponentKind,
    NORMALIZED_PLUGIN_MANIFEST_V2_SCHEMA,
};
use serde_json::json;

#[test]
fn plugin_manifest_v2_normalizes_components_dependencies_and_namespace() {
    let raw = json!({
        "schema": NORMALIZED_PLUGIN_MANIFEST_V2_SCHEMA,
        "plugin_id": "review-tools",
        "publisher": "kiana-team",
        "version": "2.0.0",
        "license": "MIT",
        "source": "local-fixture",
        "components": [
            {"id":"review-skill","kind":"skills","entry":"skills/review"},
            {"id":"review-hook","kind":"hooks","entry":"hooks/review.json"}
        ],
        "required_dependencies": {"kiana-core":"1.0.0"},
        "optional_dependencies": {"memory":"1.0.0"},
        "configuration_schema": {"type":"object"},
        "state_schema": {"type":"object"},
        "migration_refs": ["migrations/v1.json"]
    });
    let parsed: NormalizedPluginManifestV2 = parse_plugin_manifest_v2(&raw.to_string()).unwrap();
    assert_eq!(parsed.namespace, "plugin:kiana-team:review-tools");
    assert_eq!(parsed.components[0].kind, PluginComponentKind::Skills);
    assert!(parsed.required_dependencies.contains_key("kiana-core"));
    assert!(parsed.source_digest.starts_with("sha256:"));
}

#[test]
fn plugin_manifest_v2_rejects_duplicate_components_and_escape_refs() {
    let duplicate = json!({
        "schema": NORMALIZED_PLUGIN_MANIFEST_V2_SCHEMA,
        "plugin_id":"review-tools","publisher":"kiana-team","version":"2.0.0","license":"MIT","source":"fixture",
        "components":[{"id":"same","kind":"skills","entry":"skills/a"},{"id":"same","kind":"hooks","entry":"hooks/b"}]
    });
    assert!(parse_plugin_manifest_v2(&duplicate.to_string()).is_err());

    let escaped = json!({
        "schema": NORMALIZED_PLUGIN_MANIFEST_V2_SCHEMA,
        "plugin_id":"review-tools","publisher":"kiana-team","version":"2.0.0","license":"MIT","source":"fixture",
        "components":[{"id":"skill","kind":"skills","entry":"../escape"}]
    });
    assert!(parse_plugin_manifest_v2(&escaped.to_string()).is_err());
}

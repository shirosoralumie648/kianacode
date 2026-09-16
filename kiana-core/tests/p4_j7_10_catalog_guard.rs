#[test]
fn model_capability_catalog_does_not_authorize_tools_or_mutate_active_routes() {
    let domain = include_str!("../../kiana-domain/src/model_catalog.rs");
    let provider = include_str!("../../kiana-provider/src/lib.rs");
    let request = include_str!("../../kiana-provider/src/request.rs");
    for marker in [
        "ModelCatalog",
        "ModelCatalogEntry",
        "ModelCatalogSource",
        "model_catalog_ambiguous",
        "supports_tools",
        "expires_at_unix_ms",
    ] {
        assert!(domain.contains(marker), "catalog marker missing: {marker}");
    }
    assert!(provider.contains("model_catalog"));
    assert!(provider.contains("configuration_revision"));
    assert!(request.contains("connection.capabilities.tools"));
    assert!(!request.contains("discovery_grants_tools"));
}

#[test]
fn workbench_health_is_a_bounded_display_of_the_typed_projection() {
    let render = include_str!("../src/workbench_render.rs");
    let chat = include_str!("../src/workbench_chat.rs");
    let client = include_str!("../../kiana-client/src/lib.rs");
    let protocol = include_str!("../../kiana-protocol/src/ui_contracts.rs");

    for marker in [
        "UiConnectorHealthProjection",
        "render_connector_health",
        "connector_health_ui_too_large",
        "entry.health.binding_id",
        "entry.health.connector_id",
        "entry.health.status",
        "stale",
        "limitations",
        "/connector-health",
        "connector.health",
        "is_connector_health",
        "connector_health_projection_missing",
        "UI_CONNECTOR_HEALTH_SCHEMA",
        "ConnectorHealthFact",
    ] {
        assert!(
            render.contains(marker)
                || chat.contains(marker)
                || client.contains(marker)
                || protocol.contains(marker),
            "INT-09 UI marker missing: {marker}"
        );
    }

    for forbidden in [
        "std::process::Command",
        "tokio::spawn",
        "reqwest::",
        "std::fs::",
        "raw_provider_error",
        "credential_value",
        "secret_value",
    ] {
        assert!(
            !render.contains(forbidden),
            "INT-09 UI renderer crossed a side-effect or secret boundary: {forbidden}"
        );
    }
}

#[test]
fn integration_terms_do_not_create_a_second_authority_path() {
    let roadmap = include_str!("../../docs/roadmap/integrations-connectors.md");
    let core = include_str!("../src/connectors.rs");
    let daemon = include_str!("../../kiana-daemon/src/connectors.rs");
    for marker in [
        "Provider",
        "Connector",
        "MCP",
        "A2A",
        "Notification",
        "ControlPlane",
        "EventLog",
        "tool list",
        "AUTH_REQUIRED",
    ] {
        assert!(
            roadmap.contains(marker) || core.contains(marker) || daemon.contains(marker),
            "missing INT-01 boundary marker: {marker}"
        );
    }
    assert!(roadmap.contains("MCP tool list 不授予账号 scope"));
    assert!(roadmap.contains("webhook 不能直接触发副作用"));
    assert!(roadmap.contains("Notification"));
}

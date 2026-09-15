#[test]
fn integrations_baseline_keeps_provider_connector_mcp_and_a2a_boundaries() {
    let domain = include_str!("../../kiana-domain/src/connectors.rs");
    let core = include_str!("../src/connectors.rs");
    let daemon = include_str!("../../kiana-daemon/src/connectors.rs");
    let mcp = include_str!("../../kiana-daemon/src/mcp_stdio.rs");
    let protocol = include_str!("../../kiana-protocol/src/lib.rs");
    let baseline = include_str!("../../docs/roadmap/integrations-baseline.md");

    assert!(domain.contains("ConnectorDefinition"));
    assert!(domain.contains("AccountBinding"));
    assert!(domain.contains("ProviderReceipt"));
    assert!(domain.contains("local_fixture"));
    assert!(core.contains("authorize_and_execute"));
    assert!(core.contains("binding_snapshot"));
    assert!(daemon.contains("external_effect_performed"));
    assert!(daemon.contains("local_fixture"));
    assert!(daemon.contains("connector_idempotency_payload_mismatch"));
    assert!(mcp.contains("stdio"));
    assert!(protocol.contains("CONNECTOR_INVOKE_OPERATION"));
    assert!(baseline.contains("Provider"));
    assert!(baseline.contains("Connector"));
    assert!(baseline.contains("MCP"));
    assert!(baseline.contains("A2A"));
    assert!(baseline.contains("not_supported"));
    assert!(baseline.contains("INT-01"));
}

#[test]
fn integrations_baseline_rejects_direct_network_and_secret_authority_claims() {
    let baseline = include_str!("../../docs/roadmap/integrations-baseline.md");
    for required in [
        "no direct Broker",
        "raw secret",
        "scope intersection",
        "external_effect_performed=false",
        "Unknown",
        "proof ceiling",
        "INT-00",
    ] {
        assert!(baseline.contains(required), "baseline missing {required}");
    }
}

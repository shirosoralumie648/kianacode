#[test]
fn external_notification_contract_has_no_network_or_direct_effect_path() {
    let core = include_str!("../src/notification_external.rs");
    let domain = include_str!("../../kiana-domain/src/notification_external.rs");
    for marker in [
        "ExternalNotificationPolicy",
        "allowed_origins",
        "expected_nonce",
        "ReadyForExplicitConnector",
        "ReconcileRequired",
        "direct_effect: false",
        "external_notification_disabled",
        "provider_receipt_ref",
    ] {
        assert!(
            core.contains(marker) || domain.contains(marker),
            "NM-19 marker missing: {marker}"
        );
    }
    for forbidden in [
        "reqwest::Client",
        "tokio::net",
        "tokio::spawn",
        "std::net::TcpStream",
        "std::process::Command",
        "send(",
        "connect(",
        "mcp_http",
        "A2aClient",
    ] {
        assert!(
            !core.contains(forbidden),
            "NM-19 core network path widened: {forbidden}"
        );
        assert!(
            !domain.contains(forbidden),
            "NM-19 domain network path widened: {forbidden}"
        );
    }
}

#[test]
fn connector_contracts_are_typed_strict_and_receipt_bound() {
    let domain = include_str!("../../kiana-domain/src/connectors.rs");
    let core = include_str!("../src/connectors.rs");
    let daemon = include_str!("../../kiana-daemon/src/connectors.rs");
    for marker in [
        "ConnectorDefinition",
        "ConnectorOperation",
        "AccountBinding",
        "ConnectorBindingSnapshot",
        "ProviderReceipt",
        "EffectObservation",
        "deny_unknown_fields",
        "registry_version",
        "connector_binding_snapshot_required",
        "connector_reconciliation_binding_mismatch",
    ] {
        assert!(
            domain.contains(marker) || core.contains(marker) || daemon.contains(marker),
            "missing INT-02 contract marker: {marker}"
        );
    }
    assert!(!daemon.contains("ProviderClient::send"));
    assert!(!daemon.contains("authorize_and_execute"));
}

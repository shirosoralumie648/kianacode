#[test]
fn connector_operation_guard_keeps_risk_scope_retry_and_payload_contracts_typed() {
    let domain = include_str!("../../kiana-domain/src/connector_operation.rs");
    let connector = include_str!("../../kiana-domain/src/connectors.rs");
    for marker in [
        "ConnectorOperationContract",
        "ConnectorIdempotencyMode",
        "ConnectorRetryMode",
        "input_schema_digest",
        "required_scopes",
        "connector_operation_write_risk_invalid",
        "connector_operation_contract_digest_mismatch",
    ] {
        assert!(
            domain.contains(marker) || connector.contains(marker),
            "missing marker: {marker}"
        );
    }
}

use kiana_domain::{
    ConnectorEffect, ConnectorIdempotencyMode, ConnectorOperationContract, ConnectorRetryMode,
    RiskLevel,
};
use std::collections::BTreeSet;

fn digest(ch: char) -> String {
    format!("sha256:{}", ch.to_string().repeat(64))
}

fn contract(effect: ConnectorEffect, risk: RiskLevel) -> ConnectorOperationContract {
    let mut value = ConnectorOperationContract {
        schema: kiana_domain::CONNECTOR_OPERATION_CONTRACT_SCHEMA.to_owned(),
        operation_id: "list".to_owned(),
        effect,
        risk,
        input_schema_digest: digest('a'),
        output_schema_digest: digest('b'),
        required_scopes: BTreeSet::from(["read".to_owned()]),
        data_classes: BTreeSet::from(["internal".to_owned()]),
        max_input_bytes: 1024,
        timeout_ms: 5000,
        retry: ConnectorRetryMode::Never,
        idempotency: ConnectorIdempotencyMode::Optional,
        contract_digest: String::new(),
    };
    value.contract_digest = value.digest();
    value
}

#[test]
fn operation_contract_binds_read_risk_and_digest() {
    contract(ConnectorEffect::ReadOnly, RiskLevel::ReadOnly)
        .validate()
        .unwrap();
}

#[test]
fn write_risk_downgrade_and_retry_conflict_fail_closed() {
    assert_eq!(
        contract(ConnectorEffect::Write, RiskLevel::ReadOnly)
            .validate()
            .unwrap_err(),
        "connector_operation_write_risk_invalid"
    );
    let mut conflict = contract(ConnectorEffect::Write, RiskLevel::ExternalSideEffect);
    conflict.idempotency = ConnectorIdempotencyMode::Required;
    conflict.retry = ConnectorRetryMode::DeclaredIdempotent;
    conflict.contract_digest = conflict.digest();
    assert_eq!(
        conflict.validate().unwrap_err(),
        "connector_operation_retry_idempotency_conflict"
    );
}

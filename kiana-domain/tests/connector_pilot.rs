use kiana_domain::{
    ConnectorPilotEvidenceSource, ConnectorPilotGate, ConnectorPilotMode, ConnectorPilotStatus,
    CONNECTOR_PILOT_SCHEMA,
};
use serde_json::json;

const D1: &str = "sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
const D2: &str = "sha256:bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";

fn digest(gate: &ConnectorPilotGate) -> String {
    kiana_domain::json_digest(&json!({
        "schema": gate.schema,
        "connector_id": gate.connector_id,
        "binding_id": gate.binding_id,
        "isolated_account_id": gate.isolated_account_id,
        "operation": gate.operation,
        "endpoint_digest": gate.endpoint_digest,
        "credential_revision_digest": gate.credential_revision_digest,
        "scope_digest": gate.scope_digest,
        "data_epoch": gate.data_epoch,
        "revocation_epoch": gate.revocation_epoch,
        "mode": gate.mode,
        "source": gate.source,
        "operator_approval_ref": gate.operator_approval_ref,
        "provider_receipt_digest": gate.provider_receipt_digest,
        "cleanup_plan_digest": gate.cleanup_plan_digest,
        "limitations": gate.limitations,
        "status": gate.status,
    }))
}

fn gate(status: ConnectorPilotStatus) -> ConnectorPilotGate {
    let mut gate = ConnectorPilotGate {
        schema: CONNECTOR_PILOT_SCHEMA.to_owned(),
        connector_id: "connector-1".to_owned(),
        binding_id: "binding-1".to_owned(),
        isolated_account_id: "account-isolated".to_owned(),
        operation: "health".to_owned(),
        endpoint_digest: D1.to_owned(),
        credential_revision_digest: D2.to_owned(),
        scope_digest: D1.to_owned(),
        data_epoch: 2,
        revocation_epoch: 3,
        mode: if status == ConnectorPilotStatus::ReadyForOperator {
            ConnectorPilotMode::OptedIn
        } else {
            ConnectorPilotMode::DefaultOff
        },
        source: if status == ConnectorPilotStatus::ReadyForOperator {
            ConnectorPilotEvidenceSource::LiveNetwork
        } else {
            ConnectorPilotEvidenceSource::Fake
        },
        operator_approval_ref: (status == ConnectorPilotStatus::ReadyForOperator)
            .then(|| "approval:operator-1".to_owned()),
        provider_receipt_digest: (status == ConnectorPilotStatus::ReadyForOperator)
            .then(|| D2.to_owned()),
        cleanup_plan_digest: (status == ConnectorPilotStatus::ReadyForOperator)
            .then(|| D1.to_owned()),
        limitations: vec!["isolated read-only pilot".to_owned()],
        status,
        gate_digest: String::new(),
    };
    gate.gate_digest = digest(&gate);
    gate
}

#[test]
fn default_off_and_explicit_opt_in_gates_are_distinct() {
    assert!(gate(ConnectorPilotStatus::Blocked).validate().is_ok());
    assert!(gate(ConnectorPilotStatus::ReadyForOperator)
        .validate()
        .is_ok());
}

#[test]
fn fake_or_missing_evidence_cannot_be_ready_and_unknown_fields_fail() {
    let mut fake = gate(ConnectorPilotStatus::ReadyForOperator);
    fake.source = ConnectorPilotEvidenceSource::Fake;
    fake.gate_digest = digest(&fake);
    assert_eq!(fake.validate(), Err("connector_pilot_live_opt_in_required"));

    let mut missing = gate(ConnectorPilotStatus::ReadyForOperator);
    missing.cleanup_plan_digest = None;
    missing.gate_digest = digest(&missing);
    assert_eq!(missing.validate(), Err("connector_pilot_cleanup_required"));

    let mut value = serde_json::to_value(gate(ConnectorPilotStatus::Blocked)).unwrap();
    value["unexpected"] = json!(true);
    assert!(serde_json::from_value::<ConnectorPilotGate>(value).is_err());
}

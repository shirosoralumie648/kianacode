use kiana_domain::{
    ConnectorWritePilotGate, ConnectorWritePilotStatus, CONNECTOR_WRITE_PILOT_SCHEMA,
};
use serde_json::json;

const D1: &str = "sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
const D2: &str = "sha256:bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";

fn digest(gate: &ConnectorWritePilotGate) -> String {
    kiana_domain::json_digest(&json!({
        "schema": gate.schema,
        "connector_id": gate.connector_id,
        "binding_id": gate.binding_id,
        "isolated_account_id": gate.isolated_account_id,
        "operation": gate.operation,
        "approval_ref": gate.approval_ref,
        "permit_digest": gate.permit_digest,
        "idempotency_policy_digest": gate.idempotency_policy_digest,
        "final_payload_digest": gate.final_payload_digest,
        "provider_receipt_digest": gate.provider_receipt_digest,
        "cancellation_fence_digest": gate.cancellation_fence_digest,
        "compensation_plan_digest": gate.compensation_plan_digest,
        "reconciliation_case_digest": gate.reconciliation_case_digest,
        "authority_epoch": gate.authority_epoch,
        "data_epoch": gate.data_epoch,
        "request_count": gate.request_count,
        "limitations": gate.limitations,
        "status": gate.status,
    }))
}

fn gate(status: ConnectorWritePilotStatus) -> ConnectorWritePilotGate {
    let ready = status == ConnectorWritePilotStatus::ReadyForExplicitRun;
    let mut gate = ConnectorWritePilotGate {
        schema: CONNECTOR_WRITE_PILOT_SCHEMA.to_owned(),
        connector_id: "connector-1".to_owned(),
        binding_id: "binding-1".to_owned(),
        isolated_account_id: "account-isolated".to_owned(),
        operation: "create".to_owned(),
        approval_ref: ready.then(|| "approval:operator-1".to_owned()),
        permit_digest: ready.then(|| D1.to_owned()),
        idempotency_policy_digest: D1.to_owned(),
        final_payload_digest: D2.to_owned(),
        provider_receipt_digest: ready.then(|| D2.to_owned()),
        cancellation_fence_digest: ready.then(|| D1.to_owned()),
        compensation_plan_digest: ready.then(|| D2.to_owned()),
        reconciliation_case_digest: ready.then(|| D1.to_owned()),
        authority_epoch: 2,
        data_epoch: 3,
        request_count: 0,
        limitations: vec!["one explicit operation".to_owned()],
        status,
        gate_digest: String::new(),
    };
    gate.gate_digest = digest(&gate);
    gate
}

#[test]
fn blocked_and_ready_write_gates_keep_each_evidence_dimension_separate() {
    assert!(gate(ConnectorWritePilotStatus::Blocked).validate().is_ok());
    assert!(gate(ConnectorWritePilotStatus::ReadyForExplicitRun)
        .validate()
        .is_ok());
}

#[test]
fn missing_approval_or_receipt_or_repeated_request_fails_closed() {
    let mut missing = gate(ConnectorWritePilotStatus::ReadyForExplicitRun);
    missing.approval_ref = None;
    missing.gate_digest = digest(&missing);
    assert_eq!(
        missing.validate(),
        Err("connector_write_pilot_approval_required")
    );

    let mut repeated = gate(ConnectorWritePilotStatus::ReadyForExplicitRun);
    repeated.request_count = 2;
    repeated.gate_digest = digest(&repeated);
    assert_eq!(
        repeated.validate(),
        Err("connector_write_pilot_gate_invalid")
    );

    let mut value = serde_json::to_value(gate(ConnectorWritePilotStatus::Blocked)).unwrap();
    value["unexpected"] = json!(true);
    assert!(serde_json::from_value::<ConnectorWritePilotGate>(value).is_err());
}

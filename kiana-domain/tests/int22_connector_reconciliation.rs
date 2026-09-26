use kiana_domain::{
    json_digest, ConnectorHumanInboxItem, ConnectorReconciliationAction,
    ConnectorReconciliationCase, ConnectorReconciliationReason, ConnectorReconciliationSource,
    ConnectorReconciliationState, EffectObservation, InvocationId, ProviderOutcome,
    ProviderReceipt, PROVIDER_RECEIPT_SCHEMA,
};
use serde_json::json;

fn digest(label: &str) -> String {
    json_digest(&json!({"label": label}))
}

fn receipt(outcome: ProviderOutcome) -> ProviderReceipt {
    ProviderReceipt {
        schema: PROVIDER_RECEIPT_SCHEMA.to_owned(),
        connector_id: "connector-demo".to_owned(),
        binding_id: "binding-demo".to_owned(),
        account_id: "account-demo".to_owned(),
        operation: "create".to_owned(),
        idempotency_key: "idem-1".to_owned(),
        final_payload_sha256: "a".repeat(64),
        provider_receipt_id: "receipt-1".to_owned(),
        outcome,
        source: "local_fixture".to_owned(),
        result: json!({"status": "ok"}),
    }
}

fn observation(receipt: &ProviderReceipt, invocation_id: InvocationId) -> EffectObservation {
    EffectObservation::from_provider_receipt(
        receipt,
        kiana_domain::ExecutionId::new(),
        invocation_id,
        1,
        digest("owner"),
        digest("audience"),
        1_700_000_000_000,
    )
    .expect("observation")
}

#[test]
fn unknown_receipt_becomes_quarantine_and_manual_evidence_is_explicit() {
    let unknown = receipt(ProviderOutcome::Unknown);
    let invocation_id = InvocationId::new();
    let unknown_observation = observation(&unknown, invocation_id);
    let pending = ConnectorReconciliationCase::from_unknown(
        &unknown,
        &unknown_observation,
        ConnectorReconciliationReason::TransportTimeout,
    )
    .expect("pending case");
    assert_eq!(pending.state, ConnectorReconciliationState::Pending);
    assert!(pending.reconciliation_required);
    assert!(!pending.automatic_retry_allowed);
    assert!(pending
        .forbidden_actions
        .contains(&ConnectorReconciliationAction::AutomaticRetry));
    let inbox = pending.human_inbox_item().expect("inbox");
    assert_eq!(inbox.state, ConnectorReconciliationState::Pending);
    assert!(inbox.validate().is_ok());

    let resolved = receipt(ProviderOutcome::Succeeded);
    let resolved_observation = observation(&resolved, invocation_id);
    let attached = pending
        .attach_evidence(
            ConnectorReconciliationSource::ManualEvidence,
            resolved.clone(),
            resolved_observation.clone(),
            vec![digest("manual-file")],
        )
        .expect("evidence attached");
    assert_eq!(
        attached.state,
        ConnectorReconciliationState::EvidenceAttached
    );
    let reconciled = attached.commit_reconciled().expect("reconciled");
    assert_eq!(reconciled.state, ConnectorReconciliationState::Reconciled);
    assert!(!reconciled.automatic_retry_allowed);
    assert_eq!(
        reconciled.original_receipt_digest,
        pending.original_receipt_digest
    );
    assert_eq!(reconciled.evidence.as_ref().unwrap().receipt, resolved);
}

#[test]
fn mismatched_or_unknown_evidence_cannot_close_the_case() {
    let unknown = receipt(ProviderOutcome::Unknown);
    let invocation_id = InvocationId::new();
    let original_observation = observation(&unknown, invocation_id);
    let pending = ConnectorReconciliationCase::from_unknown(
        &unknown,
        &original_observation,
        ConnectorReconciliationReason::CommitUnknown,
    )
    .expect("pending case");

    let mut mismatched = receipt(ProviderOutcome::Succeeded);
    mismatched.binding_id = "other-binding".to_owned();
    let mismatched_observation = observation(&mismatched, invocation_id);
    assert_eq!(
        pending
            .attach_evidence(
                ConnectorReconciliationSource::ProviderQuery,
                mismatched,
                mismatched_observation,
                Vec::new(),
            )
            .unwrap_err(),
        "connector_reconciliation_evidence_binding_mismatch"
    );

    let unknown_observation = observation(&unknown, invocation_id);
    assert_eq!(
        pending
            .attach_evidence(
                ConnectorReconciliationSource::ProviderQuery,
                unknown,
                unknown_observation,
                Vec::new(),
            )
            .unwrap_err(),
        "connector_reconciliation_unknown_evidence_forbidden"
    );
    assert_eq!(
        pending.commit_reconciled().unwrap_err(),
        "connector_reconciliation_evidence_required"
    );
}

#[test]
fn case_and_inbox_unknown_fields_are_rejected() {
    let unknown = receipt(ProviderOutcome::Unknown);
    let case = ConnectorReconciliationCase::from_unknown(
        &unknown,
        &observation(&unknown, InvocationId::new()),
        ConnectorReconciliationReason::ProviderUnknown,
    )
    .expect("case");
    let mut encoded = serde_json::to_value(case).expect("encode");
    encoded["replace_original"] = json!(true);
    assert!(serde_json::from_value::<ConnectorReconciliationCase>(encoded).is_err());

    let inbox = ConnectorHumanInboxItem {
        schema: "kiana.connector-human-inbox-item.v1".to_owned(),
        case_digest: digest("case"),
        state: ConnectorReconciliationState::Pending,
        reason: ConnectorReconciliationReason::ProviderUnknown,
        safe_actions: vec![ConnectorReconciliationAction::HumanReview],
        forbidden_actions: vec![],
        item_digest: digest("item"),
    };
    assert!(inbox.validate().is_err());
}

//! INT-22 deny-first fixtures: Unknown receipts are quarantined, and only explicit, bound
//! evidence closes the case.
//!
//! Ordering inside this file is intentional: every refusal fixture runs before the success
//! fixture at the end.  The fixtures only construct caller-supplied domain values (receipts,
//! observations, cases); no provider, network transport, broker, event store or second execution
//! loop is involved, and the original Unknown receipt is never mutated.

use kiana_domain::{
    json_digest, ConnectorHumanInboxItem, ConnectorReconciliationAction,
    ConnectorReconciliationCase, ConnectorReconciliationEvidence, ConnectorReconciliationReason,
    ConnectorReconciliationSource, ConnectorReconciliationState, ConnectorReconciliationSuccessor,
    EffectObservation, EffectObservationState, ExecutionId, InvocationId, ProviderOutcome,
    ProviderReceipt, CONNECTOR_RECONCILIATION_SCHEMA, PROVIDER_RECEIPT_SCHEMA,
};
use serde_json::json;

fn digest(label: &str) -> String {
    json_digest(&json!({"label": label}))
}

fn payload_hash() -> String {
    "a".repeat(64)
}

fn receipt(outcome: ProviderOutcome) -> ProviderReceipt {
    ProviderReceipt {
        schema: PROVIDER_RECEIPT_SCHEMA.to_owned(),
        connector_id: "connector-demo".to_owned(),
        binding_id: "binding-demo".to_owned(),
        account_id: "account-demo".to_owned(),
        operation: "create".to_owned(),
        idempotency_key: "idem-1".to_owned(),
        final_payload_sha256: payload_hash(),
        provider_receipt_id: "receipt-1".to_owned(),
        outcome,
        source: "local_fixture".to_owned(),
        result: json!({"status": "ok"}),
    }
}

fn observation(
    receipt: &ProviderReceipt,
    invocation_id: InvocationId,
    attempt: u32,
) -> EffectObservation {
    EffectObservation::from_provider_receipt(
        receipt,
        ExecutionId::new(),
        invocation_id,
        attempt,
        digest("owner"),
        digest("audience"),
        1_700_000_000_000,
    )
    .expect("observation")
}

/// A query-backed observation carries the recorded query digest; the manual-evidence fixtures use
/// the plain provider-receipt observation above.
fn query_observation(
    receipt: &ProviderReceipt,
    invocation_id: InvocationId,
    attempt: u32,
) -> EffectObservation {
    EffectObservation::new_with_payload_sha256(
        ExecutionId::new(),
        invocation_id,
        attempt,
        digest("owner"),
        digest("audience"),
        json_digest(&json!({"idempotency_key": receipt.idempotency_key})),
        Some(receipt.provider_receipt_id.clone()),
        Some(format!("{:?}", receipt.outcome).to_ascii_lowercase()),
        1_700_000_000_000,
        Some(digest("provider-query")),
        Vec::new(),
        Some(receipt.final_payload_sha256.clone()),
        match receipt.outcome {
            ProviderOutcome::Succeeded => EffectObservationState::ConfirmedSuccess,
            ProviderOutcome::Failed => EffectObservationState::ConfirmedFailure,
            ProviderOutcome::Unknown => EffectObservationState::Unknown,
        },
    )
    .expect("query observation")
}

fn pending_case() -> (ProviderReceipt, InvocationId, ConnectorReconciliationCase) {
    let unknown = receipt(ProviderOutcome::Unknown);
    let invocation_id = InvocationId::new();
    let case = ConnectorReconciliationCase::from_unknown(
        &unknown,
        &observation(&unknown, invocation_id, 1),
        ConnectorReconciliationReason::TransportTimeout,
    )
    .expect("pending case");
    (unknown, invocation_id, case)
}

#[test]
fn a_timeout_cannot_be_recorded_as_a_failure_or_open_a_quarantine() {
    // A timeout is Unknown.  A failed receipt must never become a quarantine case (that would
    // record the timeout as a plain failure), so `from_unknown` refuses it by construction.
    let failed = receipt(ProviderOutcome::Failed);
    let failed_observation = observation(&failed, InvocationId::new(), 1);
    assert_eq!(
        ConnectorReconciliationCase::from_unknown(
            &failed,
            &failed_observation,
            ConnectorReconciliationReason::TransportTimeout,
        )
        .unwrap_err(),
        "connector_reconciliation_unknown_receipt_required"
    );

    // The same applies to a successfully reconciled outcome: only an Unknown receipt is a
    // quarantine input, and only an Unknown-state observation may describe it.
    let succeeded = receipt(ProviderOutcome::Succeeded);
    assert_eq!(
        ConnectorReconciliationCase::from_unknown(
            &succeeded,
            &observation(&succeeded, InvocationId::new(), 1),
            ConnectorReconciliationReason::CommitUnknown,
        )
        .unwrap_err(),
        "connector_reconciliation_unknown_receipt_required"
    );

    // An observation that already claims a confirmed failure cannot be re-labelled as the
    // quarantine of an Unknown receipt either: the state mismatch is refused with its own code
    // before the receipt/observation cross-check runs.
    let unknown = receipt(ProviderOutcome::Unknown);
    let mislabelled = EffectObservation::new_with_payload_sha256(
        ExecutionId::new(),
        InvocationId::new(),
        1,
        digest("owner"),
        digest("audience"),
        json_digest(&json!({"idempotency_key": unknown.idempotency_key})),
        Some(unknown.provider_receipt_id.clone()),
        Some("failed".to_owned()),
        1_700_000_000_000,
        None,
        Vec::new(),
        Some(unknown.final_payload_sha256.clone()),
        EffectObservationState::ConfirmedFailure,
    )
    .expect("mislabelled observation");
    assert_eq!(
        ConnectorReconciliationCase::from_unknown(
            &unknown,
            &mislabelled,
            ConnectorReconciliationReason::TransportTimeout,
        )
        .unwrap_err(),
        "connector_reconciliation_unknown_observation_required"
    );

    // The quarantine reason stays part of the case identity: the same Unknown receipt observed
    // with another reason yields another case digest.
    let (_, invocation_id, timeout_case) = pending_case();
    let commit_case = ConnectorReconciliationCase::from_unknown(
        &unknown,
        &observation(&unknown, invocation_id, 1),
        ConnectorReconciliationReason::CommitUnknown,
    )
    .expect("commit-unknown case");
    assert_ne!(timeout_case.case_digest, commit_case.case_digest);
    assert_eq!(timeout_case.state, ConnectorReconciliationState::Pending);
    assert_eq!(
        timeout_case.reason,
        ConnectorReconciliationReason::TransportTimeout
    );
}

#[test]
fn a_quarantine_never_authorizes_a_retry_without_evidence() {
    let (unknown, invocation_id, pending) = pending_case();

    // The quarantine is evidence work, never a retry ticket: every state refuses a retry.
    assert_eq!(
        pending.authorize_automatic_retry().unwrap_err(),
        "connector_reconciliation_automatic_retry_forbidden"
    );
    assert_eq!(
        pending.commit_reconciled().unwrap_err(),
        "connector_reconciliation_evidence_required"
    );
    assert!(!pending.automatic_retry_allowed);
    assert!(pending.reconciliation_required);
    assert!(pending
        .forbidden_actions
        .contains(&ConnectorReconciliationAction::AutomaticRetry));
    assert!(pending
        .forbidden_actions
        .contains(&ConnectorReconciliationAction::ReuseIdempotencyKey));
    assert!(pending
        .forbidden_actions
        .contains(&ConnectorReconciliationAction::ReplaceOriginalReceipt));
    assert!(pending
        .safe_actions
        .contains(&ConnectorReconciliationAction::ProviderQuery));
    assert!(pending
        .safe_actions
        .contains(&ConnectorReconciliationAction::SubmitManualEvidence));
    assert!(pending
        .safe_actions
        .contains(&ConnectorReconciliationAction::HumanReview));

    // A manual-evidence attachment without any artifact is just a re-assertion, so it is refused
    // before a case can move out of Pending.
    let resolved = receipt(ProviderOutcome::Succeeded);
    assert_eq!(
        pending
            .attach_evidence(
                ConnectorReconciliationSource::ManualEvidence,
                resolved.clone(),
                observation(&resolved, invocation_id, 1),
                Vec::new(),
            )
            .unwrap_err(),
        "connector_reconciliation_manual_evidence_required"
    );

    // A provider query must carry the recorded query digest of the observation it produced.
    assert_eq!(
        pending
            .attach_evidence(
                ConnectorReconciliationSource::ProviderQuery,
                resolved.clone(),
                observation(&resolved, invocation_id, 1),
                vec![digest("query-ref")],
            )
            .unwrap_err(),
        "connector_reconciliation_query_evidence_required"
    );

    // Even a fully bound manual attachment cannot re-open a retry: the resulting case still
    // refuses, and closing it does not change the retry verdict.
    let attached = pending
        .attach_evidence(
            ConnectorReconciliationSource::ManualEvidence,
            resolved,
            observation(&receipt(ProviderOutcome::Succeeded), invocation_id, 1),
            vec![digest("manual-file")],
        )
        .expect("evidence attached");
    assert_eq!(
        attached.authorize_automatic_retry().unwrap_err(),
        "connector_reconciliation_automatic_retry_forbidden"
    );
    let reconciled = attached.commit_reconciled().expect("reconciled");
    assert_eq!(
        reconciled.authorize_automatic_retry().unwrap_err(),
        "connector_reconciliation_automatic_retry_forbidden"
    );
    assert_eq!(
        reconciled.commit_reconciled().unwrap_err(),
        "connector_reconciliation_case_already_reconciled"
    );
    assert_eq!(
        reconciled
            .attach_evidence(
                ConnectorReconciliationSource::ManualEvidence,
                receipt(ProviderOutcome::Succeeded),
                observation(&unknown, invocation_id, 1),
                vec![digest("second-file")],
            )
            .unwrap_err(),
        "connector_reconciliation_case_not_pending"
    );
}

#[test]
fn evidence_from_another_binding_account_operation_key_or_attempt_is_refused() {
    let (_, invocation_id, pending) = pending_case();
    let manual = vec![digest("manual-file")];

    let mut binding_mismatch = receipt(ProviderOutcome::Succeeded);
    binding_mismatch.binding_id = "other-binding".to_owned();
    assert_eq!(
        pending
            .attach_evidence(
                ConnectorReconciliationSource::ManualEvidence,
                binding_mismatch.clone(),
                observation(&binding_mismatch, invocation_id, 1),
                manual.clone(),
            )
            .unwrap_err(),
        "connector_reconciliation_evidence_binding_mismatch"
    );

    let mut account_mismatch = receipt(ProviderOutcome::Succeeded);
    account_mismatch.account_id = "other-account".to_owned();
    assert_eq!(
        pending
            .attach_evidence(
                ConnectorReconciliationSource::ManualEvidence,
                account_mismatch.clone(),
                observation(&account_mismatch, invocation_id, 1),
                manual.clone(),
            )
            .unwrap_err(),
        "connector_reconciliation_evidence_binding_mismatch"
    );

    let mut operation_mismatch = receipt(ProviderOutcome::Succeeded);
    operation_mismatch.operation = "delete".to_owned();
    assert_eq!(
        pending
            .attach_evidence(
                ConnectorReconciliationSource::ManualEvidence,
                operation_mismatch.clone(),
                observation(&operation_mismatch, invocation_id, 1),
                manual.clone(),
            )
            .unwrap_err(),
        "connector_reconciliation_evidence_binding_mismatch"
    );

    let mut payload_mismatch = receipt(ProviderOutcome::Succeeded);
    payload_mismatch.final_payload_sha256 = "b".repeat(64);
    assert_eq!(
        pending
            .attach_evidence(
                ConnectorReconciliationSource::ManualEvidence,
                payload_mismatch.clone(),
                observation(&payload_mismatch, invocation_id, 1),
                manual.clone(),
            )
            .unwrap_err(),
        "connector_reconciliation_evidence_binding_mismatch"
    );

    let mut key_mismatch = receipt(ProviderOutcome::Succeeded);
    key_mismatch.idempotency_key = "idem-other".to_owned();
    assert_eq!(
        pending
            .attach_evidence(
                ConnectorReconciliationSource::ManualEvidence,
                key_mismatch.clone(),
                observation(&key_mismatch, invocation_id, 1),
                manual.clone(),
            )
            .unwrap_err(),
        "connector_reconciliation_evidence_binding_mismatch"
    );

    // A later attempt is a different attempt: its evidence cannot close attempt 1.
    let resolved = receipt(ProviderOutcome::Succeeded);
    assert_eq!(
        pending
            .attach_evidence(
                ConnectorReconciliationSource::ManualEvidence,
                resolved.clone(),
                observation(&resolved, invocation_id, 2),
                manual.clone(),
            )
            .unwrap_err(),
        "connector_reconciliation_evidence_binding_mismatch"
    );

    // A foreign invocation is refused as well.
    assert_eq!(
        pending
            .attach_evidence(
                ConnectorReconciliationSource::ManualEvidence,
                resolved.clone(),
                observation(&resolved, InvocationId::new(), 1),
                manual.clone(),
            )
            .unwrap_err(),
        "connector_reconciliation_evidence_binding_mismatch"
    );

    // Evidence that is still Unknown proves nothing and is refused before binding checks.
    let unknown = receipt(ProviderOutcome::Unknown);
    assert_eq!(
        pending
            .attach_evidence(
                ConnectorReconciliationSource::ManualEvidence,
                unknown.clone(),
                observation(&unknown, invocation_id, 1),
                manual.clone(),
            )
            .unwrap_err(),
        "connector_reconciliation_unknown_evidence_forbidden"
    );

    // Owner/audience drift is not the same attempt scope either.
    let foreign_scope = EffectObservation::from_provider_receipt(
        &resolved,
        ExecutionId::new(),
        invocation_id,
        1,
        digest("other-owner"),
        digest("audience"),
        1_700_000_000_000,
    )
    .expect("foreign scope observation");
    assert_eq!(
        pending
            .attach_evidence(
                ConnectorReconciliationSource::ManualEvidence,
                resolved,
                foreign_scope,
                manual,
            )
            .unwrap_err(),
        "connector_reconciliation_evidence_binding_mismatch"
    );
}

#[test]
fn reconciliation_appends_a_successor_and_never_rewrites_the_original_receipt() {
    let (unknown, invocation_id, pending) = pending_case();
    let original_digest = pending.original_receipt_digest.clone();
    assert!(original_digest.starts_with("sha256:"));

    // No successor exists before the case is closed.
    assert_eq!(
        pending.successor().unwrap_err(),
        "connector_reconciliation_successor_requires_reconciled_case"
    );

    let failed = receipt(ProviderOutcome::Failed);
    let attached = pending
        .attach_evidence(
            ConnectorReconciliationSource::ProviderQuery,
            failed.clone(),
            query_observation(&failed, invocation_id, 1),
            Vec::new(),
        )
        .expect("query evidence attached");
    assert_eq!(
        attached.successor().unwrap_err(),
        "connector_reconciliation_successor_requires_reconciled_case"
    );
    assert_eq!(
        attached.observed_outcome,
        Some(ProviderOutcome::Failed),
        "a provider-confirmed failure closes the case deterministically"
    );
    let reconciled = attached.commit_reconciled().expect("reconciled");

    // The successor is append-only: it references the original invocation/receipt and states
    // that the original fact is never replaced.
    let successor = reconciled.successor().expect("successor");
    assert!(successor.validate().is_ok());
    assert!(successor.appends_new_fact);
    assert!(!successor.replaces_original_fact);
    assert_eq!(successor.observed_outcome, ProviderOutcome::Failed);
    assert_eq!(successor.original_invocation_id, invocation_id);
    assert_eq!(successor.original_receipt_digest, original_digest);
    assert_ne!(
        successor.original_receipt_digest,
        successor.reconciled_receipt_digest
    );
    assert_eq!(
        successor.reconciled_observation_digest,
        reconciled
            .evidence
            .as_ref()
            .unwrap()
            .observation
            .observation_digest
    );
    successor
        .validate_for_case(&reconciled)
        .expect("successor matches its case");

    // The quarantine case keeps the original receipt digest across every transition, so a
    // reconciliation can never rewrite the original Unknown fact.
    assert_eq!(pending.original_receipt_digest, original_digest);
    assert_eq!(attached.original_receipt_digest, original_digest);
    assert_eq!(reconciled.original_receipt_digest, original_digest);

    // A successor that claims to replace the original fact, or that belongs to another case, is
    // refused even with a recomputed digest.
    let mut replacing = successor.clone();
    replacing.replaces_original_fact = true;
    replacing.successor_digest = replacing.digest();
    assert_eq!(
        replacing.validate().unwrap_err(),
        "connector_reconciliation_successor_invalid"
    );
    let mut foreign_case = successor.clone();
    foreign_case.original_receipt_digest = json_digest(&json!({"label": "other-receipt"}));
    foreign_case.successor_digest = foreign_case.digest();
    assert_eq!(
        foreign_case.validate_for_case(&reconciled).unwrap_err(),
        "connector_reconciliation_successor_mismatch"
    );
    let mut tweaked_outcome = successor.clone();
    tweaked_outcome.observed_outcome = ProviderOutcome::Succeeded;
    tweaked_outcome.successor_digest = tweaked_outcome.digest();
    assert_eq!(
        tweaked_outcome.validate_for_case(&reconciled).unwrap_err(),
        "connector_reconciliation_successor_mismatch"
    );
    assert!(successor.validate_for_case(&pending).is_err());

    // Strict decoding and self-proof: unknown fields and hand-computed digests are refused.
    let mut encoded = serde_json::to_value(&reconciled).expect("encode case");
    encoded["replace_original"] = json!(true);
    assert!(serde_json::from_value::<ConnectorReconciliationCase>(encoded).is_err());
    let mut encoded = serde_json::to_value(&successor).expect("encode successor");
    encoded["rewrites_original"] = json!(true);
    assert!(serde_json::from_value::<ConnectorReconciliationSuccessor>(encoded).is_err());
    let mut unauthorised = reconciled.clone();
    unauthorised.automatic_retry_allowed = true;
    assert_eq!(
        unauthorised.validate().unwrap_err(),
        "connector_reconciliation_case_invalid"
    );
    assert_eq!(reconciled.invocation_id, successor.original_invocation_id);
}

#[test]
fn explicit_reconcile_projects_the_human_inbox_and_binds_pending_and_query_evidence() {
    let (unknown, invocation_id, pending) = pending_case();

    // The Pending inbox item names the case and offers only safe actions.
    let pending_item = pending.human_inbox_item().expect("pending inbox item");
    assert!(pending_item.validate().is_ok());
    pending_item
        .validate_for_case(&pending)
        .expect("inbox matches its case");
    assert_eq!(pending_item.state, ConnectorReconciliationState::Pending);
    assert_eq!(
        pending_item.reason,
        ConnectorReconciliationReason::TransportTimeout
    );
    assert_eq!(pending_item.invocation_id, invocation_id);
    assert_eq!(pending_item.attempt, 1);
    assert_eq!(pending_item.connector_id, "connector-demo");
    assert_eq!(pending_item.binding_id, "binding-demo");
    assert_eq!(pending_item.account_id, "account-demo");
    assert_eq!(pending_item.operation, "create");
    assert_eq!(pending_item.payload_sha256, payload_hash());
    assert_eq!(pending_item.observed_outcome, None);
    assert_eq!(
        pending_item.forbidden_actions,
        vec![
            ConnectorReconciliationAction::AutomaticRetry,
            ConnectorReconciliationAction::ReuseIdempotencyKey,
            ConnectorReconciliationAction::ReplaceOriginalReceipt,
        ]
    );

    // A provider query with the recorded query digest closes the case; the reconciled inbox item
    // carries the observed outcome and still forbids a retry.
    let succeeded = receipt(ProviderOutcome::Succeeded);
    let attached = pending
        .attach_evidence(
            ConnectorReconciliationSource::ProviderQuery,
            succeeded.clone(),
            query_observation(&succeeded, invocation_id, 1),
            vec![digest("provider-query-ref")],
        )
        .expect("query evidence attached");
    assert_eq!(
        attached.state,
        ConnectorReconciliationState::EvidenceAttached
    );
    assert_eq!(attached.observed_outcome, Some(ProviderOutcome::Succeeded));
    let reconciled = attached.commit_reconciled().expect("reconciled");
    assert_eq!(reconciled.state, ConnectorReconciliationState::Reconciled);
    assert_eq!(
        reconciled.evidence.as_ref().unwrap().source,
        ConnectorReconciliationSource::ProviderQuery
    );
    assert_eq!(
        reconciled.evidence.as_ref().unwrap().receipt,
        succeeded,
        "the reconciled fact is a successor receipt, not a rewritten original"
    );
    assert_eq!(
        reconciled
            .evidence
            .as_ref()
            .unwrap()
            .evidence_ref_digests
            .len(),
        1
    );

    let reconciled_item = reconciled
        .human_inbox_item()
        .expect("reconciled inbox item");
    assert!(reconciled_item.validate().is_ok());
    reconciled_item
        .validate_for_case(&reconciled)
        .expect("inbox matches its case");
    assert_eq!(
        reconciled_item.state,
        ConnectorReconciliationState::Reconciled
    );
    assert_eq!(
        reconciled_item.observed_outcome,
        Some(ProviderOutcome::Succeeded)
    );
    assert!(reconciled_item
        .forbidden_actions
        .contains(&ConnectorReconciliationAction::AutomaticRetry));

    // Manual evidence with a hash-pinned artifact closes the case too, and its source stays
    // visible in the evidence contract.
    let manual = pending
        .attach_evidence(
            ConnectorReconciliationSource::ManualEvidence,
            succeeded.clone(),
            observation(&succeeded, invocation_id, 1),
            vec![digest("manual-file")],
        )
        .expect("manual evidence attached");
    assert_eq!(
        manual.evidence.as_ref().unwrap().source,
        ConnectorReconciliationSource::ManualEvidence
    );
    assert_eq!(
        manual.evidence.as_ref().unwrap().evidence_ref_digests.len(),
        1
    );

    // A hand-built inbox item is refused: unknown fields, a drifted digest and a foreign case are
    // all mismatches.
    let mut encoded = serde_json::to_value(&pending_item).expect("encode item");
    encoded["retry_allowed"] = json!(true);
    assert!(serde_json::from_value::<ConnectorHumanInboxItem>(encoded).is_err());
    let mut drifted = pending_item.clone();
    drifted.attempt = 2;
    drifted.item_digest = drifted.digest();
    assert_eq!(
        drifted.validate_for_case(&pending).unwrap_err(),
        "connector_human_inbox_item_mismatch"
    );
    assert_eq!(
        pending_item.validate_for_case(&reconciled).unwrap_err(),
        "connector_human_inbox_item_mismatch"
    );
    let mut forged = pending_item.clone();
    forged.forbidden_actions = Vec::new();
    assert_eq!(
        forged.validate().unwrap_err(),
        "connector_human_inbox_item_invalid"
    );
    let mut no_identity = pending_item.clone();
    no_identity.case_digest = "not-a-digest".to_owned();
    no_identity.item_digest = no_identity.digest();
    assert_eq!(
        no_identity.validate().unwrap_err(),
        "connector_human_inbox_item_invalid"
    );

    // Evidence serialisation round-trips and keeps the case schema stable.
    let encoded = serde_json::to_value(&reconciled).expect("encode");
    let decoded: ConnectorReconciliationCase = serde_json::from_value(encoded).expect("decode");
    assert_eq!(decoded, reconciled);
    assert_eq!(decoded.schema, CONNECTOR_RECONCILIATION_SCHEMA);
    let encoded = serde_json::to_value(&reconciled_item).expect("encode item");
    let decoded: ConnectorHumanInboxItem = serde_json::from_value(encoded).expect("decode item");
    assert_eq!(decoded, reconciled_item);
    // The evidence contract itself validates on decode and keeps its own digest.
    let encoded = serde_json::to_value(reconciled.evidence.as_ref().unwrap()).expect("encode");
    let decoded: ConnectorReconciliationEvidence =
        serde_json::from_value(encoded).expect("decode evidence");
    assert!(decoded.validate().is_ok());
    assert_eq!(&decoded, reconciled.evidence.as_ref().unwrap());
    // The original Unknown receipt is untouched and the case never claimed it was replaced.
    assert_eq!(unknown.outcome, ProviderOutcome::Unknown);
    assert_eq!(
        reconciled
            .successor()
            .expect("successor")
            .original_receipt_digest,
        pending.original_receipt_digest
    );
}

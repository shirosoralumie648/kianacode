use kiana_domain::*;
use std::collections::BTreeMap;

fn receipt(kind: CompanyCloseKind) -> CompanyClosingReceiptContract {
    let mut value = CompanyClosingReceiptContract {
        schema: COMPANY_CLOSING_RECEIPT_SCHEMA.to_owned(),
        receipt_id: format!("receipt-{kind:?}"),
        project_id: "project-1".to_owned(),
        baseline_version: 4,
        close_kind: kind,
        project_status: match kind {
            CompanyCloseKind::Success | CompanyCloseKind::Waived => ProjectStatus::Closed,
            CompanyCloseKind::Failure => ProjectStatus::Failed,
            CompanyCloseKind::Cancelled => ProjectStatus::Cancelled,
        },
        acceptance_status: match kind {
            CompanyCloseKind::Waived => AcceptanceStatus::Waived,
            _ => AcceptanceStatus::Accepted,
        },
        delivery_status: match kind {
            CompanyCloseKind::Success => Some(DeliveryStatus::Confirmed),
            _ => None,
        },
        reason: "operator close decision".to_owned(),
        packet_attempt_refs: BTreeMap::from([("packet-1".to_owned(), "attempt-1".to_owned())]),
        run_refs: vec!["run-1".to_owned()],
        review_refs: vec!["review-1".to_owned()],
        acceptance_refs: vec!["acceptance-1".to_owned()],
        delivery_refs: if kind == CompanyCloseKind::Success {
            vec!["delivery-1".to_owned()]
        } else {
            vec![]
        },
        incident_refs: if kind == CompanyCloseKind::Failure {
            vec!["incident-1".to_owned()]
        } else {
            vec![]
        },
        evidence_refs: vec!["evidence:close-1".to_owned()],
        residual_obligations: if kind == CompanyCloseKind::Waived {
            vec!["follow-up".to_owned()]
        } else {
            vec![]
        },
        all_runs_stopped: true,
        unresolved_incidents: false,
        authors: vec!["author-1".to_owned()],
        reviewer_refs: vec!["reviewer-1".to_owned()],
        closer_id: "closer-1".to_owned(),
        waiver_ref: (kind == CompanyCloseKind::Waived).then(|| "waiver-1".to_owned()),
        waiver_by: (kind == CompanyCloseKind::Waived).then(|| "sponsor-1".to_owned()),
        closed_at: 100,
        digest: String::new(),
    };
    value.digest = value.canonical_digest();
    value
}

#[test]
fn closing_rejects_missing_delivery_unresolved_effects_and_incomplete_author_set() {
    let mut missing = receipt(CompanyCloseKind::Success);
    missing.delivery_status = None;
    missing.digest = missing.canonical_digest();
    assert_eq!(
        missing.validate().unwrap_err(),
        "closing_success_chain_incomplete"
    );

    let mut unresolved = receipt(CompanyCloseKind::Failure);
    unresolved.unresolved_incidents = true;
    unresolved.digest = unresolved.canonical_digest();
    assert_eq!(
        unresolved.validate().unwrap_err(),
        "closing_receipt_unresolved_incident"
    );

    let mut overlap = receipt(CompanyCloseKind::Failure);
    overlap.closer_id = "author-1".to_owned();
    overlap.digest = overlap.canonical_digest();
    assert_eq!(
        overlap.validate().unwrap_err(),
        "closing_receipt_role_independence_invalid"
    );
}

#[test]
fn each_close_kind_produces_an_honest_queryable_closing_receipt() {
    let mut ledger = CompanyClosingReceiptLedger::default();
    for kind in [
        CompanyCloseKind::Success,
        CompanyCloseKind::Failure,
        CompanyCloseKind::Cancelled,
        CompanyCloseKind::Waived,
    ] {
        let mut value = receipt(kind);
        value.project_id = format!("project-{kind:?}");
        value.receipt_id = format!("receipt-{kind:?}");
        value.digest = value.canonical_digest();
        ledger.record(value.clone()).expect("record");
        ledger.record(value).expect("idempotent replay");
    }
    assert_eq!(ledger.receipts.len(), 4);
    assert!(ledger.for_project("project-Success").is_some());
}

#[test]
fn closing_rejects_unknown_or_unconfirmed_cancel_and_waiver_self_approval() {
    let mut cancelled = receipt(CompanyCloseKind::Cancelled);
    cancelled.all_runs_stopped = false;
    cancelled.digest = cancelled.canonical_digest();
    assert_eq!(
        cancelled.validate().unwrap_err(),
        "closing_cancel_stop_required"
    );

    let mut waived = receipt(CompanyCloseKind::Waived);
    waived.waiver_by = Some(waived.closer_id.clone());
    waived.digest = waived.canonical_digest();
    assert_eq!(
        waived.validate().unwrap_err(),
        "closing_waiver_self_approval_forbidden"
    );

    let mut stale = receipt(CompanyCloseKind::Success);
    stale.baseline_version = 5;
    stale.digest = stale.canonical_digest();
    let mut ledger = CompanyClosingReceiptLedger::default();
    ledger
        .record(receipt(CompanyCloseKind::Success))
        .expect("first");
    assert_eq!(
        ledger.record(stale).unwrap_err(),
        "closing_receipt_duplicate_digest_mismatch"
    );
}

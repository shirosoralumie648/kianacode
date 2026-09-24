use kiana_domain::{
    ApprovalId, AttemptId, CostCorrectionAppendOutcome, CostCorrectionApproval,
    CostCorrectionCommand, CostLedger, CostLedgerEntry, CostLedgerEntryKind, EventId,
    LedgerEntryId, Money, RateCardId, RequestId, RunId,
};

fn digest() -> String {
    "sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa".to_owned()
}

fn target() -> CostLedgerEntry {
    let mut entry = CostLedgerEntry::new(
        LedgerEntryId::new(),
        CostLedgerEntryKind::Consumption,
        RunId::new(),
        Some(AttemptId::new()),
        None,
        "USD",
        10,
        EventId::new(),
        1,
        1,
        digest(),
    )
    .expect("entry")
    .with_rate_card(RateCardId::new(), 7)
    .expect("card")
    .with_estimated_cost(Money::new("USD", 100).expect("money"))
    .expect("estimate");
    entry.amount = Some(Money::new("USD", 100).expect("amount"));
    entry.entry_digest = entry.digest();
    entry.validate().expect("sealed entry");
    entry
}

fn approved_command(entry: &CostLedgerEntry) -> CostCorrectionCommand {
    let draft = CostCorrectionCommand::draft(
        RequestId::new(),
        kiana_domain::CostCorrectionId::new(),
        entry,
        Some(Money::new("USD", -20).expect("delta")),
        None,
        "provider invoice reconciliation",
        vec!["invoice:opaque-ref".to_owned()],
        "operator-a",
        "cost-correction-idempotency-1",
        20,
    )
    .expect("draft");
    let approval = CostCorrectionApproval::new(
        ApprovalId::new(),
        draft.command_id,
        draft.command_digest.clone(),
        entry.entry_digest.clone(),
        "operator-b",
        21,
    )
    .expect("approval");
    draft.with_approval(approval).expect("approved command")
}

#[test]
fn correction_requires_approval_and_exact_target_fence() {
    let entry = target();
    let draft = CostCorrectionCommand::draft(
        RequestId::new(),
        kiana_domain::CostCorrectionId::new(),
        &entry,
        Some(Money::new("USD", 1).expect("delta")),
        None,
        "invoice reconciliation",
        vec!["invoice:opaque-ref".to_owned()],
        "operator-a",
        "cost-correction-idempotency-draft",
        20,
    )
    .expect("draft");
    assert_eq!(
        draft.validate(),
        Err("cost_correction_approval_required".to_owned())
    );

    let mut tampered = approved_command(&entry);
    tampered.target_entry_digest = digest();
    assert!(tampered.validate_approved().is_err());
}

#[test]
fn append_and_replay_preserve_original_and_fold_correction() {
    let entry = target();
    let command = approved_command(&entry);
    let mut ledger = CostLedger::default();
    ledger.append_entry(entry.clone()).expect("append original");
    let committed = ledger
        .append_correction(command.clone())
        .expect("append correction");
    let correction = match committed {
        CostCorrectionAppendOutcome::Committed(value) => value,
        CostCorrectionAppendOutcome::Replayed(_) => panic!("first append replayed"),
    };
    assert_eq!(correction.target_entry_digest, entry.entry_digest);
    assert_eq!(ledger.entry(entry.entry_id), Some(&entry));
    let replayed = ledger.append_correction(command).expect("replay");
    assert!(matches!(replayed, CostCorrectionAppendOutcome::Replayed(_)));
    let view = ledger.view(entry.entry_id).expect("view");
    assert_eq!(view.original.entry_digest, entry.entry_digest);
    assert_eq!(view.estimated_cost.expect("estimate").micros, 80);
    assert_eq!(view.corrections.len(), 1);
}

#[test]
fn correction_digest_does_not_accept_model_text_or_missing_evidence() {
    let entry = target();
    let result = CostCorrectionCommand::draft(
        RequestId::new(),
        kiana_domain::CostCorrectionId::new(),
        &entry,
        Some(Money::new("USD", 1).expect("delta")),
        None,
        "\nraw model transcript",
        Vec::new(),
        "operator-a",
        "cost-correction-idempotency-2",
        20,
    );
    assert!(result.is_err());
}

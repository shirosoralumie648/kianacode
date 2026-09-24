use kiana_core::CostCorrectionAdmission;
use kiana_domain::{
    ApprovalId, AttemptId, CostCorrectionApproval, CostCorrectionCommand, CostLedgerEntry,
    CostLedgerEntryKind, EventId, LedgerEntryId, Money, RateCardId, RequestId, RunId,
};

fn digest() -> String {
    "sha256:bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb".to_owned()
}

fn target() -> CostLedgerEntry {
    CostLedgerEntry::new(
        LedgerEntryId::new(),
        CostLedgerEntryKind::Consumption,
        RunId::new(),
        Some(AttemptId::new()),
        None,
        "USD",
        10,
        EventId::new(),
        4,
        2,
        digest(),
    )
    .expect("target")
    .with_rate_card(RateCardId::new(), 4)
    .expect("rate card")
    .with_estimated_cost(Money::new("USD", 500).expect("cost"))
    .expect("estimate")
}

fn command(target: &CostLedgerEntry) -> CostCorrectionCommand {
    let draft = CostCorrectionCommand::draft(
        RequestId::new(),
        kiana_domain::CostCorrectionId::new(),
        target,
        Some(Money::new("USD", -25).expect("delta")),
        None,
        "invoice receipt reconciliation",
        vec!["receipt:opaque-1".to_owned()],
        "requester",
        "bq14-core-command-1",
        20,
    )
    .expect("draft");
    let approval = CostCorrectionApproval::new(
        ApprovalId::new(),
        draft.command_id,
        draft.command_digest.clone(),
        target.entry_digest.clone(),
        "approver",
        21,
    )
    .expect("approval");
    draft.with_approval(approval).expect("command")
}

#[test]
fn admission_requires_target_digest_and_approval() {
    let target = target();
    let command = command(&target);
    let correction = CostCorrectionAdmission::authorize(&target, &command).expect("admitted");
    assert_eq!(correction.target_entry_digest, target.entry_digest);
    assert_eq!(correction.target_source_cursor, target.source_cursor);
    assert_eq!(correction.target_revision, target.revision);

    let mut stale = command;
    stale.target_revision += 1;
    assert!(CostCorrectionAdmission::authorize(&target, &stale).is_err());
}

#[test]
fn event_builder_is_append_only_and_stream_bound() {
    let target = target();
    let original = CostCorrectionAdmission::ledger_entry_event(RequestId::new(), 1, &target)
        .expect("entry event");
    assert_eq!(original.kind, kiana_domain::COST_LEDGER_ENTRY_EVENT);
    assert_eq!(original.aggregate_type.as_deref(), Some("cost_ledger"));
    assert_eq!(original.stream_version, Some(target.revision));

    let correction =
        CostCorrectionAdmission::authorize(&target, &command(&target)).expect("correction");
    let corrected = CostCorrectionAdmission::correction_event(
        RequestId::new(),
        2,
        &correction,
        target.revision + 1,
    )
    .expect("correction event");
    assert_eq!(corrected.kind, kiana_domain::COST_CORRECTION_EVENT);
    assert_eq!(
        corrected.aggregate_id.as_deref(),
        Some(target.entry_id.to_string().as_str())
    );
}

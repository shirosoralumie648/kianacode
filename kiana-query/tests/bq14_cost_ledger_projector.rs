use kiana_domain::{
    ApprovalId, AttemptId, CostCorrection, CostCorrectionApproval, CostCorrectionCommand,
    CostLedgerEntry, CostLedgerEntryKind, EventId, LedgerEntryId, Money, RateCardId, RequestId,
    RunId, RuntimeEvent, COST_CORRECTION_EVENT, COST_LEDGER_ENTRY_EVENT,
};
use kiana_query::{project_cost_ledger, CostLedgerProjectionError};

fn digest() -> String {
    "sha256:cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc".to_owned()
}

fn entry() -> CostLedgerEntry {
    CostLedgerEntry::new(
        LedgerEntryId::new(),
        CostLedgerEntryKind::Consumption,
        RunId::new(),
        Some(AttemptId::new()),
        None,
        "USD",
        10,
        EventId::new(),
        10,
        1,
        digest(),
    )
    .expect("entry")
    .with_rate_card(RateCardId::new(), 3)
    .expect("card")
    .with_estimated_cost(Money::new("USD", 900).expect("cost"))
    .expect("estimate")
}

fn correction(entry: &CostLedgerEntry) -> CostCorrection {
    let draft = CostCorrectionCommand::draft(
        RequestId::new(),
        kiana_domain::CostCorrectionId::new(),
        entry,
        Some(Money::new("USD", -100).expect("delta")),
        None,
        "invoice reconciliation",
        vec!["invoice:opaque".to_owned()],
        "requester",
        "bq14-query-command",
        11,
    )
    .expect("draft");
    let approval = CostCorrectionApproval::new(
        ApprovalId::new(),
        draft.command_id,
        draft.command_digest.clone(),
        entry.entry_digest.clone(),
        "approver",
        12,
    )
    .expect("approval");
    CostCorrection::from_command(&draft.with_approval(approval).expect("approved"), entry)
        .expect("correction")
}

fn events(entry: &CostLedgerEntry, correction: &CostCorrection) -> Vec<RuntimeEvent> {
    let original = RuntimeEvent::new(
        RequestId::new(),
        1,
        COST_LEDGER_ENTRY_EVENT,
        serde_json::to_value(entry).expect("entry json"),
    )
    .expect("event")
    .with_stream_metadata("cost_ledger", entry.entry_id.to_string(), entry.revision);
    let corrected = RuntimeEvent::new(
        RequestId::new(),
        2,
        COST_CORRECTION_EVENT,
        serde_json::to_value(correction).expect("correction json"),
    )
    .expect("event")
    .with_stream_metadata(
        "cost_ledger",
        entry.entry_id.to_string(),
        entry.revision + 1,
    );
    vec![original, corrected]
}

#[test]
fn projector_rebuilds_original_and_append_only_correction() {
    let entry = entry();
    let correction = correction(&entry);
    let projection =
        project_cost_ledger(entry.run_id, &events(&entry, &correction)).expect("projection");
    let view = projection.view_for(entry.entry_id).expect("view");
    assert_eq!(view.original.entry_digest, entry.entry_digest);
    assert_eq!(view.estimated_cost.as_ref().expect("estimate").micros, 800);
    assert_eq!(view.corrections.len(), 1);
    assert_eq!(projection.source_cursor, 2);
}

#[test]
fn projector_rejects_foreign_run_and_duplicate_source_event() {
    let entry = entry();
    let correction = correction(&entry);
    let mut source = events(&entry, &correction);
    let foreign = RunId::new();
    assert!(matches!(
        project_cost_ledger(foreign, &source),
        Err(CostLedgerProjectionError::Invalid(_))
    ));
    source.push(source[0].clone());
    assert!(matches!(
        project_cost_ledger(entry.run_id, &source),
        Err(CostLedgerProjectionError::Invalid(_))
    ));
}

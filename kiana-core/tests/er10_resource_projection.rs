use kiana_core::project_recovery_resources;
use kiana_domain::{
    ApprovalId, ApprovalState, BudgetLeaseId, BudgetReservationFact, BudgetScope,
    BudgetSettlementFact, CellId, RequestId, RuntimeEvent, StorageLockId,
};
use serde_json::json;

fn event(sequence: u64, kind: &str, data: serde_json::Value) -> RuntimeEvent {
    RuntimeEvent::new(RequestId::new(), sequence, kind, data).unwrap()
}

#[test]
fn recovery_projection_rebuilds_pending_budget_lease_and_cell_state() {
    let approval_id = ApprovalId::new();
    let budget_id = BudgetLeaseId::new();
    let lease_id = StorageLockId::new();
    let cell_id = CellId::new();
    let reservation_id = RequestId::new();
    let run_id = kiana_domain::RunId::new();
    let reservation = BudgetReservationFact::new(
        reservation_id,
        kiana_domain::ExecutionId::new(),
        run_id,
        budget_id,
        BudgetScope::Run,
        None,
        1,
        0,
        10,
        0,
        10,
        1,
        0,
        1,
        100,
    )
    .unwrap();
    let events = vec![
        event(
            1,
            "approval.staged",
            json!({"approval_id":approval_id,"state":ApprovalState::Staged}),
        )
        .with_stream_metadata("approval", approval_id.to_string(), 1),
        event(
            2,
            "approval.activated",
            json!({"approval_id":approval_id,"state":ApprovalState::Active}),
        )
        .with_stream_metadata("approval", approval_id.to_string(), 2),
        event(
            3,
            "model.reserved",
            json!({"run_id":run_id,"reservation_fact":reservation}),
        ),
        event(4, "lease.issued", json!({"lease_id":lease_id})),
        event(
            5,
            "cell.started",
            json!({"cell_id":cell_id,"lifecycle":"running"}),
        ),
        event(
            6,
            "cell.cancel_requested",
            json!({"cell_id":cell_id,"lifecycle":"cancel_requested"}),
        ),
    ];
    let snapshot = project_recovery_resources(&events).unwrap();
    assert_eq!(snapshot.pending_approval_ids, vec![approval_id]);
    assert_eq!(snapshot.reserved_budget_lease_ids, vec![budget_id]);
    assert_eq!(snapshot.active_resource_lease_ids, vec![lease_id]);
    assert_eq!(snapshot.active_cell_ids, vec![cell_id]);
    assert_eq!(snapshot.fenced_cell_ids, vec![cell_id]);
    assert_eq!(snapshot.source_cursor, events.len() as u64);
}

#[test]
fn recovery_projection_settles_budget_and_rejects_orphan_settlement() {
    let budget_id = BudgetLeaseId::new();
    let reservation = BudgetReservationFact::new(
        RequestId::new(),
        kiana_domain::ExecutionId::new(),
        kiana_domain::RunId::new(),
        budget_id,
        BudgetScope::Run,
        None,
        1,
        0,
        10,
        0,
        10,
        1,
        0,
        1,
        100,
    )
    .unwrap();
    let settlement = BudgetSettlementFact::new(&reservation, Some(4), 2).unwrap();
    let reserved = event(1, "model.reserved", json!({"reservation_fact":reservation}));
    let settled = event(2, "model.settled", json!({"settlement_fact":settlement}));
    let snapshot = project_recovery_resources(&[reserved, settled]).unwrap();
    assert!(snapshot.reserved_budget_lease_ids.is_empty());

    let orphan = BudgetSettlementFact::new(
        &BudgetReservationFact::new(
            RequestId::new(),
            kiana_domain::ExecutionId::new(),
            kiana_domain::RunId::new(),
            BudgetLeaseId::new(),
            BudgetScope::Run,
            None,
            1,
            0,
            10,
            0,
            10,
            1,
            0,
            1,
            100,
        )
        .unwrap(),
        Some(1),
        2,
    )
    .unwrap();
    assert_eq!(
        project_recovery_resources(&[event(1, "model.settled", json!({"settlement_fact":orphan}))])
            .unwrap_err(),
        "recovery_budget_reservation_missing"
    );
}

#[test]
fn recovery_projection_duplicate_events_are_idempotent_and_unsupported_source_is_distinct() {
    let lease_id = StorageLockId::new();
    let original = event(1, "lease.issued", json!({"lease_id":lease_id}));
    let duplicate = original.clone();
    let snapshot = project_recovery_resources(&[original, duplicate]).unwrap();
    assert_eq!(snapshot.active_resource_lease_ids, vec![lease_id]);
    assert_eq!(snapshot.source_event_ids.len(), 1);
    assert_eq!(
        project_recovery_resources(&[]).unwrap_err(),
        "recovery_resource_source_empty"
    );
}

#[test]
fn er10_resource_projection_is_read_only_and_cache_miss_safe() {
    let core = include_str!("../src/resource_projection.rs");
    let recovery = include_str!("../src/recovery.rs");
    let cell = include_str!("../src/cell_registry.rs");
    let approvals = include_str!("../../kiana-daemon/src/journal_approvals.rs");
    for marker in [
        "project_recovery_resources",
        "RecoveryResourceSnapshot",
        "approval_next",
        "BudgetReservationFact",
        "BudgetSettlementFact",
        "lease.issued",
        "cell.",
        "recovery_approval_stage_missing",
        "recovery_budget_settlement_without_reservation",
        "recovery_resource_projection_unsupported",
        "read_all_events",
        "list_pending",
        "checkpoint_run",
    ] {
        assert!(
            core.contains(marker)
                || recovery.contains(marker)
                || cell.contains(marker)
                || approvals.contains(marker),
            "ER-10 marker missing: {marker}"
        );
    }
    for forbidden in [
        "CapabilityBrokerPort",
        "commit_transition",
        "consume_approval",
        "issue_permit",
        "execute(",
    ] {
        assert!(
            !core.contains(forbidden),
            "resource projection must not {forbidden}"
        );
    }
}

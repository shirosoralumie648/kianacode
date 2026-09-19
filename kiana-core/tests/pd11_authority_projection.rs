use kiana_core::{project_authority_read_model, AuthorityProjectionError};
use kiana_domain::{
    BudgetLeaseId, CapabilityGrantId, CellId, RequestId, RuntimeEvent, StorageLockId,
};
use serde_json::json;

fn event(sequence: u64, kind: &str, data: serde_json::Value) -> RuntimeEvent {
    RuntimeEvent::new(RequestId::new(), sequence, kind, data).unwrap()
}

#[test]
fn authority_projection_rebuilds_cells_grants_budgets_and_leases() {
    let cell = CellId::new();
    let grant = CapabilityGrantId::new();
    let budget = BudgetLeaseId::new();
    let lease = StorageLockId::new();
    let model = project_authority_read_model(
        &[
            event(
                1,
                "cell.ready",
                json!({"cell_id": cell, "lifecycle": "ready", "authority_epoch": 1}),
            ),
            event(
                2,
                "grant.issued",
                json!({"capability_grant_id": grant, "state": "active", "expires_at_unix_ms": 10_000, "authority_epoch": 1}),
            ),
            event(
                3,
                "model.reserved",
                json!({"budget_lease_id": budget, "reservation_id": "r1", "authority_epoch": 1}),
            ),
            event(
                4,
                "model.settled",
                json!({"budget_lease_id": budget, "reservation_id": "r1", "authority_epoch": 1}),
            ),
            event(
                5,
                "lease.issued",
                json!({"lease_id": lease, "authority_epoch": 1}),
            ),
            event(
                6,
                "lease.fenced",
                json!({"lease_id": lease, "authority_epoch": 1}),
            ),
        ],
        6,
        1_000,
    )
    .unwrap();
    assert_eq!(model.authority_epoch, 1);
    assert_eq!(model.cells[0].cell_id, cell);
    assert_eq!(model.grants[0].grant_id, grant);
    assert_eq!(model.budgets[0].reservations, 1);
    assert_eq!(model.budgets[0].settlements, 1);
    assert!(model.leases[0].fenced);
    model.validate().unwrap();
}

#[test]
fn stale_epoch_unknown_lease_settlement_and_child_parent_are_rejected() {
    let cell = CellId::new();
    let parent = CellId::new();
    let budget = BudgetLeaseId::new();
    let stale = project_authority_read_model(
        &[
            event(
                1,
                "cell.ready",
                json!({"cell_id": cell, "lifecycle": "ready", "authority_epoch": 2}),
            ),
            event(
                2,
                "grant.issued",
                json!({"capability_grant_id": CapabilityGrantId::new(), "state": "active", "expires_at_unix_ms": 10_000, "authority_epoch": 1}),
            ),
        ],
        2,
        1_000,
    );
    assert_eq!(stale.unwrap_err(), AuthorityProjectionError::EpochRollback);

    let unknown_lease = project_authority_read_model(
        &[event(
            1,
            "lease.released",
            json!({"lease_id": StorageLockId::new(), "authority_epoch": 1}),
        )],
        1,
        1_000,
    );
    assert!(matches!(
        unknown_lease.unwrap_err(),
        AuthorityProjectionError::LeaseInvalid(_)
    ));

    let settlement = project_authority_read_model(
        &[event(
            1,
            "model.settled",
            json!({"budget_lease_id": budget, "reservation_id": "missing", "authority_epoch": 1}),
        )],
        1,
        1_000,
    );
    assert_eq!(
        settlement.unwrap_err(),
        AuthorityProjectionError::SettlementWithoutReservation
    );

    let child = project_authority_read_model(
        &[event(
            1,
            "cell.ready",
            json!({"cell_id": cell, "parent_cell_id": parent, "lifecycle": "ready", "authority_epoch": 1}),
        )],
        1,
        1_000,
    );
    assert_eq!(
        child.unwrap_err(),
        AuthorityProjectionError::ChildParentMissing
    );
}

#[test]
fn active_expired_grant_is_not_projected_as_authority() {
    let result = project_authority_read_model(
        &[event(
            1,
            "grant.issued",
            json!({"capability_grant_id": CapabilityGrantId::new(), "state": "active", "expires_at_unix_ms": 10, "authority_epoch": 1}),
        )],
        1,
        100,
    );
    assert!(matches!(
        result.unwrap_err(),
        AuthorityProjectionError::GrantInvalid(_)
    ));
}

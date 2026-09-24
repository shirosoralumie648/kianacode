use kiana_domain::*;
use kiana_query::{project_cost_allocations, CostAllocationProjectionError};

fn digest(seed: char) -> String {
    format!("sha256:{}", seed.to_string().repeat(64))
}

fn scope() -> AllocationScope {
    AllocationScope::new(
        OrganizationId::new(),
        ProjectId::new(),
        WorkflowInstanceId::new(),
        CellId::new(),
        RunId::new(),
    )
    .expect("scope")
}

fn allocation(
    scope: AllocationScope,
    usage_id: UsageId,
    amount: i128,
    seed: char,
) -> CostAllocation {
    CostAllocation::new(
        CostAllocationId::new(),
        usage_id,
        scope.project_id,
        scope,
        None,
        AllocationCostKind::Estimated {
            amount: Money::new("USD", amount).expect("money"),
            rate_card_id: RateCardId::new(),
            rate_card_version: 1,
        },
        EventId::new(),
        1,
        1,
        digest(seed),
    )
    .expect("allocation")
}

fn event(sequence: u64, allocation: &CostAllocation) -> RuntimeEvent {
    RuntimeEvent::new(
        RequestId::new(),
        sequence,
        COST_ALLOCATION_EVENT,
        serde_json::to_value(allocation).expect("allocation json"),
    )
    .expect("event")
    .with_stream_metadata(
        "cost_allocation",
        allocation.allocation_id.to_string(),
        allocation.revision,
    )
}

#[test]
fn projector_folds_each_leaf_once_across_all_dimensions() {
    let scope = scope();
    let first = allocation(scope, UsageId::new(), 10, 'a');
    let second = allocation(scope, UsageId::new(), 7, 'b');
    let projection = project_cost_allocations(scope.run_id, &[event(1, &first), event(2, &second)])
        .expect("projection");
    assert_eq!(projection.allocations.len(), 2);
    assert_eq!(
        projection
            .ledger_total()
            .estimated
            .as_ref()
            .expect("run")
            .micros,
        17
    );
    assert_eq!(
        projection
            .project_total(scope.project_id)
            .expect("project")
            .estimated
            .as_ref()
            .expect("project estimate")
            .micros,
        17
    );
    assert_eq!(
        projection
            .organization_total(scope.organization_id)
            .expect("org")
            .estimated
            .as_ref()
            .expect("org estimate")
            .micros,
        17
    );
    assert_eq!(
        projection
            .workflow_total(scope.workflow_id)
            .expect("workflow")
            .estimated
            .as_ref()
            .expect("workflow estimate")
            .micros,
        17
    );
    assert_eq!(
        projection
            .cell_total(scope.cell_id)
            .expect("cell")
            .estimated
            .as_ref()
            .expect("cell estimate")
            .micros,
        17
    );
    assert_eq!(projection.ledger_total().leaf_count, 2);
}

#[test]
fn projector_rejects_repeated_usage_and_cross_run_event() {
    let scope = scope();
    let usage = UsageId::new();
    let first = allocation(scope, usage, 10, 'a');
    let duplicate = allocation(scope, usage, 10, 'b');
    assert!(matches!(
        project_cost_allocations(scope.run_id, &[event(1, &first), event(2, &duplicate)]),
        Err(CostAllocationProjectionError::Invalid(reason))
            if reason == "cost_allocation_leaf_usage_repeated"
    ));
    let foreign_scope = scope();
    let foreign = allocation(foreign_scope, UsageId::new(), 3, 'c');
    assert!(matches!(
        project_cost_allocations(scope.run_id, &[event(1, &foreign)]),
        Err(CostAllocationProjectionError::Invalid(reason))
            if reason == "cost_allocation_identity_or_revision_mismatch"
    ));
}

#[test]
fn estimated_measured_unknown_stay_separate() {
    let scope = scope();
    let estimated = allocation(scope, UsageId::new(), 10, 'a');
    let measured = CostAllocation::new(
        CostAllocationId::new(),
        UsageId::new(),
        scope.project_id,
        scope,
        None,
        AllocationCostKind::Measured {
            amount: Money::new("USD", 4).expect("money"),
            provider_receipt: ProviderReceiptRef::new("provider:receipt:opaque").expect("receipt"),
        },
        EventId::new(),
        1,
        1,
        digest('b'),
    )
    .expect("measured");
    let unknown = CostAllocation::new(
        CostAllocationId::new(),
        UsageId::new(),
        scope.project_id,
        scope,
        None,
        AllocationCostKind::Unknown {
            reason: BillingUnknownReason::ResultUnknown,
        },
        EventId::new(),
        1,
        1,
        digest('c'),
    )
    .expect("unknown");
    let projection = project_cost_allocations(
        scope.run_id,
        &[
            event(1, &estimated),
            event(2, &measured),
            event(3, &unknown),
        ],
    )
    .expect("projection");
    assert_eq!(
        projection
            .ledger_total()
            .estimated
            .as_ref()
            .expect("estimate")
            .micros,
        10
    );
    assert_eq!(
        projection
            .ledger_total()
            .measured
            .as_ref()
            .expect("measured")
            .micros,
        4
    );
    assert_eq!(projection.ledger_total().unknown_count, 1);
}

#[test]
fn projector_rebuild_is_deterministic_and_read_only() {
    let scope = scope();
    let allocation = allocation(scope, UsageId::new(), 11, 'a');
    let events = vec![event(1, &allocation)];
    let left = project_cost_allocations(scope.run_id, &events).expect("left");
    let right = project_cost_allocations(scope.run_id, &events).expect("right");
    assert_eq!(left.run_totals, right.run_totals);
    assert_eq!(left.source_event_ids, right.source_event_ids);
    assert_eq!(left.allocations, right.allocations);
}

use kiana_domain::*;

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

fn estimated_allocation(scope: AllocationScope, source_project_id: ProjectId) -> CostAllocation {
    CostAllocation::new(
        CostAllocationId::new(),
        UsageId::new(),
        source_project_id,
        scope,
        None,
        AllocationCostKind::Estimated {
            amount: Money::new("USD", 17).expect("money"),
            rate_card_id: RateCardId::new(),
            rate_card_version: 4,
        },
        EventId::new(),
        7,
        1,
        digest('a'),
    )
    .expect("allocation")
}

#[test]
fn one_leaf_carries_all_dimensions_without_multiple_charge_rows() {
    let scope = scope();
    let allocation = estimated_allocation(scope, scope.project_id);
    assert_eq!(allocation.scope.run_id, scope.run_id);
    assert_eq!(allocation.scope.cell_id, scope.cell_id);
    assert_eq!(allocation.scope.workflow_id, scope.workflow_id);
    assert_eq!(allocation.validate(), Ok(()));
    assert_eq!(
        allocation.cost.estimated_amount().expect("estimate").micros,
        17
    );
}

#[test]
fn wire_owned_scope_and_cross_project_without_grant_are_denied() {
    let scope = scope();
    let allocation = estimated_allocation(scope, scope.project_id);
    let mut forged = scope;
    forged.project_id = ProjectId::new();
    assert_eq!(
        allocation.validate_wire_scope(&forged),
        Err("cost_allocation_wire_scope_mismatch".to_owned())
    );

    let target = scope();
    let cross_project = estimated_allocation(target, ProjectId::new());
    assert_eq!(
        cross_project.validate(),
        Err("cost_allocation_cross_project_sharing_grant_required".to_owned())
    );
}

#[test]
fn cross_project_allocation_requires_exact_grant_reference() {
    let target = scope();
    let source_project = ProjectId::new();
    let grant = SharingGrant::new(
        SharingGrantId::new(),
        source_project,
        target.project_id,
        vec!["usage".to_owned()],
        "cost allocation",
        vec![COST_ALLOCATION_OPERATION.to_owned()],
        10_000,
        2,
    )
    .expect("grant");
    let grant_ref = SharingGrantRef::from_grant(&grant).expect("grant ref");
    let allocation = CostAllocation::new(
        CostAllocationId::new(),
        UsageId::new(),
        source_project,
        target,
        Some(grant_ref),
        AllocationCostKind::Measured {
            amount: Money::new("USD", 19).expect("money"),
            provider_receipt: ProviderReceiptRef::new("provider:receipt:opaque").expect("receipt"),
        },
        EventId::new(),
        8,
        1,
        digest('b'),
    )
    .expect("allocation");
    assert!(allocation.validate().is_ok());
    assert!(matches!(
        allocation.cost,
        AllocationCostKind::Measured { .. }
    ));
}

#[test]
fn unknown_cost_never_becomes_estimated_or_measured_zero() {
    let scope = scope();
    let allocation = CostAllocation::new(
        CostAllocationId::new(),
        UsageId::new(),
        scope.project_id,
        scope,
        None,
        AllocationCostKind::Unknown {
            reason: BillingUnknownReason::ResultUnknown,
        },
        EventId::new(),
        9,
        1,
        digest('c'),
    )
    .expect("allocation");
    assert!(allocation.cost.estimated_amount().is_none());
    assert!(allocation.cost.measured_amount().is_none());
    assert_eq!(
        allocation.cost.unknown_reason(),
        Some(BillingUnknownReason::ResultUnknown)
    );
}

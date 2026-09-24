use kiana_core::{CostAllocationAdmission, CostAllocationAdmissionError};
use kiana_domain::*;

fn digest() -> String {
    "sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa".to_owned()
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

fn allocation(scope: AllocationScope) -> CostAllocation {
    CostAllocation::new(
        CostAllocationId::new(),
        UsageId::new(),
        scope.project_id,
        scope,
        None,
        AllocationCostKind::Estimated {
            amount: Money::new("USD", 5).expect("money"),
            rate_card_id: RateCardId::new(),
            rate_card_version: 1,
        },
        EventId::new(),
        1,
        1,
        digest(),
    )
    .expect("allocation")
}

#[test]
fn control_plane_rejects_wire_scope_drift_before_event() {
    let trusted = scope();
    let allocation = allocation(trusted);
    let mut reported = trusted;
    reported.cell_id = CellId::new();
    let error = CostAllocationAdmission::authorize_wire(
        &allocation,
        &reported,
        &trusted,
        trusted.project_id,
        None,
        100,
        1,
    )
    .expect_err("wire ownership must fail");
    assert_eq!(
        error,
        CostAllocationAdmissionError::WireScopeInvalid(
            "cost_allocation_wire_scope_mismatch".to_owned()
        )
    );
}

#[test]
fn control_plane_requires_active_sharing_grant_for_cross_project() {
    let trusted = scope();
    let source = ProjectId::new();
    let grant = SharingGrant::new(
        SharingGrantId::new(),
        source,
        trusted.project_id,
        vec!["usage".to_owned()],
        "allocation",
        vec![COST_ALLOCATION_OPERATION.to_owned()],
        500,
        3,
    )
    .expect("grant");
    let cross = CostAllocation::new(
        CostAllocationId::new(),
        UsageId::new(),
        source,
        trusted,
        Some(SharingGrantRef::from_grant(&grant).expect("grant ref")),
        AllocationCostKind::Unknown {
            reason: BillingUnknownReason::Partial,
        },
        EventId::new(),
        2,
        1,
        digest(),
    )
    .expect("allocation");
    assert!(
        CostAllocationAdmission::authorize(&cross, &trusted, source, Some(&grant), 100, 3,).is_ok()
    );
    assert!(matches!(
        CostAllocationAdmission::authorize(&cross, &trusted, source, None, 100, 3),
        Err(CostAllocationAdmissionError::SharingGrantInvalid(reason))
            if reason == "cross_project_sharing_grant_required"
    ));
}

#[test]
fn allocation_event_binds_stream_and_all_dimensions() {
    let scope = scope();
    let allocation = allocation(scope);
    let event =
        CostAllocationAdmission::allocation_event(RequestId::new(), 4, &allocation).expect("event");
    assert_eq!(event.kind, COST_ALLOCATION_EVENT);
    assert_eq!(event.aggregate_type.as_deref(), Some("cost_allocation"));
    assert_eq!(
        event.aggregate_id.as_deref(),
        Some(allocation.allocation_id.to_string().as_str())
    );
    assert_eq!(event.stream_version, Some(1));
    assert_eq!(event.data["run_id"], serde_json::json!(scope.run_id));
    assert_eq!(
        event.data["project_id"],
        serde_json::json!(scope.project_id)
    );
}

use kiana_domain::{
    BudgetLease, BudgetLeaseId, BudgetReservationFact, BudgetScope, BudgetSettlementFact,
    ExecutionId, RequestId, RunId,
};

fn reservation(
    scope: BudgetScope,
    tokens: u64,
    parent: Option<RequestId>,
) -> Result<BudgetReservationFact, String> {
    BudgetReservationFact::new(
        RequestId::new(),
        ExecutionId::new(),
        RunId::new(),
        BudgetLeaseId::new(),
        scope,
        parent,
        1,
        0,
        tokens,
        0,
        tokens,
        3,
        0,
        100,
        1_000,
    )
}

#[test]
fn budget_lease_accounts_model_and_tool_consumption_together() {
    let mut lease = BudgetLease::new(2, 100, 1_000, 2, 1);
    assert_eq!(lease.model_call_limit(), 2);
    lease.consume_model_call(40).unwrap();
    lease.consume(1, 60, 1).unwrap();
    assert_eq!(lease.model_calls_used, 1);
    assert_eq!(lease.tool_calls_used, 1);
    assert_eq!(lease.tokens_used, 100);
    assert!(lease.consume_model_call(1).is_err());
}

#[test]
fn budget_reservation_and_settlement_facts_bind_scope_and_usage() {
    let parent = reservation(BudgetScope::Parent, 80, None).unwrap();
    let child = reservation(BudgetScope::Child, 40, Some(parent.reservation_id)).unwrap();
    assert!(parent.validate().is_ok());
    assert!(child.validate().is_ok());
    let known = BudgetSettlementFact::new(&child, Some(30), 200).unwrap();
    assert!(known.validate_against(&child).is_ok());
    let unknown = BudgetSettlementFact::new(&child, None, 300).unwrap();
    assert!(!unknown.usage_known);
    assert!(unknown.validate_against(&child).is_ok());
    assert!(BudgetSettlementFact::new(&child, Some(41), 200).is_err());
}

#[test]
fn budget_contract_rejects_child_without_parent_and_zero_model_reservation() {
    assert!(reservation(BudgetScope::Child, 40, None).is_err());
    assert!(BudgetReservationFact::new(
        RequestId::new(),
        ExecutionId::new(),
        RunId::new(),
        BudgetLeaseId::new(),
        BudgetScope::Run,
        None,
        1,
        0,
        0,
        0,
        1,
        1,
        0,
        100,
        1_000,
    )
    .is_err());
}

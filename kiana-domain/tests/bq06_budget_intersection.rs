use kiana_domain::{
    derive_child_lease, intersect_budgets, BudgetLease, ProjectBudget, ProviderBudget, Quota,
    RuntimeBudget,
};
use uuid::Uuid;

fn inputs() -> (
    RuntimeBudget,
    BudgetLease,
    ProjectBudget,
    Quota,
    ProviderBudget,
) {
    let runtime = RuntimeBudget {
        max_model_calls: 10,
        max_tokens: 1_000,
        max_wall_time_ms: 10_000,
    };
    let lease = BudgetLease::new(8, 900, 9_000, 4, 6);
    let project = ProjectBudget {
        project_id: kiana_domain::ProjectId::new(),
        max_runs: 5,
        max_tokens: 800,
    };
    let quota = Quota {
        scope: "provider:test".to_owned(),
        model_calls: 7,
        tokens: 700,
        concurrency: 3,
    };
    let provider = ProviderBudget::new("provider:test", 6, 2);
    (runtime, lease, project, quota, provider)
}

#[test]
fn runtime_project_quota_lease_provider_intersection_is_conservative() {
    let (runtime, lease, project, quota, provider) = inputs();
    let effective = intersect_budgets(&runtime, &lease, &project, &quota, &provider).unwrap();
    assert_eq!(effective.max_model_calls, 6);
    assert_eq!(effective.max_tokens, 700);
    assert_eq!(effective.max_wall_time_ms, 9_000);
    assert_eq!(effective.max_concurrency, 2);
    assert_eq!(effective.max_project_runs, 5);
    effective.validate().unwrap();
}

#[test]
fn runtime_and_project_budgets_are_not_interchangeable() {
    let (mut runtime, lease, mut project, quota, provider) = inputs();
    runtime.max_model_calls = 2;
    runtime.max_tokens = 9_000;
    project.max_runs = 11;
    project.max_tokens = 3_000;

    let effective = intersect_budgets(&runtime, &lease, &project, &quota, &provider).unwrap();

    assert_eq!(effective.max_model_calls, 2);
    assert_eq!(effective.max_tokens, 700);
    assert_eq!(effective.max_project_runs, 11);

    runtime.max_model_calls = 10;
    project.max_tokens = 400;
    let narrowed = intersect_budgets(&runtime, &lease, &project, &quota, &provider).unwrap();
    assert_eq!(narrowed.max_model_calls, 6);
    assert_eq!(narrowed.max_tokens, 400);
    assert_eq!(narrowed.max_project_runs, 11);
}

#[test]
fn zero_effect_lease_remains_valid_for_read_only_work() {
    let (runtime, mut lease, project, quota, provider) = inputs();
    lease.max_effects = 0;
    let effective = intersect_budgets(&runtime, &lease, &project, &quota, &provider)
        .expect("zero-effect lease is a valid read-only budget");
    assert_eq!(effective.max_effects, 0);
    effective.validate().unwrap();
}

#[test]
fn project_budget_with_nil_identity_is_rejected() {
    let (runtime, lease, mut project, quota, provider) = inputs();
    project.project_id = kiana_domain::ProjectId::from_uuid(Uuid::nil());
    assert_eq!(
        intersect_budgets(&runtime, &lease, &project, &quota, &provider).unwrap_err(),
        "project_budget_invalid"
    );
}

#[test]
fn budget_lease_with_nil_identity_is_rejected() {
    let (runtime, mut lease, project, quota, provider) = inputs();
    lease.lease_id = kiana_domain::BudgetLeaseId::from_uuid(Uuid::nil());
    assert_eq!(
        intersect_budgets(&runtime, &lease, &project, &quota, &provider).unwrap_err(),
        "budget_lease_invalid"
    );
}

#[test]
fn child_budget_union_or_upgrade_is_rejected() {
    let parent = BudgetLease::new(8, 900, 9_000, 4, 6);
    let mut wider = BudgetLease::new(9, 900, 9_000, 4, 6);
    assert_eq!(
        derive_child_lease(&parent, &wider).unwrap_err(),
        "budget_child_widening_rejected"
    );
    wider.max_model_calls = 8;
    wider.max_tool_calls = 8;
    wider.max_reserved_budget = 8;
    let child = derive_child_lease(&parent, &wider).unwrap();
    assert_eq!(child.max_tool_calls, 8);
    assert_eq!(child.max_tokens, 900);
}

#[test]
fn child_usage_cannot_reset_parent_cumulative_counters() {
    let mut parent = BudgetLease::new(8, 900, 9_000, 4, 6);
    parent.tool_calls_used = 2;
    parent.model_calls_used = 3;
    parent.tokens_used = 400;
    parent.effects_used = 1;

    let child = BudgetLease::new(8, 900, 9_000, 4, 6);
    assert_eq!(
        derive_child_lease(&parent, &child).unwrap_err(),
        "budget_child_usage_reset_rejected"
    );
}

#[test]
fn child_usage_cannot_claim_more_than_parent_cumulative_counters() {
    let parent = BudgetLease::new(8, 900, 9_000, 4, 6);
    let mut child = parent.clone();
    child.tokens_used = 1;
    assert_eq!(
        derive_child_lease(&parent, &child).unwrap_err(),
        "budget_child_usage_widening_rejected"
    );
}

#[test]
fn child_reserved_budget_cannot_reset_parent_reservation() {
    let mut parent = BudgetLease::new(8, 900, 9_000, 4, 6);
    parent.reserved_budget = 3;
    let mut child = parent.clone();
    child.reserved_budget = 0;
    assert_eq!(
        derive_child_lease(&parent, &child).unwrap_err(),
        "budget_child_reservation_reset_rejected"
    );
}

#[test]
fn child_reserved_budget_cannot_widen_parent_reservation() {
    let mut parent = BudgetLease::new(8, 900, 9_000, 4, 6);
    parent.reserved_budget = 3;
    let mut child = parent.clone();
    child.reserved_budget = 4;
    assert_eq!(
        derive_child_lease(&parent, &child).unwrap_err(),
        "budget_child_reservation_widening_rejected"
    );
}

#[test]
fn child_zero_reservation_limit_cannot_expand_a_parent_explicit_limit() {
    let mut parent = BudgetLease::new(8, 900, 9_000, 4, 6);
    parent.max_reserved_budget = 2;
    let mut child = BudgetLease::new(8, 900, 9_000, 4, 6);
    child.max_reserved_budget = 0;
    assert_eq!(
        derive_child_lease(&parent, &child).unwrap_err(),
        "budget_child_widening_rejected"
    );
}

#[test]
fn empty_intersection_and_invalid_provider_budget_fail_closed() {
    let (runtime, lease, project, mut quota, provider) = inputs();
    quota.concurrency = 0;
    assert_eq!(
        intersect_budgets(&runtime, &lease, &project, &quota, &provider).unwrap_err(),
        "quota_invalid"
    );
    let mut invalid = ProviderBudget::new("provider:test", 1, 1);
    invalid.provider_id.clear();
    assert_eq!(invalid.validate().unwrap_err(), "provider_budget_invalid");
}

#[test]
fn lease_remaining_projection_is_exact_after_partial_usage_and_reservation() {
    let mut lease = BudgetLease::new(8, 900, 9_000, 4, 6);
    // Direct admission boundaries: exactly-the-limit is allowed, one more is not.
    assert!(lease.can_consume(8, 900, 6));
    assert!(!lease.can_consume(9, 900, 6));
    assert!(lease.can_reserve(8));
    assert!(!lease.can_reserve(9));

    lease.consume(2, 100, 1).unwrap();
    lease.consume_model_call(50).unwrap();
    lease.reserve(3).unwrap();
    assert_eq!(lease.remaining_tool_calls(), 6);
    assert_eq!(lease.remaining_model_calls(), 7);
    assert_eq!(lease.remaining_tokens(), 750);
    assert_eq!(lease.remaining_effects(), 5);
    assert_eq!(lease.remaining_reserved_budget(), 5);
    // The projection and the admission gate agree at the exact remainder.
    assert!(lease.can_consume(6, 750, 5));
    assert!(!lease.can_consume(7, 750, 5));

    let before = (
        lease.remaining_tool_calls(),
        lease.remaining_model_calls(),
        lease.remaining_tokens(),
        lease.remaining_effects(),
        lease.remaining_reserved_budget(),
    );
    lease.consume(1, 10, 1).unwrap();
    lease.consume_model_call(10).unwrap();
    lease.reserve(2).unwrap();
    assert_eq!(lease.remaining_tool_calls(), 5);
    assert_eq!(lease.remaining_model_calls(), 6);
    assert_eq!(lease.remaining_tokens(), 730);
    assert_eq!(lease.remaining_effects(), 4);
    assert_eq!(lease.remaining_reserved_budget(), 3);
    let after = (
        lease.remaining_tool_calls(),
        lease.remaining_model_calls(),
        lease.remaining_tokens(),
        lease.remaining_effects(),
        lease.remaining_reserved_budget(),
    );
    assert!(
        after.0 < before.0
            && after.1 < before.1
            && after.2 < before.2
            && after.3 < before.3
            && after.4 < before.4,
        "remaining projections must monotonically decrease with usage/reservation"
    );
    lease.validate().unwrap();
}

#[test]
fn lease_remaining_saturates_at_zero_and_deny_boundaries_hold() {
    let mut lease = BudgetLease::new(2, 10, 1_000, 1, 1);
    lease.consume(2, 10, 1).unwrap();
    lease.reserve(2).unwrap();
    lease.validate().unwrap();

    assert_eq!(lease.remaining_tool_calls(), 0);
    assert_eq!(lease.remaining_tokens(), 0);
    assert_eq!(lease.remaining_effects(), 0);
    assert_eq!(lease.remaining_reserved_budget(), 0);
    assert_eq!(lease.remaining_model_calls(), 2);

    assert!(lease.can_consume(0, 0, 0));
    assert!(!lease.can_consume(1, 0, 0));
    assert!(!lease.can_consume(0, 1, 0));
    assert!(!lease.can_consume(0, 0, 1));
    assert_eq!(lease.consume(1, 0, 0).unwrap_err(), "budget_lease_exceeded");
    assert_eq!(
        lease.consume_model_call(1).unwrap_err(),
        "budget_lease_exceeded"
    );
    assert!(lease.can_reserve(0));
    assert!(!lease.can_reserve(1));
    assert_eq!(lease.reserve(1).unwrap_err(), "spawn_budget_exceeded");
    lease.release(1).unwrap();
    assert_eq!(lease.remaining_reserved_budget(), 1);
    assert!(lease.can_reserve(1));

    // A corrupted over-consumption must saturate to zero, never wrap or widen.
    lease.tokens_used = 11;
    assert!(lease.validate().is_err());
    assert_eq!(lease.remaining_tokens(), 0);
}

#[test]
fn lease_remaining_model_calls_uses_normalized_limit_for_legacy_zero() {
    let mut lease = BudgetLease::new(3, 30, 1_000, 1, 1);
    lease.max_model_calls = 0;
    assert_eq!(lease.model_call_limit(), 3);
    assert_eq!(lease.remaining_model_calls(), 3);
    lease.consume_model_call(10).unwrap();
    assert_eq!(lease.remaining_model_calls(), 2);
    assert_eq!(lease.remaining_tokens(), 20);
}

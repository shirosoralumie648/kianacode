use kiana_domain::{
    derive_child_lease, intersect_budgets, BudgetLease, ProjectBudget, ProviderBudget, Quota,
    RuntimeBudget,
};

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
fn child_budget_union_or_upgrade_is_rejected() {
    let parent = BudgetLease::new(8, 900, 9_000, 4, 6);
    let mut wider = BudgetLease::new(9, 900, 9_000, 4, 6);
    assert_eq!(
        derive_child_lease(&parent, &wider).unwrap_err(),
        "budget_child_widening_rejected"
    );
    wider.max_tool_calls = 8;
    wider.max_reserved_budget = 8;
    let child = derive_child_lease(&parent, &wider).unwrap();
    assert_eq!(child.max_tool_calls, 8);
    assert_eq!(child.max_tokens, 900);
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

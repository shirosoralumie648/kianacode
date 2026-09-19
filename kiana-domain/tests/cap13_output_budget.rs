use kiana_domain::ExecutionOutputBudget;

#[test]
fn output_budget_separates_collection_preview_persistence_and_observation() {
    let budget = ExecutionOutputBudget::default();
    budget.validate().unwrap();
    assert!(budget.preview_max_bytes < budget.collect_max_bytes);
    assert!(budget.collect_max_bytes <= budget.persist_max_bytes);
    assert!(budget.collect_max_bytes <= budget.observed_max_bytes);
}

#[test]
fn output_budget_rejects_unbounded_or_inverted_limits() {
    let mut budget = ExecutionOutputBudget::default();
    budget.persist_max_bytes = budget.collect_max_bytes - 1;
    assert_eq!(
        budget.validate(),
        Err("execution_output_budget_invalid".to_owned())
    );
    let mut budget = ExecutionOutputBudget::default();
    budget.schema = "legacy-output-budget".to_owned();
    assert_eq!(
        budget.validate(),
        Err("execution_output_budget_invalid".to_owned())
    );
}

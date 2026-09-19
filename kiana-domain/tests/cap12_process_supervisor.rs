use kiana_domain::{ProcessGroupState, ProcessResourceBudget, StopMethod, StopReport};

#[test]
fn resource_budget_keeps_hard_and_observed_dimensions_explicit() {
    let budget = ProcessResourceBudget::default();
    budget.validate().unwrap();
    assert_eq!(budget.max_output_bytes, 1024 * 1024);
    assert!(budget.max_address_space_bytes > 0);
    assert!(budget.max_processes > 0);
    assert!(budget.max_open_files > 0);
}

#[test]
fn stop_report_requires_reaped_leader_and_empty_group_for_confirmation() {
    let confirmed = StopReport::new(
        "execution-1",
        Some(42),
        Some(42),
        StopMethod::Kill,
        true,
        true,
        true,
        ProcessGroupState::Empty,
        true,
        25,
    )
    .unwrap();
    confirmed.validate().unwrap();

    let unconfirmed = StopReport::new(
        "execution-1",
        Some(42),
        Some(42),
        StopMethod::Term,
        true,
        false,
        true,
        ProcessGroupState::Present,
        false,
        100,
    )
    .unwrap();
    assert!(!unconfirmed.confirmed);

    let mut forged = confirmed.clone();
    forged.confirmed = false;
    assert!(forged.validate().is_err());
}

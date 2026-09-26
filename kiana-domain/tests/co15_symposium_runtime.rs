use kiana_domain::*;

#[test]
fn symposium_budget_exhaustion_or_cancel_never_emits_an_approved_decision() {
    let mut budget = SymposiumRunBudget::new(2, 2, 100, 10_000, 1).expect("budget");
    budget.record_message(40).expect("message");
    budget.record_round(true).expect("round");
    assert!(budget.decision_allowed().is_ok());
    budget.record_message(60).expect("message");
    assert_eq!(
        budget.decision_allowed().unwrap_err(),
        "symposium_budget_exhausted"
    );

    let mut cancelled = SymposiumRunBudget::new(2, 2, 100, 10_000, 1).expect("budget");
    cancelled.cancel();
    assert_eq!(
        cancelled.decision_allowed().unwrap_err(),
        "symposium_cancelled"
    );
}

#[test]
fn symposium_and_async_proposal_share_the_same_decision_gate() {
    let mut budget = SymposiumRunBudget::new(3, 3, 100, 10_000, 2).expect("budget");
    budget.record_round(true).expect("round");
    budget.record_message(10).expect("message");
    assert!(budget.decision_allowed().is_ok());
    budget.cancel();
    assert_eq!(
        budget.decision_allowed().unwrap_err(),
        "symposium_cancelled"
    );
}

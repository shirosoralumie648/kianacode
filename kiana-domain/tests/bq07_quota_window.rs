use kiana_domain::{ClockObservation, QuotaGroupKey, QuotaWindow, QuotaWindowBudget};

fn clock(now: u64) -> ClockObservation {
    ClockObservation::observe("fixture", now as u128, now as u128, None, 1).unwrap()
}

#[test]
fn fixed_utc_window_and_retry_after_are_deterministic() {
    let window = QuotaWindow::from_clock(&clock(125_000), 60_000).unwrap();
    assert_eq!(window.start_unix_ms, 120_000);
    assert_eq!(window.end_unix_ms, 180_000);
    assert_eq!(window.retry_after_ms(125_000).unwrap(), 55_000);
    assert_eq!(window.until_next_window_ms(180_000).unwrap(), 0);
    assert_eq!(window.timezone, "UTC");
}

#[test]
fn clock_rollback_alias_bypass_and_empty_group_fail_closed() {
    let previous = clock(2_000);
    let rollback = ClockObservation::observe("fixture", 1_000, 1_000, Some(&previous), 2).unwrap();
    assert_eq!(
        QuotaWindow::from_clock(&rollback, 1_000).unwrap_err(),
        "clock_untrusted"
    );
    let first = QuotaGroupKey::new(
        "Provider:Test",
        "Credential:A",
        Some("Model-X".to_owned()),
        Some("Alias-One".to_owned()),
    )
    .unwrap();
    let second = QuotaGroupKey::new(
        "provider:test",
        "credential:a",
        Some("model-x".to_owned()),
        Some("alias-one".to_owned()),
    )
    .unwrap();
    assert_eq!(first, second);
    assert!(QuotaGroupKey::new("", "credential", None, None).is_err());
}

#[test]
fn quota_window_budget_rejects_over_limit_with_retry_after() {
    let group =
        QuotaGroupKey::new("provider", "credential", Some("model".to_owned()), None).unwrap();
    let window = QuotaWindow::from_clock(&clock(125_000), 60_000).unwrap();
    let mut budget = QuotaWindowBudget {
        schema: "kiana.quota-window-budget.v1".to_owned(),
        version: kiana_domain::QUOTA_SCHEMA_VERSION,
        group,
        window,
        max_requests: 2,
        max_tokens: 100,
        max_concurrency: 1,
        used_requests: 1,
        used_tokens: 80,
        active_concurrency: 1,
        budget_digest: String::new(),
    };
    budget.budget_digest = budget.digest();
    budget.validate().unwrap();
    assert!(budget.check(1, 10, 0).is_err());
    budget.used_requests = 0;
    budget.budget_digest = budget.digest();
    budget.validate().unwrap();
    budget.check(1, 10, 0).unwrap();
}

use kiana_domain::{ClockObservation, QuotaGroupKey, QuotaWindow, QuotaWindowBudget};

fn clock(now: u64) -> ClockObservation {
    ClockObservation::observe("fixture", now as u128, now as u128, None, 1).unwrap()
}

fn budget_for(
    group: QuotaGroupKey,
    window: QuotaWindow,
    max_requests: u64,
    used_requests: u64,
) -> QuotaWindowBudget {
    let mut budget = QuotaWindowBudget {
        schema: "kiana.quota-window-budget.v1".to_owned(),
        version: kiana_domain::QUOTA_SCHEMA_VERSION,
        group,
        window,
        max_requests,
        max_tokens: 100,
        max_concurrency: 1,
        used_requests,
        used_tokens: 0,
        active_concurrency: 0,
        budget_digest: String::new(),
    };
    budget.budget_digest = budget.digest();
    budget
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
    let compatibility_denial = budget.check(2, 10, 0).unwrap_err();
    assert_eq!(compatibility_denial, "quota_exhausted:retry_after_ms=60000");
    let denial = budget.check_at(&clock(125_000), 2, 10, 0).unwrap_err();
    assert_eq!(denial, "quota_exhausted:retry_after_ms=55000");
    budget.used_requests = 0;
    budget.budget_digest = budget.digest();
    budget.validate().unwrap();
    budget.check(1, 10, 0).unwrap();
}

#[test]
fn quota_budget_check_at_rejects_untrusted_and_pre_window_clock() {
    let group = QuotaGroupKey::new("provider", "credential", None, None).unwrap();
    let window = QuotaWindow::from_clock(&clock(125_000), 60_000).unwrap();
    let mut budget = QuotaWindowBudget {
        schema: "kiana.quota-window-budget.v1".to_owned(),
        version: kiana_domain::QUOTA_SCHEMA_VERSION,
        group,
        window,
        max_requests: 1,
        max_tokens: 100,
        max_concurrency: 1,
        used_requests: 1,
        used_tokens: 0,
        active_concurrency: 0,
        budget_digest: String::new(),
    };
    budget.budget_digest = budget.digest();

    let previous = clock(125_000);
    let untrusted =
        ClockObservation::observe("fixture", 124_000, 124_000, Some(&previous), 2).unwrap();
    assert_eq!(
        budget.check_at(&untrusted, 1, 0, 0).unwrap_err(),
        "clock_untrusted"
    );

    assert_eq!(
        budget.check_at(&clock(119_999), 1, 0, 0).unwrap_err(),
        "quota_clock_rollback"
    );
}

#[test]
fn quota_window_rolls_on_exact_boundaries_without_partial_second_drift() {
    // 毫秒整数运算不存在亚秒漂移：结束前一毫秒的 retry-after 恰为 1，起始和翻转时刻恰为整窗。
    let before = QuotaWindow::from_clock(&clock(119_999), 60_000).unwrap();
    assert_eq!(
        (before.start_unix_ms, before.end_unix_ms),
        (60_000, 120_000)
    );
    assert_eq!(before.retry_after_ms(119_999).unwrap(), 1);
    assert_eq!(before.until_next_window_ms(119_999).unwrap(), 1);

    let start = QuotaWindow::from_clock(&clock(120_000), 60_000).unwrap();
    assert_eq!((start.start_unix_ms, start.end_unix_ms), (120_000, 180_000));
    assert_eq!(start.retry_after_ms(120_000).unwrap(), 60_000);

    // 窗口结束的精确毫秒同时开启下一个对齐窗口；旧窗口重试延迟精确归零。
    let rolled = QuotaWindow::from_clock(&clock(180_000), 60_000).unwrap();
    assert_eq!(
        (rolled.start_unix_ms, rolled.end_unix_ms),
        (180_000, 240_000)
    );
    assert_eq!(start.until_next_window_ms(180_000).unwrap(), 0);
    assert_eq!(start.retry_after_ms(180_000).unwrap(), 0);
}

#[test]
fn quota_check_at_reports_exact_retry_after_at_window_boundaries() {
    let group = QuotaGroupKey::new("provider", "credential", None, None).unwrap();
    let window = QuotaWindow::from_clock(&clock(120_000), 60_000).unwrap();
    let mut budget = QuotaWindowBudget {
        schema: "kiana.quota-window-budget.v1".to_owned(),
        version: kiana_domain::QUOTA_SCHEMA_VERSION,
        group,
        window,
        max_requests: 1,
        max_tokens: 100,
        max_concurrency: 1,
        used_requests: 1,
        used_tokens: 0,
        active_concurrency: 0,
        budget_digest: String::new(),
    };
    budget.budget_digest = budget.digest();
    budget.validate().unwrap();

    assert_eq!(
        budget.check_at(&clock(120_000), 1, 0, 0).unwrap_err(),
        "quota_exhausted:retry_after_ms=60000"
    );
    assert_eq!(
        budget.check_at(&clock(179_999), 1, 0, 0).unwrap_err(),
        "quota_exhausted:retry_after_ms=1"
    );
    // 到期精确时刻：本窗口已结束，retry-after 精确为 0（下一窗口从该毫秒开始）。
    assert_eq!(
        budget.check_at(&clock(180_000), 1, 0, 0).unwrap_err(),
        "quota_exhausted:retry_after_ms=0"
    );
}

#[test]
fn misaligned_window_and_empty_group_scope_fail_closed() {
    assert_eq!(
        QuotaWindow::from_clock(&clock(125_000), 0).unwrap_err(),
        "quota_window_duration_invalid"
    );
    assert_eq!(
        QuotaWindow::from_clock(&clock(125_000), 604_800_001).unwrap_err(),
        "quota_window_duration_invalid"
    );

    // 窗口错位：端到端时长与声明 duration 不一致时，即使重算摘要也拒绝。
    let mut misaligned = QuotaWindow::from_clock(&clock(125_000), 60_000).unwrap();
    misaligned.end_unix_ms += 1;
    misaligned.window_digest = misaligned.digest();
    assert_eq!(misaligned.validate().unwrap_err(), "quota_window_invalid");

    assert_eq!(
        QuotaGroupKey::new("provider", "", None, None).unwrap_err(),
        "quota_group_invalid"
    );
    assert_eq!(
        QuotaGroupKey::new("provider", "credential", Some(String::new()), None).unwrap_err(),
        "quota_group_invalid"
    );
}

#[test]
fn alias_and_casing_variants_aggregate_into_one_quota_bucket() {
    // 别名只是展示名：同 provider/credential/model 的大小写与 alias 变体必须折叠成同一个
    // quota group 摘要，从而共享同一个额度桶，不能凭一个新别名另开额度来绕过 quota。
    let canonical = QuotaGroupKey::new(
        "Provider:Main",
        "Credential:Primary",
        Some("Model-X".to_owned()),
        Some("Friendly-Alias".to_owned()),
    )
    .unwrap();
    let restyled = QuotaGroupKey::new(
        "provider:main",
        "credential:primary",
        Some("model-x".to_owned()),
        Some("friendly-alias".to_owned()),
    )
    .unwrap();
    assert_eq!(canonical, restyled);
    assert_eq!(canonical.group_digest, restyled.group_digest);

    // 以 group 摘要为聚合键：canonical 用尽的桶对所有变体可见，别名无法多放行一次。
    let window = QuotaWindow::from_clock(&clock(125_000), 60_000).unwrap();
    let exhausted = budget_for(canonical.clone(), window, 1, 1);
    let mut buckets = std::collections::BTreeMap::new();
    buckets.insert(exhausted.group.group_digest.clone(), exhausted);
    let via_variant = buckets
        .get(&restyled.group_digest)
        .expect("alias/casing variant must resolve to the canonical bucket");
    assert_eq!(
        via_variant.check_at(&clock(125_000), 1, 0, 0).unwrap_err(),
        "quota_exhausted:retry_after_ms=55000"
    );

    // 不同 credential 是不同桶：额度不得跨凭据合并，也不能互相顶替。
    let second_credential = QuotaGroupKey::new(
        "provider:main",
        "credential:secondary",
        Some("model-x".to_owned()),
        Some("friendly-alias".to_owned()),
    )
    .unwrap();
    assert_ne!(canonical.group_digest, second_credential.group_digest);
    assert!(!buckets.contains_key(&second_credential.group_digest));
}

#[test]
fn tampered_group_digest_is_rejected_before_aggregation() {
    let mut key =
        QuotaGroupKey::new("provider", "credential", Some("model".to_owned()), None).unwrap();
    let other =
        QuotaGroupKey::new("provider", "credential", Some("other".to_owned()), None).unwrap();
    // 摘要与内容不一致的 group 不得参与聚合：validate 必须先拒绝。
    key.group_digest = other.group_digest;
    assert_eq!(key.validate().unwrap_err(), "quota_group_invalid");
}

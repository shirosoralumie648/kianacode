use kiana_domain::{
    ClockObservation, QuotaGroupKey, QuotaWindow, QuotaWindowBudget, QUOTA_SCHEMA_VERSION,
    QUOTA_WINDOW_BUDGET_SCHEMA,
};
use kiana_ports::{ObservabilityQueue, ObservabilityQueueClass, QueuedObservabilityItem};

#[test]
fn quota_window_rejects_request_token_and_concurrency_overages() {
    let clock = ClockObservation::observe("sc16", 1_000_001, 1, None, 1).unwrap();
    let window = QuotaWindow::from_clock(&clock, 1_000).unwrap();
    let group = QuotaGroupKey::new(
        "provider",
        "credential",
        Some("model".to_owned()),
        Some("default".to_owned()),
    )
    .unwrap();
    let mut budget = QuotaWindowBudget {
        schema: QUOTA_WINDOW_BUDGET_SCHEMA.to_owned(),
        version: QUOTA_SCHEMA_VERSION,
        group,
        window,
        max_requests: 2,
        max_tokens: 100,
        max_concurrency: 1,
        used_requests: 1,
        used_tokens: 40,
        active_concurrency: 1,
        budget_digest: String::new(),
    };
    budget.budget_digest = budget.digest();
    budget.validate().unwrap();
    budget.check(1, 60, 0).unwrap();
    assert!(budget.check(2, 1, 0).is_err());
    assert!(budget.check(1, 61, 0).is_err());
    assert!(budget.check(1, 1, 1).is_err());
}

#[test]
fn critical_observability_facts_survive_best_effort_backpressure() {
    let queue = ObservabilityQueue::new(2).unwrap();
    queue
        .try_enqueue(QueuedObservabilityItem::critical(
            ObservabilityQueueClass::Log,
            1,
        ))
        .unwrap();
    queue
        .try_enqueue(QueuedObservabilityItem::critical(
            ObservabilityQueueClass::Trace,
            2,
        ))
        .unwrap();
    queue
        .try_enqueue(QueuedObservabilityItem::critical(
            ObservabilityQueueClass::Event,
            3,
        ))
        .unwrap();
    queue
        .try_enqueue(QueuedObservabilityItem::critical(
            ObservabilityQueueClass::Audit,
            4,
        ))
        .unwrap();
    let rejected = queue.try_enqueue(QueuedObservabilityItem::critical(
        ObservabilityQueueClass::Terminal,
        5,
    ));
    assert!(rejected.is_err());
    let stats = queue.stats();
    assert_eq!(stats.depth, 2);
    assert_eq!(stats.dropped_best_effort_total, 2);
    assert_eq!(stats.critical_rejected_total, 1);
}

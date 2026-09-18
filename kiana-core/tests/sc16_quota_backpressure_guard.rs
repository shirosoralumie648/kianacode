#[test]
fn sc16_quota_reservation_and_backpressure_boundary_is_present() {
    let budget = include_str!("../../kiana-domain/src/billing_budgets.rs");
    let quota = include_str!("../../kiana-domain/src/billing_quota.rs");
    let reservation = include_str!("../../kiana-domain/src/billing_reservation.rs");
    let facts = include_str!("../../kiana-domain/src/budget_contracts.rs");
    let ports = include_str!("../../kiana-ports/src/lib.rs");
    let queue = include_str!("../../kiana-ports/src/observability_queue.rs");
    let registry = include_str!("../src/cell_registry.rs");
    let capabilities = include_str!("../src/capabilities.rs");
    let model_budget = include_str!("../src/model_budget.rs");
    let runner_budget = include_str!("../../kiana-runner/src/budget.rs");
    let metrics = include_str!("../src/metrics.rs");

    for marker in [
        "intersect_budgets",
        "budget_intersection_empty",
        "max_concurrency",
        "max_effects",
        "queue_limit",
        "provider_max_input_tokens",
    ] {
        assert!(
            budget.contains(marker),
            "SC-16 budget marker missing: {marker}"
        );
    }
    for marker in [
        "QuotaWindowBudget",
        "quota_exhausted:retry_after_ms",
        "active_concurrency",
        "used_tokens",
        "until_next_window_ms",
    ] {
        assert!(
            quota.contains(marker),
            "SC-16 quota marker missing: {marker}"
        );
    }
    for marker in [
        "QuotaReservation",
        "reservation_digest",
        "idempotency_key",
        "authority_epoch",
        "config_revision",
    ] {
        assert!(
            reservation.contains(marker),
            "SC-16 reservation marker missing: {marker}"
        );
    }
    for marker in [
        "BudgetReservationFact",
        "BudgetSettlementFact",
        "usage_known",
        "charged_tokens",
        "expected_version",
    ] {
        assert!(
            facts.contains(marker),
            "SC-16 budget fact marker missing: {marker}"
        );
    }
    for marker in [
        "QuotaReservationPort",
        "reserve_quota",
        "transition_quota",
        "quota_reservation_revision_stale",
        "quota_reservation_fence_stale",
    ] {
        assert!(
            ports.contains(marker),
            "SC-16 port marker missing: {marker}"
        );
    }
    for marker in [
        "try_enqueue",
        "CriticalQueueFull",
        "BestEffortDropped",
        "critical_admission_evicted_best_effort",
        "critical_queue_full",
        "capacity",
    ] {
        assert!(
            queue.contains(marker),
            "SC-16 queue marker missing: {marker}"
        );
    }
    for marker in [
        "begin_capability",
        "finish_capability",
        "cell_capability_concurrency_exceeded",
        "active_capabilities",
        "resources_released",
        "max_concurrency",
    ] {
        assert!(
            registry.contains(marker),
            "SC-16 cell marker missing: {marker}"
        );
    }
    for marker in [
        "begin_cell_capability_from_request",
        "finish_cell_capability",
        "CapabilityOutcome::Unknown",
        "budget_lease_id",
    ] {
        assert!(
            capabilities.contains(marker),
            "SC-16 core capability marker missing: {marker}"
        );
    }
    for marker in [
        "reserve_attempt",
        "settle_attempt",
        "unknown_attempts",
        "budget_exceeded:model_attempts",
        "budget_reported_usage_exceeds_reservation",
    ] {
        assert!(
            runner_budget.contains(marker),
            "SC-16 runner budget marker missing: {marker}"
        );
    }
    for marker in [
        "model_budget_reservation_missing",
        "model_reported_usage_exceeds_reservation",
        "reservation_exceeded",
        "usage_known",
    ] {
        assert!(
            model_budget.contains(marker),
            "SC-16 model budget marker missing: {marker}"
        );
    }
    for marker in [
        "MetricCardinalityGuard",
        "overflow_total",
        "series_count",
        "MAX_METRIC_SERIES",
    ] {
        assert!(
            metrics.contains(marker),
            "SC-16 metrics marker missing: {marker}"
        );
    }
    for source in [
        budget,
        quota,
        reservation,
        facts,
        queue,
        registry,
        capabilities,
    ] {
        for forbidden in ["unbounded_channel", "unbounded_send", "retry_after_cancel"] {
            assert!(
                !source.contains(forbidden),
                "SC-16 unbounded/retry bypass marker: {forbidden}"
            );
        }
    }
}

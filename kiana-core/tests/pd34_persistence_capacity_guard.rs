//! PD-34 source guard for persistence pressure, latency and degradation budgets.

#[test]
fn persistence_capacity_reuses_performance_contract_and_fails_closed() {
    let source = include_str!("../../kiana-domain/src/persistence_capacity.rs");
    let performance = include_str!("../../kiana-domain/src/performance.rs");
    let baseline = include_str!("../../docs/roadmap/pd34-persistence-capacity-baseline.md");
    for marker in [
        "PerformanceBaseline",
        "CapacityEnvelope",
        "BenchmarkOperation",
        "PersistenceCapacityBudget",
        "PersistencePressureObservation",
        "max_p95_micros",
        "max_p99_micros",
        "max_queue_depth",
        "max_rejection_rate_bps",
        "max_maintenance_share_bps",
        "degraded",
        "degradation_reason",
        "facts_preserved",
        "persistence_capacity_latency_budget_exceeded",
        "persistence_capacity_queue_budget_exceeded",
        "persistence_capacity_rejection_budget_exceeded",
        "persistence_capacity_maintenance_budget_exceeded",
    ] {
        assert!(
            source.contains(marker),
            "PD-34 source marker missing: {marker}"
        );
    }
    for marker in [
        "CapacityEnvelope",
        "BenchmarkSummary",
        "p50_micros",
        "p95_micros",
        "p99_micros",
        "performance_safety_guard_missing",
    ] {
        assert!(
            performance.contains(marker),
            "PD-34 performance marker missing: {marker}"
        );
    }
    for marker in [
        "P95",
        "P99",
        "queue",
        "backpressure",
        "rejection",
        "maintenance",
        "degraded",
        "facts",
        "partial",
        "stress",
    ] {
        assert!(
            baseline.contains(marker),
            "PD-34 baseline marker missing: {marker}"
        );
    }
    assert!(!source.contains("CapabilityBroker::new"));
}

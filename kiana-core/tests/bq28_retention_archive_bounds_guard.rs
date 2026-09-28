//! BQ-28 source guard: the four bounds the card names, the ledger invariant, and the fact that this
//! module prunes nothing.

#[test]
fn bq28_pins_the_four_named_failures_and_the_invariant() {
    let source = include_str!("../src/retention_archive_bounds.rs");

    for marker in [
        "rollup_unbounded_memory",
        "rollup_high_cardinality_label",
        "retention_retained_evidence_deleted",
        "archive_lease_still_settleable",
        "retention_ledger_fact_changed",
        "retention_disk_limit_exceeded",
        "retention_queue_limit_exceeded",
        "retention_latency_p95_exceeded",
        "retention_bounds_unbounded",
        "retention_bounds_digest_mismatch",
        "retention_report_binding_invalid",
        "retention_report_digest_mismatch",
        "retention_latency_fixture_empty",
        "retention_percentile_invalid",
    ] {
        assert!(source.contains(marker), "BQ-28 module lost {marker}");
    }
}

#[test]
fn bq28_reuses_the_bq25_cardinality_ceiling_rather_than_inventing_one() {
    let source = include_str!("../src/retention_archive_bounds.rs");

    for marker in [
        "metric_label_budget",
        "use crate::data_class::metric_label_budget",
        "RetentionArchiveBounds",
        "RetentionArchiveSample",
        "RetentionArchiveReport",
        "evaluate_retention_archive",
        "percentile_ms",
        "ledger_fact_digest_before",
        "ledger_fact_digest_after",
    ] {
        assert!(source.contains(marker), "BQ-28 module lost {marker}");
    }
    // A second cardinality ceiling is a second answer to "how many values may a key take".
    for forbidden in [
        "const MAX_ROLLUP_LABEL_VALUES",
        "const BQ28_CARDINALITY",
    ] {
        assert!(
            !source.contains(forbidden),
            "BQ-28 invented a parallel cardinality ceiling: {forbidden}"
        );
    }
}

#[test]
fn bq28_computes_and_does_not_prune() {
    let source = include_str!("../src/retention_archive_bounds.rs");

    for forbidden in [
        "std::fs",
        "File::",
        "Command::",
        "std::process",
        "remove_file",
        "remove_dir",
        "TcpStream",
        "reqwest",
        "tokio",
        "spawn",
        "SystemTime",
        "Instant::now",
        "EventStore",
        "append_event",
        "ControlPlane",
    ] {
        assert!(
            !source.contains(forbidden),
            "BQ-28 module gained a token a bounds check must not have: {forbidden}"
        );
    }
}

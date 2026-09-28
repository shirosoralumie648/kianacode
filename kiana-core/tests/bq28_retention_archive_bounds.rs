//! BQ-28: the bounds a retention or archive pass has to stay inside, and the one thing it must
//! never change.
//!
//! Deny-first, success last. The four failures the card names come first, and the ledger invariant
//! is tested against a sample that is otherwise entirely clean, because that is the only way to
//! show it is being checked on its own.

use kiana_core::{
    evaluate_retention_archive, metric_label_budget, percentile_ms, RetentionArchiveBounds,
    RetentionArchiveReport, RetentionArchiveSample,
};

fn digest(byte: char) -> String {
    format!("sha256:{}", byte.to_string().repeat(64))
}

fn bounds() -> RetentionArchiveBounds {
    RetentionArchiveBounds::new(1_024, 1_048_576, 10_485_760, 64, 5_000)
}

/// Inside every bound, and the ledger fact digest is the same on both sides.
fn clean() -> RetentionArchiveSample {
    RetentionArchiveSample::new(
        "window-1",
        128,
        4_096,
        8,
        1_048_576,
        8,
        vec![10, 20, 30, 40, 50, 60, 70, 80, 90, 100],
        vec!["evidence-old".to_owned()],
        vec!["evidence-kept".to_owned()],
        vec!["lease-1".to_owned()],
        vec!["lease-0".to_owned()],
        digest('a'),
        digest('a'),
    )
}

#[test]
fn a_rollup_that_grows_without_a_ceiling_is_refused() {
    let mut sample = clean();
    sample.rollup_buckets = 1_025;
    assert_eq!(
        evaluate_retention_archive(&sample, &bounds()).unwrap_err(),
        "rollup_unbounded_memory"
    );
    let mut fat = clean();
    fat.rollup_bytes = 1_048_577;
    assert_eq!(
        evaluate_retention_archive(&fat, &bounds()).unwrap_err(),
        "rollup_unbounded_memory"
    );
}

#[test]
fn a_rollup_key_cannot_exceed_the_metric_label_cardinality_ceiling() {
    // The ceiling is BQ-25's, deliberately: a rollup key is a metric label wearing a different hat.
    let mut sample = clean();
    sample.distinct_label_values = metric_label_budget() as u32 + 1;
    assert_eq!(
        evaluate_retention_archive(&sample, &bounds()).unwrap_err(),
        "rollup_high_cardinality_label"
    );
    // And exactly at the ceiling is inside it.
    let mut at_limit = clean();
    at_limit.distinct_label_values = metric_label_budget() as u32;
    assert!(evaluate_retention_archive(&at_limit, &bounds()).is_ok());
}

#[test]
fn a_disk_or_queue_over_its_limit_is_refused() {
    let mut disk = clean();
    disk.observed_disk_bytes = 10_485_761;
    assert_eq!(
        evaluate_retention_archive(&disk, &bounds()).unwrap_err(),
        "retention_disk_limit_exceeded"
    );
    let mut queue = clean();
    queue.observed_queue_depth = 65;
    assert_eq!(
        evaluate_retention_archive(&queue, &bounds()).unwrap_err(),
        "retention_queue_limit_exceeded"
    );
}

#[test]
fn a_p95_over_its_bound_is_refused_using_a_fixed_fixture() {
    let mut sample = clean();
    // One slow observation in ten is a p95 of 30 and a p100 of 100, which is the whole reason to
    // name a percentile instead of a maximum.
    sample.latency_observations_ms = vec![10, 10, 10, 10, 10, 10, 10, 10, 10, 9_000];
    assert_eq!(percentile_ms(&sample.latency_observations_ms, 95).unwrap(), 10);
    assert_eq!(percentile_ms(&sample.latency_observations_ms, 100).unwrap(), 9_000);
    assert_eq!(
        evaluate_retention_archive(&sample, &bounds()).unwrap_err(),
        "retention_latency_p95_exceeded"
    );
}

#[test]
fn deleting_evidence_the_policy_still_retains_is_refused() {
    let mut sample = clean();
    sample.evidence_retained = vec!["evidence-kept".to_owned(), "evidence-old".to_owned()];
    assert_eq!(
        evaluate_retention_archive(&sample, &bounds()).unwrap_err(),
        "retention_retained_evidence_deleted"
    );
}

#[test]
fn an_archived_lease_cannot_still_settle() {
    let mut sample = clean();
    sample.lease_settled_after_archive = vec!["lease-1".to_owned()];
    assert_eq!(
        evaluate_retention_archive(&sample, &bounds()).unwrap_err(),
        "archive_lease_still_settleable"
    );
    // The same id that was never archived is not a violation, which is what keeps the rule about
    // archives rather than about ids.
    let mut unrelated = clean();
    unrelated.lease_settled_after_archive = vec!["lease-9".to_owned()];
    assert!(evaluate_retention_archive(&unrelated, &bounds()).is_ok());
}

#[test]
fn retention_and_archiving_may_not_change_a_ledger_fact() {
    // Every other field in this sample is comfortably inside budget. It is still refused, because a
    // pass that moves a number has stopped being a retention pass.
    let mut sample = clean();
    sample.ledger_fact_digest_after = digest('b');
    assert_eq!(
        evaluate_retention_archive(&sample, &bounds()).unwrap_err(),
        "retention_ledger_fact_changed"
    );
}

#[test]
fn bounds_with_no_ceiling_are_refused_rather_than_read_as_unbounded() {
    let unbounded = RetentionArchiveBounds::new(0, 0, 0, 0, 0);
    assert_eq!(
        evaluate_retention_archive(&clean(), &unbounded).unwrap_err(),
        "retention_bounds_unbounded"
    );
}

#[test]
fn an_empty_or_impossible_latency_fixture_is_refused() {
    assert_eq!(
        percentile_ms(&[], 95).unwrap_err(),
        "retention_latency_fixture_empty"
    );
    assert_eq!(
        percentile_ms(&[1, 2, 3], 0).unwrap_err(),
        "retention_percentile_invalid"
    );
    let mut sample = clean();
    sample.latency_observations_ms = Vec::new();
    assert_eq!(
        evaluate_retention_archive(&sample, &bounds()).unwrap_err(),
        "retention_latency_fixture_empty"
    );
}

#[test]
fn a_report_cannot_be_edited_after_the_fact() {
    let sample = clean();
    let bounds = bounds();
    let report: RetentionArchiveReport =
        evaluate_retention_archive(&sample, &bounds).expect("report");
    report.validate_against(&sample, &bounds).expect("un edited");

    let mut shrunk = report.clone();
    shrunk.latency_p95_ms = 1;
    assert_eq!(
        shrunk.validate_against(&sample, &bounds).unwrap_err(),
        "retention_report_binding_invalid"
    );

    let mut lying = report;
    lying.ledger_fact_unchanged = false;
    assert_eq!(
        lying.validate_against(&sample, &bounds).unwrap_err(),
        "retention_report_binding_invalid"
    );
}

#[test]
fn a_pass_inside_every_bound_produces_a_report_with_percentiles_and_an_intact_ledger() {
    let sample = clean();
    let bounds = bounds();
    let report = evaluate_retention_archive(&sample, &bounds).expect("report");
    report.validate_against(&sample, &bounds).expect("re-derives");
    assert_eq!(report.latency_p50_ms, 50);
    assert_eq!(report.latency_p95_ms, 100);
    assert!(report.ledger_fact_unchanged);
    assert_eq!(report.bounds_digest, bounds.bounds_digest);
    assert!(!report.limitations.is_empty());
}

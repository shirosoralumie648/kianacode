use kiana_domain::*;

fn hash(byte: char) -> String {
    format!("sha256:{}", byte.to_string().repeat(64))
}

fn metric(kind: Er33MetricKind, byte: char) -> Er33DrillMetric {
    let mut value = Er33DrillMetric {
        schema: ER33_METRIC_SCHEMA.to_owned(),
        kind,
        sample_count: 10,
        p50_ms: 2,
        p95_ms: 5,
        max_ms: 8,
        observed_bytes: 128,
        configured_limit: 1024,
        bounded: true,
        digest: String::new(),
    };
    value.digest = value.canonical_digest();
    let _ = byte;
    value
}

fn migration() -> Er33MigrationCheck {
    let mut value = Er33MigrationCheck {
        source_schema: "kiana.event.v1".to_owned(),
        target_schema: "kiana.event.v2".to_owned(),
        source_digest: hash('a'),
        target_digest: hash('b'),
        known_version: true,
        downgrade_requested: true,
        read_only: true,
        facts_preserved: true,
        digest: String::new(),
    };
    value.digest = value.canonical_digest();
    value
}

fn drill() -> Er33CapacityMigrationDrill {
    let kinds = [
        Er33MetricKind::EventFrame,
        Er33MetricKind::Flush,
        Er33MetricKind::ProjectionRebuild,
        Er33MetricKind::ReceiptQuery,
        Er33MetricKind::ArtifactBytes,
        Er33MetricKind::QueueDepth,
        Er33MetricKind::Recovery,
    ];
    let mut value = Er33CapacityMigrationDrill {
        schema: ER33_DRILL_SCHEMA.to_owned(),
        seed: 33,
        metrics: kinds
            .into_iter()
            .enumerate()
            .map(|(index, kind)| metric(kind, char::from(b'a' + index as u8)))
            .collect(),
        migration: migration(),
        over_quota_rejected: true,
        partial_frame_appended: false,
        unknown_version_rejected: true,
        facts_deleted_for_capacity: false,
        digest: String::new(),
    };
    value.digest = value.canonical_digest();
    value
}

#[test]
fn bounded_metrics_and_migration_preserve_facts_and_reject_over_quota() {
    let value = drill();
    value.validate().expect("drill");
    assert!(value.over_quota_rejected);
    assert!(!value.partial_frame_appended);
    assert!(value.migration.read_only);
}

#[test]
fn partial_frame_unknown_version_and_fact_deletion_cannot_be_success() {
    let mut partial = drill();
    partial.partial_frame_appended = true;
    partial.digest = partial.canonical_digest();
    assert_eq!(
        partial.validate().unwrap_err(),
        "er33_drill_safety_or_coverage_invalid"
    );

    let mut migration = drill();
    migration.migration.known_version = false;
    migration.migration.digest = migration.migration.canonical_digest();
    migration.digest = migration.canonical_digest();
    assert_eq!(
        migration.validate().unwrap_err(),
        "er33_migration_safety_invalid"
    );

    let mut deleted = drill();
    deleted.facts_deleted_for_capacity = true;
    deleted.digest = deleted.canonical_digest();
    assert_eq!(
        deleted.validate().unwrap_err(),
        "er33_drill_safety_or_coverage_invalid"
    );
}

#[test]
fn duplicate_metric_or_unbounded_sample_fails_closed() {
    let mut duplicate = drill();
    duplicate.metrics[1].kind = duplicate.metrics[0].kind;
    duplicate.metrics[1].digest = duplicate.metrics[1].canonical_digest();
    duplicate.digest = duplicate.canonical_digest();
    assert_eq!(duplicate.validate().unwrap_err(), "er33_metric_duplicate");

    let mut unbounded = drill();
    unbounded.metrics[0].observed_bytes = 2_000;
    unbounded.metrics[0].digest = unbounded.metrics[0].canonical_digest();
    unbounded.digest = unbounded.canonical_digest();
    assert_eq!(
        unbounded.validate().unwrap_err(),
        "er33_metric_bound_invalid"
    );
}

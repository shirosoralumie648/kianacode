//! ER-33 bounded performance/capacity/migration drill evidence.
//!
//! This is a typed, source-bound report for CI fixtures. It records limits and rejection results;
//! it does not benchmark the host, rotate a journal, migrate data or delete facts.

use crate::json_digest;
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::collections::BTreeSet;

pub const ER33_DRILL_SCHEMA: &str = "kiana.er33-capacity-migration-drill.v1";
pub const ER33_METRIC_SCHEMA: &str = "kiana.er33-drill-metric.v1";

fn required(value: &str, field: &'static str) -> Result<(), String> {
    if value.trim().is_empty() || value.len() > 4_096 || value.contains(['\0', '\r', '\n']) {
        Err(field.to_owned())
    } else {
        Ok(())
    }
}

fn digest(value: &str, field: &'static str) -> Result<(), String> {
    let Some(hex) = value.strip_prefix("sha256:") else {
        return Err(field.to_owned());
    };
    if hex.len() == 64 && hex.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        Ok(())
    } else {
        Err(field.to_owned())
    }
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Er33MetricKind {
    EventFrame,
    Flush,
    ProjectionRebuild,
    ReceiptQuery,
    ArtifactBytes,
    QueueDepth,
    Recovery,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Er33DrillMetric {
    pub schema: String,
    pub kind: Er33MetricKind,
    pub sample_count: u64,
    pub p50_ms: u64,
    pub p95_ms: u64,
    pub max_ms: u64,
    pub observed_bytes: u64,
    pub configured_limit: u64,
    pub bounded: bool,
    pub digest: String,
}

impl Er33DrillMetric {
    pub fn validate(&self) -> Result<(), String> {
        if self.schema != ER33_METRIC_SCHEMA
            || self.sample_count == 0
            || self.configured_limit == 0
            || self.p50_ms > self.p95_ms
            || self.p95_ms > self.max_ms
            || !self.bounded
            || self.observed_bytes > self.configured_limit
        {
            return Err("er33_metric_bound_invalid".to_owned());
        }
        digest(&self.digest, "er33_metric_digest_invalid")?;
        if self.digest != self.canonical_digest() {
            return Err("er33_metric_digest_mismatch".to_owned());
        }
        Ok(())
    }

    pub fn canonical_digest(&self) -> String {
        json_digest(&json!({
            "schema": self.schema,
            "kind": self.kind,
            "sample_count": self.sample_count,
            "p50_ms": self.p50_ms,
            "p95_ms": self.p95_ms,
            "max_ms": self.max_ms,
            "observed_bytes": self.observed_bytes,
            "configured_limit": self.configured_limit,
            "bounded": self.bounded,
        }))
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Er33MigrationCheck {
    pub source_schema: String,
    pub target_schema: String,
    pub source_digest: String,
    pub target_digest: String,
    pub known_version: bool,
    pub downgrade_requested: bool,
    pub read_only: bool,
    pub facts_preserved: bool,
    pub digest: String,
}

impl Er33MigrationCheck {
    pub fn validate(&self) -> Result<(), String> {
        required(&self.source_schema, "er33_source_schema_required")?;
        required(&self.target_schema, "er33_target_schema_required")?;
        digest(&self.source_digest, "er33_source_schema_digest_invalid")?;
        digest(&self.target_digest, "er33_target_schema_digest_invalid")?;
        if !self.known_version
            || !self.facts_preserved
            || (self.downgrade_requested && !self.read_only)
        {
            return Err("er33_migration_safety_invalid".to_owned());
        }
        digest(&self.digest, "er33_migration_digest_invalid")?;
        if self.digest != self.canonical_digest() {
            return Err("er33_migration_digest_mismatch".to_owned());
        }
        Ok(())
    }

    pub fn canonical_digest(&self) -> String {
        json_digest(&json!({
            "source_schema": self.source_schema,
            "target_schema": self.target_schema,
            "source_digest": self.source_digest,
            "target_digest": self.target_digest,
            "known_version": self.known_version,
            "downgrade_requested": self.downgrade_requested,
            "read_only": self.read_only,
            "facts_preserved": self.facts_preserved,
        }))
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Er33CapacityMigrationDrill {
    pub schema: String,
    pub seed: u64,
    pub metrics: Vec<Er33DrillMetric>,
    pub migration: Er33MigrationCheck,
    pub over_quota_rejected: bool,
    pub partial_frame_appended: bool,
    pub unknown_version_rejected: bool,
    pub facts_deleted_for_capacity: bool,
    pub digest: String,
}

impl Er33CapacityMigrationDrill {
    pub fn validate(&self) -> Result<(), String> {
        if self.schema != ER33_DRILL_SCHEMA
            || self.seed == 0
            || self.metrics.len() != 7
            || !self.over_quota_rejected
            || self.partial_frame_appended
            || !self.unknown_version_rejected
            || self.facts_deleted_for_capacity
        {
            return Err("er33_drill_safety_or_coverage_invalid".to_owned());
        }
        let mut kinds = BTreeSet::new();
        for metric in &self.metrics {
            metric.validate()?;
            if !kinds.insert(metric.kind) {
                return Err("er33_metric_duplicate".to_owned());
            }
        }
        self.migration.validate()?;
        digest(&self.digest, "er33_drill_digest_invalid")?;
        if self.digest != self.canonical_digest() {
            return Err("er33_drill_digest_mismatch".to_owned());
        }
        Ok(())
    }

    pub fn canonical_digest(&self) -> String {
        json_digest(&json!({
            "schema": self.schema,
            "seed": self.seed,
            "metrics": self.metrics,
            "migration": self.migration,
            "over_quota_rejected": self.over_quota_rejected,
            "partial_frame_appended": self.partial_frame_appended,
            "unknown_version_rejected": self.unknown_version_rejected,
            "facts_deleted_for_capacity": self.facts_deleted_for_capacity,
        }))
    }
}

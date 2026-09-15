//! Bounded performance, capacity and migration evidence contracts.
//!
//! These records describe a reproducible benchmark observation. They are not a health signal,
//! admission grant or proof that an external provider/effect is available.

use crate::{canonical_journal_bytes, json_digest, SchemaVersion};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

pub const PERFORMANCE_BASELINE_SCHEMA: &str = "kiana.performance-baseline.v1";
pub const MIGRATION_OBSERVATION_SCHEMA: &str = "kiana.migration-observation.v1";
pub const PERFORMANCE_SCHEMA_VERSION: SchemaVersion = SchemaVersion::new(1, 0);
pub const MAX_BENCHMARK_SAMPLES: usize = 6;
pub const MAX_MIGRATION_OBSERVATIONS: usize = 8;
pub const MAX_PERFORMANCE_LIMITATIONS: usize = 16;
pub const MAX_BENCHMARK_RUNS: u32 = 4_096;

#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BenchmarkOperation {
    Append,
    Flush,
    Project,
    Rebuild,
    Query,
    Export,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MigrationOperation {
    Rotation,
    Archive,
    Upgrade,
    Downgrade,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MigrationStatus {
    ReadOnlyVerified,
    Rejected,
    Unknown,
}

fn digest(value: &str, field: &str) -> Result<(), String> {
    let Some(hex) = value.strip_prefix("sha256:") else {
        return Err(format!("{field}_invalid"));
    };
    if hex.len() != 64 || !hex.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err(format!("{field}_invalid"));
    }
    Ok(())
}

fn bounded_text(value: &str, field: &str, max: usize) -> Result<(), String> {
    if value.trim().is_empty() || value.len() > max {
        return Err(format!("{field}_invalid"));
    }
    Ok(())
}

fn bounded_limitations(limitations: &[String]) -> Result<(), String> {
    if limitations.len() > MAX_PERFORMANCE_LIMITATIONS {
        return Err("performance_limitation_limit".to_owned());
    }
    for limitation in limitations {
        bounded_text(limitation, "performance_limitation", 256)?;
    }
    Ok(())
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BenchmarkSummary {
    pub operation: BenchmarkOperation,
    pub sample_count: u32,
    pub p50_micros: u64,
    pub p95_micros: u64,
    pub p99_micros: u64,
    pub max_bytes: u64,
    pub max_items: u64,
    pub summary_digest: String,
}

impl BenchmarkSummary {
    pub fn new(
        operation: BenchmarkOperation,
        sample_count: u32,
        p50_micros: u64,
        p95_micros: u64,
        p99_micros: u64,
        max_bytes: u64,
        max_items: u64,
    ) -> Result<Self, String> {
        let mut summary = Self {
            operation,
            sample_count,
            p50_micros,
            p95_micros,
            p99_micros,
            max_bytes,
            max_items,
            summary_digest: String::new(),
        };
        summary.summary_digest = summary.digest();
        summary.validate()?;
        Ok(summary)
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.sample_count == 0
            || self.sample_count > MAX_BENCHMARK_RUNS
            || self.p50_micros > self.p95_micros
            || self.p95_micros > self.p99_micros
            || self.max_items == 0
        {
            return Err("performance_percentiles_invalid".to_owned());
        }
        digest(&self.summary_digest, "performance_summary_digest")?;
        if self.summary_digest != self.digest() {
            return Err("performance_summary_digest_mismatch".to_owned());
        }
        Ok(())
    }

    pub fn digest(&self) -> String {
        let mut value = serde_json::to_value(self).unwrap_or(serde_json::Value::Null);
        if let Some(object) = value.as_object_mut() {
            object.insert(
                "summary_digest".to_owned(),
                serde_json::Value::String(String::new()),
            );
        }
        json_digest(&value)
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CapacityEnvelope {
    pub journal_max_bytes: u64,
    pub journal_max_events: u64,
    pub max_frame_bytes: u64,
    pub max_event_bytes: u64,
    pub max_batch_events: u64,
    pub max_page_events: u64,
    pub max_export_records: u64,
    pub observability_queue_capacity: u64,
    pub max_artifact_bytes: u64,
    pub high_cardinality_rejected: bool,
    pub oversize_rejected: bool,
    pub backpressure_preserves_facts: bool,
}

impl CapacityEnvelope {
    pub fn validate(&self) -> Result<(), String> {
        if [
            self.journal_max_bytes,
            self.journal_max_events,
            self.max_frame_bytes,
            self.max_event_bytes,
            self.max_batch_events,
            self.max_page_events,
            self.max_export_records,
            self.observability_queue_capacity,
            self.max_artifact_bytes,
        ]
        .into_iter()
        .any(|value| value == 0)
        {
            return Err("performance_capacity_invalid".to_owned());
        }
        if !self.high_cardinality_rejected
            || !self.oversize_rejected
            || !self.backpressure_preserves_facts
        {
            return Err("performance_safety_guard_missing".to_owned());
        }
        Ok(())
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MigrationObservation {
    pub schema: String,
    pub version: SchemaVersion,
    pub operation: MigrationOperation,
    pub from_version: u32,
    pub to_version: u32,
    pub status: MigrationStatus,
    pub source_cursor: u64,
    pub source_digest: String,
    #[serde(default)]
    pub limitations: Vec<String>,
    pub observation_digest: String,
}

impl MigrationObservation {
    pub fn new(
        operation: MigrationOperation,
        from_version: u32,
        to_version: u32,
        status: MigrationStatus,
        source_cursor: u64,
        source_digest: impl Into<String>,
        limitations: Vec<String>,
    ) -> Result<Self, String> {
        let mut observation = Self {
            schema: MIGRATION_OBSERVATION_SCHEMA.to_owned(),
            version: PERFORMANCE_SCHEMA_VERSION,
            operation,
            from_version,
            to_version,
            status,
            source_cursor,
            source_digest: source_digest.into(),
            limitations,
            observation_digest: String::new(),
        };
        observation.observation_digest = observation.digest();
        observation.validate()?;
        Ok(observation)
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.schema != MIGRATION_OBSERVATION_SCHEMA
            || !self.version.is_compatible_with(&PERFORMANCE_SCHEMA_VERSION)
            || self.from_version == 0
            || self.to_version == 0
            || self.source_cursor == 0
        {
            return Err("migration_observation_header_invalid".to_owned());
        }
        digest(&self.source_digest, "migration_source_digest")?;
        bounded_limitations(&self.limitations)?;
        if matches!(
            self.status,
            MigrationStatus::Rejected | MigrationStatus::Unknown
        ) && self.limitations.is_empty()
        {
            return Err("migration_non_success_reason_required".to_owned());
        }
        if self.status == MigrationStatus::ReadOnlyVerified
            && self.operation == MigrationOperation::Upgrade
            && self.to_version <= self.from_version
        {
            return Err("migration_upgrade_direction_invalid".to_owned());
        }
        if self.status == MigrationStatus::ReadOnlyVerified
            && self.operation == MigrationOperation::Downgrade
            && self.to_version >= self.from_version
        {
            return Err("migration_downgrade_direction_invalid".to_owned());
        }
        digest(&self.observation_digest, "migration_observation_digest")?;
        if self.observation_digest != self.digest() {
            return Err("migration_observation_digest_mismatch".to_owned());
        }
        Ok(())
    }

    pub fn digest(&self) -> String {
        let mut value = serde_json::to_value(self).unwrap_or(serde_json::Value::Null);
        if let Some(object) = value.as_object_mut() {
            object.insert(
                "observation_digest".to_owned(),
                serde_json::Value::String(String::new()),
            );
        }
        json_digest(&value)
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PerformanceBaseline {
    pub schema: String,
    pub version: SchemaVersion,
    pub source_cursor: u64,
    pub source_digest: String,
    pub samples: Vec<BenchmarkSummary>,
    pub capacity: CapacityEnvelope,
    pub migrations: Vec<MigrationObservation>,
    #[serde(default)]
    pub limitations: Vec<String>,
    pub baseline_digest: String,
}

impl PerformanceBaseline {
    pub fn new(
        source_cursor: u64,
        source_digest: impl Into<String>,
        samples: Vec<BenchmarkSummary>,
        capacity: CapacityEnvelope,
        migrations: Vec<MigrationObservation>,
        limitations: Vec<String>,
    ) -> Result<Self, String> {
        let mut baseline = Self {
            schema: PERFORMANCE_BASELINE_SCHEMA.to_owned(),
            version: PERFORMANCE_SCHEMA_VERSION,
            source_cursor,
            source_digest: source_digest.into(),
            samples,
            capacity,
            migrations,
            limitations,
            baseline_digest: String::new(),
        };
        baseline.baseline_digest = baseline.digest();
        baseline.validate()?;
        Ok(baseline)
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.schema != PERFORMANCE_BASELINE_SCHEMA
            || !self.version.is_compatible_with(&PERFORMANCE_SCHEMA_VERSION)
            || self.source_cursor == 0
            || self.samples.is_empty()
            || self.samples.len() > MAX_BENCHMARK_SAMPLES
            || self.migrations.len() > MAX_MIGRATION_OBSERVATIONS
        {
            return Err("performance_baseline_header_invalid".to_owned());
        }
        digest(&self.source_digest, "performance_source_digest")?;
        self.capacity.validate()?;
        let mut operations = BTreeSet::new();
        for sample in &self.samples {
            sample.validate()?;
            if !operations.insert(sample.operation) {
                return Err("performance_operation_duplicate".to_owned());
            }
        }
        let mut migrations = BTreeSet::new();
        for migration in &self.migrations {
            migration.validate()?;
            if !migrations.insert(migration.operation) {
                return Err("migration_operation_duplicate".to_owned());
            }
        }
        bounded_limitations(&self.limitations)?;
        digest(&self.baseline_digest, "performance_baseline_digest")?;
        if self.baseline_digest != self.digest() {
            return Err("performance_baseline_digest_mismatch".to_owned());
        }
        Ok(())
    }

    pub fn canonical_bytes(&self) -> Result<Vec<u8>, String> {
        canonical_journal_bytes(self)
    }

    pub fn digest(&self) -> String {
        let mut value = serde_json::to_value(self).unwrap_or(serde_json::Value::Null);
        if let Some(object) = value.as_object_mut() {
            object.insert(
                "baseline_digest".to_owned(),
                serde_json::Value::String(String::new()),
            );
        }
        json_digest(&value)
    }
}

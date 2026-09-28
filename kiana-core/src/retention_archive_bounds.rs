//! BQ-28: the bounds retention and archiving have to stay inside, and the one thing they must
//! never change.
//!
//! Four failures, in the order the card names them:
//!
//! - a **rollup** that keeps growing its in-memory bucket set until the process dies;
//! - a **high-cardinality label** promoted into a rollup key, which turns every request into a new
//!   series;
//! - **retained evidence deleted** because the retention pass swept it up along with the payload;
//! - an **archived lease still settling**, which is how a charge appears for work whose data is
//!   already gone.
//!
//! And one invariant that outranks all of them: **retention and archiving do not change ledger
//! facts.** A pass that prunes storage and, in the same breath, moves a number, has stopped being a
//! retention pass. So the caller states the ledger fact digest before and after, and this module
//! refuses the two disagreeing rather than trusting that nothing moved.
//!
//! # Percentiles come from a fixed fixture, never from live traffic
//!
//! `percentile_ms` takes the observations it is given and computes nearest-rank. A p95 that came
//! from whatever the process happened to be doing is not a bound, it is a mood. The card asks for a
//! fixed fixture precisely so the number is the same on every run and in every CI job.
//!
//! Reused rather than forked: the cardinality ceiling is BQ-25's `metric_label_budget()`, so a
//! rollup key cannot smuggle in more distinct values than a metric label is already allowed, and
//! the retention vocabulary is `kiana_domain::retention`.
//!
//! This module computes. It prunes nothing, archives nothing and reads no clock.

use kiana_domain::{json_digest, SchemaVersion};
use serde::{Deserialize, Serialize};
use serde_json::json;

use crate::data_class::metric_label_budget;

pub const RETENTION_ARCHIVE_BOUNDS_SCHEMA: &str = "kiana.retention-archive-bounds.v1";
pub const RETENTION_ARCHIVE_REPORT_SCHEMA: &str = "kiana.retention-archive-report.v1";
pub const RETENTION_ARCHIVE_VERSION: SchemaVersion = SchemaVersion::new(1, 0);

/// The ceilings a retention pass claims to hold. Every one is required, and zero means
/// "no ceiling was claimed" rather than "unbounded", so a caller has to say what it means.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RetentionArchiveBounds {
    pub max_rollup_buckets: u32,
    pub max_rollup_bytes: u64,
    pub max_disk_bytes: u64,
    pub max_queue_depth: u32,
    pub max_latency_p95_ms: u64,
    pub bounds_digest: String,
}

impl RetentionArchiveBounds {
    pub fn new(
        max_rollup_buckets: u32,
        max_rollup_bytes: u64,
        max_disk_bytes: u64,
        max_queue_depth: u32,
        max_latency_p95_ms: u64,
    ) -> Self {
        let mut value = Self {
            max_rollup_buckets,
            max_rollup_bytes,
            max_disk_bytes,
            max_queue_depth,
            max_latency_p95_ms,
            bounds_digest: String::new(),
        };
        value.bounds_digest = value.digest();
        value
    }

    pub fn digest(&self) -> String {
        json_digest(&json!({
            "max_rollup_buckets": self.max_rollup_buckets,
            "max_rollup_bytes": self.max_rollup_bytes,
            "max_disk_bytes": self.max_disk_bytes,
            "max_queue_depth": self.max_queue_depth,
            "max_latency_p95_ms": self.max_latency_p95_ms,
        }))
    }

    fn validate(&self) -> Result<(), String> {
        if self.bounds_digest != self.digest() {
            return Err("retention_bounds_digest_mismatch".to_owned());
        }
        if self.max_rollup_buckets == 0
            || self.max_rollup_bytes == 0
            || self.max_disk_bytes == 0
            || self.max_queue_depth == 0
            || self.max_latency_p95_ms == 0
        {
            return Err("retention_bounds_unbounded".to_owned());
        }
        Ok(())
    }
}

/// One retention or archive pass, as observed.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RetentionArchiveSample {
    pub window_id: String,
    pub rollup_buckets: u32,
    pub rollup_bytes: u64,
    /// Distinct values the rollup key took. Compared against BQ-25's metric label budget, so a
    /// rollup key cannot be a back door around the cardinality rule.
    pub distinct_label_values: u32,
    pub observed_disk_bytes: u64,
    pub observed_queue_depth: u32,
    /// The fixed latency fixture for this window, in milliseconds, unsorted.
    pub latency_observations_ms: Vec<u64>,
    /// Evidence references the pass removed.
    pub evidence_deleted: Vec<String>,
    /// Evidence the retention policy still retains.
    pub evidence_retained: Vec<String>,
    /// Leases the pass archived.
    pub archived_lease_ids: Vec<String>,
    /// Archived leases somebody settled afterwards.
    pub lease_settled_after_archive: Vec<String>,
    /// The ledger fact digest before and after the pass. These are the invariant.
    pub ledger_fact_digest_before: String,
    pub ledger_fact_digest_after: String,
}

impl RetentionArchiveSample {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        window_id: impl Into<String>,
        rollup_buckets: u32,
        rollup_bytes: u64,
        distinct_label_values: u32,
        observed_disk_bytes: u64,
        observed_queue_depth: u32,
        latency_observations_ms: Vec<u64>,
        evidence_deleted: Vec<String>,
        evidence_retained: Vec<String>,
        archived_lease_ids: Vec<String>,
        lease_settled_after_archive: Vec<String>,
        ledger_fact_digest_before: impl Into<String>,
        ledger_fact_digest_after: impl Into<String>,
    ) -> Self {
        Self {
            window_id: window_id.into(),
            rollup_buckets,
            rollup_bytes,
            distinct_label_values,
            observed_disk_bytes,
            observed_queue_depth,
            latency_observations_ms,
            evidence_deleted,
            evidence_retained,
            archived_lease_ids,
            lease_settled_after_archive,
            ledger_fact_digest_before: ledger_fact_digest_before.into(),
            ledger_fact_digest_after: ledger_fact_digest_after.into(),
        }
    }
}

/// Nearest-rank percentile over a fixed observation set.
///
/// The list is sorted first, so the caller does not have to, and the rank is `ceil(p/100 * n)`
/// rather than an interpolation — a p95 of a five-element fixture is a real observation, not a
/// number between two of them.
pub fn percentile_ms(observations: &[u64], percentile: u64) -> Result<u64, String> {
    if observations.is_empty() {
        return Err("retention_latency_fixture_empty".to_owned());
    }
    if !(1..=100).contains(&percentile) {
        return Err("retention_percentile_invalid".to_owned());
    }
    let mut sorted = observations.to_vec();
    sorted.sort_unstable();
    let rank = (percentile * sorted.len() as u64).div_ceil(100).max(1) as usize;
    Ok(sorted[rank.min(sorted.len()) - 1])
}

/// The sealed verdict.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RetentionArchiveReport {
    pub schema: String,
    pub version: SchemaVersion,
    pub window_id: String,
    pub bounds_digest: String,
    pub rollup_buckets: u32,
    pub rollup_bytes: u64,
    pub observed_disk_bytes: u64,
    pub observed_queue_depth: u32,
    pub latency_p50_ms: u64,
    pub latency_p95_ms: u64,
    pub ledger_fact_unchanged: bool,
    /// What this report does **not** show. Never empty.
    pub limitations: Vec<String>,
    pub report_digest: String,
}

impl RetentionArchiveReport {
    pub fn validate_against(
        &self,
        sample: &RetentionArchiveSample,
        bounds: &RetentionArchiveBounds,
    ) -> Result<(), String> {
        bounds.validate()?;
        if self.schema != RETENTION_ARCHIVE_REPORT_SCHEMA
            || !self.version.is_compatible_with(&RETENTION_ARCHIVE_VERSION)
            || self.window_id != sample.window_id
            || self.bounds_digest != bounds.bounds_digest
            || self.rollup_buckets != sample.rollup_buckets
            || self.rollup_bytes != sample.rollup_bytes
            || self.observed_disk_bytes != sample.observed_disk_bytes
            || self.observed_queue_depth != sample.observed_queue_depth
            || self.latency_p50_ms != percentile_ms(&sample.latency_observations_ms, 50)?
            || self.latency_p95_ms != percentile_ms(&sample.latency_observations_ms, 95)?
        {
            return Err("retention_report_binding_invalid".to_owned());
        }
        if self.report_digest != self.digest() {
            return Err("retention_report_digest_mismatch".to_owned());
        }
        Ok(())
    }

    pub fn digest(&self) -> String {
        json_digest(&json!({
            "schema": self.schema,
            "version": self.version,
            "window_id": self.window_id,
            "bounds_digest": self.bounds_digest,
            "rollup_buckets": self.rollup_buckets,
            "rollup_bytes": self.rollup_bytes,
            "observed_disk_bytes": self.observed_disk_bytes,
            "observed_queue_depth": self.observed_queue_depth,
            "latency_p50_ms": self.latency_p50_ms,
            "latency_p95_ms": self.latency_p95_ms,
            "ledger_fact_unchanged": self.ledger_fact_unchanged,
            "limitations": self.limitations,
        }))
    }
}

/// Evaluate one pass against its bounds.
///
/// The ledger invariant is checked **last** on purpose. A pass that moved a ledger fact and also
/// blew a bucket bound has two problems, and the one an operator can act on immediately is the
/// bound. The ledger check still runs, and it is the one that cannot be waived.
pub fn evaluate_retention_archive(
    sample: &RetentionArchiveSample,
    bounds: &RetentionArchiveBounds,
) -> Result<RetentionArchiveReport, String> {
    bounds.validate()?;
    if sample.window_id.trim().is_empty() {
        return Err("retention_window_id_required".to_owned());
    }
    let p50 = percentile_ms(&sample.latency_observations_ms, 50)?;
    let p95 = percentile_ms(&sample.latency_observations_ms, 95)?;

    if sample.rollup_buckets > bounds.max_rollup_buckets
        || sample.rollup_bytes > bounds.max_rollup_bytes
    {
        return Err("rollup_unbounded_memory".to_owned());
    }
    // The ceiling is BQ-25's, not a new one. A rollup key is a metric label wearing a different
    // hat, and it has to live inside the same cardinality rule.
    if sample.distinct_label_values as usize > metric_label_budget() {
        return Err("rollup_high_cardinality_label".to_owned());
    }
    if sample.observed_disk_bytes > bounds.max_disk_bytes {
        return Err("retention_disk_limit_exceeded".to_owned());
    }
    if sample.observed_queue_depth > bounds.max_queue_depth {
        return Err("retention_queue_limit_exceeded".to_owned());
    }
    if p95 > bounds.max_latency_p95_ms {
        return Err("retention_latency_p95_exceeded".to_owned());
    }
    if sample
        .evidence_deleted
        .iter()
        .any(|reference| sample.evidence_retained.contains(reference))
    {
        return Err("retention_retained_evidence_deleted".to_owned());
    }
    if sample
        .lease_settled_after_archive
        .iter()
        .any(|lease| sample.archived_lease_ids.contains(lease))
    {
        return Err("archive_lease_still_settleable".to_owned());
    }
    // The invariant the whole card hangs on.
    if sample.ledger_fact_digest_before != sample.ledger_fact_digest_after {
        return Err("retention_ledger_fact_changed".to_owned());
    }

    let mut report = RetentionArchiveReport {
        schema: RETENTION_ARCHIVE_REPORT_SCHEMA.to_owned(),
        version: RETENTION_ARCHIVE_VERSION,
        window_id: sample.window_id.clone(),
        bounds_digest: bounds.bounds_digest.clone(),
        rollup_buckets: sample.rollup_buckets,
        rollup_bytes: sample.rollup_bytes,
        observed_disk_bytes: sample.observed_disk_bytes,
        observed_queue_depth: sample.observed_queue_depth,
        latency_p50_ms: p50,
        latency_p95_ms: p95,
        ledger_fact_unchanged: true,
        limitations: vec![
            "every count in this report was supplied; nothing was measured here".to_owned(),
            "the ledger digests are the caller's before and after, not a diff this module took"
                .to_owned(),
        ],
        report_digest: String::new(),
    };
    report.report_digest = report.digest();
    report.validate_against(sample, bounds)?;
    Ok(report)
}

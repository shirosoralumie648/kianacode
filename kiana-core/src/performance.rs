//! Deterministic helpers for bounded performance/capacity evidence.
//!
//! The benchmark runner lives in remote CI. Core only supplies checked percentile reduction and
//! baseline construction; it never turns timing observations into admission or health authority.

use kiana_domain::{
    BenchmarkOperation, BenchmarkSummary, CapacityEnvelope, MigrationObservation,
    PerformanceBaseline,
};

const MAX_TIMING_SAMPLES: usize = 4_096;

#[derive(Clone, Debug, Eq, PartialEq, thiserror::Error)]
pub enum PerformanceError {
    #[error("performance_samples_empty")]
    SamplesEmpty,
    #[error("performance_samples_limit")]
    SamplesLimit,
    #[error("performance_percentile_invalid")]
    PercentileInvalid,
    #[error("performance_baseline_invalid:{0}")]
    BaselineInvalid(String),
}

/// Return a nearest-rank percentile from a bounded, unsorted timing sample.
pub fn percentile_micros(samples: &[u64], percentile: u8) -> Result<u64, PerformanceError> {
    if samples.is_empty() {
        return Err(PerformanceError::SamplesEmpty);
    }
    if samples.len() > MAX_TIMING_SAMPLES || !matches!(percentile, 50 | 95 | 99) {
        return Err(if samples.len() > MAX_TIMING_SAMPLES {
            PerformanceError::SamplesLimit
        } else {
            PerformanceError::PercentileInvalid
        });
    }
    let mut sorted = samples.to_vec();
    sorted.sort_unstable();
    let rank = ((sorted.len() as u128 * percentile as u128).div_ceil(100) as usize).max(1);
    Ok(sorted[rank - 1])
}

/// Reduce one remote benchmark sample set to a checked p50/p95/p99 summary.
pub fn summarize_benchmark(
    operation: BenchmarkOperation,
    samples: &[u64],
    max_bytes: u64,
    max_items: u64,
) -> Result<BenchmarkSummary, PerformanceError> {
    let p50 = percentile_micros(samples, 50)?;
    let p95 = percentile_micros(samples, 95)?;
    let p99 = percentile_micros(samples, 99)?;
    BenchmarkSummary::new(
        operation,
        samples.len() as u32,
        p50,
        p95,
        p99,
        max_bytes,
        max_items,
    )
    .map_err(PerformanceError::BaselineInvalid)
}

/// Bind benchmark summaries, hard capacity limits and migration observations to one source.
pub fn build_performance_baseline(
    source_cursor: u64,
    source_digest: impl Into<String>,
    samples: Vec<BenchmarkSummary>,
    capacity: CapacityEnvelope,
    migrations: Vec<MigrationObservation>,
    limitations: Vec<String>,
) -> Result<PerformanceBaseline, PerformanceError> {
    PerformanceBaseline::new(
        source_cursor,
        source_digest,
        samples,
        capacity,
        migrations,
        limitations,
    )
    .map_err(PerformanceError::BaselineInvalid)
}

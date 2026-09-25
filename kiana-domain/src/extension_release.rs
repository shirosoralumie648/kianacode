//! Extension performance, supply-chain and release-gate evidence contracts.
//!
//! Observations are immutable metadata. A skipped/blocked benchmark never becomes a passing
//! release gate, and this module does not run a benchmark, scan dependencies, package a desktop
//! artifact or publish a release.

use crate::{canonical_journal_bytes, json_digest, redact_text, SchemaVersion};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

pub const EXTENSION_BENCHMARK_SCHEMA: &str = "kiana.extension-benchmark-observation.v1";
pub const EXTENSION_RELEASE_GATE_SCHEMA: &str = "kiana.extension-release-gate.v1";
pub const EXTENSION_RELEASE_VERSION: SchemaVersion = SchemaVersion::new(1, 0);
const MAX_METRICS: usize = 6;
const MAX_LIMITATIONS: usize = 16;

fn bounded(value: &str, field: &str, max: usize) -> Result<(), String> {
    if value.trim().is_empty() || value.len() > max || value.contains('\0') {
        return Err(format!("{field}_invalid"));
    }
    if redact_text(value) != value {
        return Err(format!("{field}_contains_secret"));
    }
    Ok(())
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

fn clear_digest<T: Serialize>(value: &T, field: &str) -> String {
    let mut value = serde_json::to_value(value).unwrap_or(serde_json::Value::Null);
    if let Some(object) = value.as_object_mut() {
        object.insert(field.to_owned(), serde_json::Value::String(String::new()));
    }
    json_digest(&value)
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ExtensionBenchmarkMetric {
    CatalogCold,
    CatalogWarm,
    ResourceRead,
    HookLatency,
    PackageVerify,
    SnapshotRebuild,
}

impl ExtensionBenchmarkMetric {
    pub const ALL: [Self; MAX_METRICS] = [
        Self::CatalogCold,
        Self::CatalogWarm,
        Self::ResourceRead,
        Self::HookLatency,
        Self::PackageVerify,
        Self::SnapshotRebuild,
    ];
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ExtensionBenchmarkStatus {
    Observed,
    Skipped,
    Blocked,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ExtensionReleaseStatus {
    Ready,
    Partial,
    Blocked,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ExtensionBenchmarkObservation {
    pub schema: String,
    pub metric: ExtensionBenchmarkMetric,
    pub status: ExtensionBenchmarkStatus,
    pub p50_ms: Option<u64>,
    pub p95_ms: Option<u64>,
    pub sample_count: u32,
    pub package_bytes: u64,
    pub memory_bytes: u64,
    pub disk_bytes: u64,
    pub concurrency: u32,
    pub environment_digest: String,
    pub toolchain_digest: String,
    pub limitation: Option<String>,
    pub observation_digest: String,
}

impl ExtensionBenchmarkObservation {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        metric: ExtensionBenchmarkMetric,
        status: ExtensionBenchmarkStatus,
        p50_ms: Option<u64>,
        p95_ms: Option<u64>,
        sample_count: u32,
        package_bytes: u64,
        memory_bytes: u64,
        disk_bytes: u64,
        concurrency: u32,
        environment_digest: impl Into<String>,
        toolchain_digest: impl Into<String>,
        limitation: Option<String>,
    ) -> Result<Self, String> {
        let mut observation = Self {
            schema: EXTENSION_BENCHMARK_SCHEMA.to_owned(),
            metric,
            status,
            p50_ms,
            p95_ms,
            sample_count,
            package_bytes,
            memory_bytes,
            disk_bytes,
            concurrency,
            environment_digest: environment_digest.into(),
            toolchain_digest: toolchain_digest.into(),
            limitation,
            observation_digest: String::new(),
        };
        observation.observation_digest = observation.digest();
        observation.validate()?;
        Ok(observation)
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.schema != EXTENSION_BENCHMARK_SCHEMA
            || self.concurrency == 0
            || self.concurrency > 128
        {
            return Err("extension_benchmark_header_invalid".to_owned());
        }
        digest(
            &self.environment_digest,
            "extension_benchmark_environment_digest",
        )?;
        digest(
            &self.toolchain_digest,
            "extension_benchmark_toolchain_digest",
        )?;
        match self.status {
            ExtensionBenchmarkStatus::Observed => {
                let (Some(p50), Some(p95)) = (self.p50_ms, self.p95_ms) else {
                    return Err("extension_benchmark_observation_missing_metrics".to_owned());
                };
                if self.sample_count == 0 || p50 > p95 || self.limitation.is_some() {
                    return Err("extension_benchmark_observation_invalid".to_owned());
                }
            }
            ExtensionBenchmarkStatus::Skipped | ExtensionBenchmarkStatus::Blocked => {
                if self.p50_ms.is_some() || self.p95_ms.is_some() || self.sample_count != 0 {
                    return Err("extension_benchmark_unobserved_metrics_claimed".to_owned());
                }
                let limitation = self
                    .limitation
                    .as_deref()
                    .ok_or_else(|| "extension_benchmark_limitation_required".to_owned())?;
                bounded(limitation, "extension_benchmark_limitation", 256)?;
            }
        }
        digest(
            &self.observation_digest,
            "extension_benchmark_observation_digest",
        )?;
        if self.observation_digest != self.digest() {
            return Err("extension_benchmark_observation_digest_mismatch".to_owned());
        }
        Ok(())
    }

    pub fn digest(&self) -> String {
        clear_digest(self, "observation_digest")
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ExtensionReleaseGate {
    pub schema: String,
    pub version: SchemaVersion,
    pub source_snapshot_digest: String,
    pub lockfile_digest: String,
    pub manifest_digest: String,
    pub supply_chain_digest: String,
    pub denied_matrix_digest: String,
    pub status: ExtensionReleaseStatus,
    pub benchmarks: Vec<ExtensionBenchmarkObservation>,
    pub limitations: Vec<String>,
    pub gate_digest: String,
}

impl ExtensionReleaseGate {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        source_snapshot_digest: impl Into<String>,
        lockfile_digest: impl Into<String>,
        manifest_digest: impl Into<String>,
        supply_chain_digest: impl Into<String>,
        denied_matrix_digest: impl Into<String>,
        status: ExtensionReleaseStatus,
        benchmarks: Vec<ExtensionBenchmarkObservation>,
        limitations: Vec<String>,
    ) -> Result<Self, String> {
        let mut gate = Self {
            schema: EXTENSION_RELEASE_GATE_SCHEMA.to_owned(),
            version: EXTENSION_RELEASE_VERSION,
            source_snapshot_digest: source_snapshot_digest.into(),
            lockfile_digest: lockfile_digest.into(),
            manifest_digest: manifest_digest.into(),
            supply_chain_digest: supply_chain_digest.into(),
            denied_matrix_digest: denied_matrix_digest.into(),
            status,
            benchmarks,
            limitations,
            gate_digest: String::new(),
        };
        gate.gate_digest = gate.digest();
        gate.validate()?;
        Ok(gate)
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.schema != EXTENSION_RELEASE_GATE_SCHEMA
            || self.version != EXTENSION_RELEASE_VERSION
            || self.benchmarks.len() != MAX_METRICS
            || self.limitations.len() > MAX_LIMITATIONS
        {
            return Err("extension_release_gate_header_invalid".to_owned());
        }
        for (value, field) in [
            (
                &self.source_snapshot_digest,
                "extension_release_source_digest",
            ),
            (&self.lockfile_digest, "extension_release_lockfile_digest"),
            (&self.manifest_digest, "extension_release_manifest_digest"),
            (
                &self.supply_chain_digest,
                "extension_release_supply_chain_digest",
            ),
            (
                &self.denied_matrix_digest,
                "extension_release_denied_matrix_digest",
            ),
            (&self.gate_digest, "extension_release_gate_digest"),
        ] {
            digest(value, field)?;
        }
        let mut metrics = BTreeSet::new();
        for benchmark in &self.benchmarks {
            benchmark.validate()?;
            if !metrics.insert(benchmark.metric) {
                return Err("extension_release_metric_duplicate".to_owned());
            }
        }
        if ExtensionBenchmarkMetric::ALL
            .iter()
            .any(|metric| !metrics.contains(metric))
        {
            return Err("extension_release_metric_coverage_missing".to_owned());
        }
        for limitation in &self.limitations {
            bounded(limitation, "extension_release_limitation", 256)?;
        }
        let observed = self
            .benchmarks
            .iter()
            .filter(|benchmark| benchmark.status == ExtensionBenchmarkStatus::Observed)
            .count();
        match self.status {
            ExtensionReleaseStatus::Ready
                if observed != MAX_METRICS || !self.limitations.is_empty() =>
            {
                return Err("extension_release_ready_evidence_incomplete".to_owned())
            }
            ExtensionReleaseStatus::Partial if self.limitations.is_empty() => {
                return Err("extension_release_partial_limitation_missing".to_owned())
            }
            ExtensionReleaseStatus::Blocked if self.limitations.is_empty() => {
                return Err("extension_release_blocked_limitation_missing".to_owned())
            }
            _ => {}
        }
        if self.gate_digest != self.digest() {
            return Err("extension_release_gate_digest_mismatch".to_owned());
        }
        Ok(())
    }

    pub fn blockers(&self) -> Result<Vec<String>, String> {
        self.validate()?;
        let mut blockers = self.limitations.clone();
        blockers.extend(
            self.benchmarks
                .iter()
                .filter(|benchmark| benchmark.status != ExtensionBenchmarkStatus::Observed)
                .map(|benchmark| format!("benchmark_unobserved:{:?}", benchmark.metric)),
        );
        Ok(blockers)
    }

    pub fn canonical_bytes(&self) -> Result<Vec<u8>, String> {
        canonical_journal_bytes(self)
    }

    pub fn digest(&self) -> String {
        clear_digest(self, "gate_digest")
    }
}

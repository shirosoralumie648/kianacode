//! Persistence capacity, pressure and degradation budget contract.
//!
//! This extends the existing PerformanceBaseline/CapacityEnvelope evidence with bounded queue,
//! rejection, maintenance-share and degradation observations. It is read-only evidence and does
//! not run a benchmark, resize a queue or turn a degraded result into a health/admission grant.

use crate::{json_digest, BenchmarkOperation, BenchmarkSummary, CapacityEnvelope, SchemaVersion};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

pub const PERSISTENCE_CAPACITY_SCHEMA: &str = "kiana.persistence-capacity-report.v1";
pub const PERSISTENCE_CAPACITY_VERSION: SchemaVersion = SchemaVersion::new(1, 0);
pub const MAX_PERSISTENCE_BUDGETS: usize = 8;

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PersistenceCapacityBudget {
    pub operation: BenchmarkOperation,
    pub max_p95_micros: u64,
    pub max_p99_micros: u64,
    pub max_queue_depth: u64,
    pub max_rejection_rate_bps: u16,
    pub max_maintenance_share_bps: u16,
}

impl PersistenceCapacityBudget {
    pub fn validate(&self) -> Result<(), String> {
        if self.max_p95_micros == 0
            || self.max_p99_micros < self.max_p95_micros
            || self.max_queue_depth == 0
            || self.max_rejection_rate_bps > 10_000
            || self.max_maintenance_share_bps > 10_000
        {
            return Err("persistence_capacity_budget_invalid".to_owned());
        }
        Ok(())
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PersistencePressureObservation {
    pub operation: BenchmarkOperation,
    pub summary: BenchmarkSummary,
    pub queue_depth: u64,
    pub rejected_count: u64,
    pub degraded: bool,
    #[serde(default)]
    pub degradation_reason: Option<String>,
    pub facts_preserved: bool,
    pub maintenance_share_bps: u16,
    pub observation_digest: String,
}

impl PersistencePressureObservation {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        operation: BenchmarkOperation,
        summary: BenchmarkSummary,
        queue_depth: u64,
        rejected_count: u64,
        degraded: bool,
        degradation_reason: Option<String>,
        facts_preserved: bool,
        maintenance_share_bps: u16,
    ) -> Result<Self, String> {
        let mut observation = Self {
            operation,
            summary,
            queue_depth,
            rejected_count,
            degraded,
            degradation_reason,
            facts_preserved,
            maintenance_share_bps,
            observation_digest: String::new(),
        };
        observation.observation_digest = observation.digest();
        observation.validate()?;
        Ok(observation)
    }

    pub fn validate(&self) -> Result<(), String> {
        self.summary.validate()?;
        if self.summary.operation != self.operation
            || self.summary.sample_count == 0
            || self.rejected_count > u64::from(self.summary.sample_count)
            || self.maintenance_share_bps > 10_000
            || (self.degraded && self.degradation_reason.is_none())
            || (!self.degraded && self.degradation_reason.is_some())
            || !self.facts_preserved
        {
            return Err("persistence_capacity_observation_invalid".to_owned());
        }
        if let Some(reason) = &self.degradation_reason {
            if reason.trim().is_empty() || reason.len() > 256 {
                return Err("persistence_capacity_degradation_reason_invalid".to_owned());
            }
        }
        validate_digest(
            &self.observation_digest,
            "persistence_capacity_observation_digest",
        )?;
        if self.observation_digest != self.digest() {
            return Err("persistence_capacity_observation_digest_mismatch".to_owned());
        }
        Ok(())
    }

    pub fn digest(&self) -> String {
        json_digest(&serde_json::json!({
            "operation": self.operation,
            "summary": self.summary,
            "queue_depth": self.queue_depth,
            "rejected_count": self.rejected_count,
            "degraded": self.degraded,
            "degradation_reason": self.degradation_reason,
            "facts_preserved": self.facts_preserved,
            "maintenance_share_bps": self.maintenance_share_bps,
        }))
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PersistenceCapacityStatus {
    Ready,
    Blocked,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PersistenceCapacityReport {
    pub schema: String,
    pub version: SchemaVersion,
    pub status: PersistenceCapacityStatus,
    pub baseline_digest: String,
    pub capacity: CapacityEnvelope,
    pub budgets: Vec<PersistenceCapacityBudget>,
    pub observations: Vec<PersistencePressureObservation>,
    pub degraded_operations: Vec<BenchmarkOperation>,
    pub reason: String,
    pub remediation: String,
    pub report_digest: String,
}

impl PersistenceCapacityReport {
    pub fn evaluate(
        baseline_digest: impl Into<String>,
        capacity: CapacityEnvelope,
        budgets: Vec<PersistenceCapacityBudget>,
        observations: Vec<PersistencePressureObservation>,
    ) -> Result<Self, String> {
        let baseline_digest = baseline_digest.into();
        validate_digest(&baseline_digest, "persistence_capacity_baseline_digest")?;
        capacity.validate()?;
        if budgets.is_empty() || budgets.len() > MAX_PERSISTENCE_BUDGETS {
            return Err("persistence_capacity_budget_count_invalid".to_owned());
        }
        let mut budget_operations = BTreeSet::new();
        for budget in &budgets {
            budget.validate()?;
            if !budget_operations.insert(budget.operation) {
                return Err("persistence_capacity_budget_duplicate".to_owned());
            }
        }
        let mut observed_operations = BTreeSet::new();
        for observation in &observations {
            observation.validate()?;
            if !budget_operations.contains(&observation.operation) {
                return Err("persistence_capacity_budget_missing".to_owned());
            }
            if !observed_operations.insert(observation.operation) {
                return Err("persistence_capacity_observation_duplicate".to_owned());
            }
        }
        if observed_operations != budget_operations {
            return Err("persistence_capacity_observation_set_incomplete".to_owned());
        }
        let mut degraded_operations = Vec::new();
        let mut decision = (
            PersistenceCapacityStatus::Ready,
            "ok",
            "within supplied budgets",
        );
        for observation in &observations {
            let budget = budgets
                .iter()
                .find(|budget| budget.operation == observation.operation)
                .expect("budget set was validated");
            let rejection_rate_bps = observation.rejected_count.saturating_mul(10_000)
                / u64::from(observation.summary.sample_count);
            if observation.summary.p95_micros > budget.max_p95_micros
                || observation.summary.p99_micros > budget.max_p99_micros
            {
                decision = (
                    PersistenceCapacityStatus::Blocked,
                    "persistence_capacity_latency_budget_exceeded",
                    "reduce load or increase capacity only through an approved reviewed change",
                );
                break;
            }
            if observation.queue_depth > budget.max_queue_depth {
                decision = (
                    PersistenceCapacityStatus::Blocked,
                    "persistence_capacity_queue_budget_exceeded",
                    "apply bounded backpressure and preserve committed facts",
                );
                break;
            }
            if rejection_rate_bps > u64::from(budget.max_rejection_rate_bps) {
                decision = (
                    PersistenceCapacityStatus::Blocked,
                    "persistence_capacity_rejection_budget_exceeded",
                    "diagnose capacity rejection before admitting more work",
                );
                break;
            }
            if observation.maintenance_share_bps > budget.max_maintenance_share_bps {
                decision = (
                    PersistenceCapacityStatus::Blocked,
                    "persistence_capacity_maintenance_budget_exceeded",
                    "bound maintenance work so user admission remains observable",
                );
                break;
            }
            if observation.degraded {
                degraded_operations.push(observation.operation);
            }
        }
        let mut report = Self {
            schema: PERSISTENCE_CAPACITY_SCHEMA.to_owned(),
            version: PERSISTENCE_CAPACITY_VERSION,
            status: decision.0,
            baseline_digest,
            capacity,
            budgets,
            observations,
            degraded_operations,
            reason: decision.1.to_owned(),
            remediation: decision.2.to_owned(),
            report_digest: String::new(),
        };
        report.report_digest = report.digest();
        report.validate()?;
        Ok(report)
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.schema != PERSISTENCE_CAPACITY_SCHEMA
            || self.version != PERSISTENCE_CAPACITY_VERSION
        {
            return Err("persistence_capacity_report_header_invalid".to_owned());
        }
        validate_digest(
            &self.baseline_digest,
            "persistence_capacity_baseline_digest",
        )?;
        validate_digest(&self.report_digest, "persistence_capacity_report_digest")?;
        self.capacity.validate()?;
        if self.report_digest != self.digest() {
            return Err("persistence_capacity_report_digest_mismatch".to_owned());
        }
        Ok(())
    }

    pub fn digest(&self) -> String {
        json_digest(&serde_json::json!({
            "schema": self.schema,
            "version": self.version,
            "status": self.status,
            "baseline_digest": self.baseline_digest,
            "capacity": self.capacity,
            "budgets": self.budgets,
            "observations": self.observations,
            "degraded_operations": self.degraded_operations,
            "reason": self.reason,
            "remediation": self.remediation,
        }))
    }
}

fn validate_digest(value: &str, field: &str) -> Result<(), String> {
    let Some(hex) = value.strip_prefix("sha256:") else {
        return Err(format!("{field}_invalid"));
    };
    if hex.len() != 64 || !hex.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err(format!("{field}_invalid"));
    }
    Ok(())
}

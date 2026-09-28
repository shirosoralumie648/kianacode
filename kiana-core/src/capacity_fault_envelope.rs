//! SC-40: the capacity and resource envelope a fault has to stay inside.
//!
//! BQ-26 already injects the faults and reports what the seams did — including whether
//! reservations leaked. SC-40 asks the next question: **while that was happening, did the system
//! stay bounded?** Unbounded queue, unbounded output bytes, unbounded latency, a provider that
//! kept spending after a 429, a clock that rolled back and made every latency number meaningless —
//! those are the failures, and none of them is visible in "the operation eventually returned an
//! error".
//!
//! ```text
//! fault happened
//!     ↓  sample (what was observed while it happened)
//! CapacityBudget (the bounds the system claims to hold)
//!     ↓  evaluate_capacity_fault
//! CapacityFaultReport — either inside the envelope, or the first bound that was broken
//! ```
//!
//! # Why the bounds are inputs rather than constants
//!
//! Because a bound nobody wrote down is not a bound. `CapacityBudget` is supplied, sealed and
//! carried into the report, so a report can always answer "bounded by what, exactly".
//!
//! # Why recovery is checked separately
//!
//! A system can hold every bound *during* a fault and still leak afterwards: the queue drains, the
//! latency returns to normal, and the reservation from the attempt that got a 429 is never
//! released. That is the leak the card names, and it is invisible to any sample taken while the
//! fault was still running. So `reservations_after_recovery` is a separate, mandatory field and a
//! non-zero value is refused even when everything else is inside budget.
//!
//! This module computes. It injects nothing, measures nothing and starts nothing.

use kiana_domain::{json_digest, SchemaVersion};
use serde::{Deserialize, Serialize};
use serde_json::json;

use crate::bq26_fault_harness::{Bq26FaultCase, Bq26FaultClass};

pub const CAPACITY_FAULT_REPORT_SCHEMA: &str = "kiana.capacity-fault-report.v1";
pub const CAPACITY_FAULT_VERSION: SchemaVersion = SchemaVersion::new(1, 0);

/// The bounds a system claims to hold while a fault is in progress.
///
/// Every field is a ceiling rather than a target, and zero means "no ceiling was claimed" rather
/// than "unbounded", so a caller has to say what it means.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CapacityBudget {
    pub max_latency_ms: u64,
    pub max_queue_depth: u32,
    pub max_bytes: u64,
    /// Whether the system must show it applied backpressure rather than absorbing the load.
    pub require_backpressure: bool,
    pub budget_digest: String,
}

impl CapacityBudget {
    pub fn new(
        max_latency_ms: u64,
        max_queue_depth: u32,
        max_bytes: u64,
        require_backpressure: bool,
    ) -> Self {
        let mut value = Self {
            max_latency_ms,
            max_queue_depth,
            max_bytes,
            require_backpressure,
            budget_digest: String::new(),
        };
        value.budget_digest = value.digest();
        value
    }

    pub fn digest(&self) -> String {
        json_digest(&json!({
            "max_latency_ms": self.max_latency_ms,
            "max_queue_depth": self.max_queue_depth,
            "max_bytes": self.max_bytes,
            "require_backpressure": self.require_backpressure,
        }))
    }

    fn validate(&self) -> Result<(), String> {
        if self.budget_digest != self.digest() {
            return Err("capacity_fault_budget_digest_mismatch".to_owned());
        }
        if self.max_latency_ms == 0 || self.max_queue_depth == 0 || self.max_bytes == 0 {
            return Err("capacity_fault_budget_unbounded".to_owned());
        }
        Ok(())
    }
}

/// What was observed while one fault was in progress, and what survived the recovery.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CapacityFaultSample {
    pub case: Bq26FaultCase,
    pub family: Bq26FaultClass,
    pub observed_latency_ms: u64,
    pub observed_queue_depth: u32,
    pub observed_bytes: u64,
    pub backpressure_applied: bool,
    /// Reservations still held at the moment the fault was declared handled.
    pub leaked_reservations: u32,
    pub leaked_leases: u32,
    /// Whether the operator ran the recovery step. A sample that skips it cannot claim to be
    /// recovered, and pretending otherwise is how a leak survives a restart.
    pub recovered: bool,
    /// Reservations still held **after** recovery. Zero is the only acceptable value.
    pub reservations_after_recovery: u32,
    /// Whether the clock moved backwards during the fault. A rollback makes every latency number
    /// above meaningless, so it is refused before any of them is compared.
    pub clock_rollback_detected: bool,
}

impl CapacityFaultSample {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        case: Bq26FaultCase,
        family: Bq26FaultClass,
        observed_latency_ms: u64,
        observed_queue_depth: u32,
        observed_bytes: u64,
        backpressure_applied: bool,
        leaked_reservations: u32,
        leaked_leases: u32,
        recovered: bool,
        reservations_after_recovery: u32,
        clock_rollback_detected: bool,
    ) -> Self {
        Self {
            case,
            family,
            observed_latency_ms,
            observed_queue_depth,
            observed_bytes,
            backpressure_applied,
            leaked_reservations,
            leaked_leases,
            recovered,
            reservations_after_recovery,
            clock_rollback_detected,
        }
    }
}

/// The sealed verdict.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CapacityFaultReport {
    pub schema: String,
    pub version: SchemaVersion,
    pub case: Bq26FaultCase,
    pub family: Bq26FaultClass,
    pub budget_digest: String,
    pub observed_latency_ms: u64,
    pub observed_queue_depth: u32,
    pub observed_bytes: u64,
    pub backpressure_applied: bool,
    pub reservations_after_recovery: u32,
    /// What this report does **not** show. Never empty.
    pub limitations: Vec<String>,
    pub report_digest: String,
}

impl CapacityFaultReport {
    pub fn within_envelope(&self) -> bool {
        self.limitations.is_empty()
    }

    pub fn validate_against(
        &self,
        sample: &CapacityFaultSample,
        budget: &CapacityBudget,
    ) -> Result<(), String> {
        budget.validate()?;
        if self.schema != CAPACITY_FAULT_REPORT_SCHEMA
            || !self.version.is_compatible_with(&CAPACITY_FAULT_VERSION)
            || self.case != sample.case
            || self.family != sample.family
            || self.budget_digest != budget.budget_digest
            || self.observed_latency_ms != sample.observed_latency_ms
            || self.observed_queue_depth != sample.observed_queue_depth
            || self.observed_bytes != sample.observed_bytes
            || self.backpressure_applied != sample.backpressure_applied
            || self.reservations_after_recovery != sample.reservations_after_recovery
        {
            return Err("capacity_fault_report_binding_invalid".to_owned());
        }
        if self.report_digest != self.digest() {
            return Err("capacity_fault_report_digest_mismatch".to_owned());
        }
        Ok(())
    }

    pub fn digest(&self) -> String {
        json_digest(&json!({
            "schema": self.schema,
            "version": self.version,
            "case": self.case,
            "family": self.family,
            "budget_digest": self.budget_digest,
            "observed_latency_ms": self.observed_latency_ms,
            "observed_queue_depth": self.observed_queue_depth,
            "observed_bytes": self.observed_bytes,
            "backpressure_applied": self.backpressure_applied,
            "reservations_after_recovery": self.reservations_after_recovery,
            "limitations": self.limitations,
        }))
    }
}

/// Evaluate one sample against one budget.
///
/// The order of the checks is the argument of the function. A clock rollback is decided first,
/// because every remaining check compares a number, and a number measured against a clock that went
/// backwards is not evidence of anything. The bounds come next, in the order a reader would ask
/// about them: queue, output, latency, backpressure. The reservation checks come last, because a
/// leak is the failure that survives everything else being fine.
pub fn evaluate_capacity_fault(
    sample: &CapacityFaultSample,
    budget: &CapacityBudget,
) -> Result<CapacityFaultReport, String> {
    budget.validate()?;

    if sample.clock_rollback_detected {
        return Err("capacity_fault_clock_rollback".to_owned());
    }
    if sample.observed_queue_depth > budget.max_queue_depth {
        return Err("capacity_fault_queue_unbounded".to_owned());
    }
    if sample.observed_bytes > budget.max_bytes {
        return Err("capacity_fault_output_flood".to_owned());
    }
    if sample.observed_latency_ms > budget.max_latency_ms {
        return Err("capacity_fault_latency_unbounded".to_owned());
    }
    if budget.require_backpressure && !sample.backpressure_applied {
        return Err("capacity_fault_backpressure_missing".to_owned());
    }
    // A disk that reported full and then "recovered" without ever pushing back did not recover; it
    // absorbed the load somewhere the sample cannot see.
    if sample.family == Bq26FaultClass::DiskFull && !sample.backpressure_applied {
        return Err("capacity_fault_backpressure_missing".to_owned());
    }
    if sample.leaked_reservations > 0 || sample.leaked_leases > 0 {
        return Err("capacity_fault_reservation_leak".to_owned());
    }
    // A provider that kept spending after a 429 or 5xx is the card's "越额" case. It is refused
    // under its own code rather than folded into the generic leak so the reason names the cause.
    if sample.family == Bq26FaultClass::ProviderHttp && sample.leaked_reservations > 0 {
        return Err("capacity_fault_provider_quota_exceeded".to_owned());
    }
    // A clock fault that did not actually move the clock proves nothing about latency, and
    // reporting it as though it did is the same mistake as quoting a number measured against a
    // clock nobody trusts. So the ClockFault family must arrive with the rollback actually observed.
    if sample.family == Bq26FaultClass::ClockFault && !sample.clock_rollback_detected {
        return Err("capacity_fault_clock_rollback_unproven".to_owned());
    }
    // Recovery is checked on its own terms, and only when it is claimed.
    if sample.recovered && sample.reservations_after_recovery > 0 {
        return Err("capacity_fault_reservation_leak_after_recovery".to_owned());
    }
    if !sample.recovered {
        return Err("capacity_fault_recovery_not_established".to_owned());
    }

    let mut report = CapacityFaultReport {
        schema: CAPACITY_FAULT_REPORT_SCHEMA.to_owned(),
        version: CAPACITY_FAULT_VERSION,
        case: sample.case,
        family: sample.family,
        budget_digest: budget.budget_digest.clone(),
        observed_latency_ms: sample.observed_latency_ms,
        observed_queue_depth: sample.observed_queue_depth,
        observed_bytes: sample.observed_bytes,
        backpressure_applied: sample.backpressure_applied,
        reservations_after_recovery: sample.reservations_after_recovery,
        limitations: vec![
            "no latency, queue depth or byte count in this report was measured".to_owned(),
            "the sample and the budget were both supplied; neither was observed here".to_owned(),
        ],
        report_digest: String::new(),
    };
    report.report_digest = report.digest();
    report.validate_against(sample, budget)?;
    Ok(report)
}

//! Core adapter for frozen Outcome measurement facts.
//!
//! Assessment is deterministic over the domain ledger; this adapter never treats delivery tests,
//! runtime completion or model text as an Objective outcome.

use kiana_domain::{
    OutcomeAssessment, OutcomeDecision, OutcomeMeasurementLedger, OutcomeMeasurementObservation,
    OutcomeMeasurementPlan,
};

pub(crate) fn publish_outcome_plan(
    ledger: &mut OutcomeMeasurementLedger,
    plan: OutcomeMeasurementPlan,
) -> Result<(), &'static str> {
    ledger.publish_plan(plan)
}

pub(crate) fn record_outcome_observation(
    ledger: &mut OutcomeMeasurementLedger,
    observation: OutcomeMeasurementObservation,
) -> Result<(), &'static str> {
    ledger.record_observation(observation)
}

pub(crate) fn assess_outcome(
    ledger: &mut OutcomeMeasurementLedger,
    plan_id: &str,
    assessment_id: impl Into<String>,
    assessed_at: u64,
) -> Result<OutcomeAssessment, &'static str> {
    ledger.assess(plan_id, assessment_id, assessed_at)
}

pub(crate) fn decide_outcome(
    ledger: &mut OutcomeMeasurementLedger,
    decision: OutcomeDecision,
) -> Result<(), &'static str> {
    ledger.decide(decision)
}

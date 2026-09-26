//! Frozen Outcome measurement plans, observations, assessments and Sponsor decisions.
//!
//! Delivery/runtime evidence is not an Outcome. The ledger computes an assessment from every
//! bound observation in the frozen window and keeps a separate human decision fact.

use crate::{json_digest, MetricDirection};
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::collections::{BTreeMap, BTreeSet};

pub const OUTCOME_MEASUREMENT_SCHEMA: &str = "kiana.outcome-measurement.v1";

fn required(value: &str, field: &'static str) -> Result<(), &'static str> {
    if value.trim().is_empty() || value.len() > 16_384 || value.contains(['\0', '\r', '\n']) {
        Err(field)
    } else {
        Ok(())
    }
}

fn digest(value: &str, field: &'static str) -> Result<(), &'static str> {
    let Some(hex) = value.strip_prefix("sha256:") else {
        return Err(field);
    };
    if hex.len() == 64 && hex.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        Ok(())
    } else {
        Err(field)
    }
}

fn references(values: &[String], field: &'static str) -> Result<(), &'static str> {
    if values.is_empty() || values.len() > 256 {
        return Err(field);
    }
    if values.iter().any(|value| value.trim().is_empty())
        || values.iter().collect::<BTreeSet<_>>().len() != values.len()
    {
        return Err(field);
    }
    Ok(())
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OutcomeAggregation {
    Mean,
    Minimum,
    Maximum,
    Latest,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OutcomeMeasurementPlan {
    pub schema: String,
    pub plan_id: String,
    pub objective_id: String,
    pub project_id: String,
    pub baseline_version: u64,
    pub metric: String,
    pub unit: String,
    pub direction: MetricDirection,
    pub baseline: f64,
    pub target: f64,
    pub dataset_id: String,
    pub method: String,
    pub source_ref: String,
    pub owner_id: String,
    pub window_start: u64,
    pub window_end: u64,
    pub minimum_samples: u32,
    pub aggregation: OutcomeAggregation,
    pub fixture: bool,
    pub digest: String,
}

impl OutcomeMeasurementPlan {
    pub fn validate(&self) -> Result<(), &'static str> {
        if self.schema != OUTCOME_MEASUREMENT_SCHEMA
            || self.baseline_version == 0
            || !self.baseline.is_finite()
            || !self.target.is_finite()
            || self.window_start == 0
            || self.window_end <= self.window_start
            || !(1..=10_000).contains(&self.minimum_samples)
        {
            return Err("outcome_plan_header_invalid");
        }
        for (value, field) in [
            (&self.plan_id, "outcome_plan_id_required"),
            (&self.objective_id, "outcome_plan_objective_required"),
            (&self.project_id, "outcome_plan_project_required"),
            (&self.metric, "outcome_plan_metric_required"),
            (&self.unit, "outcome_plan_unit_required"),
            (&self.dataset_id, "outcome_plan_dataset_required"),
            (&self.method, "outcome_plan_method_required"),
            (&self.source_ref, "outcome_plan_source_required"),
            (&self.owner_id, "outcome_plan_owner_required"),
        ] {
            required(value, field)?;
        }
        let target_direction_valid = match self.direction {
            MetricDirection::AtLeast => self.target > self.baseline,
            MetricDirection::AtMost => self.target < self.baseline,
        };
        if !target_direction_valid {
            return Err("outcome_plan_target_direction_invalid");
        }
        digest(&self.digest, "outcome_plan_digest_invalid")?;
        if self.digest != self.canonical_digest() {
            return Err("outcome_plan_digest_mismatch");
        }
        Ok(())
    }

    pub fn canonical_digest(&self) -> String {
        json_digest(&json!({
            "schema": self.schema,
            "plan_id": self.plan_id,
            "objective_id": self.objective_id,
            "project_id": self.project_id,
            "baseline_version": self.baseline_version,
            "metric": self.metric,
            "unit": self.unit,
            "direction": self.direction,
            "baseline": self.baseline,
            "target": self.target,
            "dataset_id": self.dataset_id,
            "method": self.method,
            "source_ref": self.source_ref,
            "owner_id": self.owner_id,
            "window_start": self.window_start,
            "window_end": self.window_end,
            "minimum_samples": self.minimum_samples,
            "aggregation": self.aggregation,
            "fixture": self.fixture,
        }))
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OutcomeMeasurementObservation {
    pub schema: String,
    pub observation_id: String,
    pub plan_id: String,
    pub project_id: String,
    pub dataset_id: String,
    pub metric: String,
    pub unit: String,
    pub value: f64,
    pub observed_at: u64,
    pub recorded_at: u64,
    pub source_ref: String,
    pub evidence_refs: Vec<String>,
    pub fixture: bool,
    pub digest: String,
}

impl OutcomeMeasurementObservation {
    pub fn validate_against(&self, plan: &OutcomeMeasurementPlan) -> Result<(), &'static str> {
        plan.validate()?;
        if self.schema != OUTCOME_MEASUREMENT_SCHEMA
            || self.plan_id != plan.plan_id
            || self.project_id != plan.project_id
            || self.dataset_id != plan.dataset_id
            || self.metric != plan.metric
            || self.unit != plan.unit
            || self.source_ref != plan.source_ref
            || self.fixture != plan.fixture
            || !self.value.is_finite()
            || self.observed_at < plan.window_start
            || self.observed_at > plan.window_end
            || self.recorded_at < self.observed_at
        {
            return Err("outcome_observation_binding_or_window_invalid");
        }
        for (value, field) in [
            (&self.observation_id, "outcome_observation_id_required"),
            (&self.plan_id, "outcome_observation_plan_required"),
            (&self.project_id, "outcome_observation_project_required"),
            (&self.dataset_id, "outcome_observation_dataset_required"),
            (&self.metric, "outcome_observation_metric_required"),
            (&self.unit, "outcome_observation_unit_required"),
            (&self.source_ref, "outcome_observation_source_required"),
        ] {
            required(value, field)?;
        }
        references(&self.evidence_refs, "outcome_observation_evidence_required")?;
        digest(&self.digest, "outcome_observation_digest_invalid")?;
        if self.digest != self.canonical_digest() {
            return Err("outcome_observation_digest_mismatch");
        }
        Ok(())
    }

    pub fn canonical_digest(&self) -> String {
        json_digest(&json!({
            "schema": self.schema,
            "observation_id": self.observation_id,
            "plan_id": self.plan_id,
            "project_id": self.project_id,
            "dataset_id": self.dataset_id,
            "metric": self.metric,
            "unit": self.unit,
            "value": self.value,
            "observed_at": self.observed_at,
            "recorded_at": self.recorded_at,
            "source_ref": self.source_ref,
            "evidence_refs": self.evidence_refs,
            "fixture": self.fixture,
        }))
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OutcomeAssessmentStatus {
    MissingData,
    Realized,
    PartiallyRealized,
    NotRealized,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OutcomeAssessment {
    pub schema: String,
    pub assessment_id: String,
    pub plan_id: String,
    pub project_id: String,
    pub baseline_version: u64,
    pub observation_ids: Vec<String>,
    pub aggregate: Option<f64>,
    pub status: OutcomeAssessmentStatus,
    pub missing_data: bool,
    pub assessed_at: u64,
    pub digest: String,
}

impl OutcomeAssessment {
    fn validate_against(&self, plan: &OutcomeMeasurementPlan) -> Result<(), &'static str> {
        plan.validate()?;
        if self.schema != OUTCOME_MEASUREMENT_SCHEMA
            || self.plan_id != plan.plan_id
            || self.project_id != plan.project_id
            || self.baseline_version != plan.baseline_version
            || self.assessed_at <= plan.window_end
            || self.observation_ids.is_empty() && !self.missing_data
            || self.missing_data != (self.aggregate.is_none())
            || self.aggregate.is_some_and(|value| !value.is_finite())
        {
            return Err("outcome_assessment_binding_invalid");
        }
        required(&self.assessment_id, "outcome_assessment_id_required")?;
        if self.observation_ids.iter().collect::<BTreeSet<_>>().len() != self.observation_ids.len()
        {
            return Err("outcome_assessment_duplicate_observation");
        }
        digest(&self.digest, "outcome_assessment_digest_invalid")?;
        if self.digest != self.canonical_digest() {
            return Err("outcome_assessment_digest_mismatch");
        }
        Ok(())
    }

    pub fn canonical_digest(&self) -> String {
        json_digest(&json!({
            "schema": self.schema,
            "assessment_id": self.assessment_id,
            "plan_id": self.plan_id,
            "project_id": self.project_id,
            "baseline_version": self.baseline_version,
            "observation_ids": self.observation_ids,
            "aggregate": self.aggregate,
            "status": self.status,
            "missing_data": self.missing_data,
            "assessed_at": self.assessed_at,
        }))
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OutcomeDecisionKind {
    Achieved,
    NotAchieved,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OutcomeDecision {
    pub schema: String,
    pub decision_id: String,
    pub assessment_id: String,
    pub objective_id: String,
    pub assessment_digest: String,
    pub decision: OutcomeDecisionKind,
    pub decided_by: String,
    pub decider_role: String,
    pub owner_id: String,
    pub decision_ref: String,
    pub decided_at: u64,
    pub digest: String,
}

impl OutcomeDecision {
    fn validate_against(
        &self,
        plan: &OutcomeMeasurementPlan,
        assessment: &OutcomeAssessment,
    ) -> Result<(), &'static str> {
        assessment.validate_against(plan)?;
        if self.schema != OUTCOME_MEASUREMENT_SCHEMA
            || self.assessment_id != assessment.assessment_id
            || self.objective_id != plan.objective_id
            || self.assessment_digest != assessment.digest
            || self.decider_role != "sponsor"
            || self.decided_by == plan.owner_id
            || self.owner_id != plan.owner_id
            || self.decided_at == 0
        {
            return Err("outcome_decision_binding_or_independence_invalid");
        }
        if self.decision == OutcomeDecisionKind::Achieved
            && assessment.status != OutcomeAssessmentStatus::Realized
        {
            return Err("outcome_achieved_requires_realized_assessment");
        }
        for (value, field) in [
            (&self.decision_id, "outcome_decision_id_required"),
            (&self.decided_by, "outcome_decision_decider_required"),
            (&self.decision_ref, "outcome_decision_evidence_required"),
        ] {
            required(value, field)?;
        }
        digest(
            &self.assessment_digest,
            "outcome_decision_assessment_digest_invalid",
        )?;
        digest(&self.digest, "outcome_decision_digest_invalid")?;
        if self.digest != self.canonical_digest() {
            return Err("outcome_decision_digest_mismatch");
        }
        Ok(())
    }

    pub fn canonical_digest(&self) -> String {
        json_digest(&json!({
            "schema": self.schema,
            "decision_id": self.decision_id,
            "assessment_id": self.assessment_id,
            "objective_id": self.objective_id,
            "assessment_digest": self.assessment_digest,
            "decision": self.decision,
            "decided_by": self.decided_by,
            "decider_role": self.decider_role,
            "owner_id": self.owner_id,
            "decision_ref": self.decision_ref,
            "decided_at": self.decided_at,
        }))
    }
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct OutcomeMeasurementLedger {
    pub plans: BTreeMap<String, OutcomeMeasurementPlan>,
    pub observations: BTreeMap<String, OutcomeMeasurementObservation>,
    pub assessments: BTreeMap<String, OutcomeAssessment>,
    pub decisions: BTreeMap<String, OutcomeDecision>,
}

impl OutcomeMeasurementLedger {
    pub fn publish_plan(&mut self, plan: OutcomeMeasurementPlan) -> Result<(), &'static str> {
        plan.validate()?;
        if let Some(existing) = self.plans.get(&plan.plan_id) {
            if existing.digest == plan.digest {
                return Ok(());
            }
            return Err("outcome_plan_duplicate_digest_mismatch");
        }
        if self
            .plans
            .values()
            .any(|existing| existing.objective_id == plan.objective_id)
        {
            return Err("outcome_plan_already_frozen");
        }
        self.plans.insert(plan.plan_id.clone(), plan);
        Ok(())
    }

    pub fn record_observation(
        &mut self,
        observation: OutcomeMeasurementObservation,
    ) -> Result<(), &'static str> {
        let plan = self
            .plans
            .get(&observation.plan_id)
            .ok_or("outcome_plan_not_found")?;
        observation.validate_against(plan)?;
        if let Some(existing) = self.observations.get(&observation.observation_id) {
            if existing.digest == observation.digest {
                return Ok(());
            }
            return Err("outcome_observation_duplicate_digest_mismatch");
        }
        if self.observations.values().any(|existing| {
            existing.plan_id == observation.plan_id
                && existing.observed_at == observation.observed_at
        }) {
            return Err("outcome_observation_duplicate_timestamp");
        }
        self.observations
            .insert(observation.observation_id.clone(), observation);
        Ok(())
    }

    pub fn assess(
        &mut self,
        plan_id: &str,
        assessment_id: impl Into<String>,
        assessed_at: u64,
    ) -> Result<OutcomeAssessment, &'static str> {
        let plan = self
            .plans
            .get(plan_id)
            .ok_or("outcome_plan_not_found")?
            .clone();
        if assessed_at <= plan.window_end {
            return Err("outcome_assessment_window_not_closed");
        }
        let mut observations = self
            .observations
            .values()
            .filter(|observation| observation.plan_id == plan_id)
            .collect::<Vec<_>>();
        observations
            .sort_by_key(|observation| (observation.observed_at, &observation.observation_id));
        let observation_ids = observations
            .iter()
            .map(|observation| observation.observation_id.clone())
            .collect::<Vec<_>>();
        let missing_data = observations.len() < plan.minimum_samples as usize;
        let aggregate = if missing_data {
            None
        } else {
            Some(match plan.aggregation {
                OutcomeAggregation::Mean => {
                    observations
                        .iter()
                        .map(|observation| observation.value)
                        .sum::<f64>()
                        / observations.len() as f64
                }
                OutcomeAggregation::Minimum => observations
                    .iter()
                    .map(|observation| observation.value)
                    .fold(f64::INFINITY, f64::min),
                OutcomeAggregation::Maximum => observations
                    .iter()
                    .map(|observation| observation.value)
                    .fold(f64::NEG_INFINITY, f64::max),
                OutcomeAggregation::Latest => observations.last().unwrap().value,
            })
        };
        let status = if missing_data {
            OutcomeAssessmentStatus::MissingData
        } else if aggregate.is_some_and(|value| match plan.direction {
            MetricDirection::AtLeast => value >= plan.target,
            MetricDirection::AtMost => value <= plan.target,
        }) {
            OutcomeAssessmentStatus::Realized
        } else if aggregate.is_some_and(|value| match plan.direction {
            MetricDirection::AtLeast => value > plan.baseline,
            MetricDirection::AtMost => value < plan.baseline,
        }) {
            OutcomeAssessmentStatus::PartiallyRealized
        } else {
            OutcomeAssessmentStatus::NotRealized
        };
        let mut assessment = OutcomeAssessment {
            schema: OUTCOME_MEASUREMENT_SCHEMA.to_owned(),
            assessment_id: assessment_id.into(),
            plan_id: plan.plan_id.clone(),
            project_id: plan.project_id.clone(),
            baseline_version: plan.baseline_version,
            observation_ids,
            aggregate,
            status,
            missing_data,
            assessed_at,
            digest: String::new(),
        };
        assessment.digest = assessment.canonical_digest();
        assessment.validate_against(&plan)?;
        if let Some(existing) = self.assessments.get(&assessment.assessment_id) {
            if existing.digest == assessment.digest {
                return Ok(existing.clone());
            }
            return Err("outcome_assessment_duplicate_digest_mismatch");
        }
        self.assessments
            .insert(assessment.assessment_id.clone(), assessment.clone());
        Ok(assessment)
    }

    pub fn decide(&mut self, decision: OutcomeDecision) -> Result<(), &'static str> {
        let assessment = self
            .assessments
            .get(&decision.assessment_id)
            .ok_or("outcome_assessment_not_found")?;
        let plan = self
            .plans
            .get(&assessment.plan_id)
            .ok_or("outcome_plan_not_found")?;
        decision.validate_against(plan, assessment)?;
        if let Some(existing) = self.decisions.get(&decision.decision_id) {
            if existing.digest == decision.digest {
                return Ok(());
            }
            return Err("outcome_decision_duplicate_digest_mismatch");
        }
        self.decisions
            .insert(decision.decision_id.clone(), decision);
        Ok(())
    }
}

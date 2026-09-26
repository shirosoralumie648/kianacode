use kiana_domain::*;

fn plan(minimum_samples: u32) -> OutcomeMeasurementPlan {
    let mut value = OutcomeMeasurementPlan {
        schema: OUTCOME_MEASUREMENT_SCHEMA.to_owned(),
        plan_id: "plan-1".to_owned(),
        objective_id: "objective-1".to_owned(),
        project_id: "project-1".to_owned(),
        baseline_version: 4,
        metric: "retention".to_owned(),
        unit: "percent".to_owned(),
        direction: MetricDirection::AtLeast,
        baseline: 10.0,
        target: 20.0,
        dataset_id: "dataset-1".to_owned(),
        method: "cohort mean".to_owned(),
        source_ref: "dataset:retention-v1".to_owned(),
        owner_id: "owner-1".to_owned(),
        window_start: 10,
        window_end: 20,
        minimum_samples,
        aggregation: OutcomeAggregation::Mean,
        fixture: false,
        digest: String::new(),
    };
    value.digest = value.canonical_digest();
    value
}

fn observation(id: &str, value: f64, observed_at: u64) -> OutcomeMeasurementObservation {
    let mut item = OutcomeMeasurementObservation {
        schema: OUTCOME_MEASUREMENT_SCHEMA.to_owned(),
        observation_id: id.to_owned(),
        plan_id: "plan-1".to_owned(),
        project_id: "project-1".to_owned(),
        dataset_id: "dataset-1".to_owned(),
        metric: "retention".to_owned(),
        unit: "percent".to_owned(),
        value,
        observed_at,
        recorded_at: 30,
        source_ref: "dataset:retention-v1".to_owned(),
        evidence_refs: vec![format!("artifact:{id}")],
        fixture: false,
        digest: String::new(),
    };
    item.digest = item.canonical_digest();
    item
}

fn decision(assessment: &OutcomeAssessment) -> OutcomeDecision {
    let mut item = OutcomeDecision {
        schema: OUTCOME_MEASUREMENT_SCHEMA.to_owned(),
        decision_id: "decision-1".to_owned(),
        assessment_id: assessment.assessment_id.clone(),
        objective_id: "objective-1".to_owned(),
        assessment_digest: assessment.digest.clone(),
        decision: OutcomeDecisionKind::Achieved,
        decided_by: "sponsor-1".to_owned(),
        decider_role: "sponsor".to_owned(),
        owner_id: "owner-1".to_owned(),
        decision_ref: "decision:evidence-1".to_owned(),
        decided_at: 40,
        digest: String::new(),
    };
    item.digest = item.canonical_digest();
    item
}

#[test]
fn objective_cannot_be_achieved_from_delivery_tests_or_cherry_picked_observations() {
    let mut ledger = OutcomeMeasurementLedger::default();
    ledger.publish_plan(plan(2)).expect("plan");
    ledger
        .record_observation(observation("obs-1", 21.0, 11))
        .expect("obs one");
    let assessment = ledger.assess("plan-1", "assessment-1", 21).unwrap();
    assert_eq!(assessment.status, OutcomeAssessmentStatus::MissingData);
    assert_eq!(
        ledger.decide(decision(&assessment)).unwrap_err(),
        "outcome_achieved_requires_realized_assessment"
    );

    ledger
        .record_observation(observation("obs-2", 19.0, 12))
        .expect("obs two");
    let realized = ledger.assess("plan-1", "assessment-2", 22).unwrap();
    assert_eq!(realized.status, OutcomeAssessmentStatus::Realized);
    let mut cherry = decision(&realized);
    cherry.assessment_digest = assessment.digest;
    cherry.digest = cherry.canonical_digest();
    assert_eq!(
        ledger.decide(cherry).unwrap_err(),
        "outcome_decision_binding_or_independence_invalid"
    );
}

#[test]
fn unit_window_nan_and_missing_samples_are_rejected_or_marked_missing() {
    let mut ledger = OutcomeMeasurementLedger::default();
    ledger.publish_plan(plan(2)).expect("plan");
    let mut wrong_unit = observation("obs-unit", 20.0, 11);
    wrong_unit.unit = "count".to_owned();
    wrong_unit.digest = wrong_unit.canonical_digest();
    assert_eq!(
        ledger.record_observation(wrong_unit).unwrap_err(),
        "outcome_observation_binding_or_window_invalid"
    );
    let mut outside = observation("obs-outside", 20.0, 21);
    outside.digest = outside.canonical_digest();
    assert_eq!(
        ledger.record_observation(outside).unwrap_err(),
        "outcome_observation_binding_or_window_invalid"
    );
    let mut nan = observation("obs-nan", f64::NAN, 11);
    assert_eq!(
        ledger.record_observation(nan).unwrap_err(),
        "outcome_observation_binding_or_window_invalid"
    );
    ledger
        .record_observation(observation("obs-one", 20.0, 11))
        .expect("one");
    let missing = ledger.assess("plan-1", "assessment-missing", 21).unwrap();
    assert_eq!(missing.status, OutcomeAssessmentStatus::MissingData);
    assert!(missing.missing_data);
}

#[test]
fn assessment_replays_from_frozen_rules_and_sponsor_decision_is_independent() {
    let mut ledger = OutcomeMeasurementLedger::default();
    ledger.publish_plan(plan(2)).expect("plan");
    ledger
        .record_observation(observation("obs-1", 21.0, 11))
        .expect("one");
    ledger
        .record_observation(observation("obs-2", 23.0, 12))
        .expect("two");
    let first = ledger.assess("plan-1", "assessment-1", 21).unwrap();
    let replay = ledger.assess("plan-1", "assessment-1", 21).unwrap();
    assert_eq!(first, replay);
    assert_eq!(first.status, OutcomeAssessmentStatus::Realized);
    let decision = decision(&first);
    ledger.decide(decision.clone()).expect("decision");
    ledger.decide(decision).expect("idempotent decision");

    let mut self_decision = decision(&first);
    self_decision.decided_by = "owner-1".to_owned();
    self_decision.digest = self_decision.canonical_digest();
    assert_eq!(
        ledger.decide(self_decision).unwrap_err(),
        "outcome_decision_binding_or_independence_invalid"
    );
}

#[test]
fn outcome_measurement_keeps_frozen_rules_and_sponsor_decision_separate_from_delivery() {
    let measurement = include_str!("../../kiana-domain/src/outcome_measurement.rs");
    let company = include_str!("../../kiana-domain/src/company.rs");
    let closeout = include_str!("../../kiana-domain/src/company_closeout.rs");
    let core = include_str!("../src/outcome_measurement.rs");
    for marker in [
        "OUTCOME_MEASUREMENT_SCHEMA",
        "OutcomeMeasurementPlan",
        "OutcomeMeasurementObservation",
        "OutcomeAssessment",
        "OutcomeDecision",
        "OutcomeAggregation",
        "OutcomeAssessmentStatus",
        "MetricDirection",
        "window_start",
        "window_end",
        "minimum_samples",
        "fixture",
        "outcome_achieved_requires_realized_assessment",
        "outcome_decision_binding_or_independence_invalid",
        "outcome_observation_binding_or_window_invalid",
        "PlanMeasurement",
        "AssessOutcome",
        "AchieveObjective",
        "Delivery",
        "publish_outcome_plan",
        "decide_outcome",
    ] {
        assert!(
            measurement.contains(marker)
                || company.contains(marker)
                || closeout.contains(marker)
                || core.contains(marker),
            "CO-36 marker missing: {marker}"
        );
    }
    for forbidden in [
        "delivery_tests_are_outcomes",
        "model_claimed_outcome",
        "Command::new",
    ] {
        assert!(
            !measurement.contains(forbidden),
            "CO-36 bypass marker present: {forbidden}"
        );
    }
}

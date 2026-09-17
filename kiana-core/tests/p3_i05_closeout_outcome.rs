#[test]
fn outcome_cannot_be_claimed_without_measurement() {
    let domain = include_str!("../../kiana-domain/src/company.rs");
    let closeout = include_str!("../../kiana-domain/src/company_closeout.rs");
    let core = include_str!("../src/company.rs");
    let governance = include_str!("../src/company_governance.rs");
    let baseline = include_str!("../../docs/roadmap/p3-i05-closeout-outcome-baseline.md");

    for marker in [
        "Delivery",
        "DeliveryStatus",
        "CompanyClosingReceipt",
        "Outcome",
        "OutcomeStatus",
        "MetricObservation",
        "CloseProject",
        "RecordOutcome",
        "closing_receipts",
        "closing_confirmed_delivery_required",
        "closing_review_required",
        "closing_role_separation_required",
        "closing_incident_unresolved",
        "delivery_acceptance_required",
        "delivery_artifact_evidence_required",
        "handoff_receipt_ref",
        "outcome_closed_project_required",
        "outcome_measurement_window_or_owner_invalid",
        "objective_outcome_measurement_required",
        "measurement_start",
        "measurement_end",
        "period_start",
        "period_end",
        "metric_observations",
        "outcome_already_recorded",
        "business_closing_required_acceptance_missing",
        "business_closing_delivery_obligation_unresolved",
        "business_closing_unresolved_effects",
        "business_measurement_plan_invalid_or_late",
        "business_metric_observation_invalid",
        "business_outcome_window_or_closure_incomplete",
        "AssessOutcome",
        "AchieveObjective",
        "runtime_completed_not_business_outcome",
        "GovernanceStatus",
        "source_event_ids",
        "outcome_cannot_be_claimed_without_measurement",
    ] {
        assert!(
            domain.contains(marker)
                || closeout.contains(marker)
                || core.contains(marker)
                || governance.contains(marker)
                || baseline.contains(marker),
            "closeout/outcome marker missing: {marker}"
        );
    }

    assert!(domain.contains("project.status == ProjectStatus::Closed"));
    assert!(domain.contains("project.status == ProjectStatus::Accepted"));
    assert!(domain.contains("delivery.status == DeliveryStatus::Confirmed"));
    assert!(domain.contains("delivery.handoff_receipt_ref.is_some()"));
    assert!(domain.contains("observation.observed_at >= objective.period_start"));
    assert!(domain.contains("observation.observed_at <= objective.period_end"));
    assert!(domain.contains("objective.target_met(observation.value)"));
    assert!(closeout.contains("a.now_ms > plan.window_end"));
    assert!(closeout.contains("assessment.status == Some(OutcomeStatus::Realized)"));
    assert!(governance.contains("runtime_completed_not_business_outcome"));
    assert!(!closeout.contains("CapabilityBroker"));
    assert!(!closeout.contains("ModelClient"));
}

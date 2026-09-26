use kiana_domain::*;
use serde_json::json;
use std::collections::BTreeMap;

fn objective() -> Objective {
    Objective {
        objective_id: "objective-1".to_owned(),
        organization_id: "org-1".to_owned(),
        title: "Ship the product".to_owned(),
        problem: "The product is unfinished".to_owned(),
        metric: "delivery_rate".to_owned(),
        baseline: 0.0,
        target: 1.0,
        unit: "ratio".to_owned(),
        measurement_method: Some("release acceptance ratio".to_owned()),
        direction: MetricDirection::AtLeast,
        period_start: 1,
        period_end: 100,
        owner_principal_id: "sponsor-1".to_owned(),
        status: ObjectiveStatus::Proposed,
        version: 1,
    }
}

fn criteria() -> CriteriaSnapshot {
    CriteriaSnapshot {
        project_version: 1,
        milestone_version: 1,
        packet_version: 1,
        milestone_versions: BTreeMap::new(),
        packet_versions: BTreeMap::new(),
        project_criteria: vec!["criterion".to_owned()],
        milestone_criteria: Vec::new(),
        packet_criteria: Vec::new(),
        criterion_refs: Vec::new(),
    }
}

#[test]
fn company_objects_expose_invariants() {
    let objective = objective();
    objective.validate().unwrap();
    assert!(objective.target_met(1.0));

    let initiative = Initiative {
        initiative_id: "initiative-1".to_owned(),
        organization_id: "org-1".to_owned(),
        objective_refs: vec![objective.objective_id.clone()],
        title: "Ship it".to_owned(),
        problem_statement: "No release".to_owned(),
        hypothesis: "A bounded plan helps".to_owned(),
        sponsor_id: "sponsor-1".to_owned(),
        expected_value: "value".to_owned(),
        rough_cost: "small".to_owned(),
        risk_summary: "low".to_owned(),
        decision: None,
        project_id: None,
        status: InitiativeStatus::Intake,
        version: 1,
    };
    initiative.validate().unwrap();

    let project = Project {
        project_id: "project-1".to_owned(),
        organization_id: "org-1".to_owned(),
        objective_refs: vec![objective.objective_id.clone()],
        sponsor_id: "sponsor-1".to_owned(),
        charter_ref: "artifact:charter".to_owned(),
        scope_baseline: "scope-v1".to_owned(),
        success_criteria: vec!["criterion".to_owned()],
        non_goals: vec!["none".to_owned()],
        project_budget_ref: "budget-1".to_owned(),
        risk_summary: "low".to_owned(),
        decision_ref: None,
        milestone_refs: Vec::new(),
        incident_id: None,
        acceptance_id: None,
        closing_receipt_id: None,
        status: ProjectStatus::Proposed,
        version: 1,
    };
    project.validate().unwrap();

    let milestone = Milestone {
        milestone_id: "milestone-1".to_owned(),
        project_id: project.project_id.clone(),
        objective_refs: vec![objective.objective_id.clone()],
        deliverables: vec!["artifact".to_owned()],
        acceptance_criteria: vec!["criterion".to_owned()],
        due_at: 100,
        dependency_refs: Vec::new(),
        status: MilestoneStatus::Planned,
        version: 1,
    };
    milestone.validate().unwrap();

    let run_id = RunId::new();
    let acceptance = Acceptance {
        acceptance_id: "acceptance-1".to_owned(),
        project_id: project.project_id.clone(),
        milestone_id: milestone.milestone_id.clone(),
        work_packet_id: "packet-1".to_owned(),
        criteria_snapshot: criteria(),
        evidence_refs: vec!["event:run".to_owned()],
        author_run_id: run_id,
        author_session_id: SessionId::new("builder-session"),
        reviewer_id: None,
        decision_maker_id: None,
        decision: None,
        decided_at: None,
        rejection_reasons: Vec::new(),
        status: AcceptanceStatus::Requested,
        version: 1,
    };
    acceptance.validate().unwrap();

    let review = CompanyReview {
        review_id: "review-1".to_owned(),
        acceptance_id: acceptance.acceptance_id.clone(),
        author_run_id: run_id,
        reviewer_session_id: SessionId::new("reviewer-session"),
        reviewer_id: "reviewer-1".to_owned(),
        criteria_snapshot: criteria(),
        criterion_results: BTreeMap::from([("criterion".to_owned(), true)]),
        evidence_refs: acceptance.evidence_refs.clone(),
        recorded_at: 10,
    };
    review.validate().unwrap();

    let delivery = Delivery {
        delivery_id: "delivery-1".to_owned(),
        project_id: project.project_id.clone(),
        acceptance_id: acceptance.acceptance_id.clone(),
        artifact_refs: vec!["artifact:output".to_owned()],
        recipient_ref: "operator".to_owned(),
        handoff_receipt_ref: None,
        delivered_at: None,
        incident_id: None,
        status: DeliveryStatus::Prepared,
        version: 1,
    };
    delivery.validate().unwrap();

    let observation = MetricObservation {
        value: 1.0,
        observed_at: 50,
        evidence_refs: vec!["event:measurement".to_owned()],
    };
    observation.validate().unwrap();
    let outcome = Outcome {
        outcome_id: "outcome-1".to_owned(),
        objective_id: objective.objective_id.clone(),
        project_id: project.project_id.clone(),
        metric_observations: vec![observation],
        target_snapshot: objective.clone(),
        measurement_start: 1,
        measurement_end: 100,
        owner_id: "sponsor-1".to_owned(),
        review_at: 100,
        evidence_refs: vec!["event:measurement".to_owned()],
        status: OutcomeStatus::Planned,
    };
    outcome.validate().unwrap();

    let change = ChangeRequest {
        change_id: "change-1".to_owned(),
        project_id: project.project_id.clone(),
        requested_by: "sponsor-1".to_owned(),
        reason: "clarify scope".to_owned(),
        affected_scope: vec!["src".to_owned()],
        affected_objectives: vec![objective.objective_id.clone()],
        affected_budget: "budget-1".to_owned(),
        affected_schedule: "schedule-1".to_owned(),
        affected_risk: "risk-1".to_owned(),
        proposed_baseline_version: 2,
        proposed_scope_baseline: "scope-v2".to_owned(),
        proposed_success_criteria: vec!["criterion".to_owned()],
        decision: None,
        status: ChangeStatus::Draft,
        version: 1,
    };
    change.validate().unwrap();

    let risk = Risk {
        risk_id: "risk-1".to_owned(),
        project_id: project.project_id.clone(),
        description: "provider unavailable".to_owned(),
        probability: 0.2,
        impact: "delay".to_owned(),
        trigger: "timeout".to_owned(),
        mitigation: "use cassette".to_owned(),
        contingency: "pause".to_owned(),
        owner_id: "sponsor-1".to_owned(),
        incident_id: None,
        status: RiskStatus::Identified,
    };
    risk.validate().unwrap();

    let incident = Incident {
        incident_id: "incident-1".to_owned(),
        project_id: Some(project.project_id),
        run_id: Some(run_id),
        risk_id: Some(risk.risk_id),
        delivery_id: Some(delivery.delivery_id),
        severity: "error".to_owned(),
        detected_at: 60,
        impact: "blocked".to_owned(),
        timeline: vec!["opened".to_owned()],
        owner_id: "sponsor-1".to_owned(),
        response_actions: vec!["inspect".to_owned()],
        evidence_refs: vec!["event:incident".to_owned()],
        escalation_target: None,
        status: IncidentStatus::Open,
    };
    incident.validate().unwrap();

    let mut encoded = serde_json::to_value(&acceptance).unwrap();
    encoded["unexpected"] = json!(true);
    assert!(serde_json::from_value::<Acceptance>(encoded).is_err());
}

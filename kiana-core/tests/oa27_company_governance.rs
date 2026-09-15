use kiana_core::project_company_governance;
use kiana_domain::{
    Acceptance, AcceptanceStatus, CompanyClosingReceipt, CompanyReview, CompanyRun, CompanyState,
    CriteriaSnapshot, Delivery, DeliveryStatus, EventId, ExecutionStatus, Project, ProjectStatus,
    RequestId, RunId, SessionId,
};
use serde_json::json;
use std::collections::BTreeMap;

fn project(project_id: &str, status: ProjectStatus) -> Project {
    Project {
        project_id: project_id.to_owned(),
        organization_id: "org-1".to_owned(),
        objective_refs: vec!["objective-1".to_owned()],
        sponsor_id: "sponsor-1".to_owned(),
        charter_ref: "artifact:charter".to_owned(),
        scope_baseline: "scope-v1".to_owned(),
        success_criteria: vec!["criterion".to_owned()],
        non_goals: vec!["none".to_owned()],
        project_budget_ref: "budget-1".to_owned(),
        risk_summary: "none".to_owned(),
        decision_ref: Some("decision-1".to_owned()),
        milestone_refs: Vec::new(),
        incident_id: None,
        acceptance_id: Some("acceptance-1".to_owned()),
        closing_receipt_id: Some("closing-1".to_owned()),
        status,
        version: 1,
    }
}

fn completed_run() -> CompanyRun {
    CompanyRun {
        packet_id: "packet-1".to_owned(),
        project_id: "project-1".to_owned(),
        execution_request_id: RequestId::new(),
        author_session_id: SessionId::new("builder-session"),
        run_id: Some(RunId::new()),
        status: ExecutionStatus::Completed,
        evidence_refs: vec!["event:run".to_owned()],
        incident_id: None,
    }
}

fn source() -> Vec<EventId> {
    vec![EventId::new(), EventId::new()]
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
    }
}

#[test]
fn runtime_completed_is_not_a_business_outcome() {
    let mut state = CompanyState::default();
    state.projects.insert(
        "project-1".to_owned(),
        project("project-1", ProjectStatus::Active),
    );
    state.runs.insert("packet-1".to_owned(), completed_run());
    let snapshot = project_company_governance(&state, "project-1", source()).unwrap();
    assert_eq!(snapshot.status, kiana_domain::GovernanceStatus::InProgress);
    assert!(snapshot.outcome_ids.is_empty());
    assert!(snapshot
        .limitations
        .iter()
        .any(|item| item == "runtime_completed_not_business_outcome"));
}

#[test]
fn closed_chain_requires_independent_review_confirmed_delivery_and_closer() {
    let mut state = CompanyState::default();
    state.projects.insert(
        "project-1".to_owned(),
        project("project-1", ProjectStatus::Closed),
    );
    let run = completed_run();
    let run_id = run.run_id.unwrap();
    state.runs.insert("packet-1".to_owned(), run);
    state.acceptances.insert(
        "acceptance-1".to_owned(),
        Acceptance {
            acceptance_id: "acceptance-1".to_owned(),
            project_id: "project-1".to_owned(),
            milestone_id: "milestone-1".to_owned(),
            work_packet_id: "packet-1".to_owned(),
            criteria_snapshot: criteria(),
            evidence_refs: vec!["event:run".to_owned()],
            author_run_id: run_id,
            author_session_id: SessionId::new("builder-session"),
            reviewer_id: Some("reviewer-1".to_owned()),
            decision_maker_id: Some("reviewer-1".to_owned()),
            decision: Some(kiana_domain::AcceptanceDecision::Accept),
            decided_at: Some(10),
            rejection_reasons: Vec::new(),
            status: AcceptanceStatus::Accepted,
            version: 2,
        },
    );
    state.reviews.insert(
        "review-1".to_owned(),
        CompanyReview {
            review_id: "review-1".to_owned(),
            acceptance_id: "acceptance-1".to_owned(),
            author_run_id: run_id,
            reviewer_session_id: SessionId::new("reviewer-session"),
            reviewer_id: "reviewer-1".to_owned(),
            criteria_snapshot: criteria(),
            criterion_results: BTreeMap::from([("criterion".to_owned(), true)]),
            evidence_refs: vec!["event:run".to_owned()],
            recorded_at: 11,
        },
    );
    state.deliveries.insert(
        "delivery-1".to_owned(),
        Delivery {
            delivery_id: "delivery-1".to_owned(),
            project_id: "project-1".to_owned(),
            acceptance_id: "acceptance-1".to_owned(),
            artifact_refs: vec!["artifact:output".to_owned()],
            recipient_ref: "recipient-1".to_owned(),
            handoff_receipt_ref: Some("artifact:handoff".to_owned()),
            delivered_at: Some(12),
            incident_id: None,
            status: DeliveryStatus::Confirmed,
            version: 3,
        },
    );
    state.closing_receipts.insert(
        "closing-1".to_owned(),
        CompanyClosingReceipt {
            schema: "kiana.company-closing-receipt.v1".to_owned(),
            receipt_id: "closing-1".to_owned(),
            project_id: "project-1".to_owned(),
            acceptance_id: "acceptance-1".to_owned(),
            delivery_id: "delivery-1".to_owned(),
            review_id: "review-1".to_owned(),
            author_run_id: run_id,
            author_session_id: SessionId::new("builder-session"),
            reviewer_session_id: SessionId::new("reviewer-session"),
            closer_session_id: SessionId::new("closer-session"),
            artifact_refs: vec!["artifact:output".to_owned()],
            evidence_refs: vec!["event:run".to_owned()],
            closed_at: 13,
            waiver_ref: None,
        },
    );
    let snapshot = project_company_governance(&state, "project-1", source()).unwrap();
    assert_eq!(snapshot.status, kiana_domain::GovernanceStatus::Closed);
    snapshot.validate().unwrap();
    let mut encoded = serde_json::to_value(&snapshot).unwrap();
    encoded["unexpected"] = json!(true);
    assert!(serde_json::from_value::<kiana_domain::CompanyGovernanceSnapshot>(encoded).is_err());
}

#[test]
fn forged_closed_chain_is_unknown_with_bounded_limitation() {
    let mut state = CompanyState::default();
    state.projects.insert(
        "project-1".to_owned(),
        project("project-1", ProjectStatus::Closed),
    );
    let snapshot = project_company_governance(&state, "project-1", source()).unwrap();
    assert_eq!(snapshot.status, kiana_domain::GovernanceStatus::Unknown);
    assert!(snapshot
        .limitations
        .iter()
        .any(|item| item == "closing_chain_incomplete"));
}

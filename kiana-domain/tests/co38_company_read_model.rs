use kiana_domain::*;
use std::collections::BTreeSet;

fn project(id: &str) -> Project {
    Project {
        project_id: id.to_owned(),
        organization_id: "org-1".to_owned(),
        objective_refs: vec!["objective-1".to_owned()],
        sponsor_id: "sponsor-1".to_owned(),
        charter_ref: "artifact:charter".to_owned(),
        scope_baseline: "baseline".to_owned(),
        success_criteria: vec!["criterion-1".to_owned()],
        non_goals: vec!["non-goal".to_owned()],
        project_budget_ref: "budget-1".to_owned(),
        risk_summary: "low".to_owned(),
        decision_ref: None,
        milestone_refs: Vec::new(),
        incident_id: None,
        acceptance_id: None,
        closing_receipt_id: None,
        status: ProjectStatus::Active,
        version: 1,
    }
}

#[test]
fn company_projection_never_leaks_foreign_project_or_advances_from_chat() {
    let mut state = CompanyState::default();
    state
        .projects
        .insert("project-1".to_owned(), project("project-1"));
    state
        .projects
        .insert("project-2".to_owned(), project("project-2"));
    let authorized = BTreeSet::from(["project-1".to_owned()]);
    assert_eq!(
        project_read_model(&state, "project-2", &authorized, 10, Some(10), 1, 1).unwrap_err(),
        "company_view_scope_denied"
    );
    let snapshot =
        project_read_model(&state, "project-1", &authorized, 10, Some(9), 1, 1).expect("snapshot");
    assert_eq!(snapshot.freshness, CompanyReadModelFreshness::Pending);
    assert_eq!(snapshot.view.project_status, ProjectStatus::Active);
    assert!(snapshot.view.blockers.is_empty());
}

#[test]
fn company_view_rebuilds_with_the_same_blockers_actions_and_evidence_links() {
    let mut state = CompanyState::default();
    state
        .projects
        .insert("project-1".to_owned(), project("project-1"));
    let authorized = BTreeSet::from(["project-1".to_owned()]);
    let first =
        project_read_model(&state, "project-1", &authorized, 10, Some(10), 2, 3).expect("first");
    let second =
        project_read_model(&state, "project-1", &authorized, 10, Some(10), 2, 3).expect("second");
    assert_eq!(first, second);
    assert_eq!(first.snapshot_digest, second.snapshot_digest);
}

#[test]
fn stale_or_invalid_cursor_and_revision_are_rejected() {
    let view = CompanyProjectView {
        project_id: "project-1".to_owned(),
        project_status: ProjectStatus::Active,
        baseline_version: Some(1),
        packet_views: Vec::new(),
        milestone_ids: Vec::new(),
        acceptance_status: None,
        delivery_status: None,
        closing_receipt_id: None,
        blockers: Vec::new(),
        evidence_links: Vec::new(),
    };
    assert_eq!(
        CompanyReadModelSnapshot::new("project-1", 10, Some(11), 1, 1, view.clone()).unwrap_err(),
        "company_view_snapshot_header_invalid"
    );
    assert_eq!(
        CompanyReadModelSnapshot::new("project-1", 10, Some(10), 0, 1, view).unwrap_err(),
        "company_view_snapshot_header_invalid"
    );
}

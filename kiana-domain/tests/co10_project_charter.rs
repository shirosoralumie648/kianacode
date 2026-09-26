use kiana_domain::*;

fn authority() -> CompanyAuthority {
    CompanyAuthority {
        actor_id: "sponsor-1".to_owned(),
        role_id: "sponsor".to_owned(),
        session_id: SessionId::new("co10-session"),
        now_ms: 1_000,
        execution_request_id: RequestId::new(),
        execution_cell_id: None,
    }
}

fn objective() -> Objective {
    Objective {
        objective_id: "objective-1".to_owned(),
        organization_id: "org-1".to_owned(),
        title: "Ship governed output".to_owned(),
        problem: "delivery is incomplete".to_owned(),
        metric: "delivery_rate".to_owned(),
        baseline: 0.0,
        target: 1.0,
        unit: "ratio".to_owned(),
        measurement_method: Some("acceptance observation".to_owned()),
        direction: MetricDirection::AtLeast,
        period_start: 1,
        period_end: 100,
        owner_principal_id: "sponsor-1".to_owned(),
        status: ObjectiveStatus::Active,
        version: 2,
    }
}

fn project() -> Project {
    Project {
        project_id: "project-1".to_owned(),
        organization_id: "org-1".to_owned(),
        objective_refs: vec!["objective-1".to_owned()],
        sponsor_id: "sponsor-1".to_owned(),
        charter_ref: "artifact:charter".to_owned(),
        scope_baseline: "scope-v1".to_owned(),
        success_criteria: vec!["criterion".to_owned()],
        non_goals: vec!["external publish".to_owned()],
        project_budget_ref: "budget-1".to_owned(),
        risk_summary: "bounded".to_owned(),
        decision_ref: None,
        milestone_refs: Vec::new(),
        incident_id: None,
        acceptance_id: None,
        closing_receipt_id: None,
        status: ProjectStatus::Proposed,
        version: 1,
    }
}

fn proof() -> CompanyProof {
    CompanyProof {
        events: vec![
            "artifact:charter".to_owned(),
            "artifact:decision".to_owned(),
        ],
        ..CompanyProof::default()
    }
}

fn budget() -> CompanyBudgetPolicy {
    CompanyBudgetPolicy {
        project: ProjectBudget {
            project_id: ProjectId::new(),
            max_runs: 4,
            max_tokens: 100_000,
        },
        runtime: RuntimeBudget {
            max_model_calls: 8,
            max_tokens: 100_000,
            max_wall_time_ms: 60_000,
        },
        quota: Quota {
            scope: "project-1".to_owned(),
            model_calls: 8,
            tokens: 100_000,
            concurrency: 1,
        },
    }
}

fn state() -> CompanyState {
    let mut state = CompanyState::default();
    state
        .objectives
        .insert("objective-1".to_owned(), objective());
    state.projects.insert("project-1".to_owned(), project());
    state.artifacts.insert(
        "charter".to_owned(),
        CompanyArtifact {
            artifact_id: "charter".to_owned(),
            relative_path: "charter.json".to_owned(),
            text: "immutable charter".to_owned(),
            registered_at: 1,
            typed_version: None,
        },
    );
    state.artifacts.insert(
        "decision".to_owned(),
        CompanyArtifact {
            artifact_id: "decision".to_owned(),
            relative_path: "decision.json".to_owned(),
            text: "sponsor decision".to_owned(),
            registered_at: 1,
            typed_version: None,
        },
    );
    state.charter_digests.insert(
        "project-1".to_owned(),
        json_digest(&serde_json::json!({
            "content_hash": journal_sha256(b"immutable charter"),
            "typed_version": null,
        })),
    );
    state
}

#[test]
fn project_approval_requires_charter_and_existing_budget() {
    let authority = authority();
    let mut chartering = state();
    chartering.projects.get_mut("project-1").unwrap().status = ProjectStatus::Chartering;
    assert_eq!(
        chartering
            .transition(
                &CompanyCommand::ApproveProject {
                    project_id: "project-1".to_owned(),
                    decision_ref: "artifact:decision".to_owned(),
                },
                &authority,
                &proof(),
            )
            .unwrap_err(),
        "project_budget_required"
    );

    let budgeted = state()
        .transition(
            &CompanyCommand::ConfigureBudget {
                project_id: "project-1".to_owned(),
                policy: budget(),
            },
            &authority,
            &proof(),
        )
        .expect("budget");
    let budgeted = budgeted
        .transition(
            &CompanyCommand::StartChartering {
                project_id: "project-1".to_owned(),
            },
            &authority,
            &proof(),
        )
        .expect("chartering");
    let mut changed_charter = budgeted.clone();
    changed_charter.artifacts.get_mut("charter").unwrap().text = "changed charter".to_owned();
    assert_eq!(
        changed_charter
            .transition(
                &CompanyCommand::ApproveProject {
                    project_id: "project-1".to_owned(),
                    decision_ref: "artifact:decision".to_owned(),
                },
                &authority,
                &proof(),
            )
            .unwrap_err(),
        "project_charter_changed"
    );
}

#[test]
fn project_approval_rechecks_objective_activity_after_chartering() {
    let authority = authority();
    let mut changed = state();
    changed.objectives.get_mut("objective-1").unwrap().status = ObjectiveStatus::Paused;
    changed.projects.get_mut("project-1").unwrap().status = ProjectStatus::Chartering;
    changed.budgets.insert("project-1".to_owned(), budget());
    assert_eq!(
        changed
            .transition(
                &CompanyCommand::ApproveProject {
                    project_id: "project-1".to_owned(),
                    decision_ref: "artifact:decision".to_owned(),
                },
                &authority,
                &proof(),
            )
            .unwrap_err(),
        "project_objective_not_active"
    );
}

#[test]
fn configured_budget_and_frozen_charter_enable_go_no_go_once() {
    let authority = authority();
    let state = state();
    let state = state
        .transition(
            &CompanyCommand::ConfigureBudget {
                project_id: "project-1".to_owned(),
                policy: budget(),
            },
            &authority,
            &proof(),
        )
        .expect("budget");
    let state = state
        .transition(
            &CompanyCommand::StartChartering {
                project_id: "project-1".to_owned(),
            },
            &authority,
            &proof(),
        )
        .expect("chartering");
    let approved = state
        .transition(
            &CompanyCommand::ApproveProject {
                project_id: "project-1".to_owned(),
                decision_ref: "artifact:decision".to_owned(),
            },
            &authority,
            &proof(),
        )
        .expect("approved");
    assert_eq!(
        approved.projects["project-1"].status,
        ProjectStatus::Approved
    );
    assert!(approved.budgets.contains_key("project-1"));
    assert_eq!(approved.projects["project-1"].version, 3);
    let baseline = &approved.charter_baselines["project-1"];
    assert_eq!(baseline.version, 1);
    assert_eq!(baseline.scope_baseline, "scope-v1");
    assert_eq!(baseline.success_criteria, vec!["criterion"]);
    assert_eq!(
        approved
            .transition(
                &CompanyCommand::StartChartering {
                    project_id: "project-1".to_owned(),
                },
                &authority,
                &proof(),
            )
            .unwrap_err(),
        "company_illegal_state_transition"
    );
}

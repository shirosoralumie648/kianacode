use kiana_domain::{
    CompanyAuthority, CompanyCommand, CompanyProof, CompanyState, Initiative, InitiativeStatus,
    MetricDirection, Objective, ObjectiveStatus, RequestId, SessionId,
};

fn authority(actor_id: &str) -> CompanyAuthority {
    CompanyAuthority {
        actor_id: actor_id.to_owned(),
        role_id: "sponsor".to_owned(),
        session_id: SessionId::new("co09-session"),
        now_ms: 1_000,
        execution_request_id: RequestId::new(),
        execution_cell_id: None,
    }
}

fn objective(
    id: &str,
    organization_id: &str,
    owner: &str,
    measurement_method: Option<&str>,
) -> Objective {
    Objective {
        objective_id: id.to_owned(),
        organization_id: organization_id.to_owned(),
        title: "Improve delivery".to_owned(),
        problem: "delivery is slow".to_owned(),
        metric: "delivery_rate".to_owned(),
        baseline: 0.2,
        target: 0.8,
        unit: "ratio".to_owned(),
        measurement_method: measurement_method.map(str::to_owned),
        direction: MetricDirection::AtLeast,
        period_start: 1,
        period_end: 100,
        owner_principal_id: owner.to_owned(),
        status: ObjectiveStatus::Proposed,
        version: 1,
    }
}

fn initiative(
    id: &str,
    organization_id: &str,
    objective_refs: Vec<String>,
    sponsor_id: &str,
) -> Initiative {
    Initiative {
        initiative_id: id.to_owned(),
        organization_id: organization_id.to_owned(),
        objective_refs,
        title: "Ship a bounded improvement".to_owned(),
        problem_statement: "delivery lacks focus".to_owned(),
        hypothesis: "a focused initiative improves the metric".to_owned(),
        sponsor_id: sponsor_id.to_owned(),
        expected_value: "higher delivery rate".to_owned(),
        rough_cost: "small".to_owned(),
        risk_summary: "low".to_owned(),
        decision: None,
        project_id: None,
        status: InitiativeStatus::Intake,
        version: 1,
    }
}

#[test]
fn objective_approval_requires_owner_and_measurement_method() {
    let sponsor = authority("sponsor-1");
    let mut state = CompanyState::default();
    state = state
        .transition(
            &CompanyCommand::ProposeObjective {
                objective: objective("objective-missing-method", "org-1", "sponsor-1", None),
            },
            &sponsor,
            &CompanyProof::default(),
        )
        .expect("proposal");
    assert_eq!(
        state
            .transition(
                &CompanyCommand::DecideObjective {
                    objective_id: "objective-missing-method".to_owned(),
                    approve: true,
                },
                &sponsor,
                &CompanyProof::default(),
            )
            .unwrap_err(),
        "objective_measurement_method_required"
    );

    let other = authority("other-owner");
    assert_eq!(
        state
            .transition(
                &CompanyCommand::DecideObjective {
                    objective_id: "objective-missing-method".to_owned(),
                    approve: false,
                },
                &other,
                &CompanyProof::default(),
            )
            .unwrap_err(),
        "objective_owner_mismatch"
    );
}

#[test]
fn initiative_requires_sponsor_and_same_organization_objectives() {
    let sponsor = authority("sponsor-1");
    let mut state = CompanyState::default();
    state = state
        .transition(
            &CompanyCommand::ProposeObjective {
                objective: objective("objective-1", "org-1", "sponsor-1", Some("weekly sample")),
            },
            &sponsor,
            &CompanyProof::default(),
        )
        .expect("objective");
    state = state
        .transition(
            &CompanyCommand::DecideObjective {
                objective_id: "objective-1".to_owned(),
                approve: true,
            },
            &sponsor,
            &CompanyProof::default(),
        )
        .expect("objective approval");

    assert_eq!(
        state
            .transition(
                &CompanyCommand::SubmitInitiative {
                    initiative: initiative(
                        "initiative-wrong-sponsor",
                        "org-1",
                        vec!["objective-1".to_owned()],
                        "other-sponsor",
                    ),
                },
                &sponsor,
                &CompanyProof::default(),
            )
            .unwrap_err(),
        "initiative_initial_state_invalid"
    );

    let mut foreign_state = state.clone();
    foreign_state = foreign_state
        .transition(
            &CompanyCommand::ProposeObjective {
                objective: objective(
                    "objective-foreign",
                    "org-2",
                    "sponsor-1",
                    Some("daily sample"),
                ),
            },
            &sponsor,
            &CompanyProof::default(),
        )
        .expect("foreign objective");
    assert_eq!(
        foreign_state
            .transition(
                &CompanyCommand::SubmitInitiative {
                    initiative: initiative(
                        "initiative-cross-org",
                        "org-1",
                        vec!["objective-foreign".to_owned()],
                        "sponsor-1",
                    ),
                },
                &sponsor,
                &CompanyProof::default(),
            )
            .unwrap_err(),
        "initiative_objective_missing"
    );
}

#[test]
fn objective_measurement_direction_rejects_nan_or_non_improving_target() {
    let mut invalid = objective("bad", "org-1", "sponsor-1", Some("sample"));
    invalid.target = invalid.baseline;
    assert_eq!(
        invalid.validate().unwrap_err(),
        "objective_measurement_invalid"
    );
    invalid.target = f64::NAN;
    assert_eq!(
        invalid.validate().unwrap_err(),
        "objective_measurement_invalid"
    );
}

//! CO-09 Objective and Initiative intake fixtures.
//!
//! Deny paths come first: a NaN or non-improving measurement, an inverted time window, a wrong
//! owner/organization, a missing measurement method, an Initiative that loses its approved
//! objective or sponsor decision, and a direct project intake that drops the objective ancestry
//! are all refused before the success paths convert an approved Initiative exactly once and
//! record an explicit direct-intake entry.

use kiana_domain::*;

fn authority(actor_id: &str) -> CompanyAuthority {
    authority_with_role(actor_id, "sponsor")
}

fn authority_with_role(actor_id: &str, role_id: &str) -> CompanyAuthority {
    CompanyAuthority {
        actor_id: actor_id.to_owned(),
        role_id: role_id.to_owned(),
        session_id: SessionId::new("co09-session"),
        now_ms: 1_000,
        execution_request_id: RequestId::new(),
        execution_cell_id: None,
    }
}

fn proof() -> CompanyProof {
    CompanyProof {
        events: vec![
            "artifact:charter".to_owned(),
            "event:sponsor-decision".to_owned(),
        ],
        ..CompanyProof::default()
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
        information_gaps: Some(Vec::new()),
        decision: None,
        project_id: None,
        status: InitiativeStatus::Intake,
        version: 1,
    }
}

fn project(
    id: &str,
    organization_id: &str,
    objective_refs: Vec<String>,
    sponsor_id: &str,
) -> Project {
    Project {
        project_id: id.to_owned(),
        organization_id: organization_id.to_owned(),
        objective_refs,
        sponsor_id: sponsor_id.to_owned(),
        charter_ref: "artifact:charter".to_owned(),
        scope_baseline: "scope-v1".to_owned(),
        success_criteria: vec!["criterion".to_owned()],
        non_goals: vec!["external publish".to_owned()],
        project_budget_ref: format!("budget-{id}"),
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

fn budget(project_id: &str) -> CompanyBudgetPolicy {
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
            scope: project_id.to_owned(),
            model_calls: 8,
            tokens: 100_000,
            concurrency: 1,
        },
    }
}

fn charter() -> CompanyArtifact {
    CompanyArtifact {
        artifact_id: "charter".to_owned(),
        relative_path: "charter.json".to_owned(),
        text: "immutable charter".to_owned(),
        registered_at: 1,
        typed_version: None,
    }
}

fn transition(
    state: &CompanyState,
    command: &CompanyCommand,
    authority: &CompanyAuthority,
) -> CompanyState {
    state
        .transition(command, authority, &proof())
        .expect("company transition")
}

fn propose_objective(state: &CompanyState, candidate: Objective) -> CompanyState {
    transition(
        state,
        &CompanyCommand::ProposeObjective {
            objective: candidate,
        },
        &authority("sponsor-1"),
    )
}

fn decide_objective(
    state: &CompanyState,
    objective_id: &str,
    approve: bool,
    authority: &CompanyAuthority,
) -> Result<CompanyState, String> {
    state
        .transition(
            &CompanyCommand::DecideObjective {
                objective_id: objective_id.to_owned(),
                approve,
            },
            authority,
            &proof(),
        )
        .map_err(str::to_owned)
}

fn submit_initiative(state: &CompanyState, candidate: Initiative) -> CompanyState {
    transition(
        state,
        &CompanyCommand::SubmitInitiative {
            initiative: candidate,
        },
        &authority("sponsor-1"),
    )
}

fn advance_initiative(
    state: &CompanyState,
    initiative_id: &str,
    status: InitiativeStatus,
    decision: Option<&str>,
    project_id: Option<&str>,
) -> Result<CompanyState, String> {
    state
        .transition(
            &CompanyCommand::AdvanceInitiative {
                initiative_id: initiative_id.to_owned(),
                status,
                decision: decision.map(str::to_owned),
                project_id: project_id.map(str::to_owned),
            },
            &authority("sponsor-1"),
            &proof(),
        )
        .map_err(str::to_owned)
}

fn advance_initiative_error(
    state: &CompanyState,
    initiative_id: &str,
    status: InitiativeStatus,
    decision: Option<&str>,
    project_id: Option<&str>,
    authority: &CompanyAuthority,
) -> String {
    advance_initiative_as(
        state,
        initiative_id,
        status,
        decision,
        project_id,
        authority,
    )
    .expect_err("initiative transition must be refused")
}

fn advance_initiative_as(
    state: &CompanyState,
    initiative_id: &str,
    status: InitiativeStatus,
    decision: Option<&str>,
    project_id: Option<&str>,
    authority: &CompanyAuthority,
) -> Result<CompanyState, String> {
    state
        .transition(
            &CompanyCommand::AdvanceInitiative {
                initiative_id: initiative_id.to_owned(),
                status,
                decision: decision.map(str::to_owned),
                project_id: project_id.map(str::to_owned),
            },
            authority,
            &proof(),
        )
        .map_err(str::to_owned)
}

fn record_direct_intake(
    state: &CompanyState,
    project_id: &str,
    reason: &str,
    authority: &CompanyAuthority,
) -> Result<CompanyState, String> {
    state
        .transition(
            &CompanyCommand::RecordDirectIntake {
                project_id: project_id.to_owned(),
                reason: reason.to_owned(),
            },
            authority,
            &proof(),
        )
        .map_err(str::to_owned)
}

fn record_direct_intake_error(
    state: &CompanyState,
    project_id: &str,
    reason: &str,
    authority: &CompanyAuthority,
) -> String {
    record_direct_intake(state, project_id, reason, authority)
        .expect_err("direct intake must be refused")
}

/// Objective intake with an active sponsor-owned objective and a proposed project that still has
/// no intake provenance; the intake-lane fixtures build on this state.
fn intake_state() -> CompanyState {
    let state = CompanyState::default();
    let state = propose_objective(
        &state,
        objective("objective-1", "org-1", "sponsor-1", Some("weekly sample")),
    );
    let state = decide_objective(&state, "objective-1", true, &authority("sponsor-1"))
        .expect("objective activation");
    transition(
        &state,
        &CompanyCommand::ProposeProject {
            project: project(
                "project-1",
                "org-1",
                vec!["objective-1".to_owned()],
                "sponsor-1",
            ),
        },
        &authority("sponsor-1"),
    )
}

fn assessed_initiative(state: &CompanyState, id: &str, gaps: Option<Vec<String>>) -> CompanyState {
    let mut intake = initiative(id, "org-1", vec!["objective-1".to_owned()], "sponsor-1");
    intake.information_gaps = gaps;
    let state = submit_initiative(state, intake);
    let state = transition(
        &state,
        &CompanyCommand::AdvanceInitiative {
            initiative_id: id.to_owned(),
            status: InitiativeStatus::Triaged,
            decision: None,
            project_id: None,
        },
        &authority("sponsor-1"),
    );
    transition(
        &state,
        &CompanyCommand::AdvanceInitiative {
            initiative_id: id.to_owned(),
            status: InitiativeStatus::Assessed,
            decision: None,
            project_id: None,
        },
        &authority("sponsor-1"),
    )
}

fn activate_objective(state: &CompanyState) -> CompanyState {
    let mut active = state.clone();
    active.objectives.get_mut("objective-1").unwrap().status = ObjectiveStatus::Active;
    active
}

/// Drive project-1 to an approved go/no-go after its intake provenance has been recorded.
fn approve_go_no_go(state: &CompanyState) -> CompanyState {
    let mut state = state.clone();
    state.artifacts.insert("charter".to_owned(), charter());
    let state = transition(
        &state,
        &CompanyCommand::ConfigureBudget {
            project_id: "project-1".to_owned(),
            policy: budget("project-1"),
        },
        &authority("sponsor-1"),
    );
    let state = transition(
        &state,
        &CompanyCommand::StartChartering {
            project_id: "project-1".to_owned(),
        },
        &authority("sponsor-1"),
    );
    transition(
        &state,
        &CompanyCommand::ApproveProject {
            project_id: "project-1".to_owned(),
            decision_ref: "event:sponsor-decision".to_owned(),
        },
        &authority("sponsor-1"),
    )
}

#[test]
fn objective_approval_rejects_nan_inverted_window_wrong_owner_and_organization() {
    // Schema-level refusals: non-finite values, a non-improving target and an inverted window can
    // never enter the intake record.
    let mut nan = objective("objective-nan", "org-1", "sponsor-1", Some("weekly sample"));
    nan.target = f64::NAN;
    assert_eq!(nan.validate().unwrap_err(), "objective_measurement_invalid");
    let mut flat = objective(
        "objective-flat",
        "org-1",
        "sponsor-1",
        Some("weekly sample"),
    );
    flat.target = flat.baseline;
    assert_eq!(
        flat.validate().unwrap_err(),
        "objective_measurement_invalid"
    );
    let mut reversed_direction = objective(
        "objective-reversed",
        "org-1",
        "sponsor-1",
        Some("weekly sample"),
    );
    reversed_direction.target = 0.1;
    assert_eq!(
        reversed_direction.validate().unwrap_err(),
        "objective_measurement_invalid"
    );
    let mut inverted = objective(
        "objective-inverted",
        "org-1",
        "sponsor-1",
        Some("weekly sample"),
    );
    inverted.period_start = 10;
    inverted.period_end = 10;
    assert_eq!(
        inverted.validate().unwrap_err(),
        "objective_measurement_invalid"
    );
    let mut zero_window = objective(
        "objective-zero-window",
        "org-1",
        "sponsor-1",
        Some("weekly sample"),
    );
    zero_window.period_start = 0;
    zero_window.period_end = 0;
    assert_eq!(
        zero_window.validate().unwrap_err(),
        "objective_measurement_invalid"
    );

    // Approval-level refusals: a missing measurement method, a foreign owner and an objective
    // attributed to an organization owned by somebody else cannot be activated.
    let sponsor = authority("sponsor-1");
    let mut state = CompanyState::default();
    state = propose_objective(
        &state,
        objective("objective-without-method", "org-1", "sponsor-1", None),
    );
    assert_eq!(
        decide_objective(&state, "objective-without-method", true, &sponsor).unwrap_err(),
        "objective_measurement_method_required"
    );
    state = propose_objective(
        &state,
        objective("objective-1", "org-1", "sponsor-1", Some("weekly sample")),
    );
    assert_eq!(
        decide_objective(
            &state,
            "objective-1",
            true,
            &authority_with_role("other-sponsor", "sponsor"),
        )
        .unwrap_err(),
        "objective_owner_mismatch"
    );
    let mut registered = state.clone();
    registered.business.organizations.insert(
        "org-1".to_owned(),
        BusinessOrganization {
            organization_id: "org-1".to_owned(),
            name: "Registered org".to_owned(),
            owner_principal_id: "other-sponsor".to_owned(),
            workspace: "/co09".to_owned(),
            version: 1,
        },
    );
    assert_eq!(
        decide_objective(&registered, "objective-1", true, &sponsor).unwrap_err(),
        "objective_organization_owner_mismatch"
    );

    // The same objective activates once the sponsor owns the registered organization.
    let mut owned = state.clone();
    owned.business.organizations.insert(
        "org-1".to_owned(),
        BusinessOrganization {
            organization_id: "org-1".to_owned(),
            name: "Sponsor org".to_owned(),
            owner_principal_id: "sponsor-1".to_owned(),
            workspace: "/co09".to_owned(),
            version: 1,
        },
    );
    let activated = decide_objective(&owned, "objective-1", true, &sponsor).expect("activation");
    assert_eq!(
        activated.objectives["objective-1"].status,
        ObjectiveStatus::Active
    );
}

#[test]
fn initiative_submission_rejects_foreign_sponsor_and_objectives() {
    let sponsor = authority("sponsor-1");
    let state = intake_state();
    let mut foreign_objective = state.clone();
    foreign_objective.objectives.insert(
        "objective-foreign".to_owned(),
        objective(
            "objective-foreign",
            "org-2",
            "sponsor-1",
            Some("daily sample"),
        ),
    );
    let wrong_sponsor = foreign_objective
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
            &proof(),
        )
        .unwrap_err();
    assert_eq!(wrong_sponsor, "initiative_initial_state_invalid");
    let cross_organization = foreign_objective
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
            &proof(),
        )
        .unwrap_err();
    assert_eq!(cross_organization, "initiative_objective_missing");
}

#[test]
fn initiative_cannot_convert_without_approved_objective_and_sponsor_decision() {
    let state = intake_state();
    let state = assessed_initiative(&state, "initiative-1", Some(vec!["cost model".to_owned()]));

    // The referenced objective is not approved yet, so neither approval nor conversion can pass.
    assert_eq!(
        advance_initiative(
            &state,
            "initiative-1",
            InitiativeStatus::Approved,
            Some("proceed"),
            None,
        )
        .unwrap_err(),
        "initiative_objective_approval_required"
    );

    // Activate the objective and approve the Initiative with a recorded sponsor decision.
    let active = activate_objective(&state);
    let approved = advance_initiative(
        &active,
        "initiative-1",
        InitiativeStatus::Approved,
        Some("proceed with the assessed gaps"),
        None,
    )
    .expect("initiative approval");
    assert_eq!(
        approved.initiatives["initiative-1"].decision.as_deref(),
        Some("proceed with the assessed gaps")
    );

    // A forged or legacy Approved state without the recorded decision cannot convert.
    let mut forged = approved.clone();
    forged.initiatives.get_mut("initiative-1").unwrap().decision = None;
    assert_eq!(
        advance_initiative(
            &forged,
            "initiative-1",
            InitiativeStatus::ConvertedToProject,
            Some("convert"),
            Some("project-1"),
        )
        .unwrap_err(),
        "initiative_decision_required"
    );

    // A project that does not keep the Initiative objective ancestry is refused.
    let mut foreign_project = approved.clone();
    foreign_project.projects.insert(
        "project-2".to_owned(),
        project(
            "project-2",
            "org-2",
            vec!["objective-1".to_owned()],
            "sponsor-1",
        ),
    );
    assert_eq!(
        advance_initiative(
            &foreign_project,
            "initiative-1",
            InitiativeStatus::ConvertedToProject,
            Some("convert"),
            Some("project-2"),
        )
        .unwrap_err(),
        "initiative_project_objective_ancestry_mismatch"
    );

    // Only a sponsor may record the conversion decision.
    assert_eq!(
        advance_initiative_error(
            &approved,
            "initiative-1",
            InitiativeStatus::ConvertedToProject,
            Some("convert"),
            Some("project-1"),
            &authority_with_role("pm-1", "pm"),
        ),
        "company_role_denied"
    );

    // Rejected/withdrawn Initiatives keep their history and can never convert.
    let rejected = assessed_initiative(&approved, "initiative-rejected", Some(Vec::new()));
    let rejected = advance_initiative(
        &rejected,
        "initiative-rejected",
        InitiativeStatus::Rejected,
        Some("withdrawn: the sponsor stopped the intake"),
        None,
    )
    .expect("initiative rejection");
    let record = &rejected.initiatives["initiative-rejected"];
    assert_eq!(record.status, InitiativeStatus::Rejected);
    assert_eq!(
        record.decision.as_deref(),
        Some("withdrawn: the sponsor stopped the intake")
    );
    assert_eq!(record.objective_refs, vec!["objective-1".to_owned()]);
    assert_eq!(
        advance_initiative(
            &rejected,
            "initiative-rejected",
            InitiativeStatus::ConvertedToProject,
            Some("convert anyway"),
            Some("project-1"),
        )
        .unwrap_err(),
        "initiative_conversion_requires_approval"
    );
    let archived = advance_initiative(
        &rejected,
        "initiative-rejected",
        InitiativeStatus::Archived,
        None,
        None,
    )
    .expect("archiving a rejected initiative");
    let archived_record = &archived.initiatives["initiative-rejected"];
    assert_eq!(archived_record.status, InitiativeStatus::Archived);
    assert_eq!(
        archived_record.decision.as_deref(),
        Some("withdrawn: the sponsor stopped the intake")
    );
}

#[test]
fn initiative_intake_records_hypothesis_value_cost_risk_and_gap_assessment() {
    let incomplete = initiative(
        "initiative-1",
        "org-1",
        vec!["objective-1".to_owned()],
        "s-1",
    );
    let blank_hypothesis = {
        let mut candidate = incomplete.clone();
        candidate.hypothesis = " ".to_owned();
        candidate
    };
    assert_eq!(
        blank_hypothesis.validate().unwrap_err(),
        "company_field_required_or_too_large"
    );
    let empty_value = {
        let mut candidate = incomplete.clone();
        candidate.expected_value = String::new();
        candidate
    };
    assert_eq!(
        empty_value.validate().unwrap_err(),
        "company_field_required_or_too_large"
    );
    let blank_cost = {
        let mut candidate = incomplete.clone();
        candidate.rough_cost = " ".to_owned();
        candidate
    };
    assert_eq!(
        blank_cost.validate().unwrap_err(),
        "company_field_required_or_too_large"
    );
    let empty_risk = {
        let mut candidate = incomplete.clone();
        candidate.risk_summary = String::new();
        candidate
    };
    assert_eq!(
        empty_risk.validate().unwrap_err(),
        "company_field_required_or_too_large"
    );
    incomplete.validate().expect("complete intake");

    // The declared information gaps are bounded and duplicate-free.
    let blank_gap = {
        let mut candidate = incomplete.clone();
        candidate.information_gaps = Some(vec!["  ".to_owned()]);
        candidate
    };
    assert_eq!(
        blank_gap.validate().unwrap_err(),
        "initiative_information_gaps_invalid"
    );
    let oversized_gap = {
        let mut candidate = incomplete.clone();
        candidate.information_gaps =
            Some(vec!["g".repeat(MAX_INITIATIVE_INFORMATION_GAP_BYTES + 1)]);
        candidate
    };
    assert_eq!(
        oversized_gap.validate().unwrap_err(),
        "initiative_information_gaps_invalid"
    );
    let duplicate_gaps = {
        let mut candidate = incomplete.clone();
        candidate.information_gaps = Some(vec!["same".to_owned(), "same".to_owned()]);
        candidate
    };
    assert_eq!(
        duplicate_gaps.validate().unwrap_err(),
        "initiative_information_gaps_invalid"
    );
    let too_many_gaps = {
        let mut candidate = incomplete.clone();
        candidate.information_gaps = Some(
            (0..MAX_INITIATIVE_INFORMATION_GAPS + 1)
                .map(|index| format!("gap-{index}"))
                .collect(),
        );
        candidate
    };
    assert_eq!(
        too_many_gaps.validate().unwrap_err(),
        "initiative_information_gaps_invalid"
    );
    let mut explicit_empty = incomplete.clone();
    explicit_empty.information_gaps = Some(Vec::new());
    explicit_empty
        .validate()
        .expect("an explicit empty assessment");

    // Approval requires the assessment; a legacy intake without it stays in intake.
    let state = intake_state();
    let unassessed = assessed_initiative(&state, "initiative-1", None);
    let active = activate_objective(&unassessed);
    assert_eq!(
        advance_initiative(
            &active,
            "initiative-1",
            InitiativeStatus::Approved,
            Some("approve"),
            None,
        )
        .unwrap_err(),
        "initiative_information_gaps_required"
    );

    // An explicit assessment (including "no known gaps") is enough to approve.
    let assessed = assessed_initiative(&state, "initiative-assessed", Some(Vec::new()));
    let active = activate_objective(&assessed);
    let approved = advance_initiative(
        &active,
        "initiative-assessed",
        InitiativeStatus::Approved,
        Some("approve with no known gaps"),
        None,
    )
    .expect("initiative approval");
    assert_eq!(
        approved.initiatives["initiative-assessed"].information_gaps,
        Some(Vec::new())
    );
}

#[test]
fn approved_initiative_converts_once_and_keeps_objective_ancestry() {
    let state = intake_state();
    let state = assessed_initiative(&state, "initiative-1", Some(vec!["cost model".to_owned()]));
    let active = activate_objective(&state);
    let approved = advance_initiative(
        &active,
        "initiative-1",
        InitiativeStatus::Approved,
        Some("proceed with the assessed gaps"),
        None,
    )
    .expect("initiative approval");

    let converted = advance_initiative(
        &approved,
        "initiative-1",
        InitiativeStatus::ConvertedToProject,
        Some("convert to project-1"),
        Some("project-1"),
    )
    .expect("initiative conversion");
    let first = converted.initiatives["initiative-1"].clone();
    assert_eq!(first.status, InitiativeStatus::ConvertedToProject);
    assert_eq!(first.project_id.as_deref(), Some("project-1"));
    assert_eq!(first.objective_refs, vec!["objective-1".to_owned()]);
    assert_eq!(first.information_gaps, Some(vec!["cost model".to_owned()]));
    assert_eq!(
        converted
            .project_intake_provenance("project-1")
            .expect("initiative provenance"),
        ProjectIntakeProvenance::Initiative {
            initiative_id: "initiative-1".to_owned()
        }
    );

    // Repeating the same conversion returns the original result and changes nothing but revision.
    let repeated = advance_initiative(
        &converted,
        "initiative-1",
        InitiativeStatus::ConvertedToProject,
        Some("convert to project-1"),
        Some("project-1"),
    )
    .expect("idempotent conversion");
    let again = &repeated.initiatives["initiative-1"];
    assert_eq!(again.project_id, first.project_id);
    assert_eq!(again.status, first.status);
    assert_eq!(again.version, first.version);
    assert_eq!(again.objective_refs, first.objective_refs);
    assert_eq!(repeated.projects["project-1"].version, 1);

    // A different project is refused instead of being silently repointed.
    let mut second_project = repeated.clone();
    second_project.projects.insert(
        "project-2".to_owned(),
        project(
            "project-2",
            "org-1",
            vec!["objective-1".to_owned()],
            "sponsor-1",
        ),
    );
    assert_eq!(
        advance_initiative(
            &second_project,
            "initiative-1",
            InitiativeStatus::ConvertedToProject,
            Some("convert to project-2"),
            Some("project-2"),
        )
        .unwrap_err(),
        "initiative_already_converted"
    );

    // One project keeps one conversion; a second approved Initiative cannot claim it.
    let with_second = assessed_initiative(
        &repeated,
        "initiative-2",
        Some(vec!["cost model".to_owned()]),
    );
    let second_approved = advance_initiative(
        &with_second,
        "initiative-2",
        InitiativeStatus::Approved,
        Some("second initiative"),
        None,
    )
    .expect("second initiative approval");
    assert_eq!(
        advance_initiative(
            &second_approved,
            "initiative-2",
            InitiativeStatus::ConvertedToProject,
            Some("convert to project-1"),
            Some("project-1"),
        )
        .unwrap_err(),
        "initiative_project_already_linked"
    );

    // The converted project reaches go/no-go with the Initiative ancestry intact.
    let approved_project = approve_go_no_go(&converted);
    assert_eq!(
        approved_project.projects["project-1"].status,
        ProjectStatus::Approved
    );
    assert_eq!(
        approved_project
            .project_intake_provenance("project-1")
            .expect("initiative provenance survives approval"),
        ProjectIntakeProvenance::Initiative {
            initiative_id: "initiative-1".to_owned()
        }
    );
    assert_eq!(
        approved_project.initiatives["initiative-1"].objective_refs,
        vec!["objective-1".to_owned()]
    );
}

#[test]
fn direct_project_intake_records_the_skip_reason_and_keeps_objective_ancestry() {
    let sponsor = authority("sponsor-1");
    let state = intake_state();

    // Without a recorded entry the project has no intake provenance at all.
    assert_eq!(
        state.project_intake_provenance("project-1").unwrap_err(),
        "project_intake_provenance_required"
    );
    assert_eq!(
        record_direct_intake_error(&state, "missing-project", "no initiative", &sponsor),
        "company_project_not_found"
    );
    assert_eq!(
        record_direct_intake_error(&state, "project-1", "   ", &sponsor),
        "direct_intake_reason_invalid"
    );
    assert_eq!(
        record_direct_intake_error(
            &state,
            "project-1",
            &"x".repeat(MAX_DIRECT_INTAKE_REASON_BYTES + 1),
            &sponsor,
        ),
        "direct_intake_reason_invalid"
    );
    assert_eq!(
        record_direct_intake_error(
            &state,
            "project-1",
            "builder cannot skip the intake lane",
            &authority_with_role("builder-1", "builder"),
        ),
        "company_role_denied"
    );
    let mut unreferenced = state.clone();
    unreferenced
        .projects
        .get_mut("project-1")
        .unwrap()
        .objective_refs
        .clear();
    assert_eq!(
        record_direct_intake_error(&unreferenced, "project-1", "no objective", &sponsor),
        "direct_intake_objective_required"
    );

    // The recorded entry keeps the objective ancestry and is idempotent on the same reason.
    let recorded = record_direct_intake(
        &state,
        "project-1",
        "no initiative fits the bootstrap project",
        &sponsor,
    )
    .expect("direct intake record");
    let entry = recorded.direct_intakes["project-1"].clone();
    assert_eq!(entry.version, 1);
    assert_eq!(entry.organization_id, "org-1");
    assert_eq!(entry.objective_refs, vec!["objective-1".to_owned()]);
    assert_eq!(entry.recorded_by, "sponsor-1");
    assert_eq!(
        recorded
            .project_intake_provenance("project-1")
            .expect("direct provenance"),
        ProjectIntakeProvenance::Direct {
            reason: "no initiative fits the bootstrap project".to_owned()
        }
    );
    let repeated = record_direct_intake(
        &recorded,
        "project-1",
        "no initiative fits the bootstrap project",
        &sponsor,
    )
    .expect("idempotent direct intake");
    assert_eq!(repeated.direct_intakes["project-1"].version, entry.version);
    assert_eq!(repeated.direct_intakes["project-1"].reason, entry.reason);
    assert_eq!(
        record_direct_intake_error(&recorded, "project-1", "another reason", &sponsor),
        "direct_intake_conflict"
    );

    // Provenance cannot survive an ancestry change on the project.
    let mut drifted = recorded.clone();
    drifted
        .projects
        .get_mut("project-1")
        .unwrap()
        .objective_refs = vec!["objective-other".to_owned()];
    assert_eq!(
        drifted.project_intake_provenance("project-1").unwrap_err(),
        "project_intake_objective_ancestry_mismatch"
    );
    // A malformed record (no recording time) is never accepted as provenance.
    let mut tampered = recorded.clone();
    tampered
        .direct_intakes
        .get_mut("project-1")
        .unwrap()
        .recorded_at = 0;
    assert_eq!(
        tampered.project_intake_provenance("project-1").unwrap_err(),
        "direct_intake_invalid"
    );

    // An Initiative already owns this project's intake lane.
    let mut linked = state.clone();
    linked.initiatives.insert(
        "initiative-linked".to_owned(),
        Initiative {
            initiative_id: "initiative-linked".to_owned(),
            organization_id: "org-1".to_owned(),
            objective_refs: vec!["objective-1".to_owned()],
            title: "Converted initiative".to_owned(),
            problem_statement: "already converted".to_owned(),
            hypothesis: "conversion happens once".to_owned(),
            sponsor_id: "sponsor-1".to_owned(),
            expected_value: "value".to_owned(),
            rough_cost: "small".to_owned(),
            risk_summary: "low".to_owned(),
            information_gaps: Some(Vec::new()),
            decision: Some("convert".to_owned()),
            project_id: Some("project-1".to_owned()),
            status: InitiativeStatus::ConvertedToProject,
            version: 5,
        },
    );
    assert_eq!(
        record_direct_intake_error(&linked, "project-1", "direct intake", &sponsor),
        "direct_intake_initiative_ancestry_conflict"
    );

    // The recorded entry is what lets the project reach go/no-go, and it survives approval.
    let approved = approve_go_no_go(&recorded);
    assert_eq!(
        approved.projects["project-1"].status,
        ProjectStatus::Approved
    );
    assert_eq!(
        approved
            .project_intake_provenance("project-1")
            .expect("direct provenance survives approval"),
        ProjectIntakeProvenance::Direct {
            reason: "no initiative fits the bootstrap project".to_owned()
        }
    );
    assert_eq!(
        record_direct_intake_error(&approved, "project-1", "recorded too late", &sponsor),
        "direct_intake_project_not_pending"
    );
}

//! CO-10 Charter and project go/no-go fixtures.
//!
//! Refusals come first: a non-empty `project_budget_ref` is not a validated budget, a changed
//! Charter (content or typed version), a changed Charter draft, an inactive objective, a
//! mismatched Sponsor, a missing/empty Charter artifact and unverified decision evidence all
//! stop the go/no-go before any baseline is frozen. The card-named success fixture then freezes
//! the approved scope/criteria/budget, proves later project state revisions do not advance that
//! baseline, and proves a rejected project stays queryable with its write path closed.

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

fn pm_authority() -> CompanyAuthority {
    CompanyAuthority {
        actor_id: "pm-1".to_owned(),
        role_id: "pm".to_owned(),
        session_id: SessionId::new("co10-pm-session"),
        now_ms: 1_000,
        execution_request_id: RequestId::new(),
        execution_cell_id: None,
    }
}

/// A Sponsor-role actor that is not this project's recorded sponsor.
fn foreign_sponsor_authority() -> CompanyAuthority {
    CompanyAuthority {
        actor_id: "sponsor-2".to_owned(),
        role_id: "sponsor".to_owned(),
        session_id: SessionId::new("co10-foreign-session"),
        now_ms: 1_000,
        execution_request_id: RequestId::new(),
        execution_cell_id: None,
    }
}

fn builder_authority() -> CompanyAuthority {
    CompanyAuthority {
        actor_id: "builder-1".to_owned(),
        role_id: "builder".to_owned(),
        session_id: SessionId::new("co10-builder-session"),
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

fn milestone() -> Milestone {
    Milestone {
        milestone_id: "milestone-1".to_owned(),
        project_id: "project-1".to_owned(),
        objective_refs: vec!["objective-1".to_owned()],
        deliverables: vec!["OUTPUT.txt".to_owned()],
        acceptance_criteria: vec!["criterion".to_owned()],
        due_at: 10_000,
        dependency_refs: Vec::new(),
        status: MilestoneStatus::Planned,
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

fn stakeholder(
    principal: &str,
    responsibility: &str,
    delivery_recipient: bool,
) -> CharterStakeholder {
    CharterStakeholder {
        principal_id: principal.to_owned(),
        responsibility: responsibility.to_owned(),
        delivery_recipient,
    }
}

fn draft() -> ProjectCharterDraft {
    ProjectCharterDraft {
        schema: PROJECT_CHARTER_DRAFT_SCHEMA.to_owned(),
        project_id: "project-1".to_owned(),
        organization_id: "org-1".to_owned(),
        scope: "scope-v1".to_owned(),
        non_goals: vec!["external publish".to_owned()],
        stakeholders: vec![
            stakeholder("sponsor-1", "owns the go/no-go decision", true),
            stakeholder("qa-1", "verifies the acceptance criteria", false),
        ],
        success_criteria: vec!["criterion".to_owned()],
        risk_summary: "bounded".to_owned(),
        project_budget_ref: "budget-1".to_owned(),
        drafted_by: "pm-1".to_owned(),
        drafted_at: 900,
        version: 1,
    }
}

/// A typed artifact identity whose content hash is valid on its own.
fn typed_version(content: &[u8]) -> ArtifactVersion {
    ArtifactVersion::new(
        ArtifactId::new(),
        1,
        "kiana.company-artifact.v1",
        content,
        json_digest(&serde_json::json!({"scope": "project-1"})),
        ArtifactProvenance {
            producer_kind: "company".to_owned(),
            producer_id: "sponsor-1".to_owned(),
            source_event_id: None,
            source_run_id: None,
            recorded_by: "sponsor-1".to_owned(),
        },
        1,
    )
    .expect("artifact version")
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

fn approve() -> CompanyCommand {
    CompanyCommand::ApproveProject {
        project_id: "project-1".to_owned(),
        decision_ref: "artifact:decision".to_owned(),
    }
}

fn reject() -> CompanyCommand {
    CompanyCommand::RejectProject {
        project_id: "project-1".to_owned(),
        decision_ref: "artifact:decision".to_owned(),
    }
}

/// Proposed project with the chartering role's draft recorded and the draft identity frozen.
fn chartering_state() -> CompanyState {
    state()
        .transition(
            &CompanyCommand::DraftCharter { draft: draft() },
            &pm_authority(),
            &proof(),
        )
        .expect("draft")
        .transition(
            &CompanyCommand::StartChartering {
                project_id: "project-1".to_owned(),
            },
            &authority(),
            &proof(),
        )
        .expect("chartering")
}

fn budgeted_chartering_state() -> CompanyState {
    chartering_state()
        .transition(
            &CompanyCommand::ConfigureBudget {
                project_id: "project-1".to_owned(),
                policy: budget(),
            },
            &authority(),
            &proof(),
        )
        .expect("budget")
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
fn project_charter_draft_rejects_role_drift_and_stakeholder_gaps() {
    let pm = pm_authority();
    let mut malformed = draft();
    malformed.schema = "kiana.project-charter-draft.v2".to_owned();
    assert_eq!(
        state()
            .transition(
                &CompanyCommand::DraftCharter { draft: malformed },
                &pm,
                &proof(),
            )
            .unwrap_err(),
        "charter_draft_schema_invalid"
    );

    let mut wrong_version = draft();
    wrong_version.version = 2;
    assert_eq!(
        state()
            .transition(
                &CompanyCommand::DraftCharter {
                    draft: wrong_version,
                },
                &pm,
                &proof(),
            )
            .unwrap_err(),
        "charter_draft_version_invalid"
    );

    let mut no_stakeholders = draft();
    no_stakeholders.stakeholders = Vec::new();
    assert_eq!(
        state()
            .transition(
                &CompanyCommand::DraftCharter {
                    draft: no_stakeholders,
                },
                &pm,
                &proof(),
            )
            .unwrap_err(),
        "charter_draft_stakeholders_invalid"
    );

    let mut no_recipient = draft();
    no_recipient.stakeholders = vec![stakeholder("sponsor-1", "owns the decision", false)];
    assert_eq!(
        state()
            .transition(
                &CompanyCommand::DraftCharter {
                    draft: no_recipient,
                },
                &pm,
                &proof(),
            )
            .unwrap_err(),
        "charter_draft_delivery_recipient_required"
    );

    let mut blank_responsibility = draft();
    blank_responsibility.stakeholders = vec![stakeholder("sponsor-1", "   ", true)];
    assert_eq!(
        state()
            .transition(
                &CompanyCommand::DraftCharter {
                    draft: blank_responsibility,
                },
                &pm,
                &proof(),
            )
            .unwrap_err(),
        "charter_draft_stakeholder_responsibility_invalid"
    );

    let mut duplicated_non_goals = draft();
    duplicated_non_goals.non_goals =
        vec!["external publish".to_owned(), "external publish".to_owned()];
    assert_eq!(
        state()
            .transition(
                &CompanyCommand::DraftCharter {
                    draft: duplicated_non_goals,
                },
                &pm,
                &proof(),
            )
            .unwrap_err(),
        "charter_draft_non_goals_invalid"
    );

    let mut oversized_scope = draft();
    oversized_scope.scope = "s".repeat(MAX_CHARTER_DRAFT_TEXT_BYTES + 1);
    assert_eq!(
        state()
            .transition(
                &CompanyCommand::DraftCharter {
                    draft: oversized_scope,
                },
                &pm,
                &proof(),
            )
            .unwrap_err(),
        "charter_draft_scope_invalid"
    );

    // A draft cannot widen the proposed project: scope/criteria/non-goals/risk must match the
    // frozen proposal fields, and the budget reference must be the project's own reference.
    let mut widened = draft();
    widened.scope = "scope-v2".to_owned();
    assert_eq!(
        state()
            .transition(
                &CompanyCommand::DraftCharter { draft: widened },
                &pm,
                &proof(),
            )
            .unwrap_err(),
        "project_charter_draft_project_mismatch"
    );

    let mut foreign_budget = draft();
    foreign_budget.project_budget_ref = "budget-other".to_owned();
    assert_eq!(
        state()
            .transition(
                &CompanyCommand::DraftCharter {
                    draft: foreign_budget,
                },
                &pm,
                &proof(),
            )
            .unwrap_err(),
        "project_charter_draft_budget_ref_mismatch"
    );

    // The recorded drafter is the authenticated actor, and the draft cannot be future-dated.
    let mut forged_author = draft();
    forged_author.drafted_by = "sponsor-1".to_owned();
    assert_eq!(
        state()
            .transition(
                &CompanyCommand::DraftCharter {
                    draft: forged_author,
                },
                &pm,
                &proof(),
            )
            .unwrap_err(),
        "project_charter_draft_author_mismatch"
    );

    let mut future_dated = draft();
    future_dated.drafted_at = 10_000;
    assert_eq!(
        state()
            .transition(
                &CompanyCommand::DraftCharter {
                    draft: future_dated,
                },
                &pm,
                &proof(),
            )
            .unwrap_err(),
        "project_charter_draft_author_mismatch"
    );

    // A Sponsor-role actor that is not this project's sponsor cannot draft for it, and a Builder
    // cannot draft at all.
    let mut foreign_draft = draft();
    foreign_draft.drafted_by = "sponsor-2".to_owned();
    assert_eq!(
        state()
            .transition(
                &CompanyCommand::DraftCharter {
                    draft: foreign_draft,
                },
                &foreign_sponsor_authority(),
                &proof(),
            )
            .unwrap_err(),
        "project_charter_draft_role_denied"
    );
    assert_eq!(
        state()
            .transition(
                &CompanyCommand::DraftCharter { draft: draft() },
                &builder_authority(),
                &proof(),
            )
            .unwrap_err(),
        "company_role_denied"
    );

    // The draft is recorded once: an identical re-submission is idempotent, different content is
    // refused, and every refusal above left the state without a draft.
    let recorded = state()
        .transition(
            &CompanyCommand::DraftCharter { draft: draft() },
            &pm,
            &proof(),
        )
        .expect("draft");
    assert_eq!(recorded.charter_drafts.len(), 1);
    let replayed = recorded
        .transition(
            &CompanyCommand::DraftCharter { draft: draft() },
            &pm,
            &proof(),
        )
        .expect("idempotent draft");
    assert_eq!(replayed.charter_drafts, recorded.charter_drafts);
    let mut revised = draft();
    revised.stakeholders = vec![stakeholder("sponsor-1", "owns the go/no-go decision", true)];
    assert_eq!(
        recorded
            .transition(
                &CompanyCommand::DraftCharter { draft: revised },
                &pm,
                &proof(),
            )
            .unwrap_err(),
        "project_charter_draft_exists"
    );
    assert_eq!(state().charter_drafts.len(), 0);
    assert_eq!(state().charter_draft_digests.len(), 0);

    // Once the project is approved the draft surface is closed.
    let approved = budgeted_chartering_state()
        .transition(&approve(), &authority(), &proof())
        .expect("approved");
    assert_eq!(
        approved
            .transition(
                &CompanyCommand::DraftCharter { draft: draft() },
                &pm,
                &proof(),
            )
            .unwrap_err(),
        "project_charter_draft_not_pending"
    );
}

#[test]
fn project_approval_rejects_missing_budget_changed_charter_and_inactive_objective() {
    let authority = authority();

    // 1) A non-empty `project_budget_ref` string is not a validated budget: the project needs a
    //    configured CompanyBudgetPolicy before go/no-go.
    let unbudgeted = chartering_state();
    assert_eq!(
        unbudgeted
            .transition(&approve(), &authority, &proof())
            .unwrap_err(),
        "project_budget_required"
    );
    assert_eq!(
        unbudgeted.projects["project-1"].status,
        ProjectStatus::Chartering
    );
    assert_eq!(
        unbudgeted.projects["project-1"].project_budget_ref,
        "budget-1"
    );
    assert!(!unbudgeted.charter_baselines.contains_key("project-1"));

    // 2) Charter content drift after the proposal digest was captured.
    let mut changed_charter = budgeted_chartering_state();
    changed_charter.artifacts.get_mut("charter").unwrap().text = "changed charter".to_owned();
    assert_eq!(
        changed_charter
            .transition(&approve(), &authority, &proof())
            .unwrap_err(),
        "project_charter_changed"
    );
    assert!(!changed_charter.charter_baselines.contains_key("project-1"));

    // 3) A typed Charter identity whose content hash no longer matches the registered text.
    let mut typed_drift = budgeted_chartering_state();
    typed_drift
        .artifacts
        .get_mut("charter")
        .unwrap()
        .typed_version = Some(typed_version(b"different charter bytes"));
    typed_drift.charter_digests.remove("project-1");
    assert_eq!(
        typed_drift
            .transition(&approve(), &authority, &proof())
            .unwrap_err(),
        "project_charter_changed"
    );

    // 4) The approved draft is the version frozen on entry: editing it afterwards is refused.
    let mut draft_drift = budgeted_chartering_state();
    draft_drift
        .charter_drafts
        .get_mut("project-1")
        .unwrap()
        .scope = "scope-v2".to_owned();
    assert_eq!(
        draft_drift
            .transition(&approve(), &authority, &proof())
            .unwrap_err(),
        "project_charter_draft_changed"
    );

    // 5) An objective that is no longer active (or left the organization) stops the go/no-go.
    let mut inactive = budgeted_chartering_state();
    inactive.objectives.get_mut("objective-1").unwrap().status = ObjectiveStatus::Paused;
    assert_eq!(
        inactive
            .transition(&approve(), &authority, &proof())
            .unwrap_err(),
        "project_objective_not_active"
    );
    let mut foreign_org = budgeted_chartering_state();
    foreign_org
        .objectives
        .get_mut("objective-1")
        .unwrap()
        .organization_id = "org-2".to_owned();
    assert_eq!(
        foreign_org
            .transition(&approve(), &authority, &proof())
            .unwrap_err(),
        "project_objective_not_active"
    );

    // 6) Only the recorded Sponsor decides.
    assert_eq!(
        budgeted_chartering_state()
            .transition(&approve(), &foreign_sponsor_authority(), &proof())
            .unwrap_err(),
        "project_sponsor_mismatch"
    );

    // 7) A project that never entered Chartering cannot be approved.
    let mut proposed = state();
    proposed.budgets.insert("project-1".to_owned(), budget());
    assert_eq!(
        proposed
            .transition(&approve(), &authority, &proof())
            .unwrap_err(),
        "project_charter_not_ready_for_approval"
    );

    // 8) The registered Charter artifact must exist and carry text.
    let mut empty_charter = budgeted_chartering_state();
    empty_charter.artifacts.get_mut("charter").unwrap().text = "   ".to_owned();
    assert_eq!(
        empty_charter
            .transition(&approve(), &authority, &proof())
            .unwrap_err(),
        "project_charter_artifact_required"
    );
    let mut missing_charter = budgeted_chartering_state();
    missing_charter.artifacts.remove("charter");
    assert_eq!(
        missing_charter
            .transition(&approve(), &authority, &proof())
            .unwrap_err(),
        "project_charter_artifact_required"
    );

    // 9) The Sponsor decision has to reference committed Company facts.
    assert_eq!(
        budgeted_chartering_state()
            .transition(&approve(), &authority, &CompanyProof::default())
            .unwrap_err(),
        "company_evidence_unverified"
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

#[test]
fn project_charter_approval_freezes_scope_criteria_and_budget() {
    let authority = authority();
    let pm = pm_authority();
    let approved = budgeted_chartering_state()
        .transition(&approve(), &authority, &proof())
        .expect("approved");
    assert_eq!(
        approved.projects["project-1"].status,
        ProjectStatus::Approved
    );
    assert_eq!(approved.projects["project-1"].version, 3);

    // The go/no-go freezes the reviewed Charter version and the validated budget reference.
    let baseline = approved.charter_baselines["project-1"].clone();
    assert_eq!(baseline.version, 1);
    assert_eq!(baseline.charter_ref, "artifact:charter");
    assert_eq!(baseline.scope_baseline, "scope-v1");
    assert_eq!(baseline.success_criteria, vec!["criterion"]);
    assert_eq!(baseline.non_goals, vec!["external publish"]);
    assert_eq!(baseline.risk_summary, "bounded");
    assert_eq!(baseline.project_budget_ref, "budget-1");
    assert_eq!(
        baseline.budget_digest,
        json_digest(&serde_json::json!(budget()))
    );
    assert_eq!(
        baseline.charter_draft_digest.as_deref(),
        Some(draft().identity_digest().as_str())
    );
    assert_eq!(baseline.charter_draft_version, Some(1));
    assert_eq!(
        baseline.stakeholder_principals,
        vec!["sponsor-1".to_owned(), "qa-1".to_owned()]
    );

    // Later project state revisions advance the project, never the frozen baseline.
    let planned = approved
        .transition(
            &CompanyCommand::CreateMilestone {
                milestone: milestone(),
            },
            &pm,
            &proof(),
        )
        .expect("milestone");
    assert_eq!(planned.projects["project-1"].version, 4);
    assert_eq!(planned.charter_baselines["project-1"], baseline);
    assert_eq!(planned.charter_baselines["project-1"].version, 1);
    assert_eq!(
        planned.charter_baselines["project-1"]
            .charter_draft_digest
            .as_deref(),
        Some(draft().identity_digest().as_str())
    );
    assert_eq!(
        planned
            .transition(&approve(), &authority, &proof())
            .unwrap_err(),
        "project_charter_not_ready_for_approval"
    );

    // A rejected project stays queryable and its write path stays closed.
    let rejected = budgeted_chartering_state()
        .transition(&reject(), &authority, &proof())
        .expect("rejected");
    assert_eq!(
        rejected.projects["project-1"].status,
        ProjectStatus::Rejected
    );
    assert_eq!(
        rejected.projects["project-1"].decision_ref.as_deref(),
        Some("artifact:decision")
    );
    assert!(!rejected.charter_baselines.contains_key("project-1"));
    assert_eq!(
        rejected
            .transition(
                &CompanyCommand::CreateMilestone {
                    milestone: milestone(),
                },
                &pm,
                &proof(),
            )
            .unwrap_err(),
        "company_project_not_approved_or_active"
    );
    assert!(rejected
        .transition(
            &CompanyCommand::PlanProject {
                project_id: "project-1".to_owned(),
            },
            &pm,
            &proof(),
        )
        .is_err());
    assert_eq!(
        rejected.projects["project-1"].status,
        ProjectStatus::Rejected
    );
}

//! CO-16 aggregate approval CAS: the structured intake, the reviewed Plan preview and the
//! Sponsor approval are one transition on the single Company aggregate, and the Company command
//! vocabulary has no text path.
use kiana_domain::*;

fn digest_a() -> String {
    format!("sha256:{}", "a".repeat(64))
}

fn node(id: &str, kind: PlanNodeKind) -> PlanNode {
    PlanNode {
        node_id: id.to_owned(),
        project_id: "project-1".to_owned(),
        kind,
        version: 1,
        required: true,
        criterion_refs: Vec::new(),
        input_refs: Vec::new(),
    }
}

fn plan() -> PlanProposal {
    PlanProposal::new(
        "project-1",
        1,
        digest_a(),
        vec![
            node("milestone-1", PlanNodeKind::Milestone),
            node("packet-1", PlanNodeKind::Packet),
            node("packet-2", PlanNodeKind::Packet),
        ],
        vec![
            PlanEdge {
                from: "milestone-1".to_owned(),
                to: "packet-1".to_owned(),
                kind: PlanEdgeKind::ParentChild,
                reference: None,
            },
            PlanEdge {
                from: "milestone-1".to_owned(),
                to: "packet-2".to_owned(),
                kind: PlanEdgeKind::ParentChild,
                reference: None,
            },
            PlanEdge {
                from: "packet-1".to_owned(),
                to: "packet-2".to_owned(),
                kind: PlanEdgeKind::RequiresRunSuccess,
                reference: Some("run:1".to_owned()),
            },
        ],
    )
    .expect("plan")
}

/// Same Plan contract, but the final packet names two parent milestones, so the reference set
/// cannot be derived. The failure happens after the earlier nodes validated.
fn plan_with_invalid_later_packet() -> PlanProposal {
    PlanProposal::new(
        "project-1",
        1,
        digest_a(),
        vec![
            node("milestone-1", PlanNodeKind::Milestone),
            node("milestone-2", PlanNodeKind::Milestone),
            node("packet-1", PlanNodeKind::Packet),
            node("packet-2", PlanNodeKind::Packet),
            node("packet-3", PlanNodeKind::Packet),
        ],
        vec![
            PlanEdge {
                from: "milestone-1".to_owned(),
                to: "packet-1".to_owned(),
                kind: PlanEdgeKind::ParentChild,
                reference: None,
            },
            PlanEdge {
                from: "milestone-1".to_owned(),
                to: "packet-2".to_owned(),
                kind: PlanEdgeKind::ParentChild,
                reference: None,
            },
            PlanEdge {
                from: "milestone-2".to_owned(),
                to: "packet-3".to_owned(),
                kind: PlanEdgeKind::ParentChild,
                reference: None,
            },
            PlanEdge {
                from: "milestone-1".to_owned(),
                to: "packet-3".to_owned(),
                kind: PlanEdgeKind::ParentChild,
                reference: None,
            },
        ],
    )
    .expect("plan contract")
}

fn result() -> ResultContract {
    ResultContract {
        schema: RESULT_CONTRACT_SCHEMA.to_owned(),
        version: 1,
        output_schema: "kiana.plan-preview.v1".to_owned(),
        required_refs: vec!["artifact:preview".to_owned()],
        max_bytes: 16_384,
    }
}

fn proposal_with(plan: PlanProposal) -> CompanyPlanProposal {
    CompanyPlanProposal::new(
        "proposal-1",
        "project-1",
        "assignment:pm",
        "session:pm",
        vec!["artifact:charter".to_owned()],
        plan,
        result(),
    )
    .expect("proposal")
}

fn task(proposal: CompanyPlanProposal) -> CompanyRoleTaskResult {
    CompanyRoleTaskResult::new("task-1", "assignment:pm", ROLE_PM, "session:pm", proposal)
        .expect("task")
}

fn basis_for(plan: &PlanProposal, revision: u64) -> CompanyPlanMaterializationBasis {
    CompanyPlanMaterializationBasis {
        project_id: "project-1".to_owned(),
        expected_revision: revision,
        charter_baseline_version: 1,
        coverage_digest: digest_a(),
        required_criteria: Vec::new(),
        node_versions: plan
            .nodes
            .iter()
            .map(|(id, node)| (id.clone(), node.version))
            .collect(),
        max_nodes: 64,
        max_packets: 16,
    }
}

fn approval(preview: &CompanyPlanPreview) -> CompanyPlanApproval {
    let mut approval = CompanyPlanApproval {
        schema: COMPANY_PLAN_APPROVAL_SCHEMA.to_owned(),
        approval_id: "approval-1".to_owned(),
        proposal_id: preview.proposal_id.clone(),
        project_id: preview.project_id.clone(),
        proposal_digest: preview.proposal_digest.clone(),
        preview_digest: preview.digest.clone(),
        expected_revision: preview.expected_revision,
        decision_ref: "artifact:sponsor-decision".to_owned(),
        approver_assignment: "assignment:sponsor".to_owned(),
        approver_role: "sponsor".to_owned(),
        approved_at: 100,
        digest: String::new(),
    };
    approval.digest = approval.canonical_digest();
    approval
}

fn project() -> Project {
    Project {
        project_id: "project-1".to_owned(),
        organization_id: "org-1".to_owned(),
        objective_refs: vec!["objective-1".to_owned()],
        sponsor_id: "sponsor-1".to_owned(),
        charter_ref: "artifact:charter".to_owned(),
        scope_baseline: "scope".to_owned(),
        success_criteria: vec!["output exists".to_owned()],
        non_goals: Vec::new(),
        project_budget_ref: "budget:1".to_owned(),
        risk_summary: "low".to_owned(),
        decision_ref: Some("artifact:charter-decision".to_owned()),
        milestone_refs: Vec::new(),
        incident_id: None,
        acceptance_id: None,
        closing_receipt_id: None,
        status: ProjectStatus::Approved,
        version: 2,
    }
}

fn baseline() -> ProjectCharterBaseline {
    ProjectCharterBaseline {
        project_id: "project-1".to_owned(),
        version: 1,
        charter_ref: "artifact:charter".to_owned(),
        scope_baseline: "scope".to_owned(),
        success_criteria: vec!["output exists".to_owned()],
        non_goals: Vec::new(),
        risk_summary: "low".to_owned(),
        project_budget_ref: "budget:1".to_owned(),
        budget_digest: digest_a(),
        charter_draft_digest: None,
        charter_draft_version: None,
        stakeholder_principals: Vec::new(),
    }
}

fn budget() -> CompanyBudgetPolicy {
    CompanyBudgetPolicy {
        project: ProjectBudget {
            project_id: ProjectId::new(),
            max_runs: 4,
            max_tokens: 10_000,
        },
        runtime: RuntimeBudget {
            max_model_calls: 8,
            max_tokens: 10_000,
            max_wall_time_ms: 60_000,
        },
        quota: Quota {
            scope: "project-1".to_owned(),
            model_calls: 8,
            tokens: 10_000,
            concurrency: 1,
        },
    }
}

fn authority(role: &str) -> CompanyAuthority {
    CompanyAuthority {
        actor_id: "human-sponsor".to_owned(),
        role_id: role.to_owned(),
        session_id: SessionId::new("co16-session"),
        now_ms: 42,
        execution_request_id: RequestId::new(),
        execution_cell_id: None,
    }
}

/// A project already frozen through the CO-12 Plan graph command, with the Charter baseline and
/// the configured budget the approval re-reads.
fn approved_state(plan: &PlanProposal) -> CompanyState {
    let mut state = CompanyState::default();
    state.projects.insert("project-1".to_owned(), project());
    state
        .charter_baselines
        .insert("project-1".to_owned(), baseline());
    state.budgets.insert("project-1".to_owned(), budget());
    state.plan_graphs.insert(
        "project-1".to_owned(),
        FrozenPlanGraph::from_plan(plan).unwrap(),
    );
    state
}

fn preview_of(plan: &PlanProposal, state: &CompanyState) -> CompanyPlanPreview {
    let mut ledger = CompanyProposalLedger::default();
    ledger.revision = state.revision;
    let intake = CompanyPlanProposalIntake::role_task(task(proposal_with(plan.clone())));
    ledger
        .submit(intake, basis_for(plan, state.revision))
        .expect("preview")
}

fn command(
    plan: &PlanProposal,
    state: &CompanyState,
    approval: CompanyPlanApproval,
) -> CompanyCommand {
    CompanyCommand::ApproveCompanyPlan {
        intake: CompanyPlanProposalIntake::role_task(task(proposal_with(plan.clone()))),
        basis: basis_for(plan, state.revision),
        approval,
    }
}

#[test]
fn approve_company_plan_refuses_prose_and_invalid_later_packets_without_mutation() {
    let plan = plan();
    let state = approved_state(&plan);

    // README / stdout / assistant text that embeds a complete command or proposal body is a
    // refused intake, not an effect.
    let command_json = serde_json::json!({
        "type": "approve_company_plan",
        "intake": { "schema": "kiana.company-plan-proposal-intake.v1" },
        "command": { "FreezePlanGraph": { "plan": plan } },
    })
    .to_string();
    for source in [
        CompanyProposalSourceKind::Readme,
        CompanyProposalSourceKind::Stdout,
        CompanyProposalSourceKind::AssistantMessage,
        CompanyProposalSourceKind::Transcript,
    ] {
        let prose = CompanyCommand::ApproveCompanyPlan {
            intake: CompanyPlanProposalIntake::text_surface(source, command_json.clone()),
            basis: basis_for(&plan, state.revision),
            approval: approval(&preview_of(&plan, &state)),
        };
        assert_eq!(
            state
                .clone()
                .transition(&prose, &authority(ROLE_SPONSOR), &CompanyProof::default()),
            Err("company_proposal_source_not_role_task_result")
        );
        assert!(state.company_proposals.proposals.is_empty());
    }

    // No role other than the Sponsor can sign the CAS, even with a valid preview.
    let valid = command(&plan, &state, approval(&preview_of(&plan, &state)));
    assert_eq!(
        state
            .clone()
            .transition(&valid, &authority(ROLE_PM), &CompanyProof::default()),
        Err("company_role_denied")
    );

    // The third packet cannot be given one parent Milestone. The whole transition fails and no
    // half-approved plan is left behind.
    let invalid_plan = plan_with_invalid_later_packet();
    let invalid_state = approved_state(&invalid_plan);
    assert_eq!(
        invalid_state.clone().transition(
            &command(
                &invalid_plan,
                &invalid_state,
                approval(&preview_of(&plan, &state))
            ),
            &authority(ROLE_SPONSOR),
            &CompanyProof::default(),
        ),
        Err("company_plan_preview_packet_parent_invalid")
    );
    assert!(invalid_state.company_proposals.proposals.is_empty());
    assert!(invalid_state.company_proposals.materializations.is_empty());

    // A stale CAS revision is refused, not applied to a newer state.
    let mut stale = approval(&preview_of(&plan, &state));
    stale.expected_revision = state.revision + 5;
    stale.digest = stale.canonical_digest();
    assert_eq!(
        state.clone().transition(
            &command(&plan, &state, stale),
            &authority(ROLE_SPONSOR),
            &CompanyProof::default()
        ),
        Err("company_plan_approval_revision_conflict")
    );

    // A project with no configured budget cannot materialize a plan.
    let mut unbudgeted = approved_state(&plan);
    unbudgeted.budgets.clear();
    assert_eq!(
        unbudgeted.clone().transition(
            &command(
                &plan,
                &unbudgeted,
                approval(&preview_of(&plan, &unbudgeted))
            ),
            &authority(ROLE_SPONSOR),
            &CompanyProof::default()
        ),
        Err("company_plan_budget_missing")
    );

    // A Plan revision whose dependency versions drift from the server-side frozen graph is
    // refused before any preview is committed.
    let frozen = approved_state(&plan);
    let mut drifted_plan = plan.clone();
    drifted_plan
        .nodes
        .get_mut("packet-2")
        .expect("packet-2")
        .version = 2;
    drifted_plan.digest = drifted_plan.canonical_digest();
    assert_eq!(
        frozen.clone().transition(
            &command(
                &drifted_plan,
                &frozen,
                approval(&preview_of(&plan, &frozen))
            ),
            &authority(ROLE_SPONSOR),
            &CompanyProof::default()
        ),
        Err("frozen_plan_graph_node_version_mismatch")
    );
    assert!(frozen.company_proposals.proposals.is_empty());
}

#[test]
fn approved_plan_is_one_cas_and_replay_does_not_duplicate_packets() {
    let plan = plan();
    let state = approved_state(&plan);
    let preview = preview_of(&plan, &state);
    let approved = state
        .clone()
        .transition(
            &command(&plan, &state, approval(&preview)),
            &authority(ROLE_SPONSOR),
            &CompanyProof::default(),
        )
        .expect("sponsor approves the reviewed preview");
    let materialization = &approved.company_proposals.materializations["proposal-1"];
    // The hash the user approved equals the effective graph.
    assert_eq!(materialization.approved_plan_digest, preview.plan_digest);
    assert_eq!(materialization.frozen_plan.plan_digest, preview.plan_digest);
    assert_eq!(materialization.packets.len(), 2);
    assert_eq!(materialization.handoffs.len(), 2);
    assert!(materialization
        .handoffs
        .iter()
        .all(|handoff| !handoff.runtime_authority));
    assert_eq!(approved.company_proposals.approvals.len(), 1);
    // The approval creates no milestone, packet, claim, lease or run.
    assert!(approved.milestones.is_empty());
    assert!(approved.packets.is_empty());
    assert!(approved.runs.is_empty());

    // Replaying the exact approval is idempotent at the aggregate boundary.
    let replay = approved
        .clone()
        .transition(
            &command(&plan, &state, approval(&preview)),
            &authority(ROLE_SPONSOR),
            &CompanyProof::default(),
        )
        .expect("exact replay is idempotent");
    assert_eq!(
        replay.company_proposals.materializations["proposal-1"]
            .packets
            .len(),
        2
    );
    assert_eq!(replay.company_proposals.approvals.len(), 1);
    assert!(replay.packets.is_empty());
}

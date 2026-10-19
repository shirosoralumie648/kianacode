use kiana_domain::*;

fn digest_a() -> String {
    format!("sha256:{}", "a".repeat(64))
}

fn node(id: &str, kind: PlanNodeKind, required: bool) -> PlanNode {
    PlanNode {
        node_id: id.to_owned(),
        project_id: "project-1".to_owned(),
        kind,
        version: 1,
        required,
        criterion_refs: Vec::new(),
        input_refs: Vec::new(),
    }
}

fn edge(from: &str, to: &str, kind: PlanEdgeKind) -> PlanEdge {
    PlanEdge {
        from: from.to_owned(),
        to: to.to_owned(),
        kind,
        reference: (!matches!(kind, PlanEdgeKind::ParentChild | PlanEdgeKind::Related))
            .then(|| "run:1".to_owned()),
    }
}

fn plan() -> PlanProposal {
    PlanProposal::new(
        "project-1",
        1,
        digest_a(),
        vec![
            node("milestone-1", PlanNodeKind::Milestone, true),
            node("packet-1", PlanNodeKind::Packet, true),
            node("packet-2", PlanNodeKind::Packet, true),
        ],
        vec![
            edge("milestone-1", "packet-1", PlanEdgeKind::ParentChild),
            edge("milestone-1", "packet-2", PlanEdgeKind::ParentChild),
            edge("packet-1", "packet-2", PlanEdgeKind::RequiresRunSuccess),
        ],
    )
    .expect("plan")
}

/// A graph that passes the Plan contract but cannot become a reference set: the final packet
/// names two parent milestones. It is used to prove the Nth-node failure commits nothing.
fn plan_with_invalid_later_packet() -> PlanProposal {
    PlanProposal::new(
        "project-1",
        1,
        digest_a(),
        vec![
            node("milestone-1", PlanNodeKind::Milestone, true),
            node("milestone-2", PlanNodeKind::Milestone, true),
            node("packet-1", PlanNodeKind::Packet, true),
            node("packet-2", PlanNodeKind::Packet, true),
            node("packet-3", PlanNodeKind::Packet, true),
        ],
        vec![
            edge("milestone-1", "packet-1", PlanEdgeKind::ParentChild),
            edge("milestone-1", "packet-2", PlanEdgeKind::ParentChild),
            edge("milestone-2", "packet-3", PlanEdgeKind::ParentChild),
            edge("milestone-1", "packet-3", PlanEdgeKind::ParentChild),
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
        vec![
            "artifact:charter".to_owned(),
            "artifact:coverage".to_owned(),
        ],
        plan,
        result(),
    )
    .expect("proposal")
}

fn task(proposal: CompanyPlanProposal) -> CompanyRoleTaskResult {
    CompanyRoleTaskResult::new("task-1", "assignment:pm", ROLE_PM, "session:pm", proposal)
        .expect("role task result")
}

fn basis_for(plan: &PlanProposal) -> CompanyPlanMaterializationBasis {
    CompanyPlanMaterializationBasis {
        project_id: "project-1".to_owned(),
        expected_revision: 0,
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

fn submitted() -> (CompanyProposalLedger, CompanyPlanPreview) {
    let mut ledger = CompanyProposalLedger::default();
    let plan = plan();
    let preview = ledger
        .submit(
            CompanyPlanProposalIntake::role_task(task(proposal_with(plan.clone()))),
            basis_for(&plan),
        )
        .expect("submit");
    (ledger, preview)
}

/// Card deny: README, stdout and assistant text that contains a full command/proposal JSON body
/// cannot enter the ledger, because the intake only accepts an explicit structured role-task
/// result. Nothing is committed and no command text is ever parsed.
#[test]
fn proposal_text_cannot_execute_commands_and_invalid_plan_commits_nothing() {
    let plan = plan();
    let command_json = serde_json::json!({
        "schema": "kiana.company-command.v1",
        "expected_revision": 0,
        "idempotency_key": "free-text",
        "command": { "FreezePlanGraph": { "plan": plan } },
    })
    .to_string();
    let mut ledger = CompanyProposalLedger::default();
    for source in [
        CompanyProposalSourceKind::Readme,
        CompanyProposalSourceKind::Stdout,
        CompanyProposalSourceKind::AssistantMessage,
        CompanyProposalSourceKind::Transcript,
    ] {
        let intake = CompanyPlanProposalIntake::text_surface(source, command_json.clone());
        assert_eq!(
            ledger
                .submit(intake, basis_for(&plan))
                .expect_err("prose surfaces must not become facts"),
            "company_proposal_source_not_role_task_result"
        );
    }
    assert!(ledger.proposals.is_empty());
    assert!(ledger.previews.is_empty());
    assert!(ledger.approvals.is_empty());
    assert!(ledger.materializations.is_empty());

    // An intake that claims the structured kind but carries no typed result is refused too.
    let mut untyped = CompanyPlanProposalIntake::text_surface(
        CompanyProposalSourceKind::RoleTaskResult,
        command_json,
    );
    untyped.task = None;
    assert_eq!(
        ledger
            .submit(untyped, basis_for(&plan))
            .expect_err("kind without a typed result is refused"),
        "company_proposal_source_not_role_task_result"
    );
    assert!(ledger.proposals.is_empty());

    // Only an authorized planning role may produce the result.
    let builder_task = CompanyRoleTaskResult::new(
        "task-1",
        "assignment:builder",
        ROLE_BUILDER,
        "session:builder",
        proposal_with(plan.clone()),
    );
    assert_eq!(
        builder_task.unwrap_err(),
        "company_proposal_source_role_denied"
    );

    // The Nth packet failure: the Plan contract passes, but packet-3 cannot be given one parent
    // milestone. Submit refuses the whole batch, so no proposal or preview survives.
    let mut ledger = CompanyProposalLedger::default();
    let invalid_plan = plan_with_invalid_later_packet();
    assert_eq!(
        ledger
            .submit(
                CompanyPlanProposalIntake::role_task(task(proposal_with(invalid_plan.clone()))),
                basis_for(&invalid_plan),
            )
            .expect_err("the whole batch must be refused"),
        "company_plan_preview_packet_parent_invalid"
    );
    assert!(ledger.proposals.is_empty());
    assert!(ledger.previews.is_empty());
    assert!(ledger.approvals.is_empty());
    assert!(ledger.materializations.is_empty());
    assert_eq!(ledger.revision, 0);
}

/// Input version, graph, scope, budget and criteria coverage are validated before a preview
/// exists, and one approval CAS rejects stale revisions and drifted preview digests.
#[test]
fn preview_refuses_version_budget_and_criteria_drift_before_any_preview_exists() {
    let plan = plan();

    let mut ledger = CompanyProposalLedger::default();
    let mut drifted = basis_for(&plan);
    drifted.node_versions.insert("packet-2".to_owned(), 2);
    assert_eq!(
        ledger
            .submit(
                CompanyPlanProposalIntake::role_task(task(proposal_with(plan.clone()))),
                drifted,
            )
            .unwrap_err(),
        "company_plan_preview_node_version_mismatch"
    );
    assert!(ledger.proposals.is_empty());

    let mut missing = basis_for(&plan);
    missing.node_versions.remove("packet-2");
    assert_eq!(
        ledger
            .submit(
                CompanyPlanProposalIntake::role_task(task(proposal_with(plan.clone()))),
                missing,
            )
            .unwrap_err(),
        "company_plan_preview_node_version_missing"
    );

    let mut over_budget = basis_for(&plan);
    over_budget.max_packets = 1;
    assert_eq!(
        ledger
            .submit(
                CompanyPlanProposalIntake::role_task(task(proposal_with(plan.clone()))),
                over_budget,
            )
            .unwrap_err(),
        "company_plan_preview_packet_budget_exceeded"
    );

    let mut wrong_coverage = basis_for(&plan);
    wrong_coverage.coverage_digest = format!("sha256:{}", "b".repeat(64));
    assert_eq!(
        ledger
            .submit(
                CompanyPlanProposalIntake::role_task(task(proposal_with(plan.clone()))),
                wrong_coverage,
            )
            .unwrap_err(),
        "company_plan_preview_coverage_mismatch"
    );

    let (mut ledger, preview) = submitted();

    // A sponsor approval bound to a different preview digest cannot consume the CAS.
    let mut forged = approval(&preview);
    forged.preview_digest = format!("sha256:{}", "f".repeat(64));
    forged.digest = forged.canonical_digest();
    assert_eq!(
        ledger.approve(forged).unwrap_err(),
        "company_plan_approval_preview_mismatch"
    );
    assert!(ledger.approvals.is_empty());
    assert!(ledger.materializations.is_empty());

    // A stale revision cannot consume the CAS either.
    let mut stale = approval(&preview);
    stale.expected_revision = 9;
    stale.digest = stale.canonical_digest();
    assert_eq!(
        ledger.approve(stale).unwrap_err(),
        "company_plan_approval_revision_conflict"
    );
    assert!(ledger.approvals.is_empty());
    assert!(ledger.materializations.is_empty());
    assert_eq!(ledger.revision, 0);

    // A non-Sponsor approver is refused by the approval contract.
    let mut non_sponsor = approval(&preview);
    non_sponsor.approver_role = "pm".to_owned();
    non_sponsor.digest = non_sponsor.canonical_digest();
    assert_eq!(
        ledger.approve(non_sponsor).unwrap_err(),
        "company_plan_approval_sponsor_required"
    );
    assert!(ledger.approvals.is_empty());
}

#[test]
fn approved_plan_materializes_the_exact_reviewed_packet_graph_once() {
    let (mut ledger, preview) = submitted();
    // The preview is the fully validated Plan the user reviews: every node version is frozen and
    // the digest covers the complete reference set.
    assert_eq!(preview.plan_digest, plan().digest);
    assert_eq!(preview.frozen_plan.plan_digest, plan().digest);
    assert_eq!(preview.packets.len(), 2);
    assert_eq!(preview.milestones.len(), 1);
    assert_eq!(preview.handoffs.len(), 2);
    assert!(preview.handoffs.iter().all(|h| !h.runtime_authority));
    assert_eq!(
        preview
            .packets
            .iter()
            .find(|packet| packet.packet_id == "packet-2")
            .unwrap()
            .dependencies,
        vec!["packet-1".to_owned()]
    );

    let materialization = ledger.approve(approval(&preview)).expect("approve");
    // The approved hash equals the effective graph.
    assert_eq!(materialization.approved_plan_digest, preview.plan_digest);
    assert_eq!(materialization.frozen_plan.plan_digest, preview.plan_digest);
    assert_eq!(
        materialization.approved_plan_digest,
        ledger.approved_plan("proposal-1").unwrap().digest
    );
    assert_eq!(materialization.packets.len(), 2);
    assert_eq!(materialization.handoffs.len(), 2);
    assert_eq!(materialization.company_revision, 1);
    assert_eq!(ledger.revision, 1);

    // Repeating the exact approval is idempotent: the same materialization comes back and no
    // packet reference is duplicated.
    let replay = ledger.approve(approval(&preview)).expect("replay approval");
    assert_eq!(replay, materialization);
    assert_eq!(ledger.materializations["proposal-1"].packets.len(), 2);
    assert_eq!(ledger.approvals.len(), 1);
    assert_eq!(ledger.revision, 1);

    // A different approval for the same proposal cannot approve it a second time.
    let mut duplicate = approval(&preview);
    duplicate.approval_id = "approval-2".to_owned();
    duplicate.digest = duplicate.canonical_digest();
    assert_eq!(
        ledger.approve(duplicate).unwrap_err(),
        "company_plan_approval_duplicate"
    );
    assert_eq!(ledger.materializations["proposal-1"].packets.len(), 2);
}

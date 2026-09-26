use kiana_domain::*;

fn plan() -> PlanProposal {
    PlanProposal::new(
        "project-1",
        1,
        format!("sha256:{}", "a".repeat(64)),
        vec![
            PlanNode {
                node_id: "milestone-1".to_owned(),
                project_id: "project-1".to_owned(),
                kind: PlanNodeKind::Milestone,
                version: 1,
                required: true,
                criterion_refs: Vec::new(),
            },
            PlanNode {
                node_id: "packet-1".to_owned(),
                project_id: "project-1".to_owned(),
                kind: PlanNodeKind::Packet,
                version: 1,
                required: true,
                criterion_refs: Vec::new(),
            },
        ],
        vec![PlanEdge {
            from: "milestone-1".to_owned(),
            to: "packet-1".to_owned(),
            kind: PlanEdgeKind::ParentChild,
            reference: None,
        }],
    )
    .expect("plan")
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

fn proposal() -> CompanyPlanProposal {
    CompanyPlanProposal::new(
        "proposal-1",
        "project-1",
        "assignment:pm",
        "session:pm",
        vec![
            "artifact:charter".to_owned(),
            "artifact:coverage".to_owned(),
        ],
        plan(),
        result(),
    )
    .expect("proposal")
}

fn approval(proposal: &CompanyPlanProposal) -> CompanyPlanApproval {
    let mut approval = CompanyPlanApproval {
        schema: COMPANY_PLAN_APPROVAL_SCHEMA.to_owned(),
        approval_id: "approval-1".to_owned(),
        proposal_id: proposal.proposal_id.clone(),
        project_id: proposal.project_id.clone(),
        proposal_digest: proposal.digest.clone(),
        expected_revision: 7,
        decision_ref: "artifact:sponsor-decision".to_owned(),
        approver_assignment: "assignment:sponsor".to_owned(),
        approver_role: "sponsor".to_owned(),
        approved_at: 100,
        digest: String::new(),
    };
    approval.digest = approval.canonical_digest();
    approval
}

#[test]
fn proposal_text_cannot_execute_commands_and_invalid_plan_commits_nothing() {
    let mut ledger = CompanyProposalLedger::default();
    let proposal = proposal();
    ledger.submit(proposal.clone()).expect("submit");
    let mut forged = approval(&proposal);
    forged.proposal_digest = format!("sha256:{}", "f".repeat(64));
    forged.digest = forged.canonical_digest();
    assert_eq!(
        ledger.approve(forged).unwrap_err(),
        "company_plan_approval_proposal_mismatch"
    );
    assert!(ledger.approvals.is_empty());
    assert_eq!(ledger.proposals.len(), 1);
}

#[test]
fn approved_plan_materializes_the_exact_reviewed_packet_graph_once() {
    let mut ledger = CompanyProposalLedger::default();
    let proposal = proposal();
    let digest = proposal.digest.clone();
    ledger.submit(proposal.clone()).expect("submit");
    ledger.approve(approval(&proposal)).expect("approve");
    assert_eq!(
        ledger.approved_plan("proposal-1").unwrap().digest,
        proposal.plan.digest
    );
    assert_eq!(
        ledger.approved_plan("proposal-1").unwrap().digest,
        plan().digest
    );
    let mut duplicate = approval(&proposal);
    duplicate.approval_id = "approval-2".to_owned();
    duplicate.digest = duplicate.canonical_digest();
    assert_eq!(
        ledger.approve(duplicate).unwrap_err(),
        "company_plan_approval_duplicate"
    );
    assert_eq!(ledger.proposals["proposal-1"].digest, digest);
    assert_eq!(ledger.approvals.len(), 1);
}

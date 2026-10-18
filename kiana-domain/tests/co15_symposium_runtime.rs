//! CO-15 deny-first fixtures: the bounded role meeting budget, the structured contribution
//! contract, and the single decision gate shared by convened and anti-meeting paths.

use kiana_domain::*;
use serde_json::json;

/// Deny first: every limit and cancellation refuses the closing decision with a stable code.
#[test]
fn symposium_budget_exhaustion_or_cancel_never_emits_an_approved_decision() {
    // The schedule headroom: rounds + 1 and (rounds * attendees) + 1.
    let budget = SymposiumRunBudget::for_schedule(1, 2).expect("schedule budget");
    assert_eq!(budget.max_rounds, 2);
    assert_eq!(budget.max_messages, 3);
    assert_eq!(budget.max_tokens, 3 * SYMPOSIUM_TOKENS_PER_CONTRIBUTION);

    let mut rounds = SymposiumRunBudget::for_schedule(1, 2).expect("budget");
    rounds.record_round(true).expect("round one");
    rounds.record_round(true).expect("round two");
    assert_eq!(
        rounds.record_round(true),
        Err("symposium_round_budget_exhausted")
    );
    assert_eq!(
        rounds.decision_allowed().unwrap_err(),
        "symposium_round_budget_exhausted"
    );

    let mut messages = SymposiumRunBudget::new(2, 2, 1_000, 10_000, 1).expect("budget");
    messages.record_message(10).expect("message one");
    messages.record_message(10).expect("message two");
    assert_eq!(
        messages.record_message(10),
        Err("symposium_message_budget_exhausted")
    );
    assert_eq!(
        messages.decision_allowed().unwrap_err(),
        "symposium_message_budget_exhausted"
    );

    let mut tokens = SymposiumRunBudget::new(2, 4, 100, 10_000, 1).expect("budget");
    tokens.record_message(100).expect("message at ceiling");
    assert_eq!(
        tokens.record_message(1),
        Err("symposium_token_budget_exhausted")
    );
    assert_eq!(tokens.tokens_used, 100);
    assert_eq!(
        tokens.decision_allowed().unwrap_err(),
        "symposium_token_budget_exhausted"
    );

    let mut wall = SymposiumRunBudget::new(2, 4, 1_000, 10_000, 1).expect("budget");
    wall.record_elapsed(1_000).expect("elapsed below ceiling");
    assert_eq!(
        wall.record_elapsed(10_000),
        Err("symposium_wall_time_exhausted")
    );
    assert_eq!(
        wall.decision_allowed().unwrap_err(),
        "symposium_wall_time_exhausted"
    );

    let mut stalled = SymposiumRunBudget::new(4, 8, 1_000, 10_000, 1).expect("budget");
    stalled.record_round(false).expect("first silent round");
    assert_eq!(
        stalled.record_round(false),
        Err("symposium_stall_limit_exhausted")
    );
    assert_eq!(
        stalled.decision_allowed().unwrap_err(),
        "symposium_stall_limit_exhausted"
    );

    let mut cancelled = SymposiumRunBudget::new(2, 2, 100, 10_000, 1).expect("budget");
    cancelled.cancel();
    assert_eq!(cancelled.record_message(1), Err("symposium_cancelled"));
    assert_eq!(
        cancelled.decision_allowed().unwrap_err(),
        "symposium_cancelled"
    );

    // A malformed ledger can never be read as "allowed".
    let mut invalid = SymposiumRunBudget::new(1, 1, 1, 1, 1).expect("budget");
    invalid.max_rounds = 0;
    assert_eq!(
        invalid.decision_allowed().unwrap_err(),
        "symposium_run_budget_invalid"
    );
}

/// Deny first: the model only proposes. Core refuses any payload that is not one of the accepted
/// structured shapes, and refuses unknown kinds, unknown visibility and unknown fields.
#[test]
fn symposium_structured_contribution_rejects_unshaped_model_payloads() {
    let proposal: SymposiumContributionProposal = serde_json::from_value(json!({
        "kind": "claim",
        "text": "ship one vertical slice",
        "evidence_refs": ["plan/agenda.md"],
    }))
    .expect("typed proposal");
    assert_eq!(
        proposal.contribution_kind(),
        Ok(SymposiumContributionKind::Claim)
    );
    assert_eq!(proposal.visibility(), Ok(SymposiumVisibility::Blackboard));

    let wrapped = parse_symposium_contributions(&json!({
        "contributions": [{"kind": "vote", "text": "agree", "evidence_refs": ["plan/agenda.md"]}],
    }))
    .expect("wrapped list");
    assert_eq!(wrapped.len(), 1);
    assert_eq!(
        wrapped[0].contribution_kind(),
        Ok(SymposiumContributionKind::Vote)
    );

    // A bare array is accepted; an empty array is an honest abstention, not an error.
    assert!(parse_symposium_contributions(&json!([]))
        .expect("empty list")
        .is_empty());

    for invalid in [
        json!("plain transcript text"),
        json!(7),
        json!({"kind": "claim"}),
        json!({"kind": "claim", "text": "x", "evidence_refs": ["ref"], "may_decide": true}),
        json!({"contributions": "not a list"}),
    ] {
        assert!(
            parse_symposium_contributions(&invalid).is_err(),
            "unshaped payload accepted: {invalid}"
        );
    }
    // Missing evidence is parsed but never admitted: the governed board requires evidence.
    assert!(parse_symposium_contributions(&json!({"kind": "claim", "text": "x"})).is_ok());
    assert_eq!(
        SymposiumContribution::new(
            "c-no-evidence",
            "sym-1",
            "sym-1-pm",
            1,
            SymposiumContributionKind::Claim,
            "x",
            SymposiumVisibility::Blackboard,
            Vec::new(),
        ),
        Err("symposium_evidence_required")
    );

    let bad_kind = SymposiumContributionProposal {
        kind: "decision".to_owned(),
        text: "x".to_owned(),
        evidence_refs: vec!["ref".to_owned()],
        visibility: None,
    };
    assert_eq!(
        bad_kind.contribution_kind(),
        Err("symposium_contribution_kind_invalid")
    );
    let bad_visibility = SymposiumContributionProposal {
        kind: "claim".to_owned(),
        text: "x".to_owned(),
        evidence_refs: vec!["ref".to_owned()],
        visibility: Some("broadcast_everything".to_owned()),
    };
    assert_eq!(
        bad_visibility.visibility(),
        Err("symposium_contribution_visibility_invalid")
    );
}

fn planning_governance() -> SymposiumGovernance {
    let mut assignments = std::collections::BTreeMap::new();
    assignments.insert(ROLE_PM.to_owned(), "sym-1-pm".to_owned());
    assignments.insert(ROLE_ARCHITECT.to_owned(), "sym-1-architect".to_owned());
    SymposiumGovernance::new(
        "sym-1",
        "project-1",
        1,
        "plan/agenda.md",
        "sym-1-pm",
        assignments,
        2,
        SYMPOSIUM_CONTRIBUTION_PROPOSAL_SCHEMA,
        SymposiumVisibility::Blackboard,
    )
    .expect("governance")
}

/// Deny first: the board refuses uninvited contributors, stale baselines, duplicate identity,
/// architect private scratch and a decision that rests only on votes. Private scratch never
/// reaches another member's view. The invited, evidence-backed claim is then recorded.
#[test]
fn symposium_board_refuses_scope_breach_and_private_scratch_broadcast() {
    let mut board = SymposiumBoard::new(planning_governance()).expect("board");

    let uninvited = SymposiumContribution::new(
        "c-uninvited",
        "sym-1",
        "sym-1-reviewer",
        1,
        SymposiumContributionKind::Claim,
        "not invited",
        SymposiumVisibility::Blackboard,
        vec!["plan/agenda.md".to_owned()],
    )
    .expect("contribution shape");
    assert_eq!(
        board.record(uninvited),
        Err("symposium_contributor_uninvited")
    );

    let architect_private = SymposiumContribution::new(
        "c-private",
        "sym-1",
        "sym-1-architect",
        1,
        SymposiumContributionKind::Claim,
        "private note",
        SymposiumVisibility::PrivateBrief,
        vec!["plan/agenda.md".to_owned()],
    )
    .expect("contribution shape");
    assert_eq!(
        board.record(architect_private),
        Err("symposium_private_brief_visibility_denied")
    );

    let stale = SymposiumContribution::new(
        "c-stale",
        "sym-1",
        "sym-1-architect",
        2,
        SymposiumContributionKind::Claim,
        "stale baseline",
        SymposiumVisibility::Blackboard,
        vec!["plan/agenda.md".to_owned()],
    )
    .expect("contribution shape");
    assert_eq!(board.record(stale), Err("symposium_contribution_stale"));

    let claim = SymposiumContribution::new(
        "c-1",
        "sym-1",
        "sym-1-pm",
        1,
        SymposiumContributionKind::Claim,
        "ship one vertical slice",
        SymposiumVisibility::Blackboard,
        vec!["plan/agenda.md".to_owned()],
    )
    .expect("contribution");
    board.record(claim.clone()).expect("invited claim");
    assert_eq!(board.record(claim), Err("symposium_contribution_duplicate"));

    // The chair's private scratch is admissible to the chair only; it is never broadcast.
    let chair_private = SymposiumContribution::new(
        "c-chair-private",
        "sym-1",
        "sym-1-pm",
        1,
        SymposiumContributionKind::Claim,
        "chair scratch",
        SymposiumVisibility::PrivateBrief,
        vec!["plan/agenda.md".to_owned()],
    )
    .expect("contribution shape");
    board.record(chair_private).expect("chair private brief");
    let architect_view = board.visible_to("sym-1-architect");
    assert!(architect_view
        .iter()
        .all(|contribution| contribution.visibility == SymposiumVisibility::Blackboard));
    assert!(architect_view
        .iter()
        .any(|contribution| contribution.contribution_id == "c-1"));
    assert!(!architect_view
        .iter()
        .any(|contribution| contribution.contribution_id == "c-chair-private"));
    assert!(board.visible_to("sym-1-reviewer").is_empty());
}

/// Deny first: a decision that cites only recorded votes is refused even though the deciding
/// assignment is the chair. Vote weight is advisory and can never stand in for approval.
#[test]
fn symposium_vote_only_decision_is_refused() {
    let mut board = SymposiumBoard::new(planning_governance()).expect("board");
    let vote = SymposiumContribution::new(
        "v-1",
        "sym-1",
        "sym-1-architect",
        1,
        SymposiumContributionKind::Vote,
        "agree",
        SymposiumVisibility::Blackboard,
        vec!["plan/agenda.md".to_owned()],
    )
    .expect("vote");
    board.record(vote).expect("vote recorded");
    assert!(board.has_votes());

    let mut vote_only = SymposiumDecision {
        schema: SYMPOSIUM_DECISION_SCHEMA.to_owned(),
        symposium_id: "sym-1".to_owned(),
        baseline_version: 1,
        summary: "votes only".to_owned(),
        decision: "approve".to_owned(),
        alternatives: Vec::new(),
        dissent: Vec::new(),
        unresolved: Vec::new(),
        decided_by_assignment: "sym-1-pm".to_owned(),
        evidence_refs: vec!["v-1".to_owned()],
        sponsor_approval_ref: None,
        digest: String::new(),
    };
    vote_only.digest = vote_only.canonical_digest();
    assert_eq!(
        board.publish_decision(vote_only),
        Err("symposium_vote_does_not_approve")
    );
    assert!(board.decision.is_none());

    // A decision citing a claim as well as the vote is admissible; the gate is about basis.
    let claim = SymposiumContribution::new(
        "c-1",
        "sym-1",
        "sym-1-pm",
        1,
        SymposiumContributionKind::Claim,
        "ship one vertical slice",
        SymposiumVisibility::Blackboard,
        vec!["plan/agenda.md".to_owned()],
    )
    .expect("claim");
    board.record(claim).expect("claim recorded");
    let mut decided = SymposiumDecision {
        schema: SYMPOSIUM_DECISION_SCHEMA.to_owned(),
        symposium_id: "sym-1".to_owned(),
        baseline_version: 1,
        summary: "claim with vote".to_owned(),
        decision: "approve".to_owned(),
        alternatives: Vec::new(),
        dissent: Vec::new(),
        unresolved: Vec::new(),
        decided_by_assignment: "sym-1-pm".to_owned(),
        evidence_refs: vec!["c-1".to_owned(), "v-1".to_owned()],
        sponsor_approval_ref: None,
        digest: String::new(),
    };
    decided.digest = decided.canonical_digest();
    board
        .publish_decision(decided)
        .expect("claim-backed decision");
}

/// Success last: a convened bounded meeting and the anti-meeting shortcut close through the same
/// decision gate and emit the same `DecisionRecord` contract; both replay deterministically.
#[test]
fn symposium_and_async_proposal_share_the_same_decision_gate() {
    let mut convened = SymposiumRunBudget::for_schedule(1, 2).expect("budget");
    convened.record_round(true).expect("round");
    convened.record_message(64).expect("speaker one");
    convened.record_message(64).expect("speaker two");
    assert_eq!(convened.decision_allowed(), Ok(()));

    // The anti-meeting path makes no speaker call; its ledger is untouched but still admitted.
    let anti_meeting = SymposiumRunBudget::for_schedule(1, 2).expect("budget");
    assert_eq!(anti_meeting.decision_allowed(), Ok(()));

    let mut skipped_meeting = Symposium::planning("sym-anti", "create GOLDEN_PATH.txt", 1);
    let skipped = skipped_meeting.close(true).expect("anti-meeting close");
    let mut meeting = Symposium::planning("sym-convened", "create GOLDEN_PATH.txt", 1);
    meeting
        .blackboard
        .claims
        .push(SymposiumClaim::new(ROLE_PM, "create GOLDEN_PATH.txt"));
    let closed = meeting.close(false).expect("convened close");

    assert_eq!(skipped.0.schema, closed.0.schema);
    assert!(skipped.0.skipped_meeting);
    assert!(!closed.0.skipped_meeting);
    let anti_packet = skipped.1.expect("anti-meeting packet");
    let convened_packet = closed.1.expect("convened packet");
    assert_eq!(anti_packet.schema, convened_packet.schema);
    assert_eq!(anti_packet.goal, convened_packet.goal);
    assert_eq!(skipped.0.work_packet_id, anti_packet.id);
    assert_eq!(closed.0.work_packet_id, convened_packet.id);

    // Once cancelled, the same gate refuses the close on the convened path.
    let mut cancelled = SymposiumRunBudget::for_schedule(1, 2).expect("budget");
    cancelled.cancel();
    assert_eq!(
        cancelled.decision_allowed().unwrap_err(),
        "symposium_cancelled"
    );
}

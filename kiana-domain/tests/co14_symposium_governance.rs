use kiana_domain::*;
use std::collections::BTreeMap;

fn governance() -> SymposiumGovernance {
    SymposiumGovernance::new(
        "symposium-1",
        "project-1",
        3,
        "artifact:agenda",
        "assignment:pm",
        BTreeMap::from([
            ("pm".to_owned(), "assignment:pm".to_owned()),
            ("architect".to_owned(), "assignment:architect".to_owned()),
            ("sponsor".to_owned(), "assignment:sponsor".to_owned()),
        ]),
        4,
        "kiana.symposium-output.v1",
        SymposiumVisibility::Blackboard,
    )
    .expect("governance")
}

fn contribution(
    id: &str,
    assignment: &str,
    version: u64,
    visibility: SymposiumVisibility,
) -> SymposiumContribution {
    SymposiumContribution::new(
        id,
        "symposium-1",
        assignment,
        version,
        SymposiumContributionKind::Claim,
        "bounded proposal",
        visibility,
        vec!["artifact:evidence".to_owned()],
    )
    .expect("contribution")
}

#[test]
fn symposium_rejects_uninvited_stale_and_duplicate_contributions() {
    let mut board = SymposiumBoard::new(governance()).expect("board");
    assert_eq!(
        board
            .record(contribution(
                "c-uninvited",
                "assignment:unknown",
                3,
                SymposiumVisibility::Blackboard,
            ))
            .unwrap_err(),
        "symposium_contributor_uninvited"
    );
    assert_eq!(
        board
            .record(contribution(
                "c-stale",
                "assignment:architect",
                2,
                SymposiumVisibility::Blackboard,
            ))
            .unwrap_err(),
        "symposium_contribution_stale"
    );
    board
        .record(contribution(
            "c-valid",
            "assignment:architect",
            3,
            SymposiumVisibility::Blackboard,
        ))
        .expect("valid contribution");
    assert_eq!(
        board
            .record(contribution(
                "c-valid",
                "assignment:architect",
                3,
                SymposiumVisibility::Blackboard,
            ))
            .unwrap_err(),
        "symposium_contribution_duplicate"
    );
    assert_eq!(board.visible_to("assignment:pm").len(), 1);
}

#[test]
fn symposium_decision_preserves_alternatives_dissent_and_evidence() {
    let mut board = SymposiumBoard::new(governance()).expect("board");
    board
        .record(contribution(
            "c-private",
            "assignment:pm",
            3,
            SymposiumVisibility::PrivateBrief,
        ))
        .expect("chair private brief");
    let mut decision = SymposiumDecision {
        schema: SYMPOSIUM_DECISION_SCHEMA.to_owned(),
        symposium_id: "symposium-1".to_owned(),
        baseline_version: 3,
        summary: "select bounded plan".to_owned(),
        decision: "option-a".to_owned(),
        alternatives: vec!["option-b".to_owned()],
        dissent: vec!["architect requests a smaller scope".to_owned()],
        unresolved: vec!["cost observation".to_owned()],
        decided_by_assignment: "assignment:pm".to_owned(),
        evidence_refs: vec!["artifact:evidence".to_owned()],
        sponsor_approval_ref: None,
        digest: String::new(),
    };
    decision.digest = decision.canonical_digest();
    board.publish_decision(decision).expect("decision");
    assert!(board
        .decision
        .as_ref()
        .unwrap()
        .sponsor_approval_ref
        .is_none());
    board
        .attach_sponsor_approval("decision:sponsor-1", "assignment:sponsor")
        .expect("sponsor approval");
    assert_eq!(
        board
            .decision
            .as_ref()
            .unwrap()
            .sponsor_approval_ref
            .as_deref(),
        Some("decision:sponsor-1")
    );
}

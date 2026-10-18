use kiana_domain::*;
use std::collections::BTreeMap;

fn attendee_assignments() -> BTreeMap<String, String> {
    BTreeMap::from([
        ("pm".to_owned(), "assignment:pm".to_owned()),
        ("architect".to_owned(), "assignment:architect".to_owned()),
        ("sponsor".to_owned(), "assignment:sponsor".to_owned()),
    ])
}

fn scope(
    assignment: &str,
    role: &str,
    kinds: Vec<SymposiumContributionKind>,
    may_decide: bool,
) -> SymposiumAssignmentScope {
    SymposiumAssignmentScope::new(assignment, role, kinds, may_decide).expect("scope")
}

/// The cross-department composition contract: the chair may decide, the architect sees and
/// records claims/votes without deciding, and the sponsor holds the separate approval authority.
/// The product rule still refuses to convene a joint meeting.
fn governance() -> SymposiumGovernance {
    SymposiumGovernance::joint(
        "symposium-1",
        "project-1",
        3,
        "artifact:agenda",
        "assignment:pm",
        attendee_assignments(),
        4,
        "kiana.symposium-output.v1",
        SymposiumVisibility::Blackboard,
        BTreeMap::from([
            (
                "assignment:pm".to_owned(),
                scope(
                    "assignment:pm",
                    "pm",
                    SymposiumContributionKind::ALL.to_vec(),
                    true,
                ),
            ),
            (
                "assignment:architect".to_owned(),
                scope(
                    "assignment:architect",
                    "architect",
                    vec![
                        SymposiumContributionKind::Claim,
                        SymposiumContributionKind::Vote,
                    ],
                    false,
                ),
            ),
            (
                "assignment:sponsor".to_owned(),
                scope(
                    "assignment:sponsor",
                    "sponsor",
                    vec![
                        SymposiumContributionKind::Claim,
                        SymposiumContributionKind::Draft,
                    ],
                    true,
                ),
            ),
        ]),
    )
    .expect("governance")
}

fn contribution(
    id: &str,
    assignment: &str,
    version: u64,
    visibility: SymposiumVisibility,
) -> SymposiumContribution {
    contribution_of(
        id,
        assignment,
        version,
        SymposiumContributionKind::Claim,
        visibility,
        vec!["artifact:evidence".to_owned()],
    )
}

fn contribution_of(
    id: &str,
    assignment: &str,
    version: u64,
    kind: SymposiumContributionKind,
    visibility: SymposiumVisibility,
    evidence_refs: Vec<String>,
) -> SymposiumContribution {
    SymposiumContribution::new(
        id,
        "symposium-1",
        assignment,
        version,
        kind,
        "bounded proposal",
        visibility,
        evidence_refs,
    )
    .expect("contribution")
}

fn decision(decided_by: &str, evidence_refs: Vec<String>) -> SymposiumDecision {
    let mut decision = SymposiumDecision {
        schema: SYMPOSIUM_DECISION_SCHEMA.to_owned(),
        symposium_id: "symposium-1".to_owned(),
        baseline_version: 3,
        summary: "select bounded plan".to_owned(),
        decision: "option-a".to_owned(),
        alternatives: vec!["option-b".to_owned()],
        dissent: vec!["architect requests a smaller scope".to_owned()],
        unresolved: vec!["cost observation".to_owned()],
        decided_by_assignment: decided_by.to_owned(),
        evidence_refs,
        sponsor_approval_ref: None,
        digest: String::new(),
    };
    decision.digest = decision.canonical_digest();
    decision
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
    // An assignment outside the contract sees nothing at all, not the public board.
    assert!(board.visible_to("assignment:unknown").is_empty());
}

#[test]
fn symposium_refuses_scope_breach_votes_as_approval_and_foreign_sponsor_approval() {
    let mut board = SymposiumBoard::new(governance()).expect("board");
    // Votes are advisory: a decision citing only vote contributions is refused.
    board
        .record(contribution_of(
            "c-vote",
            "assignment:architect",
            3,
            SymposiumContributionKind::Vote,
            SymposiumVisibility::Blackboard,
            vec!["artifact:evidence".to_owned()],
        ))
        .expect("vote record");
    assert!(board.has_votes());
    assert_eq!(
        board
            .publish_decision(decision("assignment:pm", vec!["c-vote".to_owned()]))
            .unwrap_err(),
        "symposium_vote_does_not_approve"
    );
    // Naming one non-vote record removes the "solely votes" objection.
    board
        .record(contribution(
            "c-claim",
            "assignment:architect",
            3,
            SymposiumVisibility::Blackboard,
        ))
        .expect("claim record");
    board
        .publish_decision(decision(
            "assignment:pm",
            vec!["c-claim".to_owned(), "c-vote".to_owned()],
        ))
        .expect("decision with claim evidence");
    // Sponsor approval stays a separate attachment; a non-sponsor assignment cannot supply it.
    assert_eq!(
        board
            .attach_sponsor_approval("decision:sponsor-1", "assignment:pm")
            .unwrap_err(),
        "symposium_sponsor_required"
    );
    assert_eq!(
        board
            .attach_sponsor_approval("decision:sponsor-1", "assignment:not-invited")
            .unwrap_err(),
        "symposium_sponsor_uninvited"
    );
    board
        .attach_sponsor_approval("decision:sponsor-1", "assignment:sponsor")
        .expect("sponsor approval");
    assert_eq!(
        board
            .attach_sponsor_approval("decision:sponsor-2", "assignment:sponsor")
            .unwrap_err(),
        "symposium_sponsor_approval_duplicate"
    );
}

#[test]
fn symposium_governance_refuses_undeclared_scope_and_non_convening_department() {
    // A scope map that omits a named assignment fails closed.
    let missing = BTreeMap::from([
        (
            "assignment:pm".to_owned(),
            scope(
                "assignment:pm",
                "pm",
                SymposiumContributionKind::ALL.to_vec(),
                true,
            ),
        ),
        (
            "assignment:architect".to_owned(),
            scope(
                "assignment:architect",
                "architect",
                SymposiumContributionKind::ALL.to_vec(),
                false,
            ),
        ),
    ]);
    assert_eq!(
        SymposiumGovernance::with_assignment_scopes(
            "symposium-1",
            "project-1",
            3,
            "artifact:agenda",
            "assignment:pm",
            attendee_assignments(),
            4,
            "kiana.symposium-output.v1",
            SymposiumVisibility::Blackboard,
            SymposiumGovernanceKind::Joint,
            missing,
        )
        .unwrap_err(),
        "symposium_assignment_scope_missing"
    );
    // A scope bound to a different assignment is refused.
    let mut mismatched = governance().assignment_scopes;
    mismatched.insert(
        "assignment:sponsor".to_owned(),
        scope(
            "assignment:not-this-one",
            "sponsor",
            SymposiumContributionKind::ALL.to_vec(),
            true,
        ),
    );
    assert_eq!(
        SymposiumGovernance::with_assignment_scopes(
            "symposium-1",
            "project-1",
            3,
            "artifact:agenda",
            "assignment:pm",
            attendee_assignments(),
            4,
            "kiana.symposium-output.v1",
            SymposiumVisibility::Blackboard,
            SymposiumGovernanceKind::Joint,
            mismatched,
        )
        .unwrap_err(),
        "symposium_assignment_scope_mismatch"
    );
    // The chair must be the department's can_convene role, not merely a named assignment.
    assert_eq!(
        SymposiumGovernance::with_assignment_scopes(
            "symposium-1",
            "project-1",
            3,
            "artifact:agenda",
            "assignment:architect",
            BTreeMap::from([
                ("pm".to_owned(), "assignment:pm".to_owned()),
                ("architect".to_owned(), "assignment:architect".to_owned()),
            ]),
            4,
            "kiana.symposium-output.v1",
            SymposiumVisibility::Blackboard,
            SymposiumGovernanceKind::Department,
            BTreeMap::from([
                (
                    "assignment:pm".to_owned(),
                    scope(
                        "assignment:pm",
                        "pm",
                        SymposiumContributionKind::ALL.to_vec(),
                        false,
                    ),
                ),
                (
                    "assignment:architect".to_owned(),
                    scope(
                        "assignment:architect",
                        "architect",
                        SymposiumContributionKind::ALL.to_vec(),
                        true,
                    ),
                ),
            ]),
        )
        .unwrap_err(),
        "symposium_decision_chair_required"
    );
    // Roles spanning departments cannot be a department meeting.
    assert_eq!(
        SymposiumGovernance::new(
            "symposium-1",
            "project-1",
            3,
            "artifact:agenda",
            "assignment:pm",
            BTreeMap::from([
                ("pm".to_owned(), "assignment:pm".to_owned()),
                ("reviewer".to_owned(), "assignment:reviewer".to_owned()),
            ]),
            4,
            "kiana.symposium-output.v1",
            SymposiumVisibility::Blackboard,
        )
        .unwrap_err(),
        "symposium_department_cannot_convene"
    );
}

#[test]
fn symposium_governance_contract_is_usable_by_all_five_departments() {
    let mut seen = Vec::new();
    for department in DepartmentSpec::catalog() {
        assert!(department.can_convene, "{}", department.department_id);
        let chair_role = department
            .convene_chair_id()
            .unwrap_or_else(|| panic!("no convening chair for {}", department.department_id));
        let chair = RoleSpec::lookup(&chair_role).expect("convening chair role");
        assert!(chair.can_convene, "{}", department.department_id);
        let mut board = SymposiumBoard::new(
            SymposiumGovernance::new(
                format!("symposium-{}", department.department_id),
                "project-1",
                3,
                "artifact:agenda",
                format!("assignment:{chair_role}"),
                department
                    .roles
                    .iter()
                    .map(|role_id| (role_id.clone(), format!("assignment:{role_id}")))
                    .collect(),
                2,
                "kiana.symposium-output.v1",
                SymposiumVisibility::Blackboard,
            )
            .unwrap_or_else(|error| panic!("{} governance: {error}", department.department_id)),
        )
        .expect("board");
        assert_eq!(
            board.governance.department().map(|spec| spec.department_id),
            Some(department.department_id.clone())
        );
        assert!(board.governance.chair_is_department_convenor());
        assert_eq!(
            board.governance.decision_assignments(),
            vec![format!("assignment:{chair_role}").as_str()]
        );
        board
            .record(
                SymposiumContribution::new(
                    "c-chair",
                    board.governance.symposium_id.clone(),
                    format!("assignment:{chair_role}"),
                    3,
                    SymposiumContributionKind::Claim,
                    "department decision",
                    SymposiumVisibility::Blackboard,
                    vec!["artifact:evidence".to_owned()],
                )
                .expect("chair contribution"),
            )
            .expect("record");
        let mut decision = decision(
            &format!("assignment:{chair_role}"),
            vec!["c-chair".to_owned()],
        );
        decision.symposium_id = board.governance.symposium_id.clone();
        decision.digest = decision.canonical_digest();
        board
            .publish_decision(decision)
            .expect("department decision");
        assert_eq!(
            board.visible_to(&format!("assignment:{chair_role}")).len(),
            1
        );
        seen.push(department.department_id);
    }
    assert_eq!(seen.len(), 5);
    assert_eq!(
        seen,
        DepartmentSpec::catalog()
            .map(|spec| spec.department_id)
            .to_vec()
    );
}

#[test]
fn joint_symposium_lists_visibility_and_decision_scope_per_assignment() {
    let governance = SymposiumGovernance::joint(
        "symposium-joint",
        "project-1",
        3,
        "artifact:agenda",
        "assignment:pm",
        BTreeMap::from([
            ("pm".to_owned(), "assignment:pm".to_owned()),
            ("reviewer".to_owned(), "assignment:reviewer".to_owned()),
            ("sponsor".to_owned(), "assignment:sponsor".to_owned()),
        ]),
        4,
        "kiana.symposium-output.v1",
        SymposiumVisibility::Blackboard,
        BTreeMap::from([
            (
                "assignment:pm".to_owned(),
                scope(
                    "assignment:pm",
                    "pm",
                    SymposiumContributionKind::ALL.to_vec(),
                    true,
                ),
            ),
            (
                "assignment:reviewer".to_owned(),
                scope(
                    "assignment:reviewer",
                    "reviewer",
                    vec![SymposiumContributionKind::Claim],
                    false,
                ),
            ),
            (
                "assignment:sponsor".to_owned(),
                scope(
                    "assignment:sponsor",
                    "sponsor",
                    vec![
                        SymposiumContributionKind::Claim,
                        SymposiumContributionKind::Draft,
                    ],
                    true,
                ),
            ),
        ]),
    )
    .expect("joint contract");
    assert_eq!(governance.kind, SymposiumGovernanceKind::Joint);
    // Every assignment lists what it may see and whether it may decide.
    for (assignment, role, kinds, may_decide) in [
        (
            "assignment:pm",
            "pm",
            SymposiumContributionKind::ALL.to_vec(),
            true,
        ),
        (
            "assignment:reviewer",
            "reviewer",
            vec![SymposiumContributionKind::Claim],
            false,
        ),
        (
            "assignment:sponsor",
            "sponsor",
            vec![
                SymposiumContributionKind::Claim,
                SymposiumContributionKind::Draft,
            ],
            true,
        ),
    ] {
        let declared = governance
            .scope_for_assignment(assignment)
            .unwrap_or_else(|| panic!("missing scope for {assignment}"));
        assert_eq!(declared.role_id, role);
        assert_eq!(declared.visible_kinds, kinds);
        assert_eq!(declared.may_decide, may_decide);
    }
    assert_eq!(
        governance.decision_assignments(),
        vec!["assignment:pm", "assignment:sponsor"]
    );
    let mut board = SymposiumBoard::new(governance).expect("joint board");
    board
        .record(
            SymposiumContribution::new(
                "c-claim",
                "symposium-joint",
                "assignment:pm",
                3,
                SymposiumContributionKind::Claim,
                "joint claim",
                SymposiumVisibility::Blackboard,
                vec!["artifact:evidence".to_owned()],
            )
            .expect("joint claim"),
        )
        .expect("record claim");
    board
        .record(
            SymposiumContribution::new(
                "c-vote",
                "symposium-joint",
                "assignment:pm",
                3,
                SymposiumContributionKind::Vote,
                "joint vote",
                SymposiumVisibility::Blackboard,
                vec!["artifact:evidence".to_owned()],
            )
            .expect("joint vote"),
        )
        .expect("record vote");
    // The reviewer's declared visibility is Claim only: it cannot see the vote.
    let visible_to_reviewer = board.visible_to("assignment:reviewer");
    assert_eq!(visible_to_reviewer.len(), 1);
    assert_eq!(
        visible_to_reviewer[0].kind,
        SymposiumContributionKind::Claim
    );
    // Recorded scope is also enforced on contribution: the reviewer cannot add a vote.
    assert_eq!(
        board
            .record(
                SymposiumContribution::new(
                    "c-reviewer-vote",
                    "symposium-joint",
                    "assignment:reviewer",
                    3,
                    SymposiumContributionKind::Vote,
                    "reviewer vote",
                    SymposiumVisibility::Blackboard,
                    vec!["artifact:evidence".to_owned()],
                )
                .expect("reviewer vote record")
            )
            .unwrap_err(),
        "symposium_contribution_scope_denied"
    );
    // An assignment without declared decision authority cannot decide.
    let mut reviewer_decision = decision("assignment:reviewer", vec!["c-claim".to_owned()]);
    reviewer_decision.symposium_id = "symposium-joint".to_owned();
    reviewer_decision.digest = reviewer_decision.canonical_digest();
    assert_eq!(
        board.publish_decision(reviewer_decision).unwrap_err(),
        "symposium_decision_chair_required"
    );
    let mut chair = decision("assignment:pm", vec!["c-claim".to_owned()]);
    chair.symposium_id = "symposium-joint".to_owned();
    chair.digest = chair.canonical_digest();
    board.publish_decision(chair).expect("chair decision");
    // The sponsor's separate decision authority is what carries the approval attachment here.
    board
        .attach_sponsor_approval("decision:sponsor-1", "assignment:sponsor")
        .expect("sponsor approval within declared scope");
    // The frozen product rule still refuses to convene a joint meeting.
    let mut frozen = Symposium::planning("sym-joint", "joint vertical slice", 2);
    frozen.attendees = vec![ROLE_PM.to_owned(), ROLE_REVIEWER.to_owned()];
    assert_eq!(frozen.validate(), Err("joint_symposium_frozen"));
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
    board
        .publish_decision(decision(
            "assignment:pm",
            vec!["artifact:evidence".to_owned()],
        ))
        .expect("decision");
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
    let published = board.decision.as_ref().unwrap();
    assert_eq!(published.alternatives, vec!["option-b".to_owned()]);
    assert_eq!(published.dissent.len(), 1);
    assert_eq!(published.unresolved, vec!["cost observation".to_owned()]);
    assert_eq!(
        published.evidence_refs,
        vec!["artifact:evidence".to_owned()]
    );
}

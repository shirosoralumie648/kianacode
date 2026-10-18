//! CO-14 source guards: the governed symposium ledger stays a pure contract on the single
//! ControlPlane path, enforces per-assignment visibility/decision scope and never turns votes
//! into approval or convenes a frozen joint meeting.
//!
//! The first guard pins the contract surface: typed contribution kinds, per-assignment scopes,
//! stable refusal codes and the explicit separation of votes and sponsor approval. The second
//! guard pins the wiring: the board methods that enforce scope, the deny-first fixture order, the
//! five-department and joint-scope fixtures, the manual lane and the shard ledger.
#[path = "support/source_guard.rs"]
mod source_guard;

#[test]
fn symposium_governance_binds_assignment_baseline_and_decision_evidence() {
    let governance = include_str!("../../kiana-domain/src/symposium_governance.rs");
    let existing = include_str!("../../kiana-domain/src/symposiums.rs");
    let core = include_str!("../src/company.rs");

    let variants = source_guard::item_source(governance, "pub enum SymposiumContributionKind");
    for marker in [
        "GOVERNED_SYMPOSIUM_SCHEMA",
        "SYMPOSIUM_CONTRIBUTION_SCHEMA",
        "SYMPOSIUM_DECISION_SCHEMA",
        "SYMPOSIUM_ASSIGNMENT_SCOPE_SCHEMA",
        "SymposiumGovernance",
        "SymposiumBoard",
        "SymposiumContributionKind::Claim",
        "SymposiumContributionKind::Vote",
        "SymposiumContributionKind::Draft",
        "PrivateBrief",
        "attendee_assignments",
        "target_baseline_version",
        "symposium_contributor_uninvited",
        "symposium_contribution_stale",
        "symposium_contribution_duplicate",
        "symposium_decision_chair_required",
        "attach_sponsor_approval",
        "sponsor_approval_ref",
        "dissent",
        "unresolved",
        "evidence_refs",
    ] {
        if let Some(variant) = marker.strip_prefix("SymposiumContributionKind::") {
            assert!(
                variants
                    .lines()
                    .any(|line| line.trim() == format!("{variant},")),
                "typed variant missing: {marker}"
            );
        } else {
            assert!(
                governance.contains(marker) || existing.contains(marker) || core.contains(marker),
                "CO-14 marker missing: {marker}"
            );
        }
    }
    assert!(existing.contains("pub struct Symposium"));
    assert!(existing.contains("pub struct DecisionRecord"));
    assert!(!governance.contains("CapabilityBroker"));
    assert!(!governance.contains("EventStorePort"));
    assert!(!core.contains("SymposiumBoard::execute"));

    // Per-assignment scope is a typed contract: visible contribution kinds plus decision
    // authority, declared for every named assignment. The joint kind is a composition contract
    // only; the frozen product rule still lives in the legacy Symposium.
    let board = source_guard::item_source(governance, "impl SymposiumBoard");
    for marker in [
        "SymposiumAssignmentScope",
        "visible_kinds",
        "may_decide",
        "SymposiumGovernanceKind",
        "Joint",
        "scope_for_assignment",
        "decision_assignments",
        "symposium_contribution_scope_denied",
        "symposium_decision_authority_denied",
        "symposium_decision_authority_missing",
        "symposium_assignment_scope_missing",
        "symposium_assignment_scope_mismatch",
        "symposium_vote_does_not_approve",
        "symposium_sponsor_scope_denied",
    ] {
        assert!(
            governance.contains(marker),
            "CO-14 scope contract marker missing: {marker}"
        );
    }
    // The board enforces scope at write and read time and keeps votes advisory.
    for call in [
        "scope.sees(contribution.kind)",
        "scope.may_vote()",
        "!scope.may_decide",
        "decision_rests_solely_on_votes",
    ] {
        assert!(
            board.contains(call),
            "CO-14 board enforcement missing: {call}"
        );
    }
    assert!(
        existing.contains("joint_symposium_frozen"),
        "the frozen joint-meeting rule must stay in the legacy Symposium"
    );
    assert!(
        !board.contains("joint_symposium_frozen"),
        "the governance board must not veto a joint contract; the product rule stays in Symposium"
    );
}

/// CO-14 completion slice: the per-assignment scope is wired through record/visible_to/
/// publish_decision/attach_sponsor_approval, the card fixtures stay deny-first with the success
/// fixture last, five DepartmentSpec ids build one board each, and the manual lane plus shard
/// ledger discover the same targets.
#[test]
fn symposium_scope_is_enforced_on_every_board_method_and_the_ci_lane() {
    let governance = include_str!("../../kiana-domain/src/symposium_governance.rs");
    let fixture = include_str!("../../kiana-domain/tests/co14_symposium_governance.rs");
    let legacy_tests = include_str!("../../kiana-domain/src/tests.rs");
    let workflow = include_str!("../../.github/workflows/co14-symposium-governance.yml");
    let shards = include_str!("../../scripts/ci/test-shards.json");

    // Each board method consults the declared scope, not only the attendee map.
    for item in [
        "pub fn record(",
        "pub fn visible_to(",
        "pub fn publish_decision(",
        "pub fn attach_sponsor_approval(",
    ] {
        let body = source_guard::item_source(governance, item);
        assert!(
            body.contains("scope"),
            "CO-14 board method must consult the declared scope: {item}"
        );
    }
    let visible = source_guard::item_source(governance, "pub fn visible_to(");
    assert!(
        visible.contains("scope.sees(contribution.kind)"),
        "visible_to must filter by declared visible kinds"
    );
    assert!(
        !source_guard::compact_code(&visible)
            .contains("||assignment_ref==self.governance.chair_assignment"),
        "visible_to must not keep the unconditional chair full-visibility shortcut"
    );
    let publish = source_guard::item_source(governance, "pub fn publish_decision(");
    assert!(publish.contains("symposium_decision_authority_denied"));
    assert!(publish.contains("symposium_vote_does_not_approve"));
    let sponsor = source_guard::item_source(governance, "pub fn attach_sponsor_approval(");
    assert!(sponsor.contains("symposium_sponsor_scope_denied"));
    assert!(sponsor.contains("role != \"sponsor\""));

    // The card-named fixtures stay deny-first: the refusal fixtures run before the success
    // fixture, and the card success fixture is the final test in the file.
    for fixture_name in [
        "symposium_rejects_uninvited_stale_and_duplicate_contributions",
        "symposium_refuses_scope_breach_votes_as_approval_and_foreign_sponsor_approval",
        "symposium_governance_refuses_undeclared_scope_and_non_convening_department",
        "symposium_governance_contract_is_usable_by_all_five_departments",
        "joint_symposium_lists_visibility_and_decision_scope_per_assignment",
        "symposium_decision_preserves_alternatives_dissent_and_evidence",
    ] {
        assert!(
            fixture.contains(fixture_name),
            "CO-14 domain fixture missing: {fixture_name}"
        );
    }
    let deny_first = fixture
        .find("symposium_rejects_uninvited_stale_and_duplicate_contributions")
        .expect("deny fixture");
    let card_success = fixture
        .find("symposium_decision_preserves_alternatives_dissent_and_evidence")
        .expect("success fixture");
    assert!(
        deny_first < card_success,
        "the deny-first fixture must precede the card success fixture"
    );
    assert_eq!(
        fixture.rfind("#[test]"),
        fixture
            .find("fn symposium_decision_preserves_alternatives_dissent_and_evidence")
            .map(|position| position.saturating_sub("#[test]\n".len())),
        "the card success fixture must stay the final test"
    );
    assert!(!fixture.contains("#[ignore]"));
    // Five departments build one governed board each using DepartmentSpec ids and the
    // can_convene chair; the joint fixture proves per-assignment visibility and decision scope.
    assert!(fixture.contains("DepartmentSpec::catalog()"));
    assert!(fixture.contains("convene_chair_id()"));
    assert!(fixture.contains("SymposiumGovernance::joint("));
    assert!(fixture.contains("joint_symposium_frozen"));
    assert!(
        legacy_tests.contains("each_department_can_convene_without_a_joint_meeting"),
        "the legacy five-department convene fixture must stay intact"
    );

    // The manual lane stays workflow_dispatch-only, fetches locked deps, gates formatting, runs
    // the target serially and compiles it; it is an extension of the existing CO-14 lane.
    assert!(
        workflow.contains("cargo test -p kiana-core --test co14_symposium_governance_guard"),
        "CO-14 guard target missing from the lane"
    );
    assert!(
        workflow.contains("cargo test -p kiana-domain --test co14_symposium_governance"),
        "CO-14 domain target missing from the lane"
    );
    for marker in [
        "cargo fetch --locked",
        "cargo fmt --all --check",
        "--test-threads=1",
        "--no-run",
        "permissions:\n  contents: read",
        "workflow_dispatch:",
    ] {
        assert!(
            workflow.contains(marker),
            "CO-14 lane step missing: {marker}"
        );
    }
    for forbidden in [
        "push:",
        "pull_request:",
        "continue-on-error",
        "--ignored",
        "|| true",
    ] {
        assert!(
            !workflow.contains(forbidden),
            "CO-14 lane must not contain: {forbidden}"
        );
    }

    // Both targets stay registered in the chunked shard ledger.
    let shards: serde_json::Value =
        serde_json::from_str(shards).expect("the shard ledger must stay valid JSON");
    for (shard, target) in [
        ("kiana-domain-s2/4", "co14_symposium_governance"),
        ("kiana-core-s2/6", "co14_symposium_governance_guard"),
    ] {
        assert!(
            shards[shard]["targets"]
                .as_array()
                .is_some_and(|targets| targets.iter().any(|entry| entry.as_str() == Some(target))),
            "CO-14 target {target} must stay in {shard}"
        );
    }
}

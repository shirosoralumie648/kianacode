#[path = "support/source_guard.rs"]
mod source_guard;

#[test]
fn company_plan_graph_is_versioned_and_deterministic() {
    let plan = include_str!("../../kiana-domain/src/plan.rs");
    let domain = include_str!("../../kiana-domain/src/company.rs");
    let core = include_str!("../src/company.rs");

    let variants = source_guard::item_source(plan, "pub enum PlanEdgeKind");
    for marker in [
        "PLAN_PROPOSAL_SCHEMA",
        "PlanProposal",
        "PlanNode",
        "PlanEdge",
        "PlanEdgeKind::ParentChild",
        "PlanEdgeKind::RequiresArtifact",
        "PlanEdgeKind::RequiresRunSuccess",
        "PlanEdgeKind::RequiresAcceptance",
        "PlanEdgeKind::Related",
        "plan_edge_from_missing",
        "plan_edge_to_missing",
        "plan_dependency_cycle",
        "plan_required_node_isolated",
        "plan_parent_child_kind_invalid",
        "topological_order",
        "charter_baseline_version",
        "coverage_digest",
        "validate_dependency_dag",
    ] {
        if let Some(variant) = marker.strip_prefix("PlanEdgeKind::") {
            assert!(
                variants
                    .lines()
                    .any(|line| line.trim() == format!("{variant},")),
                "typed variant missing: {marker}"
            );
        } else {
            assert!(
                plan.contains(marker) || domain.contains(marker) || core.contains(marker),
                "CO-12 marker missing: {marker}"
            );
        }
    }
    assert!(plan.contains("self.digest != self.canonical_digest()"));
    assert!(plan.contains("self.nodes.is_empty()"));
    assert!(!plan.contains("CapabilityBroker"));
    assert!(!plan.contains("EventStorePort"));
    assert!(!core.contains("PlanProposal::topological_order"));
}

/// CO-12 completion slice: the candidate graph is admitted by one Company approval transaction,
/// the daemon spine proves the deny-first boundary creates no related object, and the frozen
/// dependency versions stay bound to the manual lane and the automatic shards.
#[test]
fn plan_graph_is_wired_into_the_company_transaction_and_the_ci_lane() {
    let plan = include_str!("../../kiana-domain/src/plan.rs");
    let domain = include_str!("../../kiana-domain/src/company.rs");
    let policy = include_str!("../../kiana-domain/src/company_policy.rs");
    let criteria = include_str!("../../kiana-domain/src/criteria.rs");
    let core = include_str!("../src/company.rs");
    let fixture = include_str!("../../kiana-domain/tests/co12_plan_graph.rs");
    let spine = include_str!("../../kiana-daemon/tests/co12_plan_graph_transactions.rs");
    let workflow = include_str!("../../.github/workflows/co12-plan-graph.yml");
    let shards = include_str!("../../scripts/ci/test-shards.json");

    // The command, its event name and the state projection live on the single Company aggregate.
    for marker in [
        "FreezePlanGraph",
        "PlanGraphFrozen",
        "plan_graphs",
        "plan_graph_freeze_verdict",
        "freeze_plan_graph",
        "require_project_writable",
    ] {
        assert!(
            domain.contains(marker),
            "CO-12 company marker missing: {marker}"
        );
    }
    assert!(policy.contains("CompanyCommand::FreezePlanGraph { .. } => &[\"pm\", \"sponsor\"]"));
    assert!(policy.contains("CompanyMutationClass::Project => &[\"sponsor\", \"pm\", \"closer\"]"));
    assert!(policy.contains("\"plan_graph_baseline_missing\""));
    assert!(policy.contains("\"plan_graph_already_frozen\""));

    // The ControlPlane revalidates the complete candidate graph before the transition, so a batch
    // draft that is invalid at any later position still changes no related object.
    let gate = core
        .find("state.plan_graph_freeze_verdict(plan)")
        .expect("the plan graph freeze gate must exist in the handler");
    let transition = core
        .find("state.transition(&request.command")
        .expect("the Company transition must stay in ControlPlane");
    assert!(
        gate < transition,
        "the plan graph verdict must run before the transition"
    );
    assert!(!core.contains("PlanProposal::topological_order"));
    assert!(!core.contains("PlanProposalDraft"));

    // The rejection codes are a closed, stable set; a coverage-bound plan keeps its own reason
    // from the CO-11 contract instead of a generic one.
    for code in [
        "plan_field_required_or_too_large",
        "plan_read_scope_reference_invalid",
        "plan_coverage_digest_invalid",
        "plan_schema_or_version_invalid",
        "plan_nodes_required_or_too_large",
        "plan_node_key_mismatch",
        "plan_node_project_or_version_invalid",
        "plan_node_criterion_duplicate",
        "plan_node_input_reference_invalid",
        "plan_edges_too_large",
        "plan_edge_duplicate",
        "plan_edge_from_missing",
        "plan_edge_to_missing",
        "plan_edge_self_reference",
        "plan_edge_reference_required",
        "plan_parent_child_kind_invalid",
        "plan_dependency_cycle",
        "plan_required_node_isolated",
        "plan_required_milestone_empty",
        "plan_required_criteria_too_large",
        "plan_required_criterion_duplicate",
        "plan_required_criterion_isolated",
        "plan_read_scope_too_large",
        "plan_read_scope_reference_duplicate",
        "plan_read_scope_insufficient",
        "plan_digest_mismatch",
        "plan_draft_schema_invalid",
        "plan_node_duplicate",
        "plan_graph_baseline_missing",
        "plan_graph_baseline_mismatch",
        "plan_graph_already_frozen",
        "frozen_plan_graph_schema_invalid",
        "frozen_plan_graph_version_invalid",
        "frozen_plan_graph_plan_digest_invalid",
        "frozen_plan_graph_coverage_digest_invalid",
        "frozen_plan_graph_nodes_invalid",
        "frozen_plan_graph_node_versions_invalid",
        "frozen_plan_graph_freeze_digest_mismatch",
        "frozen_plan_graph_project_mismatch",
        "frozen_plan_graph_node_missing",
        "frozen_plan_graph_node_version_mismatch",
        "frozen_plan_graph_node_unknown",
        "frozen_plan_graph_plan_version_mismatch",
        "frozen_plan_graph_baseline_mismatch",
        "frozen_plan_graph_coverage_mismatch",
        "frozen_plan_graph_plan_digest_mismatch",
    ] {
        assert!(
            plan.contains(code) || domain.contains(code) || policy.contains(code),
            "CO-12 rejection code missing: {code}"
        );
    }
    assert!(criteria.contains("plan_coverage_digest_mismatch"));
    assert!(!plan.contains("plan_coverage_digest_mismatch"));

    // The card-named fixtures stay deny-first: the refusal fixture is declared before the success
    // fixture, and the daemon spine keeps the approval-transaction behaviour.
    for fixture_name in [
        "plan_rejects_missing_dependencies_cycles_and_cross_project_edges",
        "plan_rejects_empty_required_milestones_isolated_criteria_and_narrow_read_scope",
        "plan_draft_accepts_any_insert_order_and_validates_once_at_freeze",
        "frozen_plan_graph_pins_dependency_versions_and_rejects_every_drift",
        "plan_graph_freeze_command_admits_the_exact_revision_once",
        "two_milestone_plan_preserves_dependency_and_acceptance_boundaries",
    ] {
        assert!(
            fixture.contains(fixture_name),
            "CO-12 domain fixture missing: {fixture_name}"
        );
    }
    let deny_first = fixture
        .find("plan_rejects_missing_dependencies_cycles_and_cross_project_edges")
        .expect("deny fixture");
    let card_success = fixture
        .find("two_milestone_plan_preserves_dependency_and_acceptance_boundaries")
        .expect("success fixture");
    assert!(
        deny_first < card_success,
        "the deny-first fixture must precede the card success fixture"
    );
    assert!(!fixture.contains("#[ignore]"));
    for fixture_name in [
        "co12_invalid_plan_graph_commits_no_related_object",
        "co12_two_milestone_plan_commits_once_through_the_approval_transaction",
    ] {
        assert!(
            spine.contains(fixture_name),
            "CO-12 spine fixture missing: {fixture_name}"
        );
    }
    assert!(!spine.contains("#[ignore]"));
    assert!(!spine.contains("CapabilityBroker"));
    assert!(!spine.contains("KianaHarness::run"));

    // The manual lane stays workflow_dispatch-only, runs three targets serially and compiles them
    // before the automatic shards pick the targets up.
    for target in [
        "cargo test -p kiana-core --test co12_plan_graph_guard",
        "cargo test -p kiana-domain --test co12_plan_graph",
        "cargo test -p kiana-daemon --test co12_plan_graph_transactions",
    ] {
        assert!(
            workflow.contains(target),
            "CO-12 lane target missing: {target}"
        );
    }
    for marker in [
        "cargo fetch --locked",
        "cargo fmt --all --check",
        "--test-threads=1",
        "--no-run",
        "permissions:\n  contents: read",
    ] {
        assert!(
            workflow.contains(marker),
            "CO-12 lane step missing: {marker}"
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
            "CO-12 lane must not contain: {forbidden}"
        );
    }

    let shards: serde_json::Value =
        serde_json::from_str(shards).expect("the shard ledger must stay valid JSON");
    for (shard, target) in [
        ("kiana-domain-s2/4", "co12_plan_graph"),
        ("kiana-core-s2/6", "co12_plan_graph_guard"),
    ] {
        assert!(
            shards[shard]["targets"]
                .as_array()
                .is_some_and(|targets| targets.iter().any(|entry| entry.as_str() == Some(target))),
            "CO-12 target {target} must stay in {shard}"
        );
    }
    assert!(
        shards["kiana-daemon"]["targets"].is_null(),
        "the daemon spine is discovered by the whole-crate shard"
    );
}

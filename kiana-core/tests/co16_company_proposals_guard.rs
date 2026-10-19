//! CO-16 source guards: proposals are inert structured facts, and approval is one atomic CAS
//! over a fully validated preview on the single Company aggregate.
//!
//! The first guard pins the intake/preview/materialization contract and the absence of any
//! text-to-command path. The second pins the deny-first fixture order, the commit-after-validate
//! order inside `approve`, the manual lane and the automatic shard bindings.
#[path = "support/source_guard.rs"]
mod source_guard;

use source_guard::{compact_code, item_source, production_source};

#[test]
fn company_proposals_accept_only_structured_role_task_results_and_freeze_the_reference_set() {
    let proposals = include_str!("../../kiana-domain/src/company_proposals.rs");
    let plan = include_str!("../../kiana-domain/src/plan.rs");
    let core = include_str!("../src/company.rs");

    for marker in [
        "COMPANY_PLAN_PROPOSAL_SCHEMA",
        "COMPANY_PLAN_APPROVAL_SCHEMA",
        "COMPANY_ROLE_TASK_RESULT_SCHEMA",
        "COMPANY_PLAN_PROPOSAL_INTAKE_SCHEMA",
        "COMPANY_PLAN_PREVIEW_SCHEMA",
        "COMPANY_PLAN_MATERIALIZATION_SCHEMA",
        "CompanyPlanProposal",
        "CompanyRoleTaskResult",
        "CompanyPlanProposalIntake",
        "CompanyProposalSourceKind",
        "company_proposal_source_not_role_task_result",
        "company_proposal_source_role_denied",
        "company_proposal_source_binding_mismatch",
        "company_role_task_output_schema_mismatch",
        "CompanyPlanPreview",
        "company_plan_preview_node_version_missing",
        "company_plan_preview_node_version_mismatch",
        "company_plan_preview_packet_budget_exceeded",
        "company_plan_preview_criteria_mismatch",
        "company_plan_preview_packet_parent_invalid",
        "CompanyPlanMaterialization",
        "company_plan_approval_preview_mismatch",
        "company_plan_approval_revision_conflict",
        "company_plan_approval_duplicate",
        "approved_plan_digest",
        "runtime_authority",
        "CompanyProposalLedger",
        "canonical_digest",
        "PlanProposal",
        "ResultContract",
    ] {
        assert!(
            proposals.contains(marker) || plan.contains(marker) || core.contains(marker),
            "CO-16 marker missing: {marker}"
        );
    }

    // The intake refuses every prose surface before it looks at the body. Only the structured
    // role-task kind can pass, and the ledger never parses text.
    let intake = item_source(proposals, "impl CompanyPlanProposalIntake");
    assert!(intake.contains("CompanyProposalSourceKind::RoleTaskResult"));
    assert!(intake.contains("company_proposal_source_not_role_task_result"));
    let source_kinds = item_source(proposals, "pub enum CompanyProposalSourceKind");
    for prose in ["Readme", "Stdout", "AssistantMessage", "Transcript"] {
        assert!(
            source_kinds.contains(prose),
            "CO-16 prose source must stay a named refusal surface: {prose}"
        );
    }
    for forbidden in [
        "serde_json::from_str::<CompanyCommand",
        "CompanyCommand::from",
        "std::process::Command",
        "CapabilityBroker",
        "EventStorePort",
        "KianaHarness::run",
        "runtime_grant",
        "budget_lease",
    ] {
        assert!(
            !proposals.contains(forbidden),
            "CO-16 proposal source must not contain: {forbidden}"
        );
    }
    // `observed_text` is audit data only: it is stored, never parsed.
    assert!(!compact_code(proposals).contains("observed_text.as_deref().map(serde_json"));
}

/// The completion slice: intake, preview and approval are wired in the required order and keep
/// the deny-first fixtures plus the CI bindings they name.
#[test]
fn plan_approval_is_one_atomic_cas_and_the_ci_lane_keeps_the_deny_first_order() {
    let proposals = include_str!("../../kiana-domain/src/company_proposals.rs");
    let fixture = include_str!("../../kiana-domain/tests/co16_company_proposals.rs");
    // The aggregate fixtures run the same deny-first order on the real Company transition path.
    let command_fixture =
        include_str!("../../kiana-domain/tests/co16_company_proposals_command.rs");
    let workflow = include_str!("../../.github/workflows/co16-company-proposals.yml");
    let shards = include_str!("../../scripts/ci/test-shards.json");

    // One CAS: every reference is derived into a local value before the first mutation, and the
    // revision check plus digest checks run before anything is inserted.
    let approve = item_source(proposals, "pub fn approve(");
    let build_materialization = approve
        .find("preview.materialize(")
        .expect("materialization must be built inside approve");
    let first_mutation = approve
        .find("self.revision += 1")
        .expect("the revision CAS must commit");
    let approval_insert = approve
        .rfind("self.approvals")
        .expect("the approval fact must be committed");
    let materialization_insert = approve
        .rfind("self.materializations")
        .expect("the materialization fact must be committed");
    assert!(
        build_materialization < first_mutation
            && first_mutation < approval_insert
            && approval_insert < materialization_insert,
        "the whole preview must materialize before any field changes"
    );
    let approve_code = compact_code(&approve);
    for ordered in [
        "ifpreview.digest!=approval.preview_digest",
        "ifapproval.expected_revision!=preview.expected_revision||approval.expected_revision!=self.revision",
        "company_plan_approval_duplicate",
    ] {
        assert!(
            approve_code.contains(ordered),
            "CO-16 approve order marker missing: {ordered}"
        );
    }
    // Submit also builds the preview before inserting either fact.
    let submit = item_source(proposals, "pub fn submit(");
    let build_preview = submit
        .find("CompanyPlanPreview::build(proposal, &basis)")
        .expect("the preview must be built in submit");
    let first_insert = submit
        .rfind("self.proposals")
        .expect("the proposal fact must be committed");
    assert!(
        build_preview < first_insert,
        "submit must validate the whole graph before committing any fact"
    );

    // The card-named fixtures stay deny-first with the success fixture last.
    for fixture_name in [
        "proposal_text_cannot_execute_commands_and_invalid_plan_commits_nothing",
        "preview_refuses_version_budget_and_criteria_drift_before_any_preview_exists",
        "approved_plan_materializes_the_exact_reviewed_packet_graph_once",
    ] {
        assert!(
            fixture.contains(fixture_name),
            "CO-16 fixture missing: {fixture_name}"
        );
    }
    let deny_first = fixture
        .find("proposal_text_cannot_execute_commands_and_invalid_plan_commits_nothing")
        .expect("deny fixture");
    let drift = fixture
        .find("preview_refuses_version_budget_and_criteria_drift_before_any_preview_exists")
        .expect("drift fixture");
    let card_success = fixture
        .find("approved_plan_materializes_the_exact_reviewed_packet_graph_once")
        .expect("success fixture");
    assert!(
        deny_first < drift && drift < card_success,
        "the deny-first fixtures must precede the card success fixture"
    );
    assert_eq!(
        fixture.rfind("#[test]"),
        fixture
            .find("fn approved_plan_materializes_the_exact_reviewed_packet_graph_once")
            .map(|position| position.saturating_sub("#[test]\n".len())),
        "the card success fixture must stay the final test"
    );
    assert!(!fixture.contains("#[ignore]"));
    for prose in [
        "CompanyProposalSourceKind::Readme",
        "CompanyProposalSourceKind::Stdout",
        "CompanyProposalSourceKind::AssistantMessage",
        "CompanyProposalSourceKind::Transcript",
    ] {
        assert!(
            fixture.contains(prose),
            "the deny fixture must exercise the prose surface: {prose}"
        );
    }
    assert!(fixture.contains("company_plan_preview_packet_parent_invalid"));
    let command_deny = command_fixture
        .find("approve_company_plan_refuses_prose_and_invalid_later_packets_without_mutation")
        .expect("aggregate deny fixture");
    let command_success = command_fixture
        .find("approved_plan_is_one_cas_and_replay_does_not_duplicate_packets")
        .expect("aggregate success fixture");
    assert!(
        command_deny < command_success,
        "the aggregate deny fixture must precede the success fixture"
    );
    for marker in [
        "company_proposal_source_not_role_task_result",
        "company_plan_preview_packet_parent_invalid",
        "company_plan_approval_revision_conflict",
    ] {
        assert!(
            command_fixture.contains(marker),
            "the aggregate deny fixture must exercise: {marker}"
        );
    }

    // The manual lane stays workflow_dispatch-only, fetches locked dependencies, runs the two
    // focused targets serially and compiles them before the automatic shards discover them.
    for target in [
        "cargo test -p kiana-core --test co16_company_proposals_guard",
        "cargo test -p kiana-domain --test co16_company_proposals",
        "cargo test -p kiana-domain --test co16_company_proposals_command",
    ] {
        assert!(
            workflow.contains(target),
            "CO-16 lane target missing: {target}"
        );
    }
    for marker in [
        "workflow_dispatch:",
        "cargo fetch --locked",
        "cargo fmt --all --check",
        "--test-threads=1",
        "--no-run",
        "permissions:\n  contents: read",
    ] {
        assert!(
            workflow.contains(marker),
            "CO-16 lane step missing: {marker}"
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
            "CO-16 lane must not contain: {forbidden}"
        );
    }

    let shards: serde_json::Value =
        serde_json::from_str(shards).expect("the shard ledger must stay valid JSON");
    for (shard, target) in [
        ("kiana-domain-s2/4", "co16_company_proposals"),
        ("kiana-domain-s2/4", "co16_company_proposals_command"),
        ("kiana-core-s2/6", "co16_company_proposals_guard"),
    ] {
        assert!(
            shards[shard]["targets"]
                .as_array()
                .is_some_and(|targets| targets.iter().any(|entry| entry.as_str() == Some(target))),
            "CO-16 target {target} must stay in {shard}"
        );
    }

    // No second execution surface and no runtime authority on the proposal path.
    let production = production_source(proposals);
    assert!(!production.contains("fn run("));
    assert!(!production.contains("CapabilityBroker"));
    assert!(!production.contains("EventStorePort"));
    assert!(!production.contains("std::process"));
}

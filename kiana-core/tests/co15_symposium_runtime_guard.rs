//! CO-15 source guards: the bounded role meeting runs on the existing Harness, every model turn
//! is only a structured proposal that Core validates, and the convened and anti-meeting paths
//! share one fail-closed decision gate.

#[path = "support/source_guard.rs"]
mod source_guard;

#[test]
fn symposium_runtime_contract_is_typed_bounded_and_fail_closed() {
    let runtime = include_str!("../../kiana-domain/src/symposium_runtime.rs");
    let governance = include_str!("../../kiana-domain/src/symposium_governance.rs");

    for marker in [
        "SYMPOSIUM_RUN_BUDGET_SCHEMA",
        "SYMPOSIUM_CONTRIBUTION_PROPOSAL_SCHEMA",
        "SYMPOSIUM_TOKENS_PER_CONTRIBUTION",
        "SYMPOSIUM_MAX_WALL_TIME_MS",
        "SYMPOSIUM_MAX_STALL_ROUNDS",
        "pub struct SymposiumRunBudget",
        "pub struct SymposiumContributionProposal",
        "pub fn parse_symposium_contributions",
        "pub fn decision_allowed",
    ] {
        assert!(
            runtime.contains(marker),
            "CO-15 contract marker missing: {marker}"
        );
    }
    let budget = source_guard::item_source(runtime, "pub struct SymposiumRunBudget");
    for field in [
        "max_rounds",
        "max_messages",
        "max_tokens",
        "max_wall_time_ms",
        "max_stall_rounds",
        "rounds_used",
        "messages_used",
        "tokens_used",
        "wall_time_used_ms",
        "stall_rounds",
        "cancelled",
    ] {
        assert!(budget.contains(field), "budget field missing: {field}");
    }
    for code in [
        "symposium_round_budget_exhausted",
        "symposium_message_budget_exhausted",
        "symposium_token_budget_exhausted",
        "symposium_wall_time_exhausted",
        "symposium_stall_limit_exhausted",
        "symposium_cancelled",
        "symposium_run_budget_invalid",
    ] {
        assert!(
            runtime.contains(code),
            "stable budget refusal missing: {code}"
        );
    }
    // The attribute precedes the item, so pin it in source order rather than in the item body.
    assert!(runtime.contains(
        "#[serde(deny_unknown_fields)]
pub struct SymposiumContributionProposal"
    ));
    let proposal = source_guard::item_source(runtime, "pub struct SymposiumContributionProposal");
    for field in [
        "pub kind: String",
        "pub text: String",
        "pub evidence_refs: Vec<String>",
    ] {
        assert!(proposal.contains(field), "proposal field missing: {field}");
    }
    for code in [
        "symposium_contribution_invalid",
        "symposium_contribution_kind_invalid",
        "symposium_contribution_visibility_invalid",
    ] {
        assert!(
            runtime.contains(code),
            "stable proposal refusal missing: {code}"
        );
    }
    assert!(!runtime.contains("CapabilityBroker"));
    assert!(!runtime.contains("EventStorePort"));
    assert!(!runtime.contains("KianaHarness"));
    assert!(governance.contains("symposium_private_brief_visibility_denied"));
    assert!(governance.contains("symposium_vote_does_not_approve"));
}

#[test]
fn symposium_runtime_reuses_bounded_harness_path_and_never_approves_on_budget_failure() {
    let core = include_str!("../src/collaboration.rs");
    let domain_tests = include_str!("../../kiana-domain/tests/co15_symposium_runtime.rs");
    let core_tests = include_str!("control_plane.rs");
    let workflow = include_str!("../../.github/workflows/co15-symposium-runtime.yml");
    let shards = include_str!("../../scripts/ci/test-shards.json");

    let convene = source_guard::item_source(core, "pub async fn convene_symposium(");
    for marker in [
        "for round in 0..max_rounds",
        "for role_id in &attendees",
        "speaker_session_id(role_id)",
        "self.start_run(",
        "self.continue_run(",
        "meeting.speaker_prompt(role_id)",
        "SYMPOSIUM_TOKENS_PER_CONTRIBUTION",
    ] {
        assert!(
            convene.contains(marker),
            "CO-15 schedule marker missing: {marker}"
        );
    }
    assert!(
        !convene.contains("KianaHarness::new"),
        "the convened meeting must reuse the injected Harness, not build a second one"
    );
    let parse_at = convene
        .find("parse_symposium_contributions(structured)")
        .expect("structured contributions must be parsed by Core");
    let record_at = convene
        .find("board_state.record(contribution.clone())")
        .expect("validated contributions must be recorded on the governed board");
    let broadcast_at = convene
        .find("meeting.blackboard")
        .expect("blackboard publication must be explicit");
    assert!(
        parse_at < record_at,
        "validation must precede board publication"
    );
    assert!(
        record_at < broadcast_at,
        "publication must follow board acceptance"
    );
    for code in [
        "symposium_contribution_required",
        "symposium_member_failed",
        "symposium_member_cancelled",
    ] {
        assert!(convene.contains(code), "CO-15 refusal missing: {code}");
    }

    let gate_count = convene.matches("budget.decision_allowed()").count();
    assert_eq!(gate_count, 1, "one shared decision gate, not one per path");
    let gate_at = convene.find("budget.decision_allowed()").unwrap();
    let close_at = convene
        .find("meeting.close(anti_meeting)")
        .expect("meeting.close");
    assert!(
        gate_at < close_at,
        "the gate must run before the decision is published"
    );
    let anti_branch = convene
        .find("if !anti_meeting {")
        .expect("the speaker schedule is skipped for anti-meeting");
    assert!(
        anti_branch < gate_at,
        "the gate must sit outside the convened-only block so anti-meeting shares it"
    );
    assert!(convene.contains("budget.cancel()"));
    assert!(convene.contains("\"symposium_run_budget\""));

    let domain_deny = domain_tests
        .find("fn symposium_budget_exhaustion_or_cancel_never_emits_an_approved_decision")
        .expect("domain deny fixture");
    let domain_success = domain_tests
        .find("fn symposium_and_async_proposal_share_the_same_decision_gate")
        .expect("domain success fixture");
    assert!(
        domain_deny < domain_success,
        "domain fixtures must stay deny-first"
    );
    assert_eq!(
        domain_tests.rfind("#[test]"),
        domain_tests
            .find("fn symposium_and_async_proposal_share_the_same_decision_gate")
            .map(|position| position.saturating_sub("#[test]\n".len())),
        "the success fixture must stay last"
    );
    assert!(!domain_tests.contains("#[ignore]"));
    let core_deny = core_tests
        .find("fn convene_refuses_unstructured_member_contribution")
        .expect("core deny fixture");
    let core_success = core_tests
        .find("fn convened_and_anti_meeting_share_one_decision_gate")
        .expect("core success fixture");
    assert!(
        core_deny < core_success,
        "core fixtures must stay deny-first"
    );
    assert!(!core_tests.contains("#[ignore]"));

    for marker in [
        "cargo fetch --locked",
        "cargo fmt --all --check",
        "cargo test -p kiana-core --test co15_symposium_runtime_guard",
        "cargo test -p kiana-domain --test co15_symposium_runtime",
        "cargo test -p kiana-core --test control_plane",
        "--test-threads=1",
        "--no-run",
        "permissions:\n  contents: read",
        "workflow_dispatch:",
    ] {
        assert!(
            workflow.contains(marker),
            "CO-15 lane step missing: {marker}"
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
            "CO-15 lane must not contain: {forbidden}"
        );
    }

    let shards: serde_json::Value =
        serde_json::from_str(shards).expect("the shard ledger must stay valid JSON");
    for (shard, target) in [
        ("kiana-domain-s2/4", "co15_symposium_runtime"),
        ("kiana-core-s2/6", "co15_symposium_runtime_guard"),
    ] {
        assert!(
            shards[shard]["targets"]
                .as_array()
                .is_some_and(|targets| targets.iter().any(|entry| entry.as_str() == Some(target))),
            "CO-15 target {target} must stay in {shard}"
        );
    }
}

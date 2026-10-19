//! AUT-20 source guard：补偿计划合同、Core 只读门面、拒绝码矩阵、夹具顺序与手动 lane 必须保持绑定。
//!
//! 两个测试分别固定：(1) 领域契约 marker 与稳定拒绝码，以及“不执行副作用”的负断言；
//! (2) 接线：Core/Workflow 门面签名、夹具名称与“先拒绝、成功最后”的顺序、lane 命令与分片登记。

#[test]
fn compensation_contract_and_refusal_codes_stay_fixed() {
    let domain = include_str!("../../kiana-domain/src/automation_compensation.rs");
    let workflow = include_str!("../../kiana-workflow/src/compensation.rs");
    let core = include_str!("../src/automation_compensation.rs");

    // v2 绑定字段：原实例/执行/attempt/授权 epoch + 独立补偿实例/执行/动作 + fresh 授权。
    for marker in [
        "pub const AUTOMATION_COMPENSATION_SCHEMA: &str = \"kiana.automation-compensation.v2\"",
        "pub struct AutomationCompensationPlan",
        "pub original_instance_id: String",
        "pub original_execution_id: String",
        "pub original_attempt: u32",
        "pub original_authorization_epoch: u64",
        "pub original_authorization_digest: String",
        "pub compensation_instance_id: String",
        "pub compensation_execution_id: String",
        "pub compensation_action_digest: String",
        "pub fresh_authorization_digest: String",
        "pub reused_original_permit: bool",
        "pub fn validate(",
        "pub fn seal(",
    ] {
        assert!(
            domain.contains(marker),
            "AUT-20 domain marker missing: {marker}"
        );
    }

    // 每个拒绝面都有名字固定的错误码。
    for code in [
        "automation_compensation_plan_invalid",
        "automation_compensation_identity_collision",
        "automation_compensation_original_permit_reused",
        "automation_compensation_action_unchanged",
        "automation_compensation_authorization_not_fresh",
        "automation_compensation_original_instance_missing",
        "automation_compensation_original_not_terminal",
        "automation_compensation_original_attempt_missing",
        "automation_compensation_original_attempt_mismatch",
        "automation_compensation_already_requested",
        "automation_compensation_not_declared",
        "automation_compensation_execution_identity_mismatch",
        "automation_compensation_authorization_stale",
        "automation_compensation_original_mutated",
        "automation_compensation_admission_invalid",
    ] {
        assert!(
            domain.contains(code) || workflow.contains(code) || core.contains(code),
            "AUT-20 refusal code missing: {code}"
        );
    }

    // 权威比较必须规范化大小写，否则 `sha256:AAA…` 会绕过“同一授权/同一动作”拒绝。
    assert!(
        domain.contains("to_ascii_lowercase()"),
        "AUT-20 digest comparison must be case-insensitive"
    );
    // 准入必须对照 live epoch，而不是只信计划自报。
    assert!(
        workflow.contains("live_authority_epoch")
            && workflow.contains("plan.authority_epoch != live_authority_epoch"),
        "AUT-20 admission must compare the declared epoch with the live epoch"
    );
    // 补偿必须复用 durable planner，而不是复制状态机。
    assert!(
        workflow.contains("plan_command(state, &command, authority, proof)"),
        "AUT-20 must reuse the durable planner"
    );
    assert!(
        workflow.contains("AutomationCommand::Compensate"),
        "AUT-20 must plan through the existing Compensate command"
    );

    // 负断言：领域/Core/Workflow 补偿模块都不得执行副作用或起第二条执行循环。
    for forbidden in [
        "CapabilityBroker::new",
        "ModelClient::new",
        "std::process::Command",
        "tokio::spawn",
        "EventStore::append",
        "DaemonHost",
        "KianaHarness",
        "commit_protected_event",
    ] {
        assert!(
            !domain.contains(forbidden)
                && !workflow.contains(forbidden)
                && !core.contains(forbidden),
            "AUT-20 boundary executes effects: {forbidden}"
        );
    }
}

#[test]
fn compensation_completion_slice_stays_wired_to_fixtures_lane_and_shards() {
    let core = include_str!("../src/automation_compensation.rs");
    let core_lib = include_str!("../src/lib.rs");
    let domain_fixtures = include_str!("../../kiana-domain/tests/automation_compensation.rs");
    let workflow_fixtures =
        include_str!("../../kiana-workflow/tests/aut20_compensation_admission.rs");
    let lane = include_str!("../../.github/workflows/aut20-compensation.yml");
    let shards = include_str!("../../scripts/ci/test-shards.json");

    // 1. Core 门面：只读转发，签名固定。
    for marker in [
        "pub fn validate_compensation_plan(",
        "pub fn admit_compensation_plan(",
        "state: &AutomationState,",
        "live_authority_epoch: u64,",
        "plan_compensation_admission(state, plan, authority, proof, live_authority_epoch)",
    ] {
        assert!(
            core.contains(marker),
            "AUT-20 core marker missing: {marker}"
        );
    }
    assert!(
        core_lib.contains(
            "pub use automation_compensation::{admit_compensation_plan, validate_compensation_plan};"
        ),
        "AUT-20 core exports must stay wired to lib.rs"
    );

    // 2. 领域夹具：先拒绝、成功最后。
    let deny_first = [
        "fn reused_original_permit_is_refused_by_flag_and_by_identical_digest",
        "fn authorization_epoch_must_be_strictly_after_the_original",
        "fn compensation_cannot_reuse_the_original_instance_execution_or_action_identity",
        "fn structural_drift_and_unknown_fields_fail_closed",
    ];
    for fixture in deny_first {
        assert!(
            domain_fixtures.contains(fixture),
            "AUT-20 deny-first fixture missing: {fixture}"
        );
    }
    let success = "fn a_valid_plan_binds_original_and_compensation_with_a_fresh_epoch";
    assert!(
        domain_fixtures.contains(success),
        "AUT-20 success fixture missing"
    );
    let success_at = domain_fixtures
        .find(success)
        .expect("success fixture present");
    for fixture in deny_first {
        let at = domain_fixtures.find(fixture).expect("deny fixture present");
        assert!(
            at < success_at,
            "AUT-20 deny fixture must precede the success fixture: {fixture}"
        );
    }

    // 3. Workflow 准入夹具：先拒绝、成功最后。
    let workflow_deny_first = [
        "fn source_instance_must_exist_and_be_known_failed_or_cancelled",
        "fn original_execution_and_attempt_must_match_a_real_node_attempt",
        "fn compensation_must_use_fresh_execution_and_live_authorization_epoch",
        "fn compensation_action_must_differ_from_the_original_action",
        "fn second_compensation_for_one_source_is_refused",
        "fn source_without_declared_compensation_nodes_is_refused",
    ];
    for fixture in workflow_deny_first {
        assert!(
            workflow_fixtures.contains(fixture),
            "AUT-20 workflow deny-first fixture missing: {fixture}"
        );
    }
    let workflow_success =
        "fn admitted_compensation_references_the_original_and_leaves_it_unchanged";
    assert!(
        workflow_fixtures.contains(workflow_success),
        "AUT-20 workflow success fixture missing"
    );
    let workflow_success_at = workflow_fixtures
        .find(workflow_success)
        .expect("workflow success fixture present");
    for fixture in workflow_deny_first {
        let at = workflow_fixtures
            .find(fixture)
            .expect("workflow deny fixture present");
        assert!(
            at < workflow_success_at,
            "AUT-20 workflow deny fixture must precede the success fixture: {fixture}"
        );
    }
    assert!(
        !domain_fixtures.contains("#[ignore]") && !workflow_fixtures.contains("#[ignore]"),
        "AUT-20 fixtures must not be ignored"
    );

    // 4. 手动 lane：workflow_dispatch-only、contents: read、locked fetch、fmt 门、三个 target、
    //    串行与 --no-run。
    for marker in [
        "workflow_dispatch:",
        "contents: read",
        "cargo fetch --locked",
        "cargo fmt --all --check",
        "cargo test -p kiana-domain --test automation_compensation --locked",
        "cargo test -p kiana-workflow --test aut20_compensation_admission --locked",
        "cargo test -p kiana-core --test automation_compensation_guard --locked",
        "--test-threads=1",
        "--no-run",
    ] {
        assert!(
            lane.contains(marker),
            "AUT-20 lane marker missing: {marker}"
        );
    }
    assert!(
        !lane.contains("push:") && !lane.contains("pull_request:") && !lane.contains("schedule:"),
        "AUT-20 lane must stay manual-only"
    );

    // 5. 分片：domain/core target 已被既有 chunked shard 认领；workflow 整 crate 自动发现。
    for target in [
        "\"automation_compensation\"",
        "\"automation_compensation_guard\"",
    ] {
        assert!(
            shards.contains(target),
            "AUT-20 target must be claimed by an existing shard: {target}"
        );
    }
    assert!(
        shards.contains("\"kiana-workflow\""),
        "kiana-workflow shard must exist for the new target"
    );
}

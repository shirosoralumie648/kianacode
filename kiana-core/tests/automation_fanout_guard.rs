//! AUT-19 source guard：领域扇出账本、Core 只读门面、夹具顺序与手动 lane 必须保持绑定。
//!
//! 两个测试分别固定：(1) 契约 marker 与稳定拒绝码矩阵，以及“账本不执行副作用”的负断言；
//! (2) 接线：门面签名、夹具名称与“先拒绝、成功最后”的顺序、lane 命令与分片登记。

#[test]
fn fanout_ledger_contract_and_refusal_code_matrix_stay_fixed() {
    let domain = include_str!("../../kiana-domain/src/automation_fanout.rs");
    let core = include_str!("../src/automation_fanout.rs");

    // 真实 scope/预算/角色/TTL/循环/fail-fast/fan-in 合同与推导入口。
    for marker in [
        "pub const AUTOMATION_FANOUT_PLAN_SCHEMA: &str = \"kiana.automation-fanout.v2\"",
        "pub struct AutomationFanoutParent",
        "pub struct AutomationFanoutChildPlan",
        "pub fn admit(",
        "pub struct AutomationFanoutPlanV2",
        "pub struct AutomationFanoutLedger",
        "pub enum AutomationFanoutChildState",
        "pub enum AutomationFanoutFact",
        "pub struct AutomationFanoutAdmission",
        "pub struct AutomationFanoutFanInInput",
        "pub struct AutomationFanoutMerge",
        "pub enum AutomationFanoutParentState",
        "pub fn start_child(",
        "pub fn observe_child(",
        "pub fn parent_state(",
        "pub fn merge(",
        "pub fn admission(",
        "fn child_scope_within_parent(",
        "child_scope.intersect(parent_scope)",
        ".is_subset_of(parent_scope)",
        "checked_add(",
        "pub fn validate_with_scopes(",
        "pub fn derive_declarations(",
        "fn actual_parent_id_in_children(",
    ] {
        assert!(
            domain.contains(marker),
            "AUT-19 domain marker missing: {marker}"
        );
    }

    // 稳定拒绝码：每个拒绝面都必须有名字固定的错误码。
    for code in [
        "automation_fanout_scope_widening",
        "automation_fanout_scope_invalid",
        "automation_fanout_budget_exceeded",
        "automation_fanout_role_widening",
        "automation_fanout_cycle_refused",
        "automation_fanout_parent_invalid",
        "automation_fanout_child_invalid",
        "automation_fanout_child_count_invalid",
        "automation_fanout_child_duplicate",
        "automation_fanout_ttl_exceeded",
        "automation_fanout_depth_exceeded",
        "automation_fanout_concurrency_exceeded",
        "automation_fanout_child_unknown",
        "automation_fanout_child_skipped",
        "automation_fanout_child_already_started",
        "automation_fanout_child_not_started",
        "automation_fanout_evidence_invalid",
        "automation_fanout_fan_in_duplicate",
        "automation_fanout_fan_in_unknown_child",
        "automation_fanout_fan_in_missing_child",
        "automation_fanout_fan_in_incomplete",
        "automation_fanout_fan_in_sequence_invalid",
        "automation_fanout_scope_declaration_mismatch",
        "automation_fanout_cycle_declaration_mismatch",
    ] {
        assert!(domain.contains(code), "AUT-19 refusal code missing: {code}");
    }

    // fail-fast 跳过原因必须是固定文本，合并/回执才可复现。
    assert!(
        domain.contains("\"automation_fanout_fail_fast_skip\""),
        "AUT-19 fail-fast skip reason must be stable"
    );
    // 父状态只能由事实推导：状态枚举存在 derive，不接受调用方传入 status。
    assert!(
        !domain.contains("pub status:") && !domain.contains("pub fn set_status("),
        "AUT-19 parent status must stay derived from facts"
    );

    // 负断言：领域与 Core 门面都不得执行任何副作用或起第二条执行循环。
    for forbidden in [
        "CapabilityBroker::new",
        "ModelClient::new",
        "std::process::Command",
        "tokio::spawn",
        "EventStore::append",
        "DaemonHost",
        "KianaHarness",
    ] {
        assert!(
            !domain.contains(forbidden) && !core.contains(forbidden),
            "AUT-19 boundary executes effects: {forbidden}"
        );
    }
}

#[test]
fn fanout_completion_slice_stays_wired_to_fixtures_lane_and_shards() {
    let core = include_str!("../src/automation_fanout.rs");
    let core_lib = include_str!("../src/lib.rs");
    let domain_fixtures = include_str!("../../kiana-domain/tests/automation_fanout.rs");
    let lane = include_str!("../../.github/workflows/aut19-fanout.yml");
    let shards = include_str!("../../scripts/ci/test-shards.json");

    // 1. Core 门面签名与导出：只读转发，不新增执行入口。
    for marker in [
        "pub fn validate_fanout_plan(",
        "pub fn admit_fanout_plan(",
        "pub fn merge_fanout_ledger(",
        "plan.admit()?",
        "ledger.admission()",
        "ledger.merge(inputs)",
    ] {
        assert!(
            core.contains(marker),
            "AUT-19 core marker missing: {marker}"
        );
    }
    assert!(
        core_lib.contains(
            "pub use automation_fanout::{admit_fanout_plan, merge_fanout_ledger, validate_fanout_plan};"
        ),
        "AUT-19 core exports must stay wired to lib.rs"
    );

    // 2. 夹具名必须与真实测试函数一一对应，且拒绝夹具在前、成功夹具最后。
    let deny_first = [
        "fn child_scope_widening_is_refused_with_a_stable_code",
        "fn budget_role_ttl_and_child_shape_beyond_parent_are_refused",
        "fn cycle_parent_and_duplicate_children_are_refused",
        "fn start_after_fail_fast_is_refused_and_unknown_or_unstarted_children_are_refused",
        "fn started_children_are_bounded_by_parent_concurrency",
        "fn fan_in_refuses_duplicate_unknown_missing_and_incomplete_inputs",
        "fn legacy_plan_declarations_are_checked_against_real_scopes",
    ];
    for fixture in deny_first {
        assert!(
            domain_fixtures.contains(fixture),
            "AUT-19 deny-first fixture missing: {fixture}"
        );
    }
    let success =
        "fn admitted_two_child_plan_fails_one_skips_the_other_and_merges_deterministically";
    let skip_order = "fn fail_fast_skip_order_is_lexicographic_and_never_starts";
    assert!(
        domain_fixtures.contains(success),
        "AUT-19 success fixture missing"
    );
    assert!(
        domain_fixtures.contains(skip_order),
        "AUT-19 skip-order fixture missing"
    );
    let success_at = domain_fixtures
        .find(success)
        .expect("success fixture is present");
    for fixture in deny_first {
        let at = domain_fixtures
            .find(fixture)
            .expect("deny fixture is present");
        assert!(
            at < success_at,
            "AUT-19 deny fixture must precede the success fixture: {fixture}"
        );
    }
    assert!(
        !domain_fixtures.contains("#[ignore]"),
        "AUT-19 fixtures must not be ignored"
    );

    // 3. 手动 lane：只允许 workflow_dispatch、contents: read，包含 locked fetch、fmt 门、
    //    两个聚焦 target、串行执行与 --no-run 编译。
    for marker in [
        "workflow_dispatch:",
        "contents: read",
        "cargo fetch --locked",
        "cargo fmt --all --check",
        "cargo test -p kiana-domain --test automation_fanout --locked",
        "cargo test -p kiana-core --test automation_fanout_guard --locked",
        "--test-threads=1",
        "--no-run",
    ] {
        assert!(
            lane.contains(marker),
            "AUT-19 lane marker missing: {marker}"
        );
    }
    assert!(
        !lane.contains("push:") && !lane.contains("pull_request:") && !lane.contains("schedule:"),
        "AUT-19 lane must stay manual-only"
    );

    // 4. 分片：两个 target 已由既有 chunked shard 发现，不做增量改动也必须在册。
    assert!(
        shards.contains("\"automation_fanout\""),
        "AUT-19 domain target must be claimed by kiana-domain-s1/4"
    );
    assert!(
        shards.contains("\"automation_fanout_guard\""),
        "AUT-19 core guard must be claimed by kiana-core-s1/6"
    );
}

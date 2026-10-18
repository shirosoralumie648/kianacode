//! AUT-18 source guard：pause/resume/consume signal 的契约、Core 协调器、store 适配器、runner
//! 边界、夹具与 CI 接线必须保持绑定，防止后续改动放宽“先拒绝”顺序、把单条事实的字段漂移当成
//! 重放，或让 signal 决策变成第二条执行入口。

#[test]
fn automation_signal_is_owner_and_checkpoint_bound() {
    let domain = include_str!("../../kiana-domain/src/automation_signal.rs");
    let core = include_str!("../src/automation_signal.rs");
    for marker in [
        "AutomationSignalFact",
        "Paused",
        "Resumed",
        "Consumed",
        "owner_id",
        "checkpoint_sequence",
        "validate_automation_signal_fact",
    ] {
        assert!(
            domain.contains(marker) || core.contains(marker),
            "AUT-18 marker missing: {marker}"
        );
    }
    for forbidden in [
        "CapabilityBroker::new",
        "ModelClient::new",
        "std::process::Command",
        "tokio::spawn",
        "EventStore::append",
    ] {
        assert!(
            !domain.contains(forbidden) && !core.contains(forbidden),
            "AUT-18 boundary executes effects: {forbidden}"
        );
    }
}

/// AUT-18 完成切片守卫：把 pause/resume/consume 状态、审批单次消费、checkpoint 绑定、store
/// 适配器、runner 边界、夹具名、手动 lane 与分片绑定在源码字面量上。
#[test]
fn aut18_completion_slice_stays_wired_to_the_store_lane_and_shard() {
    let domain = include_str!("../../kiana-domain/src/automation_signal.rs");
    let core = include_str!("../src/automation_signal.rs");
    let store = include_str!("../../kiana-eventlog/src/automation_signal.rs");
    let runner_gate = include_str!("../../kiana-runner/src/signal.rs");
    let runner_lib = include_str!("../../kiana-runner/src/lib.rs");
    let domain_fixtures = include_str!("../../kiana-domain/tests/automation_signal.rs");
    let core_fixtures = include_str!("automation_signal_worker.rs");
    let store_fixtures = include_str!("../../kiana-eventlog/tests/aut18_signal_store.rs");
    let runner_fixtures = include_str!("../../kiana-runner/tests/aut18_signal_gate.rs");
    let lane = include_str!("../../.github/workflows/aut18-signal.yml");
    let shards = include_str!("../../scripts/ci/test-shards.json");

    // 1. Domain：四类 schema、request/approval/checkpoint DTO、计划函数、账本与单次消费豁免。
    for marker in [
        "AUTOMATION_SIGNAL_SCHEMA",
        "AUTOMATION_SIGNAL_REQUEST_SCHEMA",
        "AUTOMATION_SIGNAL_APPROVAL_SCHEMA",
        "AUTOMATION_CHECKPOINT_SCHEMA",
        "pub struct AutomationSignalFact",
        "pub enum AutomationSignalState",
        "pub struct AutomationSignalRequest",
        "pub struct AutomationSignalApproval",
        "pub struct AutomationCheckpoint",
        "pub struct AutomationSignalPlan",
        "pub struct AutomationSignalCommit",
        "pub struct AutomationSignalLedger",
        "pub fn plan_automation_signal",
        "pub fn automation_signal_transition_allowed",
        "pub fn validate_automation_signal(",
        "pub fn record_recovered",
        "fn approval_reuse_allowed",
        "automation_signal_ui_context_forbidden",
        "automation_signal_approval_expired",
        "automation_signal_approval_consumed",
        "automation_signal_approval_digest_drift",
        "automation_signal_recovery_conflict",
        "dispatch_permitted",
    ] {
        assert!(
            domain.contains(marker),
            "AUT-18 domain marker missing: {marker}"
        );
    }
    // 2. Core 协调器：端口顺序（形状 → 重取 → 重建 → 计划 → CAS）与四个去向。
    for marker in [
        "pub async fn apply_automation_signal",
        "pub fn validate_automation_signal_fact",
        "pub enum AutomationSignalOutcome",
        "AutomationSignalOutcome::Paused(fact)",
        "AutomationSignalOutcome::Resumed(fact)",
        "AutomationSignalOutcome::Consumed(fact)",
        "AutomationSignalOutcome::Replayed(fact)",
        "fn rebuild_signal_ledger",
        "record_recovered",
        "plan_automation_signal",
        "fetch_signals",
        "commit_signal",
        "automation_signal_error",
    ] {
        assert!(
            core.contains(marker),
            "AUT-18 core marker missing: {marker}"
        );
    }
    // 3. Store 适配器：摘要 CAS、signal_id 去重、序号、转移白名单、epoch 与审批单次消费。
    for marker in [
        "pub struct MemoryAutomationSignalStore",
        "impl AutomationSignalStore for MemoryAutomationSignalStore",
        "automation_signal_digest_conflict",
        "automation_signal_signal_conflict",
        "automation_signal_sequence_regression",
        "automation_signal_sequence_gap",
        "automation_signal_transition_refused",
        "automation_signal_owner_drift",
        "automation_signal_definition_digest_drift",
        "automation_signal_action_digest_drift",
        "automation_signal_path_scope_drift",
        "automation_signal_authority_epoch_stale",
        "automation_signal_authority_epoch_ahead",
        "automation_signal_checkpoint_sequence_regression",
        "automation_signal_approval_consumed",
        "automation_signal_approval_digest_drift",
        "automation_signal_fact_invalid",
    ] {
        assert!(
            store.contains(marker),
            "AUT-18 store marker missing: {marker}"
        );
    }
    assert!(
        store.contains("没有 fsync") && store.contains("绝不能据此声称 durable"),
        "AUT-18 store must declare that it is not durable"
    );
    // 4. Runner 边界：纯折叠器与四个动作，复用 domain 转移白名单，不执行任何副作用。
    for marker in [
        "pub struct RunnerSignalGate",
        "pub struct RunnerSignalIdentity",
        "pub enum RunnerSignalDirective",
        "pub fn fold(",
        "model_calls_permitted",
        "automation_signal_transition_allowed",
        "runner_signal_checkpoint_unknown",
    ] {
        assert!(
            runner_gate.contains(marker) || runner_lib.contains(marker),
            "AUT-18 runner marker missing: {marker}"
        );
    }
    assert!(
        runner_lib.contains(
            "pub use signal::{RunnerSignalDirective, RunnerSignalGate, RunnerSignalIdentity};"
        ),
        "runner signal exports must stay wired to lib.rs"
    );
    for forbidden in [
        "CapabilityBroker::new",
        "ModelClient::new",
        "std::process::Command",
        "tokio::spawn",
        "EventStore::append",
    ] {
        assert!(
            !runner_gate.contains(forbidden),
            "AUT-18 runner gate executes effects: {forbidden}"
        );
    }
    // 5. 夹具名 ↔ 真实测试函数一一对应，且新增夹具没有忽略测试。
    for fixture in [
        "request_and_fact_shapes_fail_closed",
        "approver_role_and_self_approval_are_refused",
        "approval_window_and_kind_drift_fail_closed",
        "ui_context_is_refused_before_any_plan",
        "pause_requires_a_bound_checkpoint_and_no_approval",
        "owner_definition_action_and_path_drift_are_refused",
        "sequence_order_and_dedupe_are_stable",
        "authority_epoch_rules_are_enforced",
        "restart_resume_requires_matching_identity_and_checkpoint",
        "approval_must_bind_the_pause_epoch_and_digest",
        "approval_expiry_is_strict_at_the_window_boundary",
        "checkpoint_sequence_cannot_regress",
        "resumed_signal_is_consumed_exactly_once",
        "resume_and_consume_require_a_paused_or_resumed_predecessor",
        "ledger_recovery_fails_closed_on_conflicting_history",
        "consumed_fact_must_reuse_the_resume_approval_digest",
        "plan_never_permits_dispatch",
    ] {
        assert!(
            domain_fixtures.contains(&format!("fn {fixture}()")),
            "AUT-18 domain fixture missing: {fixture}"
        );
    }
    for fixture in [
        "shape_failures_never_touch_the_store",
        "pause_is_checkpoint_bound_and_requires_no_approval",
        "caller_snapshots_cannot_resume_after_identity_drift",
        "duplicate_signal_replays_without_a_second_commit",
        "restart_resume_is_rebuilt_from_committed_history",
        "approval_expiry_and_single_use_are_enforced",
        "consume_is_terminal_and_cannot_be_resurrected",
        "store_history_that_cannot_be_replayed_fails_closed",
        "uncertain_commits_are_never_reported_as_success",
    ] {
        assert!(
            core_fixtures.contains(&format!("async fn {fixture}()")),
            "AUT-18 core fixture missing: {fixture}"
        );
    }
    for fixture in [
        "first_commit_requires_the_absent_history_and_a_paused_fact",
        "identical_replays_return_the_stored_copy_without_rewriting",
        "signal_sequence_regression_and_gaps_are_refused",
        "state_transitions_stay_within_the_pause_resume_consume_whitelist",
        "identity_drift_inside_one_execution_is_refused",
        "authority_epoch_can_only_advance_on_a_restart_resume",
        "an_approval_digest_supports_exactly_one_resume_and_one_consume_reference",
        "fetch_is_a_pure_read_and_missing_executions_return_an_empty_history",
    ] {
        assert!(
            store_fixtures.contains(&format!("async fn {fixture}()")),
            "AUT-18 store fixture missing: {fixture}"
        );
    }
    for fixture in [
        "identity_shape_failures_refuse_before_any_fact",
        "empty_history_continues_and_pause_holds",
        "resume_and_consume_map_to_resume_and_closed",
        "restart_epoch_advance_is_honored_only_on_resume_facts",
        "execution_and_identity_drift_are_refused",
        "sequences_must_start_at_one_and_stay_contiguous",
        "duplicate_facts_dedupe_stably_and_conflicts_are_refused",
        "checkpoint_binding_is_exact",
        "transitions_stay_within_the_pause_resume_consume_whitelist",
        "authority_epoch_never_goes_backwards",
        "invalid_facts_are_refused_before_any_directive",
        "aut18_runner_gate_stays_a_pure_source_boundary",
    ] {
        assert!(
            runner_fixtures.contains(&format!("fn {fixture}()")),
            "AUT-18 runner fixture missing: {fixture}"
        );
    }
    for fixtures in [
        &domain_fixtures,
        &core_fixtures,
        &store_fixtures,
        &runner_fixtures,
    ] {
        assert!(!fixtures.contains("#[ignore"));
    }
    // 6. 手动 lane 逐字绑定五个测试 target 与五个 --no-run 编译步，且保持 workflow_dispatch-only。
    for line in [
        "cargo test -p kiana-domain --test automation_signal --locked -- --test-threads=1",
        "cargo test -p kiana-core --test automation_signal_worker --locked -- --test-threads=1",
        "cargo test -p kiana-core --test automation_signal_guard --locked -- --test-threads=1",
        "cargo test -p kiana-eventlog --test aut18_signal_store --locked -- --test-threads=1",
        "cargo test -p kiana-runner --test aut18_signal_gate --locked -- --test-threads=1",
        "cargo test -p kiana-domain --test automation_signal --no-run --locked",
        "cargo test -p kiana-core --test automation_signal_worker --no-run --locked",
        "cargo test -p kiana-core --test automation_signal_guard --no-run --locked",
        "cargo test -p kiana-eventlog --test aut18_signal_store --no-run --locked",
        "cargo test -p kiana-runner --test aut18_signal_gate --no-run --locked",
    ] {
        assert!(lane.contains(line), "AUT-18 lane step missing: {line}");
    }
    assert!(
        lane.contains("workflow_dispatch:")
            && !lane.contains("pull_request:")
            && !lane.contains("push:")
            && !lane.contains("continue-on-error")
            && !lane.contains("--ignored")
    );
    // 7. 新增 core target 必须登记在分片册里，否则统一门不会跑它。
    assert!(
        shards.contains("\"automation_signal_worker\""),
        "AUT-18 core target is not registered in the CI shard inventory"
    );
}

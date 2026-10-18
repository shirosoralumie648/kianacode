//! AUT-15 source guard：dispatch/observation 的契约、协调器顺序、store 适配器、夹具与 CI 接线
//! 必须保持绑定，防止后续改动放宽“先拒绝”顺序或 effect 边界。

#[test]
fn automation_dispatch_keeps_worker_and_observation_separate() {
    let domain = include_str!("../../kiana-domain/src/automation_dispatch.rs");
    let core = include_str!("../src/automation_dispatch.rs");
    for marker in [
        "AutomationDispatchIntent",
        "AutomationObservation",
        "AutomationObservationOutcome",
        "worker_id",
        "authority_epoch",
        "Unknown",
        "observe_automation_dispatch",
    ] {
        assert!(
            domain.contains(marker) || core.contains(marker),
            "AUT-15 marker missing: {marker}"
        );
    }
    for forbidden in [
        "CapabilityBroker::new",
        "ModelClient::new",
        "std::process::Command",
        "tokio::spawn",
        "EventStore::append",
        "dispatch_effect",
    ] {
        assert!(
            !domain.contains(forbidden) && !core.contains(forbidden),
            "AUT-15 boundary executes effects: {forbidden}"
        );
    }
}

/// AUT-15 完成切片守卫：把可重取命令契约、协调器顺序、store 适配器、夹具名、手动 lane 与分片
/// 绑定在源码字面量上，防止后续改动把“先拒绝”顺序或 effect 边界悄悄放宽。
#[test]
fn aut15_completion_slice_stays_wired_to_the_store_lane_and_shard() {
    let domain = include_str!("../../kiana-domain/src/automation_dispatch.rs");
    let core = include_str!("../src/automation_dispatch.rs");
    let store = include_str!("../../kiana-eventlog/src/automation_dispatch.rs");
    let domain_fixtures = include_str!("../../kiana-domain/tests/automation_dispatch.rs");
    let core_fixtures = include_str!("automation_dispatch_worker.rs");
    let store_fixtures = include_str!("../../kiana-eventlog/tests/aut15_dispatch_store.rs");
    let daemon = include_str!("../../kiana-daemon/src/automation_dispatch.rs");
    let daemon_guard = include_str!("../../kiana-daemon/tests/aut15_dispatch_observation_guard.rs");
    let lane = include_str!("../../.github/workflows/aut15-dispatch-observation.yml");
    let shards = include_str!("../../scripts/ci/test-shards.json");

    // 1. Domain：可重取命令、唯一 claim、revision 围栏与结构化结果的转移函数都在源码里。
    for marker in [
        "AUTOMATION_DISPATCH_COMMAND_SCHEMA",
        "pub struct AutomationDispatchCommand",
        "pub struct AutomationDispatchResult",
        "pub struct AutomationDispatchFailure",
        "pub enum AutomationDispatchClaim",
        "pub enum AutomationWorkerEffectOutcome",
        "pub fn prepare_automation_dispatch_command",
        "pub fn claim_automation_dispatch_command",
        "pub fn observe_automation_dispatch_command",
        "pub fn automation_dispatch_result",
        "pub struct AutomationDispatchCommit",
        "pub struct AutomationDispatchClaimRequest",
        "pub struct AutomationDispatchObservationRequest",
    ] {
        assert!(
            domain.contains(marker),
            "AUT-15 domain marker missing: {marker}"
        );
    }
    // 2. 拒绝码矩阵：准备/claim/观察/结果四类都必须有稳定错误码。
    for code in [
        "automation_dispatch_command_invalid",
        "automation_dispatch_command_intent_not_prepared",
        "automation_dispatch_command_result_unexpected",
        "automation_dispatch_command_result_required",
        "automation_dispatch_command_result_drift",
        "automation_dispatch_command_state_outcome_drift",
        "automation_dispatch_revision_conflict",
        "automation_dispatch_revision_overflow",
        "automation_dispatch_worker_conflict",
        "automation_dispatch_observation_not_claimed",
        "automation_dispatch_observation_conflict",
        "automation_dispatch_failure_required",
        "automation_dispatch_failure_unexpected",
        "automation_dispatch_failure_invalid",
        "automation_dispatch_claim_worker_required",
    ] {
        assert!(
            domain.contains(code),
            "AUT-15 rejection code missing: {code}"
        );
    }
    // 3. Core 协调器顺序：重取 -> CAS claim -> 恰好一次 runner -> 提交观察，缺一不可。
    let fetch = core.find("store.fetch_command").expect("re-fetch first");
    let claim = core.find(".claim_command(").expect("CAS claim");
    let run = core
        .find("runner.run_dispatch_attempt")
        .expect("single effect attempt");
    let observe = core.find(".observe_command(").expect("observation commit");
    assert!(
        fetch < claim && claim < run && run < observe,
        "AUT-15 coordinator must re-fetch, claim, run and observe in that order"
    );
    for marker in [
        "pub async fn dispatch_automation_worker_command",
        "pub enum AutomationWorkerDispatchOutcome",
        "AutomationDispatchStore",
        "AutomationWorkerEffectRunner",
        "AutomationDispatchClaim::AlreadyClaimed",
        "AutomationDispatchClaim::Terminal",
        "PortError::Conflict",
    ] {
        assert!(
            core.contains(marker),
            "AUT-15 core marker missing: {marker}"
        );
    }
    // 4. Store 适配器：CAS 提交、幂等重取、唯一 claim 与观察围栏都在这里，且声明 CI 语义。
    for marker in [
        "pub struct MemoryAutomationDispatchStore",
        "impl AutomationDispatchStore for MemoryAutomationDispatchStore",
        "automation_dispatch_idempotency_conflict",
        "automation_dispatch_command_conflict",
        "automation_dispatch_revision_conflict",
        "automation_dispatch_command_unknown",
        "automation_dispatch_store_corrupt",
        "fetch_command_by_idempotency_key",
        "fn domain_error(",
    ] {
        assert!(
            store.contains(marker),
            "AUT-15 store marker missing: {marker}"
        );
    }
    assert!(
        store.contains("没有 fsync") && store.contains("绝不能据此声称 durable"),
        "AUT-15 store must declare that it is not durable"
    );
    // 5. Daemon 服务：有界 channel、单 worker、恢复计数与 Host 接线，且没有第二个执行面。
    for marker in [
        "pub struct AutomationDispatchService",
        "AUTOMATION_DISPATCH_CHANNEL_CAPACITY",
        "pub struct AutomationDispatchShutdownReport",
        "recovery_required_count",
        "dispatch_automation_worker_command",
    ] {
        assert!(
            daemon.contains(marker),
            "AUT-15 daemon marker missing: {marker}"
        );
    }
    assert!(daemon_guard.contains("aut15_daemon_fixtures_stay_wired_to_the_service_contract"));
    // 6. 夹具名 ↔ 真实测试函数一一对应，且新增夹具没有忽略测试。
    for fixture in [
        "prepared_command_binds_identity_and_refuses_incoherent_payloads",
        "claim_binds_one_worker_attempt_and_refuses_duplicates",
        "observation_requires_a_claim_and_structured_failure",
        "terminal_results_cannot_be_overwritten_or_resurrected",
        "command_payloads_fail_closed_on_unknown_fields_and_drift",
    ] {
        assert!(
            domain_fixtures.contains(&format!("fn {fixture}()")),
            "AUT-15 domain fixture missing: {fixture}"
        );
    }
    for fixture in [
        "dispatch_command_is_refetched_and_replays_without_a_second_effect",
        "unknown_commands_and_illegal_identity_never_reach_the_runner",
        "structured_failure_and_unknown_are_recorded_without_a_second_effect",
        "uncertain_result_commits_stay_unknown_and_never_fabricate_success",
        "runner_reported_unknown_and_illegal_shapes_never_become_success",
        "concurrent_workers_execute_exactly_one_effect",
    ] {
        assert!(
            core_fixtures.contains(&format!("fn {fixture}()")),
            "AUT-15 core fixture missing: {fixture}"
        );
    }
    for fixture in [
        "committed_command_is_refetched_after_a_crash_and_never_recommits",
        "one_command_yields_exactly_one_worker_attempt",
        "observation_revision_races_never_overwrite_a_terminal_result",
        "concurrent_workers_leave_exactly_one_winning_claim",
    ] {
        assert!(
            store_fixtures.contains(&format!("async fn {fixture}()")),
            "AUT-15 store fixture missing: {fixture}"
        );
    }
    for fixture in [&domain_fixtures, &core_fixtures, &store_fixtures] {
        assert!(!fixture.contains("#[ignore"));
    }
    // 7. 手动 lane 逐字绑定六个测试 target 与六个 --no-run 编译步，且保持 workflow_dispatch-only。
    for line in [
        "cargo test -p kiana-domain --test automation_dispatch --locked -- --test-threads=1",
        "cargo test -p kiana-core --test automation_dispatch_worker --locked -- --test-threads=1",
        "cargo test -p kiana-core --test automation_dispatch_guard --locked -- --test-threads=1",
        "cargo test -p kiana-eventlog --test aut15_dispatch_store --locked -- --test-threads=1",
        "cargo test -p kiana-daemon --test aut15_dispatch_observation --locked -- --test-threads=1",
        "cargo test -p kiana-daemon --test aut15_dispatch_observation_guard --locked -- --test-threads=1",
        "cargo test -p kiana-domain --test automation_dispatch --no-run --locked",
        "cargo test -p kiana-core --test automation_dispatch_worker --no-run --locked",
        "cargo test -p kiana-core --test automation_dispatch_guard --no-run --locked",
        "cargo test -p kiana-eventlog --test aut15_dispatch_store --no-run --locked",
        "cargo test -p kiana-daemon --test aut15_dispatch_observation --no-run --locked",
        "cargo test -p kiana-daemon --test aut15_dispatch_observation_guard --no-run --locked",
        "cargo fetch --locked",
        "cargo fmt --all --check",
    ] {
        assert!(lane.contains(line), "AUT-15 lane step missing: {line}");
    }
    for forbidden in [
        "\n  push:",
        "\n  pull_request:",
        "\n  schedule:",
        "continue-on-error",
        "--ignored",
        "cargo test --workspace",
    ] {
        assert!(
            !lane.contains(forbidden),
            "AUT-15 lane must stay manual and bounded: {forbidden}"
        );
    }
    assert!(lane.contains("workflow_dispatch:"));
    assert!(lane.contains("permissions:\n  contents: read"));
    // 8. 新 core target 必须被唯一分片认领（daemon/eventlog 为整 crate 分片自动发现）。
    assert!(
        shards.contains("\"automation_dispatch_worker\""),
        "AUT-15 core fixture target must be claimed by a shard"
    );
}

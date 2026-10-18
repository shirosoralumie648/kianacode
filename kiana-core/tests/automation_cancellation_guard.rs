//! AUT-17 source guard：取消代次、stop report、迟到结果 fence 与 result_unknown 的契约、协调器
//! 顺序、store 适配器、daemon 服务、夹具与 CI 接线必须保持绑定，防止后续改动放宽“先拒绝”顺序或
//! 把 cancelled/停止证据悄悄放宽。

#[test]
fn automation_cancellation_is_a_fenced_observation_boundary() {
    let domain = include_str!("../../kiana-domain/src/automation_cancellation.rs");
    let core = include_str!("../src/automation_cancellation.rs");
    for marker in [
        "AutomationCancellationFact",
        "AutomationCancelState",
        "cancel_generation",
        "stop_confirmed",
        "ResultUnknown",
        "late_result",
        "validate_automation_cancel",
    ] {
        assert!(
            domain.contains(marker) || core.contains(marker),
            "AUT-17 marker missing: {marker}"
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
            "AUT-17 boundary executes effects: {forbidden}"
        );
    }
}

/// AUT-17 完成切片守卫：把取消状态矩阵、代次/epoch 围栏、stop report 证据、迟到结果 fence、
/// store 适配器、daemon 服务、夹具名、手动 lane 与分片绑定在源码字面量上。
#[test]
fn aut17_completion_slice_stays_wired_to_the_store_lane_and_shard() {
    let domain = include_str!("../../kiana-domain/src/automation_cancellation.rs");
    let core = include_str!("../src/automation_cancellation.rs");
    let store = include_str!("../../kiana-eventlog/src/automation_cancellation.rs");
    let domain_fixtures = include_str!("../../kiana-domain/tests/automation_cancellation.rs");
    let core_fixtures = include_str!("automation_cancellation_worker.rs");
    let store_fixtures = include_str!("../../kiana-eventlog/tests/aut17_cancellation_store.rs");
    let daemon = include_str!("../../kiana-daemon/src/automation_cancellation.rs");
    let daemon_fixtures = include_str!("../../kiana-daemon/tests/aut17_cancellation.rs");
    let daemon_guard = include_str!("../../kiana-daemon/tests/aut17_cancellation_guard.rs");
    let lane = include_str!("../../.github/workflows/aut17-cancellation.yml");
    let shards = include_str!("../../scripts/ci/test-shards.json");

    // 1. Domain：请求/报告/迟到结果 DTO、完整状态矩阵与三个纯转移函数。
    for marker in [
        "AUTOMATION_CANCEL_REQUEST_SCHEMA",
        "AUTOMATION_STOP_REPORT_SCHEMA",
        "AUTOMATION_LATE_RESULT_SCHEMA",
        "pub struct AutomationCancelRequest",
        "pub struct AutomationStopReport",
        "pub struct AutomationLateResult",
        "pub enum AutomationStopState",
        "NotExecuted",
        "stop_evidence_digest",
        "pub fn begin_automation_cancellation",
        "pub fn bind_automation_stop_report",
        "pub fn fold_automation_stop_report",
        "pub fn fence_automation_late_result",
        "pub fn automation_cancel_transition_allowed",
        "pub struct AutomationCancelPlan",
        "pub fn recovery_required",
    ] {
        assert!(
            domain.contains(marker),
            "AUT-17 domain marker missing: {marker}"
        );
    }
    // 2. Core 协调器：端口顺序（重取 → 落库 → stop → 折叠 → CAS）与四个去向。
    for marker in [
        "pub async fn cancel_automation_worker_command",
        "pub async fn record_automation_late_result",
        "pub enum AutomationCancellationOutcome",
        "AutomationCancellationOutcome::NotExecuted",
        "AutomationCancellationOutcome::Stopped",
        "AutomationCancellationOutcome::RecoveryRequired",
        "AutomationCancellationOutcome::Replayed",
        "_ => AutomationCancellationOutcome::RecoveryRequired(fact)",
        "fetch_command",
        "fetch_cancellation",
        "commit_cancellation",
        "request_stop",
        "fold_automation_stop_report",
        "fence_automation_late_result",
        "automation_cancel_fact_unknown",
    ] {
        assert!(
            core.contains(marker),
            "AUT-17 core marker missing: {marker}"
        );
    }
    // 3. Store 适配器：摘要 CAS、代次单调、同代次转移白名单与 CI 语义声明。
    for marker in [
        "pub struct MemoryAutomationCancellationStore",
        "impl AutomationCancellationStore for MemoryAutomationCancellationStore",
        "automation_cancellation_digest_conflict",
        "automation_cancel_generation_stale",
        "automation_cancel_authority_epoch_conflict",
        "automation_cancel_transition_refused",
        "automation_cancellation_fact_invalid",
        "fetch_cancellation",
    ] {
        assert!(
            store.contains(marker),
            "AUT-17 store marker missing: {marker}"
        );
    }
    assert!(
        store.contains("没有 fsync") && store.contains("绝不能据此声称 durable"),
        "AUT-17 store must declare that it is not durable"
    );
    // 4. Daemon 服务：有界 channel、单 worker、恢复计数与 Host 接线，且没有第二个执行面。
    for marker in [
        "pub struct AutomationCancellationService",
        "AUTOMATION_CANCELLATION_CHANNEL_CAPACITY",
        "pub struct AutomationCancellationShutdownReport",
        "recovery_required_count",
        "cancel_automation_worker_command",
        "record_automation_late_result",
    ] {
        assert!(
            daemon.contains(marker),
            "AUT-17 daemon marker missing: {marker}"
        );
    }
    assert!(daemon_guard.contains("aut17_daemon_fixtures_stay_wired_to_the_service_contract"));
    // 5. 夹具名 ↔ 真实测试函数一一对应，且新增夹具没有忽略测试。
    for fixture in [
        "stopped_and_unknown_are_visible_and_late_requires_fence",
        "queued_cancel_is_not_executed_and_never_requests_a_stop",
        "claimed_cancel_requests_a_stop_before_any_confirmation",
        "unknown_execution_enters_recovery_and_finished_execution_cannot_be_cancelled",
        "generation_and_epoch_fences_refuse_stale_cancels",
        "stop_reports_require_evidence_and_matching_effect_state",
        "unconfirmed_stop_reports_never_produce_a_confirmed_cancel",
        "late_results_are_fenced_and_cannot_resurrect_any_cancel_state",
        "cancel_state_matrix_is_closed",
        "request_and_late_result_shapes_fail_closed",
    ] {
        assert!(
            domain_fixtures.contains(&format!("fn {fixture}()")),
            "AUT-17 domain fixture missing: {fixture}"
        );
    }
    for fixture in [
        "queued_cancellation_is_not_executed_and_never_touches_the_stop_controller",
        "claimed_cancellation_requests_exactly_one_stop_and_records_the_evidence",
        "unconfirmed_or_unavailable_stops_are_recovery_only",
        "drifted_stop_reports_are_never_adopted",
        "terminal_and_unknown_commands_fail_closed",
        "stale_or_conflicting_requests_never_re_request_a_stop",
        "late_results_are_fenced_and_cannot_become_a_success",
    ] {
        assert!(
            core_fixtures.contains(&format!("async fn {fixture}()")),
            "AUT-17 core fixture missing: {fixture}"
        );
    }
    for fixture in [
        "first_commit_requires_an_absent_fact_and_stale_digests_are_refused",
        "identical_replays_return_the_stored_copy_without_rewriting",
        "stale_generations_cannot_advance_a_recorded_cancel",
        "closed_states_cannot_be_reopened_within_a_generation",
        "epoch_drift_within_a_generation_is_refused",
        "invalid_facts_never_reach_the_store",
        "fetch_is_a_pure_read_and_missing_keys_return_none",
    ] {
        assert!(
            store_fixtures.contains(&format!("async fn {fixture}()")),
            "AUT-17 store fixture missing: {fixture}"
        );
    }
    for fixture in [
        "daemon_host_owns_the_cancellation_service_and_fails_closed_before_start",
        "queued_cancellation_is_not_executed_and_claimed_cancellation_stops_with_evidence",
        "unconfirmed_and_unknown_cancellations_are_counted_as_recovery",
        "late_results_are_fenced_and_never_counted_as_stopped",
    ] {
        assert!(
            daemon_fixtures.contains(&format!("async fn {fixture}()")),
            "AUT-17 daemon fixture missing: {fixture}"
        );
    }
    for fixtures in [
        &domain_fixtures,
        &core_fixtures,
        &store_fixtures,
        &daemon_fixtures,
    ] {
        assert!(!fixtures.contains("#[ignore"));
    }
    // 6. 手动 lane 逐字绑定六个测试 target 与六个 --no-run 编译步，且保持 workflow_dispatch-only。
    for line in [
        "cargo test -p kiana-domain --test automation_cancellation --locked -- --test-threads=1",
        "cargo test -p kiana-core --test automation_cancellation_worker --locked -- --test-threads=1",
        "cargo test -p kiana-core --test automation_cancellation_guard --locked -- --test-threads=1",
        "cargo test -p kiana-eventlog --test aut17_cancellation_store --locked -- --test-threads=1",
        "cargo test -p kiana-daemon --test aut17_cancellation --locked -- --test-threads=1",
        "cargo test -p kiana-daemon --test aut17_cancellation_guard --locked -- --test-threads=1",
        "cargo test -p kiana-domain --test automation_cancellation --no-run --locked",
        "cargo test -p kiana-core --test automation_cancellation_worker --no-run --locked",
        "cargo test -p kiana-core --test automation_cancellation_guard --no-run --locked",
        "cargo test -p kiana-eventlog --test aut17_cancellation_store --no-run --locked",
        "cargo test -p kiana-daemon --test aut17_cancellation --no-run --locked",
        "cargo test -p kiana-daemon --test aut17_cancellation_guard --no-run --locked",
    ] {
        assert!(lane.contains(line), "AUT-17 lane step missing: {line}");
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
        shards.contains("\"automation_cancellation_worker\""),
        "AUT-17 core target is not registered in the CI shard inventory"
    );
}

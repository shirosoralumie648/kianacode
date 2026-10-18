//! AUT-16 source guard：重试分类器、attempt/lease/deadline 账本、幂等描述符、Core 协调器的
//! 只读边界、夹具名、手动 lane 与分片必须保持绑定，防止后续改动放宽“先拒绝”顺序或 effect 边界。

#[test]
fn automation_retry_is_bounded_and_never_executes_effects() {
    let workflow = include_str!("../../kiana-workflow/src/retry.rs");
    let core = include_str!("../src/automation_retry_attempt.rs");
    for marker in [
        "plan_automation_retry_attempt",
        "AutomationRetryRequest",
        "AutomationRetryAttempt",
        "AutomationRetryAttemptLedger",
        "AutomationIdempotencyDescriptor",
        "AutomationRetryRefusal",
        "record_retry_attempt",
        "validate_retry_attempt_start",
        "ReconcileUnknown",
        "EffectReceiptMissing",
        "NonIdempotentCapability",
        "ApprovalExpired",
        "BudgetInsufficient",
        "AttemptBoundExhausted",
        "DeadlineExceeded",
        "lease_expires_at_ms",
        "deadline_ms",
        "idempotency_key",
        "record_automation_retry_attempt",
    ] {
        assert!(
            workflow.contains(marker) || core.contains(marker),
            "AUT-16 marker missing: {marker}"
        );
    }
    for forbidden in [
        "CapabilityBroker::new",
        "ModelClient::new",
        "KianaHarness",
        "std::process::Command",
        "tokio::spawn",
        "EventStore::append",
        "dispatch_effect",
        "apply_patch",
        "reqwest::",
    ] {
        assert!(
            !workflow.contains(forbidden) && !core.contains(forbidden),
            "AUT-16 retry boundary executes effects: {forbidden}"
        );
    }
    // Core 协调器对 store 只读：不得出现 claim/observe/commit 命令事实或调用 runner 的字面量。
    for forbidden in [
        "claim_command",
        "observe_command",
        "commit_command",
        "run_dispatch_attempt",
        "AutomationWorkerEffectRunner",
    ] {
        assert!(
            !core.contains(forbidden),
            "AUT-16 core retry coordinator must stay read-only: {forbidden}"
        );
    }
}

#[test]
fn aut16_completion_slice_stays_wired_to_the_fixture_lane_and_shard() {
    let workflow = include_str!("../../kiana-workflow/src/retry.rs");
    let core = include_str!("../src/automation_retry_attempt.rs");
    let workflow_fixtures = include_str!("../../kiana-workflow/tests/aut16_retry_attempt.rs");
    let core_fixtures = include_str!("aut16_retry_command.rs");
    let lane = include_str!("../../.github/workflows/aut16-retry.yml");
    let shards = include_str!("../../scripts/ci/test-shards.json");

    // 1. 分类器与账本的稳定 refusal/拒绝码必须在源码与夹具中同时存在。
    for code in [
        "automation_retry_effect_receipt_missing",
        "automation_retry_not_retryable_failure",
        "automation_retry_descriptor_required",
        "automation_retry_effect_class_mismatch",
        "automation_retry_non_idempotent_capability",
        "automation_retry_idempotency_key_missing",
        "automation_retry_approval_invalid",
        "automation_retry_approval_expired",
        "automation_retry_budget_insufficient",
        "automation_retry_attempt_bound_exhausted",
        "automation_retry_deadline_exceeded",
        "automation_retry_retry_after_exceeds_deadline",
        "automation_retry_lease_held",
        "automation_retry_lease_exceeds_deadline",
        "automation_retry_action_changed",
    ] {
        assert!(
            workflow.contains(code),
            "AUT-16 refusal code missing from workflow source: {code}"
        );
    }
    for code in [
        "automation_retry_attempt_conflict",
        "automation_retry_idempotency_key_conflict",
        "automation_retry_attempt_sequence_invalid",
        "automation_retry_attempt_not_found",
        "automation_retry_lease_owner_mismatch",
        "automation_retry_clock_rollback",
    ] {
        assert!(
            workflow.contains(code),
            "AUT-16 ledger/start code missing from workflow source: {code}"
        );
    }
    for code in [
        "automation_retry_command_not_failed",
        "automation_retry_command_succeeded",
        "automation_retry_command_outcome_unknown",
        "automation_retry_command_request_invalid",
    ] {
        assert!(
            core.contains(code),
            "AUT-16 command code missing from core source: {code}"
        );
    }

    // 2. 夹具名必须与真实测试函数一一对应。
    for name in [
        "unknown_effect_is_reconciled_and_never_retried",
        "missing_receipt_descriptor_idempotency_and_action_consistency_are_refused",
        "approval_budget_and_attempt_or_deadline_bounds_are_refused",
        "pure_no_dispatch_failure_creates_a_bounded_attempt_without_a_descriptor",
        "idempotent_known_failure_creates_a_bounded_attempt_with_lease_deadline_and_decision_digest",
        "ledger_records_replays_conflicts_and_keeps_an_ordered_audit_chain",
        "ledger_refuses_a_second_attempt_while_a_lease_is_active_and_allows_it_after_expiry",
        "ledger_refuses_one_idempotency_key_bound_to_a_different_attempt",
        "attempt_start_requires_the_lease_owner_a_monotonic_clock_and_a_live_window",
        "tampered_digests_wrong_shapes_and_unknown_fields_fail_closed",
    ] {
        assert!(
            workflow_fixtures.contains(&format!("fn {name}(")),
            "AUT-16 workflow fixture missing: {name}"
        );
    }
    for name in [
        "prepared_or_succeeded_commands_are_refused_without_a_record_or_effect",
        "claimed_and_unknown_commands_require_reconciliation_not_retry",
        "known_retryable_failure_records_one_auditable_attempt_and_replays",
        "receipt_idempotency_approval_and_budget_fences_refuse_with_zero_records",
        "ledger_lease_fence_blocks_a_second_attempt_for_the_same_subject",
        "invalid_or_tampered_requests_fail_before_any_store_read",
        "unknown_fields_and_schema_drift_fail_closed",
    ] {
        assert!(
            core_fixtures.contains(&format!("fn {name}(")),
            "AUT-16 core fixture missing: {name}"
        );
    }

    // 3. 拒绝路径必须证明 0 次 store 写入与 0 条 attempt 事实。
    for marker in [
        "store.mutations(), 0",
        "ledger.is_empty()",
        "assert_eq!(store.reads(), 0)",
    ] {
        assert!(
            core_fixtures.contains(marker),
            "AUT-16 core fixture must keep the zero-effect evidence: {marker}"
        );
    }

    // 4. 手动 lane 绑定五个 target、格式门与 locked fetch，且不引入 push/PR 触发或忽略测试。
    for marker in [
        "workflow_dispatch",
        "contents: read",
        "cargo fetch --locked",
        "cargo fmt --all --check",
        "cargo test -p kiana-domain --test automation_retry --locked -- --test-threads=1",
        "cargo test -p kiana-core --test automation_retry_guard --locked -- --test-threads=1",
        "cargo test -p kiana-workflow --test aut16_retry_attempt --locked -- --test-threads=1",
        "cargo test -p kiana-core --test aut16_retry_command --locked -- --test-threads=1",
        "cargo test -p kiana-core --test aut16_retry_command_guard --locked -- --test-threads=1",
        "cargo test -p kiana-workflow --test aut16_retry_attempt --no-run --locked",
        "cargo test -p kiana-core --test aut16_retry_command --no-run --locked",
    ] {
        assert!(lane.contains(marker), "AUT-16 lane missing: {marker}");
    }
    for forbidden in [
        "push:",
        "pull_request:",
        "continue-on-error",
        "|| true",
        "--ignored",
    ] {
        assert!(
            !lane.contains(forbidden),
            "AUT-16 lane must fail closed: {forbidden}"
        );
    }

    // 5. 分片册：两个新 core target 显式登记，kiana-workflow 整 crate 自动发现，旧分类器仍在册。
    let shards: serde_json::Value = serde_json::from_str(shards).expect("shard registry parses");
    let core_targets = shards["kiana-core-s1/6"]["targets"]
        .as_array()
        .expect("kiana-core-s1/6 targets")
        .iter()
        .filter_map(|target| target.as_str())
        .collect::<Vec<_>>();
    for target in ["aut16_retry_command", "aut16_retry_command_guard"] {
        assert!(
            core_targets.contains(&target),
            "AUT-16 core shard missing target: {target}"
        );
    }
    assert!(
        shards["kiana-workflow"]["targets"].is_null(),
        "kiana-workflow shard must auto-discover every test target"
    );
    let domain_targets = shards["kiana-domain-s1/4"]["targets"]
        .as_array()
        .expect("kiana-domain-s1/4 targets")
        .iter()
        .filter_map(|target| target.as_str())
        .collect::<Vec<_>>();
    assert!(
        domain_targets.contains(&"automation_retry"),
        "AUT-16 legacy classifier target must stay in the domain shard"
    );
}

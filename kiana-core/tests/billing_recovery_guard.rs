//! BQ-21 重启/继续/重放/恢复准入的源码与接线守卫（先拒绝、后成功）。
//!
//! 守卫与夹具同处一个 target：源码 marker/拒绝码矩阵/夹具名/CI 接线检查只在源码层钉住事实，
//! 行为夹具则通过 `BillingRecoveryAdmission` 与 `ControlPlane` facade 证明同一份 Domain 规则
//! 被复用。所有夹具都在内存中合成，不连接 EventStore、不派发 capability、不调用 provider，
//! 也不把 `include_str!` 命中当成运行时证据。

use async_trait::async_trait;
use kiana_capability_broker::CapabilityBroker;
use kiana_core::{BillingRecoveryAdmission, BillingRecoveryAdmissionError, ControlPlane};
use kiana_domain::{
    BillingAttemptDisposition, BillingAttemptSnapshot, BillingBootFence, BillingReconciliation,
    BillingReconciliationOutcome, BillingRecoveryAction, BillingRecoveryApplyOutcome,
    BillingRecoveryLedger, BillingRecoveryRequest, BillingRecoveryState, CapabilityRequest,
    PendingApproval, RequestContext,
};
use kiana_eventlog::MemoryEventLog;
use kiana_gates::DefaultGateEngine;
use kiana_policy::DefaultPolicyEngine;
use kiana_ports::{ApprovalStorePort, PortError, RunnerPort};
use kiana_runner_protocol::{RunnerCommand, RunnerEvent};
use std::sync::Arc;

const DOMAIN_SOURCE: &str = include_str!("../../kiana-domain/src/billing_recovery.rs");
const CORE_SOURCE: &str = include_str!("../src/billing_recovery.rs");
const DOMAIN_FIXTURES: &str = include_str!("../../kiana-domain/tests/billing_recovery.rs");
const MANUAL_LANE: &str = include_str!("../../.github/workflows/bq21-recovery.yml");
const BASELINE: &str = include_str!("../../docs/roadmap/bq21-recovery-baseline.md");
const SHARDS: &str = include_str!("../../scripts/ci/test-shards.json");

const RESERVATION_DIGEST: &str =
    "sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
const SOURCE_EVENT_DIGEST: &str =
    "sha256:bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";
const RECONCILED_EVENT_DIGEST: &str =
    "sha256:cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc";
const ATTEMPT: &str = "attempt:billing-1";

fn boot_fence() -> BillingBootFence {
    BillingBootFence::new(9, 7, 3, 42, SOURCE_EVENT_DIGEST).expect("boot fence")
}

fn snapshot(disposition: BillingAttemptDisposition) -> BillingAttemptSnapshot {
    let reconciliation_ref = match disposition {
        BillingAttemptDisposition::Consumed => Some("reconcile:settle-1".to_owned()),
        BillingAttemptDisposition::Released => Some("reconcile:release-1".to_owned()),
        _ => None,
    };
    BillingAttemptSnapshot::new(
        ATTEMPT,
        RESERVATION_DIGEST,
        disposition,
        120,
        reconciliation_ref,
        SOURCE_EVENT_DIGEST,
        42,
    )
    .expect("attempt snapshot")
}

fn request(action: BillingRecoveryAction, accumulated_units: u64) -> BillingRecoveryRequest {
    BillingRecoveryRequest::new(
        action,
        ATTEMPT,
        RESERVATION_DIGEST,
        9,
        7,
        3,
        accumulated_units,
        SOURCE_EVENT_DIGEST,
    )
    .expect("recovery request")
}

fn reconciliation(
    outcome: BillingReconciliationOutcome,
    usage_known: bool,
    consumed_units: u64,
) -> BillingReconciliation {
    BillingReconciliation::new(
        ATTEMPT,
        RESERVATION_DIGEST,
        SOURCE_EVENT_DIGEST,
        42,
        7,
        3,
        "reconcile:billing-1",
        usage_known,
        consumed_units,
        outcome,
        RECONCILED_EVENT_DIGEST,
        43,
    )
    .expect("reconciliation")
}

struct UnavailableRunner;

#[async_trait]
impl RunnerPort for UnavailableRunner {
    async fn send(&self, _command: RunnerCommand) -> Result<Vec<RunnerEvent>, PortError> {
        Err(PortError::Unavailable("runner_unavailable".to_owned()))
    }
}

struct UnavailableApprovalStore;

#[async_trait]
impl ApprovalStorePort for UnavailableApprovalStore {
    async fn stage(
        &self,
        _context: &RequestContext,
        _request: CapabilityRequest,
        _reason: &str,
    ) -> Result<kiana_domain::ApprovalChallenge, PortError> {
        Err(PortError::Unavailable("approval_unavailable".to_owned()))
    }

    async fn activate(&self, _approval_id: kiana_domain::ApprovalId) -> Result<(), PortError> {
        Err(PortError::Unavailable("approval_unavailable".to_owned()))
    }

    async fn consume(
        &self,
        _context: &RequestContext,
        _approval_id: kiana_domain::ApprovalId,
    ) -> Result<PendingApproval, PortError> {
        Err(PortError::Unavailable("approval_unavailable".to_owned()))
    }
}

fn plane() -> ControlPlane {
    ControlPlane::new(
        Arc::new(DefaultPolicyEngine),
        Arc::new(DefaultGateEngine),
        Arc::new(MemoryEventLog::new()),
        Arc::new(CapabilityBroker::new()),
        Arc::new(UnavailableApprovalStore),
        Arc::new(UnavailableRunner),
    )
}

#[test]
fn billing_recovery_keeps_fence_and_reconciliation_visible() {
    for marker in [
        "BillingRecoveryFact",
        "BillingRecoveryState",
        "BillingRecoveryAction",
        "BillingAttemptDisposition",
        "BillingAttemptSnapshot",
        "BillingBootFence",
        "BillingRecoveryRequest",
        "BillingRecoveryPlan",
        "BillingReconciliation",
        "BillingReconciliationOutcome",
        "BillingRecoveryLedger",
        "BillingRecoveryApplyOutcome",
        "plan_billing_recovery",
        "reconcile_attempt",
        "settled_by",
        "matches_fact",
        "reservation_digest",
        "source_cursor",
        "authority_epoch",
        "lease_epoch",
        "boot_generation",
        "NeedsReconciliation",
        "continue_resets_usage",
        "settlement_count",
        "validate_billing_recovery",
    ] {
        assert!(
            DOMAIN_SOURCE.contains(marker) || CORE_SOURCE.contains(marker),
            "BQ-21 marker missing: {marker}"
        );
    }
    for marker in [
        "BillingRecoveryAdmission",
        "BillingRecoveryAdmissionError",
        "authorize_restart",
        "authorize_reconciliation",
        "admit",
        "check_fact",
        "ControlPlane",
        "admit_billing_recovery_restart",
        "admit_billing_recovery_reconciliation",
        "admit_billing_recovery_plan",
    ] {
        assert!(
            CORE_SOURCE.contains(marker),
            "BQ-21 core marker missing: {marker}"
        );
    }
    // Core 只分类 Domain 的拒绝码，不自己再实现 epoch/cursor/累计规则。
    assert!(CORE_SOURCE.contains("plan_domain_billing_recovery"));
    assert!(!CORE_SOURCE.contains("fn validate_reconciliation"));
}

#[test]
fn recovery_admission_refuses_stale_fences_terminal_attempts_and_second_settlement() {
    let fence = boot_fence();
    let in_flight = snapshot(BillingAttemptDisposition::InFlight);
    let unknown = snapshot(BillingAttemptDisposition::Unknown);
    let completed = snapshot(BillingAttemptDisposition::Consumed);

    // 旧 lease epoch 不能继续结算。
    let mut stale = request(BillingRecoveryAction::Continue, 120);
    stale.lease_epoch = 2;
    stale.request_digest = stale.digest();
    assert_eq!(
        BillingRecoveryAdmission::authorize_restart(&in_flight, &fence, &stale),
        Err(BillingRecoveryAdmissionError::Refused(
            "billing_recovery_lease_epoch_stale".to_owned()
        ))
    );

    // 重启不重发已完成 attempt。
    assert_eq!(
        BillingRecoveryAdmission::authorize_restart(
            &completed,
            &fence,
            &request(BillingRecoveryAction::Restart, 120)
        ),
        Err(BillingRecoveryAdmissionError::Refused(
            "billing_recovery_attempt_already_terminal".to_owned()
        ))
    );

    // Continue 不能重置累计。
    assert_eq!(
        BillingRecoveryAdmission::authorize_restart(
            &in_flight,
            &fence,
            &request(BillingRecoveryAction::Continue, 119)
        ),
        Err(BillingRecoveryAdmissionError::Refused(
            "billing_recovery_continue_resets_accumulation".to_owned()
        ))
    );

    // unknown 不能靠“仍然 unknown”的所谓证据自动成功。
    assert_eq!(
        BillingRecoveryAdmission::authorize_reconciliation(
            &unknown,
            &fence,
            &reconciliation(BillingReconciliationOutcome::Consumed, false, 120)
        ),
        Err(BillingRecoveryAdmissionError::Refused(
            "billing_recovery_unknown_cannot_be_success".to_owned()
        ))
    );

    // 被篡改的服务端 snapshot 是形状失败，不是策略拒绝。
    let mut tampered = in_flight.clone();
    tampered.accumulated_units = 121;
    assert_eq!(
        BillingRecoveryAdmission::authorize_restart(
            &tampered,
            &fence,
            &request(BillingRecoveryAction::Restart, 120)
        ),
        Err(BillingRecoveryAdmissionError::SnapshotInvalid(
            "billing_recovery_snapshot_invalid".to_owned()
        ))
    );

    // 显式 reconcile 只结算一次：同摘要幂等重放，第二个不同结算被拒绝。
    let plan = BillingRecoveryAdmission::authorize_reconciliation(
        &unknown,
        &fence,
        &reconciliation(BillingReconciliationOutcome::Consumed, true, 120),
    )
    .expect("reconcile plan");
    assert_eq!(plan.state, BillingRecoveryState::Recovered);
    assert_eq!(plan.settlements, 1);
    let mut ledger = BillingRecoveryLedger::default();
    assert!(matches!(
        BillingRecoveryAdmission::admit(&mut ledger, plan.clone()),
        Ok(BillingRecoveryApplyOutcome::Applied(_))
    ));
    assert!(matches!(
        BillingRecoveryAdmission::admit(&mut ledger, plan.clone()),
        Ok(BillingRecoveryApplyOutcome::Replayed(_))
    ));
    assert_eq!(ledger.settlement_count(ATTEMPT), 1);
    let mut second = plan;
    second.action = BillingRecoveryAction::Replay;
    second.plan_digest = second.digest();
    assert_eq!(
        BillingRecoveryAdmission::admit(&mut ledger, second),
        Err(BillingRecoveryAdmissionError::ReplayConflict(
            "billing_recovery_second_settlement".to_owned()
        ))
    );
}

#[test]
fn control_plane_facade_exposes_only_the_single_recovery_rule() {
    let plane = plane();
    let fence = boot_fence();
    let unknown = snapshot(BillingAttemptDisposition::Unknown);
    let in_flight = snapshot(BillingAttemptDisposition::InFlight);

    // 恢复默认 paused/needs-reconciliation：两者都不是成功、都继续占用预留。
    let paused = plane
        .admit_billing_recovery_restart(
            &in_flight,
            &fence,
            &request(BillingRecoveryAction::Restart, 120),
        )
        .expect("paused plan");
    assert_eq!(paused.state, BillingRecoveryState::Paused);
    assert!(paused.reservation_held);
    assert_eq!(paused.settlements, 0);

    let needs = plane
        .admit_billing_recovery_restart(
            &unknown,
            &fence,
            &request(BillingRecoveryAction::Restart, 120),
        )
        .expect("needs reconciliation plan");
    assert_eq!(needs.state, BillingRecoveryState::NeedsReconciliation);
    assert!(needs.usage_unknown);
    assert_eq!(needs.settlements, 0);

    let plan = plane
        .admit_billing_recovery_reconciliation(
            &unknown,
            &fence,
            &reconciliation(BillingReconciliationOutcome::Consumed, true, 120),
        )
        .expect("settlement plan");
    assert_eq!(plan.state, BillingRecoveryState::Recovered);
    let mut ledger = BillingRecoveryLedger::default();
    assert!(matches!(
        plane.admit_billing_recovery_plan(&mut ledger, plan.clone()),
        Ok(BillingRecoveryApplyOutcome::Applied(_))
    ));
    assert!(matches!(
        plane.admit_billing_recovery_plan(&mut ledger, plan.clone()),
        Ok(BillingRecoveryApplyOutcome::Replayed(_))
    ));
    assert_eq!(ledger.settlement_count(ATTEMPT), 1);

    // facade 仍拒绝旧 epoch 与已完成 attempt：没有第二条恢复写入路径。
    assert!(matches!(
        plane.admit_billing_recovery_restart(
            &snapshot(BillingAttemptDisposition::Consumed),
            &fence,
            &request(BillingRecoveryAction::Restart, 120)
        ),
        Err(BillingRecoveryAdmissionError::Refused(code))
            if code == "billing_recovery_attempt_already_terminal"
    ));
}

#[test]
fn recovery_sources_own_no_dispatch_append_release_or_second_authority() {
    for forbidden in [
        "fn update",
        "fn delete",
        "fn replace",
        "fn mutate",
        "DELETE FROM",
        "UPDATE ",
        "CapabilityBrokerPort",
        "EventStorePort",
        "reqwest",
        "TcpStream",
        "std::fs",
        "std::net",
        "std::process",
        "unsafe",
        "#[ignore",
        "tokio::spawn",
        "ModelOutput",
        "Uuid::new_v4",
        "auto_success",
        "release_reservation",
        "EventStore::append",
        "CapabilityBroker::new",
        "ModelClient::new",
    ] {
        for (name, source) in [("domain", DOMAIN_SOURCE), ("core", CORE_SOURCE)] {
            assert!(
                !source.contains(forbidden),
                "BQ-21 {name} authority widened: {forbidden}"
            );
        }
    }
    // 五个模型可见工具面不变：恢复路径不注册工具、不新增 harness 入口。
    for forbidden in ["apply_patch", "memory.search", "memory.write", "ToolSchema"] {
        for source in [DOMAIN_SOURCE, CORE_SOURCE] {
            assert!(
                !source.contains(forbidden),
                "BQ-21 recovery path gained a model tool surface: {forbidden}"
            );
        }
    }
    // 夹具不得被禁用。行为夹具与守卫同文件，所以只扫 Domain 夹具与守卫里真正写成属性的行；
    // 守卫自己的禁止字符串列表里含 `#[ignore` 片段，因此按行首属性判定而不是子串判定。
    assert!(!DOMAIN_FIXTURES.contains("#[ignore"));
    assert!(
        !include_str!("billing_recovery_guard.rs")
            .lines()
            .any(|line| line.trim_start().starts_with("#[ignore")),
        "BQ-21 core fixtures must not be ignored"
    );
}

#[test]
fn refusal_codes_and_fixture_names_stay_pinned_in_the_fixtures() {
    for code in [
        "billing_recovery_attempt_already_terminal",
        "billing_recovery_continue_resets_accumulation",
        "billing_recovery_unknown_requires_reconciliation",
        "billing_recovery_unknown_cannot_be_success",
        "billing_recovery_reconcile_requires_evidence",
        "billing_recovery_reconcile_requires_unknown",
        "billing_recovery_reconcile_already_settled",
        "billing_recovery_boot_generation_stale",
        "billing_recovery_authority_epoch_stale",
        "billing_recovery_lease_epoch_stale",
        "billing_recovery_epoch_ahead",
        "billing_recovery_attempt_binding_mismatch",
        "billing_recovery_source_event_mismatch",
        "billing_recovery_source_cursor_regression",
        "billing_recovery_snapshot_invalid",
        "billing_recovery_boot_fence_invalid",
        "billing_recovery_request_invalid",
        "billing_recovery_plan_invalid",
        "billing_recovery_second_settlement",
        "billing_recovery_reconciliation_skipped",
        "billing_recovery_decision_downgrade",
        "billing_recovery_replay_conflict",
        "billing_recovery_reconcile_source_mismatch",
        "billing_recovery_release_claims_consumption",
        "billing_recovery_plan_fact_settlement_mismatch",
        "billing_recovery_fact_invalid",
    ] {
        assert!(
            DOMAIN_FIXTURES.contains(code),
            "BQ-21 domain fixture refusal code missing: {code}"
        );
    }
    for name in [
        "restart_never_redispatches_a_completed_attempt",
        "continue_never_resets_accumulated_usage",
        "stale_boot_lease_and_authority_epochs_cannot_continue_settlement",
        "unknown_attempts_default_to_needs_reconciliation_and_never_auto_succeed",
        "explicit_reconciliation_settles_exactly_once",
        "in_flight_reservation_stays_held_across_restart_and_replay",
        "recovery_ledger_cannot_skip_reconciliation_or_downgrade_a_decision",
        "recovery_requests_and_snapshots_are_digest_and_binding_fenced",
        "reconciliation_evidence_is_bound_and_cannot_claim_a_release_with_consumption",
        "recovered_facts_and_plans_agree_on_one_settlement",
    ] {
        assert!(
            DOMAIN_FIXTURES.contains(name),
            "BQ-21 domain fixture missing: {name}"
        );
    }
    // Core 行为夹具与守卫同文件，因此把夹具名钉在 baseline 卡上，避免只做自我引用。
    for name in [
        "recovery_admission_refuses_stale_fences_terminal_attempts_and_second_settlement",
        "control_plane_facade_exposes_only_the_single_recovery_rule",
        "recovery_sources_own_no_dispatch_append_release_or_second_authority",
    ] {
        assert!(
            BASELINE.contains(name),
            "BQ-21 baseline must pin the core fixture: {name}"
        );
    }
}

#[test]
fn bq21_fixtures_are_wired_to_the_manual_lane_and_the_auto_shards() {
    for command in [
        "cargo fetch --locked",
        "cargo fmt --all --check",
        "cargo test -p kiana-domain --test billing_recovery --locked -- --test-threads=1",
        "cargo test -p kiana-core --test billing_recovery_guard --locked -- --test-threads=1",
        "cargo test -p kiana-domain --test billing_recovery --no-run --locked",
        "cargo test -p kiana-core --test billing_recovery_guard --no-run --locked",
    ] {
        assert!(
            MANUAL_LANE.contains(command),
            "BQ-21 manual lane missing: {command}"
        );
    }
    for forbidden in ["push:", "pull_request:", "continue-on-error", "--ignored"] {
        assert!(
            !MANUAL_LANE.contains(forbidden),
            "BQ-21 manual lane must stay dispatch-only: {forbidden}"
        );
    }
    assert!(MANUAL_LANE.contains("workflow_dispatch:"));
    assert!(MANUAL_LANE.contains("permissions:"));

    // 自动 lane 仍是统一 ci.yml 的既有分片：两个 target 都必须被登记且只登记一次。
    let shards: serde_json::Value = serde_json::from_str(SHARDS).expect("shard registry json");
    let domain_targets = shards["kiana-domain-s1/4"]["targets"]
        .as_array()
        .expect("domain shard targets");
    assert!(domain_targets
        .iter()
        .any(|target| target.as_str() == Some("billing_recovery")));
    let core_targets = shards["kiana-core-s1/6"]["targets"]
        .as_array()
        .expect("core shard targets");
    assert!(core_targets
        .iter()
        .any(|target| target.as_str() == Some("billing_recovery_guard")));
}

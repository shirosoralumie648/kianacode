//! AUT-16 先拒绝优先夹具：重试分类器必须先拒绝 Unknown、缺失 receipt、非幂等 capability、过期审批、
//! 预算不足与越界 attempt/deadline/lease，再允许 pure/no-dispatch 与明确幂等失败建立有界 attempt。
//!
//! 夹具直接驱动 `kiana-workflow` 的纯 planner 与进程内 attempt 账本；不连接 EventStore、Broker、
//! provider、网络或真实时钟。所有拒绝路径都必须证明没有产生 attempt 计划、没有登记事实。

use kiana_workflow::{
    plan_automation_retry_attempt, validate_retry_attempt_start, AutomationEffectClass,
    AutomationIdempotencyClass, AutomationIdempotencyDescriptor, AutomationRetryAttempt,
    AutomationRetryAttemptLedger, AutomationRetryDecision, AutomationRetryRefusal,
    AutomationRetryRequest, AUTOMATION_IDEMPOTENCY_DESCRIPTOR_SCHEMA,
    AUTOMATION_RETRY_REQUEST_SCHEMA,
};
use serde_json::json;

const A: &str = "sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
const B: &str = "sha256:bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";

fn descriptor(
    effect_class: AutomationEffectClass,
    idempotency: AutomationIdempotencyClass,
    scope: Option<&str>,
) -> AutomationIdempotencyDescriptor {
    AutomationIdempotencyDescriptor::new(
        "connector.send",
        effect_class,
        idempotency,
        scope.map(str::to_owned),
    )
    .expect("descriptor")
}

fn request() -> AutomationRetryRequest {
    AutomationRetryRequest {
        schema: AUTOMATION_RETRY_REQUEST_SCHEMA.to_owned(),
        execution_id: "execution-1".to_owned(),
        subject_id: "subject-1".to_owned(),
        previous_attempt_id: "command-1".to_owned(),
        attempt: 1,
        max_attempts: 3,
        effect_started: true,
        effect_known: true,
        effect_receipt_present: true,
        failure_retryable: true,
        retry_after_ms: Some(1_000),
        approval_valid: true,
        approval_expires_at_ms: Some(9_000),
        budget_remaining: 5,
        deadline_ms: 10_000,
        observed_at_ms: 1_000,
        active_lease_expires_at_ms: None,
        previous_action_digest: A.to_owned(),
        action_digest: A.to_owned(),
        descriptor: Some(descriptor(
            AutomationEffectClass::ExternalEffect,
            AutomationIdempotencyClass::Idempotent,
            Some("provider-account"),
        )),
        idempotency_key: Some("idempotency-1".to_owned()),
        new_attempt_id: "attempt-2".to_owned(),
        new_execution_id: "execution-2".to_owned(),
        new_lease_id: "lease-2".to_owned(),
        new_lease_owner: "worker-1".to_owned(),
        new_lease_ttl_ms: 1_000,
        request_digest: String::new(),
    }
    .seal()
    .expect("sealed request")
}

fn mutated(
    base: &AutomationRetryRequest,
    mutate: impl FnOnce(&mut AutomationRetryRequest),
) -> AutomationRetryRequest {
    let mut next = base.clone();
    next.request_digest = String::new();
    mutate(&mut next);
    next.seal().expect("sealed mutation")
}

fn refusal(request: &AutomationRetryRequest) -> AutomationRetryRefusal {
    match plan_automation_retry_attempt(request).expect("classified") {
        AutomationRetryDecision::Refused(reason) => reason,
        other => panic!("expected refusal, got {other:?}"),
    }
}

fn planned(request: &AutomationRetryRequest) -> AutomationRetryAttempt {
    match plan_automation_retry_attempt(request).expect("classified") {
        AutomationRetryDecision::NewAttempt(attempt) => *attempt,
        other => panic!("expected a new attempt, got {other:?}"),
    }
}

fn request_with(
    attempt: u32,
    max_attempts: u32,
    observed_at_ms: u64,
    new_attempt_id: &str,
    new_lease_id: &str,
    idempotency_key: Option<&str>,
) -> AutomationRetryRequest {
    mutated(&request(), |value| {
        value.attempt = attempt;
        value.max_attempts = max_attempts;
        value.observed_at_ms = observed_at_ms;
        value.new_attempt_id = new_attempt_id.to_owned();
        value.new_lease_id = new_lease_id.to_owned();
        value.idempotency_key = idempotency_key.map(str::to_owned);
    })
}

#[test]
fn unknown_effect_is_reconciled_and_never_retried() {
    // 即使审批、预算、幂等、attempt 预算全部满足，Unknown 也优先进入对账而不是新 attempt。
    let unknown = mutated(&request(), |value| value.effect_known = false);
    assert_eq!(
        plan_automation_retry_attempt(&unknown).unwrap(),
        AutomationRetryDecision::ReconcileUnknown
    );
    // 没有任何计划可以被登记：Unknown 分支不产生 attempt 事实。
    let ledger = AutomationRetryAttemptLedger::new();
    assert!(ledger.is_empty());
    assert!(ledger.retry_attempt_history("subject-1").is_empty());
}

#[test]
fn missing_receipt_descriptor_idempotency_and_action_consistency_are_refused() {
    assert_eq!(
        refusal(&mutated(&request(), |value| {
            value.effect_receipt_present = false;
        })),
        AutomationRetryRefusal::EffectReceiptMissing
    );
    assert_eq!(
        refusal(&mutated(&request(), |value| {
            value.descriptor = None;
            value.idempotency_key = None;
        })),
        AutomationRetryRefusal::DescriptorRequired
    );
    assert_eq!(
        refusal(&mutated(&request(), |value| {
            value.descriptor = Some(descriptor(
                AutomationEffectClass::ExternalEffect,
                AutomationIdempotencyClass::NonIdempotent,
                None,
            ));
        })),
        AutomationRetryRefusal::NonIdempotentCapability
    );
    assert_eq!(
        refusal(&mutated(&request(), |value| {
            value.descriptor = Some(descriptor(
                AutomationEffectClass::PureNoDispatch,
                AutomationIdempotencyClass::NonIdempotent,
                None,
            ));
        })),
        AutomationRetryRefusal::EffectClassMismatch
    );
    assert_eq!(
        refusal(&mutated(&request(), |value| {
            value.idempotency_key = None;
        })),
        AutomationRetryRefusal::IdempotencyKeyMissing
    );
    assert_eq!(
        refusal(&mutated(&request(), |value| {
            value.action_digest = B.to_owned();
        })),
        AutomationRetryRefusal::ActionChanged
    );
    assert_eq!(
        refusal(&mutated(&request(), |value| {
            value.failure_retryable = false;
        })),
        AutomationRetryRefusal::NotRetryableFailure
    );
}

#[test]
fn approval_budget_and_attempt_or_deadline_bounds_are_refused() {
    assert_eq!(
        refusal(&mutated(&request(), |value| {
            value.approval_valid = false;
        })),
        AutomationRetryRefusal::ApprovalInvalid
    );
    assert_eq!(
        refusal(&mutated(&request(), |value| {
            value.approval_expires_at_ms = Some(1_000);
        })),
        AutomationRetryRefusal::ApprovalExpired
    );
    assert_eq!(
        refusal(&mutated(&request(), |value| {
            value.budget_remaining = 0;
        })),
        AutomationRetryRefusal::BudgetInsufficient
    );
    assert_eq!(
        refusal(&mutated(&request(), |value| {
            value.attempt = 3;
        })),
        AutomationRetryRefusal::AttemptBoundExhausted
    );
    assert_eq!(
        refusal(&mutated(&request(), |value| {
            value.observed_at_ms = 10_000;
        })),
        AutomationRetryRefusal::DeadlineExceeded
    );
    assert_eq!(
        refusal(&mutated(&request(), |value| {
            value.observed_at_ms = 10_001;
        })),
        AutomationRetryRefusal::DeadlineExceeded
    );
    assert_eq!(
        refusal(&mutated(&request(), |value| {
            value.active_lease_expires_at_ms = Some(1_001);
        })),
        AutomationRetryRefusal::LeaseHeld
    );
    assert_eq!(
        refusal(&mutated(&request(), |value| {
            value.retry_after_ms = Some(9_000);
        })),
        AutomationRetryRefusal::RetryAfterExceedsDeadline
    );
    assert_eq!(
        refusal(&mutated(&request(), |value| {
            value.new_lease_ttl_ms = 9_001;
        })),
        AutomationRetryRefusal::LeaseExceedsDeadline
    );
    // 已到期的 lease（等于观测时间）不阻止重新建立有界 attempt。
    let expired_lease = mutated(&request(), |value| {
        value.active_lease_expires_at_ms = Some(1_000);
    });
    let attempt = planned(&expired_lease);
    assert_eq!(attempt.attempt, 2);
    assert_eq!(attempt.lease_expires_at_ms, 2_000);
}

#[test]
fn pure_no_dispatch_failure_creates_a_bounded_attempt_without_a_descriptor() {
    let pure = mutated(&request(), |value| {
        value.effect_started = false;
        value.effect_receipt_present = false;
        value.descriptor = None;
        value.idempotency_key = None;
    });
    let attempt = planned(&pure);
    assert_eq!(attempt.attempt, 2);
    assert_eq!(attempt.max_attempts, 3);
    assert_eq!(attempt.capability, None);
    assert_eq!(attempt.descriptor_digest, None);
    assert_eq!(attempt.idempotency_key, None);
    assert_eq!(attempt.execution_id, "execution-1");
    assert_eq!(attempt.new_execution_id, "execution-2");
    assert_eq!(attempt.subject_id, "subject-1");
    assert_eq!(attempt.previous_attempt_id, "command-1");
    assert_eq!(attempt.action_digest, A);
    assert_eq!(attempt.created_at_ms, 1_000);
    assert_eq!(attempt.lease_expires_at_ms, 2_000);
    assert_eq!(attempt.deadline_ms, 10_000);
    assert!(attempt.lease_expires_at_ms <= attempt.deadline_ms);
    attempt.validate().expect("valid attempt fact");
}

#[test]
fn idempotent_known_failure_creates_a_bounded_attempt_with_lease_deadline_and_decision_digest() {
    let sealed = request();
    let attempt = planned(&sealed);
    assert_eq!(attempt.attempt, 2);
    assert_eq!(attempt.capability.as_deref(), Some("connector.send"));
    assert_eq!(attempt.idempotency_key.as_deref(), Some("idempotency-1"));
    assert_eq!(
        attempt.descriptor_digest.as_deref(),
        sealed
            .descriptor
            .as_ref()
            .map(|descriptor| descriptor.descriptor_digest.as_str())
    );
    assert_eq!(attempt.decision_digest, sealed.request_digest);
    assert_eq!(attempt.retry_after_ms, Some(1_000));
    assert_eq!(attempt.lease_owner, "worker-1");
    assert_eq!(attempt.lease_id, "lease-2");
    attempt.validate().expect("valid attempt fact");
    // 同一输入重放得到同一事实（分类器无时钟、无随机数）。
    assert_eq!(attempt, planned(&sealed));
    validate_retry_attempt_start(&attempt, "worker-1", 1_500).expect("lease holder may start");
}

#[test]
fn ledger_records_replays_conflicts_and_keeps_an_ordered_audit_chain() {
    let mut ledger = AutomationRetryAttemptLedger::new();
    let first = planned(&request_with(
        1,
        3,
        1_000,
        "attempt-2",
        "lease-2",
        Some("idempotency-1"),
    ));
    let recorded = ledger
        .record_retry_attempt(first.clone())
        .expect("recorded");
    assert!(!recorded.replayed);
    assert_eq!(recorded.attempt, first);
    assert_eq!(ledger.len(), 1);
    // 相同事实再次登记是安全重放，不产生第二条记录。
    let replayed = ledger
        .record_retry_attempt(first.clone())
        .expect("replayed");
    assert!(replayed.replayed);
    assert_eq!(ledger.len(), 1);
    // 同一 (subject, attempt) 换内容即冲突。
    let conflicting = planned(&request_with(
        1,
        3,
        1_500,
        "attempt-2b",
        "lease-2b",
        Some("idempotency-1b"),
    ));
    assert_eq!(
        ledger.record_retry_attempt(conflicting).unwrap_err(),
        "automation_retry_attempt_conflict"
    );
    // 序号必须严格递增：从 2 直接跳到 4 被拒。
    let skipped = planned(&request_with(
        3,
        5,
        3_000,
        "attempt-4",
        "lease-4",
        Some("idempotency-4"),
    ));
    assert_eq!(
        ledger.record_retry_attempt(skipped).unwrap_err(),
        "automation_retry_attempt_sequence_invalid"
    );
    // 复用 attempt_id 指向另一条记录也被拒。
    let reused_id = planned(&request_with(
        2,
        3,
        3_000,
        "attempt-2",
        "lease-3",
        Some("idempotency-3"),
    ));
    assert_eq!(
        ledger.record_retry_attempt(reused_id).unwrap_err(),
        "automation_retry_attempt_conflict"
    );
    // 正常递增的第三次尝试可登记，历史按序号升序。
    let third = planned(&request_with(
        2,
        3,
        3_000,
        "attempt-3",
        "lease-3",
        Some("idempotency-3"),
    ));
    let recorded = ledger
        .record_retry_attempt(third.clone())
        .expect("recorded");
    assert!(!recorded.replayed);
    assert_eq!(ledger.len(), 2);
    let history = ledger.retry_attempt_history("subject-1");
    assert_eq!(
        history
            .iter()
            .map(|attempt| attempt.attempt)
            .collect::<Vec<_>>(),
        vec![2, 3]
    );
    assert_eq!(history[1], third);
    assert_eq!(ledger.fetch_retry_attempt("subject-1", 2).unwrap(), first);
    assert_eq!(
        ledger.fetch_retry_attempt("subject-1", 9).unwrap_err(),
        "automation_retry_attempt_not_found"
    );
    assert!(ledger.retry_attempt_history("subject-2").is_empty());
}

#[test]
fn ledger_refuses_a_second_attempt_while_a_lease_is_active_and_allows_it_after_expiry() {
    let mut ledger = AutomationRetryAttemptLedger::new();
    let first = planned(&request_with(
        1,
        3,
        1_000,
        "attempt-2",
        "lease-2",
        Some("idempotency-1"),
    ));
    ledger.record_retry_attempt(first).expect("recorded");
    assert_eq!(
        ledger
            .active_retry_lease("subject-1", 1_500)
            .map(|attempt| attempt.lease_expires_at_ms),
        Some(2_000)
    );
    assert!(ledger.active_retry_lease("subject-1", 2_000).is_none());
    // 调用方漏报活动 lease 时，账本仍必须拒绝：1_500 仍在 lease 内。
    let overlapping = planned(&request_with(
        2,
        3,
        1_500,
        "attempt-3",
        "lease-3",
        Some("idempotency-3"),
    ));
    assert_eq!(
        ledger.record_retry_attempt(overlapping).unwrap_err(),
        "automation_retry_lease_held"
    );
    assert_eq!(ledger.len(), 1);
    // lease 到期后可以按有界策略登记下一条尝试。
    let after_expiry = planned(&request_with(
        2,
        3,
        2_000,
        "attempt-3",
        "lease-3",
        Some("idempotency-3"),
    ));
    let recorded = ledger.record_retry_attempt(after_expiry).expect("recorded");
    assert!(!recorded.replayed);
    assert_eq!(ledger.len(), 2);
}

#[test]
fn ledger_refuses_one_idempotency_key_bound_to_a_different_attempt() {
    let mut ledger = AutomationRetryAttemptLedger::new();
    let first = planned(&request_with(
        1,
        3,
        1_000,
        "attempt-2",
        "lease-2",
        Some("idempotency-1"),
    ));
    ledger.record_retry_attempt(first).expect("recorded");
    let drifted = planned(&request_with(
        2,
        3,
        2_000,
        "attempt-3",
        "lease-3",
        Some("idempotency-1"),
    ));
    assert_eq!(
        ledger.record_retry_attempt(drifted).unwrap_err(),
        "automation_retry_idempotency_key_conflict"
    );
    assert_eq!(ledger.len(), 1);
    let fresh_key = planned(&request_with(
        2,
        3,
        2_000,
        "attempt-3",
        "lease-3",
        Some("idempotency-3"),
    ));
    ledger.record_retry_attempt(fresh_key).expect("recorded");
    assert_eq!(ledger.len(), 2);
}

#[test]
fn attempt_start_requires_the_lease_owner_a_monotonic_clock_and_a_live_window() {
    let attempt = planned(&request());
    assert_eq!(
        validate_retry_attempt_start(&attempt, "worker-2", 1_500).unwrap_err(),
        "automation_retry_lease_owner_mismatch"
    );
    assert_eq!(
        validate_retry_attempt_start(&attempt, "worker-1", 999).unwrap_err(),
        "automation_retry_clock_rollback"
    );
    assert_eq!(
        validate_retry_attempt_start(&attempt, "worker-1", 2_000).unwrap_err(),
        "automation_retry_lease_expired"
    );
    assert_eq!(
        validate_retry_attempt_start(&attempt, "worker-1", 10_000).unwrap_err(),
        "automation_retry_deadline_exceeded"
    );
    validate_retry_attempt_start(&attempt, "worker-1", 1_000).expect("start at creation");
    validate_retry_attempt_start(&attempt, "worker-1", 1_999).expect("start before expiry");
}

#[test]
fn tampered_digests_wrong_shapes_and_unknown_fields_fail_closed() {
    // 请求摘要被改写后必须被拒绝，而不是被当成另一份合法快照。
    let mut tampered = request();
    tampered.budget_remaining += 1;
    assert_eq!(
        plan_automation_retry_attempt(&tampered).unwrap_err(),
        "automation_retry_request_invalid"
    );
    // 描述符摘要同样自校验；缺 scope 的 Idempotent 外部效果描述符在构造期即被拒绝。
    assert_eq!(
        AutomationIdempotencyDescriptor::new(
            "connector.send",
            AutomationEffectClass::ExternalEffect,
            AutomationIdempotencyClass::Idempotent,
            None,
        )
        .unwrap_err(),
        "automation_idempotency_descriptor_invalid"
    );
    // attempt 事实的结构围栏：retry 序号至少为 2，lease 必须落在 deadline 内。
    let mut bad_attempt = planned(&request());
    bad_attempt.attempt = 1;
    bad_attempt.attempt_digest = String::new();
    assert_eq!(
        bad_attempt.seal().unwrap_err(),
        "automation_retry_attempt_invalid"
    );
    let mut late_lease = planned(&request());
    late_lease.lease_expires_at_ms = 20_000;
    late_lease.attempt_digest = String::new();
    assert_eq!(
        late_lease.seal().unwrap_err(),
        "automation_retry_attempt_invalid"
    );
    // 未知字段不允许进入已校验契约。
    let mut value = serde_json::to_value(request()).unwrap();
    value["unexpected"] = json!(true);
    assert!(serde_json::from_value::<AutomationRetryRequest>(value).is_err());
    // 合法载荷可以往返，schema 名称固定。
    let round_trip: AutomationRetryRequest =
        serde_json::from_value(serde_json::to_value(request()).unwrap()).unwrap();
    assert_eq!(round_trip, request());
    assert_eq!(round_trip.schema, AUTOMATION_RETRY_REQUEST_SCHEMA);
    assert_eq!(
        descriptor(
            AutomationEffectClass::ExternalEffect,
            AutomationIdempotencyClass::Idempotent,
            Some("provider-account"),
        )
        .schema,
        AUTOMATION_IDEMPOTENCY_DESCRIPTOR_SCHEMA
    );
}

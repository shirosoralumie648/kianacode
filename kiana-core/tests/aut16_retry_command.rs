//! AUT-16 先拒绝优先夹具：Core retry 协调器只从权威 dispatch command 派生失败事实，先拒绝
//! Unknown、未失败/已成功、缺失 receipt、非幂等 capability、过期审批与预算不足，再登记一条可审计
//! 的 retry attempt 事实。
//!
//! 夹具用真实 `MemoryAutomationDispatchStore` 播种权威命令，再用计数包装证明：任何路径都不会
//! claim/observe/commit 命令事实，也不会调用 runner/Broker。Unknown 与拒绝路径保持 0 条 attempt。

use async_trait::async_trait;
use kiana_core::{
    record_automation_retry_attempt, AutomationRetryCommandOutcome, AutomationRetryCommandRequest,
    AUTOMATION_RETRY_COMMAND_NOT_FAILED, AUTOMATION_RETRY_COMMAND_REQUEST_SCHEMA,
    AUTOMATION_RETRY_COMMAND_SUCCEEDED,
};
use kiana_domain::{
    automation_dispatch_result, claim_automation_dispatch_command,
    prepare_automation_dispatch_command, AutomationDispatchClaim, AutomationDispatchClaimCommit,
    AutomationDispatchClaimRequest, AutomationDispatchCommand, AutomationDispatchCommit,
    AutomationDispatchIntent, AutomationDispatchObservationCommit,
    AutomationDispatchObservationRequest, AutomationDispatchState, AutomationWorkerEffectOutcome,
    AUTOMATION_DISPATCH_SCHEMA,
};
use kiana_eventlog::MemoryAutomationDispatchStore;
use kiana_ports::{AutomationDispatchStore, PortError};
use kiana_workflow::{
    validate_retry_attempt_start, AutomationEffectClass, AutomationIdempotencyClass,
    AutomationIdempotencyDescriptor, AutomationRetryAttemptLedger, AUTOMATION_RETRY_ATTEMPT_SCHEMA,
};
use serde_json::json;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

const A: &str = "sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
const B: &str = "sha256:bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";

/// 按 Domain 私有 digest 的同一字段集合重算 intent 摘要；测试与 Domain 夹具使用同一序列化。
fn recompute_intent(intent: &mut AutomationDispatchIntent) {
    intent.intent_digest = kiana_domain::json_digest(&json!({
        "schema": intent.schema,
        "execution_id": intent.execution_id,
        "reservation_id": intent.reservation_id,
        "action_digest": intent.action_digest,
        "worker_id": intent.worker_id,
        "attempt": intent.attempt,
        "authority_epoch": intent.authority_epoch,
        "state": intent.state,
        "observation_digest": intent.observation_digest,
    }));
}

fn prepared_command(
    command_id: &str,
    idempotency_key: &str,
    execution_id: &str,
) -> AutomationDispatchCommand {
    let mut intent = AutomationDispatchIntent {
        schema: AUTOMATION_DISPATCH_SCHEMA.to_owned(),
        execution_id: execution_id.to_owned(),
        reservation_id: "reservation-1".to_owned(),
        action_digest: A.to_owned(),
        worker_id: None,
        attempt: 1,
        authority_epoch: 4,
        state: AutomationDispatchState::Prepared,
        observation_digest: None,
        intent_digest: String::new(),
    };
    recompute_intent(&mut intent);
    prepare_automation_dispatch_command(command_id, idempotency_key, &intent).expect("prepared")
}

/// 在真实 store 中播种一条命令：commit → 唯一 claim → observe 指定 outcome。
async fn seed_dispatch(
    store: &MemoryAutomationDispatchStore,
    command_id: &str,
    execution_id: &str,
    idempotency_key: &str,
    outcome: AutomationWorkerEffectOutcome,
) -> AutomationDispatchCommand {
    let prepared = prepared_command(command_id, idempotency_key, execution_id);
    let commit = store.commit_command(prepared, 0).await.expect("committed");
    assert_eq!(commit.command.revision, 1);
    let claim =
        claim_automation_dispatch_command(&commit.command, "worker-1", 1, 1).expect("claimable");
    let AutomationDispatchClaim::Claimed(_) = claim else {
        panic!("expected a claimable command");
    };
    let claimed = store
        .claim_command(AutomationDispatchClaimRequest {
            command_id: command_id.to_owned(),
            worker_id: "worker-1".to_owned(),
            attempt: 1,
            expected_revision: 1,
        })
        .await
        .expect("claimed");
    assert!(claimed.claimed);
    let result = automation_dispatch_result(&claimed.command, outcome).expect("structured result");
    let observed = store
        .observe_command(AutomationDispatchObservationRequest {
            command_id: command_id.to_owned(),
            expected_revision: claimed.command.revision,
            result,
        })
        .await
        .expect("observed");
    observed.command
}

async fn seed_claimed(
    store: &MemoryAutomationDispatchStore,
    command_id: &str,
    execution_id: &str,
    idempotency_key: &str,
) -> AutomationDispatchCommand {
    let prepared = prepared_command(command_id, idempotency_key, execution_id);
    store.commit_command(prepared, 0).await.expect("committed");
    store
        .claim_command(AutomationDispatchClaimRequest {
            command_id: command_id.to_owned(),
            worker_id: "worker-1".to_owned(),
            attempt: 1,
            expected_revision: 1,
        })
        .await
        .expect("claimed")
        .command
}

fn failed_outcome(retryable: bool) -> AutomationWorkerEffectOutcome {
    AutomationWorkerEffectOutcome::Failed {
        failure_code: "connector_timeout".to_owned(),
        detail_digest: B.to_owned(),
        retryable,
        result_digest: B.to_owned(),
    }
}

fn succeeded_outcome() -> AutomationWorkerEffectOutcome {
    AutomationWorkerEffectOutcome::Succeeded {
        result_digest: A.to_owned(),
    }
}

fn unknown_outcome() -> AutomationWorkerEffectOutcome {
    AutomationWorkerEffectOutcome::Unknown {
        result_digest: A.to_owned(),
    }
}

/// 计数 store：委托给真实内存适配器，并记录读/写次数以证明协调器不写事实。
struct CountingStore {
    inner: Arc<MemoryAutomationDispatchStore>,
    fetches: Arc<AtomicUsize>,
    commits: Arc<AtomicUsize>,
    claims: Arc<AtomicUsize>,
    observes: Arc<AtomicUsize>,
}

impl CountingStore {
    fn wrap(inner: Arc<MemoryAutomationDispatchStore>) -> Self {
        Self {
            inner,
            fetches: Arc::new(AtomicUsize::new(0)),
            commits: Arc::new(AtomicUsize::new(0)),
            claims: Arc::new(AtomicUsize::new(0)),
            observes: Arc::new(AtomicUsize::new(0)),
        }
    }

    fn reads(&self) -> usize {
        self.fetches.load(Ordering::SeqCst)
    }

    fn mutations(&self) -> usize {
        self.commits.load(Ordering::SeqCst)
            + self.claims.load(Ordering::SeqCst)
            + self.observes.load(Ordering::SeqCst)
    }
}

#[async_trait]
impl AutomationDispatchStore for CountingStore {
    async fn commit_command(
        &self,
        command: AutomationDispatchCommand,
        expected_revision: u64,
    ) -> Result<AutomationDispatchCommit, PortError> {
        self.commits.fetch_add(1, Ordering::SeqCst);
        self.inner.commit_command(command, expected_revision).await
    }

    async fn claim_command(
        &self,
        request: AutomationDispatchClaimRequest,
    ) -> Result<AutomationDispatchClaimCommit, PortError> {
        self.claims.fetch_add(1, Ordering::SeqCst);
        self.inner.claim_command(request).await
    }

    async fn observe_command(
        &self,
        request: AutomationDispatchObservationRequest,
    ) -> Result<AutomationDispatchObservationCommit, PortError> {
        self.observes.fetch_add(1, Ordering::SeqCst);
        self.inner.observe_command(request).await
    }

    async fn fetch_command(
        &self,
        command_id: &str,
    ) -> Result<AutomationDispatchCommand, PortError> {
        self.fetches.fetch_add(1, Ordering::SeqCst);
        self.inner.fetch_command(command_id).await
    }

    async fn fetch_command_by_idempotency_key(
        &self,
        idempotency_key: &str,
    ) -> Result<AutomationDispatchCommand, PortError> {
        self.fetches.fetch_add(1, Ordering::SeqCst);
        self.inner
            .fetch_command_by_idempotency_key(idempotency_key)
            .await
    }
}

fn descriptor(
    effect_class: AutomationEffectClass,
    idempotency: AutomationIdempotencyClass,
) -> AutomationIdempotencyDescriptor {
    let scope = (effect_class == AutomationEffectClass::ExternalEffect
        && idempotency == AutomationIdempotencyClass::Idempotent)
        .then(|| "provider-account".to_owned());
    AutomationIdempotencyDescriptor::new("connector.send", effect_class, idempotency, scope)
        .expect("descriptor")
}

fn request() -> AutomationRetryCommandRequest {
    AutomationRetryCommandRequest {
        schema: AUTOMATION_RETRY_COMMAND_REQUEST_SCHEMA.to_owned(),
        retry_subject_id: "subject-1".to_owned(),
        new_attempt_id: "attempt-2".to_owned(),
        new_execution_id: "execution-2".to_owned(),
        new_lease_id: "lease-2".to_owned(),
        new_lease_owner: "worker-1".to_owned(),
        new_lease_ttl_ms: 1_000,
        max_attempts: 3,
        deadline_ms: 10_000,
        observed_at_ms: 1_000,
        approval_valid: true,
        approval_expires_at_ms: Some(9_000),
        budget_remaining: 5,
        retry_after_ms: Some(1_000),
        effect_receipt_present: true,
        capability: descriptor(
            AutomationEffectClass::ExternalEffect,
            AutomationIdempotencyClass::Idempotent,
        ),
        idempotency_key: "idempotency-1".to_owned(),
        request_digest: String::new(),
    }
    .seal()
    .expect("sealed request")
}

fn mutated(
    base: &AutomationRetryCommandRequest,
    mutate: impl FnOnce(&mut AutomationRetryCommandRequest),
) -> AutomationRetryCommandRequest {
    let mut next = base.clone();
    next.request_digest = String::new();
    mutate(&mut next);
    next.seal().expect("sealed mutation")
}

fn refusal_reason(outcome: AutomationRetryCommandOutcome) -> &'static str {
    match outcome {
        AutomationRetryCommandOutcome::Refused { reason, .. } => reason,
        other => panic!("expected refusal, got {other:?}"),
    }
}

fn recorded_attempt(
    outcome: AutomationRetryCommandOutcome,
) -> kiana_workflow::AutomationRetryAttempt {
    match outcome {
        AutomationRetryCommandOutcome::AttemptRecorded { attempt, .. } => attempt,
        other => panic!("expected recorded attempt, got {other:?}"),
    }
}

#[tokio::test]
async fn prepared_or_succeeded_commands_are_refused_without_a_record_or_effect() {
    let inner = Arc::new(MemoryAutomationDispatchStore::new());
    seed_claimed(&inner, "command-1", "execution-1", "idempotency-1").await;
    seed_dispatch(
        &inner,
        "command-2",
        "execution-2",
        "idempotency-2",
        succeeded_outcome(),
    )
    .await;
    // 第三条命令保持 Prepared（从未 claim/dispatch），在包装前播种。
    inner
        .commit_command(
            prepared_command("command-3", "idempotency-3", "execution-3"),
            0,
        )
        .await
        .expect("committed");
    let store = CountingStore::wrap(inner);
    let mut ledger = AutomationRetryAttemptLedger::new();

    // Prepared（从未 claim/dispatch）不是失败；Observed+Succeeded 也不能被重试改写。
    let prepared_outcome =
        record_automation_retry_attempt(&store, &mut ledger, "command-3", &request())
            .await
            .expect("evaluated");
    assert_eq!(
        refusal_reason(prepared_outcome),
        AUTOMATION_RETRY_COMMAND_NOT_FAILED
    );
    let succeeded_outcome =
        record_automation_retry_attempt(&store, &mut ledger, "command-2", &request())
            .await
            .expect("evaluated");
    assert_eq!(
        refusal_reason(succeeded_outcome),
        AUTOMATION_RETRY_COMMAND_SUCCEEDED
    );
    assert!(ledger.is_empty());
    assert_eq!(store.mutations(), 0);
}

#[tokio::test]
async fn claimed_and_unknown_commands_require_reconciliation_not_retry() {
    let inner = Arc::new(MemoryAutomationDispatchStore::new());
    seed_claimed(&inner, "command-1", "execution-1", "idempotency-1").await;
    seed_dispatch(
        &inner,
        "command-2",
        "execution-2",
        "idempotency-2",
        unknown_outcome(),
    )
    .await;
    let store = CountingStore::wrap(inner);
    let mut ledger = AutomationRetryAttemptLedger::new();

    for command_id in ["command-1", "command-2"] {
        let outcome = record_automation_retry_attempt(&store, &mut ledger, command_id, &request())
            .await
            .expect("evaluated");
        assert!(matches!(
            outcome,
            AutomationRetryCommandOutcome::ReconcileUnknown { .. }
        ));
    }
    assert!(ledger.is_empty());
    assert_eq!(store.mutations(), 0);
}

#[tokio::test]
async fn known_retryable_failure_records_one_auditable_attempt_and_replays() {
    let inner = Arc::new(MemoryAutomationDispatchStore::new());
    let source = seed_dispatch(
        &inner,
        "command-1",
        "execution-1",
        "idempotency-1",
        failed_outcome(true),
    )
    .await;
    let store = CountingStore::wrap(inner);
    let mut ledger = AutomationRetryAttemptLedger::new();

    let attempt = recorded_attempt(
        record_automation_retry_attempt(&store, &mut ledger, "command-1", &request())
            .await
            .expect("recorded"),
    );
    assert_eq!(attempt.schema, AUTOMATION_RETRY_ATTEMPT_SCHEMA);
    assert_eq!(attempt.attempt, 2);
    assert_eq!(attempt.max_attempts, 3);
    assert_eq!(attempt.execution_id, "execution-1");
    assert_eq!(attempt.new_execution_id, "execution-2");
    assert_eq!(attempt.subject_id, "subject-1");
    assert_eq!(attempt.previous_attempt_id, "command-1");
    assert_eq!(attempt.action_digest, source.intent.action_digest);
    assert_eq!(attempt.capability.as_deref(), Some("connector.send"));
    assert_eq!(attempt.idempotency_key.as_deref(), Some("idempotency-1"));
    assert_eq!(attempt.lease_owner, "worker-1");
    assert_eq!(attempt.lease_expires_at_ms, 2_000);
    assert_eq!(attempt.deadline_ms, 10_000);
    attempt.validate().expect("valid attempt fact");
    validate_retry_attempt_start(&attempt, "worker-1", 1_500).expect("lease holder may start");
    assert_eq!(ledger.len(), 1);
    assert_eq!(
        ledger.retry_attempt_history("subject-1"),
        vec![attempt.clone()]
    );

    // 同一请求重放命中既有事实：没有第二条记录，也没有任何 store 写入。
    let replayed = record_automation_retry_attempt(&store, &mut ledger, "command-1", &request())
        .await
        .expect("evaluated");
    match replayed {
        AutomationRetryCommandOutcome::AlreadyRecorded {
            attempt: existing, ..
        } => assert_eq!(existing, attempt),
        other => panic!("expected replay, got {other:?}"),
    }
    assert_eq!(ledger.len(), 1);
    assert_eq!(store.mutations(), 0);
    assert_eq!(store.reads(), 2);
}

#[tokio::test]
async fn receipt_idempotency_approval_and_budget_fences_refuse_with_zero_records() {
    let inner = Arc::new(MemoryAutomationDispatchStore::new());
    seed_dispatch(
        &inner,
        "command-1",
        "execution-1",
        "idempotency-1",
        failed_outcome(true),
    )
    .await;
    seed_dispatch(
        &inner,
        "command-2",
        "execution-2",
        "idempotency-2",
        failed_outcome(false),
    )
    .await;
    let store = CountingStore::wrap(inner);
    let mut ledger = AutomationRetryAttemptLedger::new();

    // 非 retryable 的已知失败：重试不会改变结果。
    assert_eq!(
        refusal_reason(
            record_automation_retry_attempt(&store, &mut ledger, "command-2", &request())
                .await
                .expect("evaluated")
        ),
        "automation_retry_not_retryable_failure"
    );
    // 外部 effect receipt 缺失：没有证据可绑定。
    assert_eq!(
        refusal_reason(
            record_automation_retry_attempt(
                &store,
                &mut ledger,
                "command-1",
                &mutated(&request(), |value| value.effect_receipt_present = false),
            )
            .await
            .expect("evaluated")
        ),
        "automation_retry_effect_receipt_missing"
    );
    // 非幂等 capability：禁止自动重试。
    assert_eq!(
        refusal_reason(
            record_automation_retry_attempt(
                &store,
                &mut ledger,
                "command-1",
                &mutated(&request(), |value| {
                    value.capability = descriptor(
                        AutomationEffectClass::ExternalEffect,
                        AutomationIdempotencyClass::NonIdempotent,
                    );
                }),
            )
            .await
            .expect("evaluated")
        ),
        "automation_retry_non_idempotent_capability"
    );
    // 审批过期与预算不足。
    assert_eq!(
        refusal_reason(
            record_automation_retry_attempt(
                &store,
                &mut ledger,
                "command-1",
                &mutated(&request(), |value| value.approval_expires_at_ms =
                    Some(1_000)),
            )
            .await
            .expect("evaluated")
        ),
        "automation_retry_approval_expired"
    );
    assert_eq!(
        refusal_reason(
            record_automation_retry_attempt(
                &store,
                &mut ledger,
                "command-1",
                &mutated(&request(), |value| value.budget_remaining = 0),
            )
            .await
            .expect("evaluated")
        ),
        "automation_retry_budget_insufficient"
    );
    // attempt 上界：命令已用掉的 attempt 与请求上界共同决定。
    assert_eq!(
        refusal_reason(
            record_automation_retry_attempt(
                &store,
                &mut ledger,
                "command-1",
                &mutated(&request(), |value| value.max_attempts = 1),
            )
            .await
            .expect("evaluated")
        ),
        "automation_retry_attempt_bound_exhausted"
    );
    assert!(ledger.is_empty());
    assert_eq!(store.mutations(), 0);
}

#[tokio::test]
async fn ledger_lease_fence_blocks_a_second_attempt_for_the_same_subject() {
    let inner = Arc::new(MemoryAutomationDispatchStore::new());
    seed_dispatch(
        &inner,
        "command-1",
        "execution-1",
        "idempotency-1",
        failed_outcome(true),
    )
    .await;
    seed_dispatch(
        &inner,
        "command-2",
        "execution-9",
        "idempotency-9",
        failed_outcome(true),
    )
    .await;
    let store = CountingStore::wrap(inner);
    let mut ledger = AutomationRetryAttemptLedger::new();

    let attempt = recorded_attempt(
        record_automation_retry_attempt(&store, &mut ledger, "command-1", &request())
            .await
            .expect("recorded"),
    );
    assert_eq!(ledger.len(), 1);
    // 第二条命令属于同一 subject，且观测时间落在第一条 attempt 的 lease 内：活动 lease 由账本
    // 派生（调用方没有可伪造的 lease 字段），分类器必须拒绝。
    let overlapping = mutated(&request(), |value| {
        value.observed_at_ms = 1_500;
        value.new_attempt_id = "attempt-3".to_owned();
        value.new_lease_id = "lease-3".to_owned();
        value.idempotency_key = "idempotency-3".to_owned();
    });
    assert_eq!(
        refusal_reason(
            record_automation_retry_attempt(&store, &mut ledger, "command-2", &overlapping)
                .await
                .expect("evaluated")
        ),
        "automation_retry_lease_held"
    );
    assert_eq!(ledger.len(), 1);
    assert_eq!(ledger.retry_attempt_history("subject-1"), vec![attempt]);
    // lease 到期后再登记第二次尝试：序号递增，历史按序保留。
    let after_expiry = mutated(&request(), |value| {
        value.observed_at_ms = 2_000;
        value.new_attempt_id = "attempt-3".to_owned();
        value.new_lease_id = "lease-3".to_owned();
        value.idempotency_key = "idempotency-3".to_owned();
    });
    let second = recorded_attempt(
        record_automation_retry_attempt(&store, &mut ledger, "command-2", &after_expiry)
            .await
            .expect("recorded"),
    );
    assert_eq!(second.attempt, 3);
    assert_eq!(
        ledger
            .retry_attempt_history("subject-1")
            .iter()
            .map(|attempt| attempt.attempt)
            .collect::<Vec<_>>(),
        vec![2, 3]
    );
    assert_eq!(store.mutations(), 0);
}

#[tokio::test]
async fn invalid_or_tampered_requests_fail_before_any_store_read() {
    let inner = Arc::new(MemoryAutomationDispatchStore::new());
    seed_dispatch(
        &inner,
        "command-1",
        "execution-1",
        "idempotency-1",
        failed_outcome(true),
    )
    .await;
    let store = CountingStore::wrap(inner);
    let mut ledger = AutomationRetryAttemptLedger::new();

    let mut tampered = request();
    tampered.budget_remaining += 1;
    let error = record_automation_retry_attempt(&store, &mut ledger, "command-1", &tampered)
        .await
        .expect_err("tampered request must fail");
    assert_eq!(
        error,
        PortError::Failed("automation_retry_command_request_invalid".to_owned())
    );
    // 结构错误在任何 store 读取之前就被拒绝。
    assert_eq!(store.reads(), 0);
    assert_eq!(store.mutations(), 0);
    assert!(ledger.is_empty());
}

#[tokio::test]
async fn unknown_fields_and_schema_drift_fail_closed() {
    let sealed = request();
    let round_trip: AutomationRetryCommandRequest =
        serde_json::from_value(serde_json::to_value(&sealed).unwrap()).unwrap();
    assert_eq!(round_trip, sealed);
    assert_eq!(round_trip.schema, AUTOMATION_RETRY_COMMAND_REQUEST_SCHEMA);
    let mut value = serde_json::to_value(&sealed).unwrap();
    value["unexpected"] = json!(true);
    assert!(serde_json::from_value::<AutomationRetryCommandRequest>(value).is_err());
    let mut drifted = sealed;
    drifted.schema = "kiana.automation-retry-command-request.v2".to_owned();
    assert_eq!(
        drifted.validate().unwrap_err(),
        "automation_retry_command_request_invalid"
    );
}

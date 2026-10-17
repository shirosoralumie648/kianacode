use async_trait::async_trait;
use kiana_domain::{
    ModelError, ModelOutput, ModelReply, ModelRequest, ModelRetryClass, RequestId, RunId,
};
use kiana_runner::{HarnessBudgetConfig, KianaHarness, ModelClient, RuntimeConfig};
use kiana_runner_protocol::{RunnerCommand, RunnerEvent};
use std::sync::{
    atomic::{AtomicUsize, Ordering},
    Arc,
};
use tokio::sync::Notify;

struct SequenceModel {
    outcomes: Vec<Result<ModelOutput, ModelError>>,
    calls: Arc<AtomicUsize>,
    attempt_ids: Arc<std::sync::Mutex<Vec<RequestId>>>,
    entered: Arc<Notify>,
    rejected: bool,
    retry_after_ms: Option<u64>,
    emit_delta_before_error: bool,
}

#[async_trait]
impl ModelClient for SequenceModel {
    async fn complete(&self, _request: ModelRequest) -> Result<ModelOutput, String> {
        Err("p4_j7_23_prepared_path_required".to_owned())
    }

    async fn complete_prepared(
        &self,
        prepared: kiana_domain::PreparedModelCall,
        on_delta: &mut (dyn FnMut(kiana_domain::ModelDelta) -> Result<(), String> + Send),
    ) -> Result<ModelReply, ModelError> {
        let index = self.calls.fetch_add(1, Ordering::SeqCst);
        self.attempt_ids
            .lock()
            .unwrap()
            .push(prepared.spec.attempt_id);
        if index == 0 && self.emit_delta_before_error {
            on_delta(kiana_domain::ModelDelta::Text {
                text: "partial response".to_owned(),
            })
            .map_err(ModelError::invalid)?;
        }
        self.entered.notify_one();
        match self.outcomes.get(index).cloned().unwrap_or_else(|| {
            Err(ModelError::transport(
                "p4_j7_23_unexpected_extra_attempt",
                ModelRetryClass::Never,
                true,
            ))
        }) {
            Ok(output) => ModelReply::legacy(output),
            Err(mut error) => {
                if self.rejected && error.retry_class == ModelRetryClass::Rejected {
                    error.side_effect_state = kiana_domain::ModelSideEffectState::None;
                }
                error.retry_after_ms = self.retry_after_ms;
                Err(error)
            }
        }
    }
}

fn transient(code: &str, class: ModelRetryClass, sent: bool) -> Result<ModelOutput, ModelError> {
    Err(ModelError::transport(code, class, sent))
}

fn harness(
    model: SequenceModel,
    budget: std::time::Duration,
) -> (
    Arc<KianaHarness>,
    Arc<Notify>,
    Arc<AtomicUsize>,
    Arc<std::sync::Mutex<Vec<RequestId>>>,
) {
    harness_with_config(
        model,
        RuntimeConfig {
            wall_time_budget: Some(budget),
            ..RuntimeConfig::default()
        },
    )
}

fn harness_with_config(
    model: SequenceModel,
    config: RuntimeConfig,
) -> (
    Arc<KianaHarness>,
    Arc<Notify>,
    Arc<AtomicUsize>,
    Arc<std::sync::Mutex<Vec<RequestId>>>,
) {
    let entered = model.entered.clone();
    let calls = model.calls.clone();
    let attempt_ids = model.attempt_ids.clone();
    let harness = Arc::new(KianaHarness::with_config(Arc::new(model), config));
    (harness, entered, calls, attempt_ids)
}

#[tokio::test]
async fn post_send_unknown_is_not_retried_automatically() {
    let model = SequenceModel {
        outcomes: vec![transient("post_send_unknown", ModelRetryClass::Never, true)],
        calls: Arc::new(AtomicUsize::new(0)),
        attempt_ids: Arc::new(std::sync::Mutex::new(Vec::new())),
        entered: Arc::new(Notify::new()),
        rejected: false,
        retry_after_ms: None,
        emit_delta_before_error: false,
    };
    let (harness, _, calls, attempt_ids) = harness(model, std::time::Duration::from_secs(2));
    let events = harness
        .send(RunnerCommand::start_in(
            RunId::new(),
            "unknown response",
            "/p4-j7-23-unknown",
            "read-only",
        ))
        .await
        .unwrap();
    assert!(!events
        .iter()
        .any(|event| matches!(event, RunnerEvent::Completed { .. })));
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    assert_eq!(harness_model_calls(&events), 1);
    assert_eq!(attempt_ids.lock().unwrap().len(), 1);
}

#[tokio::test]
async fn retryable_429_then_success_records_two_attempts() {
    let model = SequenceModel {
        outcomes: vec![
            transient("provider_http_429", ModelRetryClass::Rejected, true),
            Ok(ModelOutput::text("recovered")),
        ],
        calls: Arc::new(AtomicUsize::new(0)),
        attempt_ids: Arc::new(std::sync::Mutex::new(Vec::new())),
        entered: Arc::new(Notify::new()),
        rejected: true,
        retry_after_ms: Some(1),
        emit_delta_before_error: false,
    };
    let (harness, _, calls, attempt_ids) = harness(model, std::time::Duration::from_secs(2));
    let events = harness
        .send(RunnerCommand::start_in(
            RunId::new(),
            "retry 429",
            "/p4-j7-23-retry",
            "read-only",
        ))
        .await
        .unwrap();
    assert!(events
        .iter()
        .any(|event| matches!(event, RunnerEvent::Completed { .. })));
    assert_eq!(harness_model_calls(&events), 2);
    assert_eq!(calls.load(Ordering::SeqCst), 2);
    let attempt_ids = attempt_ids.lock().unwrap();
    assert_eq!(attempt_ids.len(), 2);
    assert_ne!(attempt_ids[0], attempt_ids[1]);
    let attempts = events
        .iter()
        .filter(|event| matches!(event, RunnerEvent::ModelTurn { .. }))
        .count();
    assert_eq!(attempts, 2);
}

#[tokio::test]
async fn provider_503_rejection_does_not_retry_even_with_no_known_effect() {
    let model = SequenceModel {
        outcomes: vec![
            transient("provider_http_503", ModelRetryClass::Rejected, true),
            Ok(ModelOutput::text("must not run")),
        ],
        calls: Arc::new(AtomicUsize::new(0)),
        attempt_ids: Arc::new(std::sync::Mutex::new(Vec::new())),
        entered: Arc::new(Notify::new()),
        rejected: true,
        retry_after_ms: Some(1),
        emit_delta_before_error: false,
    };
    let (harness, _, calls, _) = harness(model, std::time::Duration::from_secs(2));
    let events = harness
        .send(RunnerCommand::start_in(
            RunId::new(),
            "unsafe rejection",
            "/p4-j7-23-503",
            "read-only",
        ))
        .await
        .unwrap();
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    assert_eq!(harness_model_calls(&events), 1);
    assert!(events.iter().any(
        |event| matches!(event, RunnerEvent::Failed { error, .. } if error == "provider_http_503")
    ));
    assert!(!events.iter().any(|event| matches!(
        event,
        RunnerEvent::Completed { .. } | RunnerEvent::CapabilityRequested { .. }
    )));
}

#[tokio::test]
async fn provider_retries_do_not_consume_the_separate_repair_budget() {
    let model = SequenceModel {
        outcomes: vec![
            transient("provider_http_429", ModelRetryClass::Rejected, true),
            transient("provider_http_408", ModelRetryClass::Rejected, true),
            Ok(ModelOutput::text("recovered after two rejections")),
        ],
        calls: Arc::new(AtomicUsize::new(0)),
        attempt_ids: Arc::new(std::sync::Mutex::new(Vec::new())),
        entered: Arc::new(Notify::new()),
        rejected: true,
        retry_after_ms: Some(1),
        emit_delta_before_error: false,
    };
    let mut config = RuntimeConfig::default();
    config.wall_time_budget = Some(std::time::Duration::from_secs(5));
    config.budget = HarnessBudgetConfig {
        max_attempts_per_task: 3,
        max_repairs_per_task: 1,
        ..HarnessBudgetConfig::default()
    };
    let (harness, _, calls, attempt_ids) = harness_with_config(model, config);
    let events = harness
        .send(RunnerCommand::start_in(
            RunId::new(),
            "transport retry budget separation",
            "/p4-j7-23-repair-budget",
            "read-only",
        ))
        .await
        .unwrap();

    assert!(events
        .iter()
        .any(|event| matches!(event, RunnerEvent::Completed { .. })));
    assert_eq!(calls.load(Ordering::SeqCst), 3);
    assert_eq!(harness_model_calls(&events), 3);
    let attempt_ids = attempt_ids.lock().unwrap();
    assert_eq!(attempt_ids.len(), 3);
    assert_ne!(attempt_ids[0], attempt_ids[1]);
    assert_ne!(attempt_ids[0], attempt_ids[2]);
    assert_ne!(attempt_ids[1], attempt_ids[2]);
    let repair_counts = events
        .iter()
        .filter_map(|event| match event {
            RunnerEvent::ModelTurn { metadata, .. } => {
                metadata["harness_budget"]["repairs"].as_u64()
            }
            _ => None,
        })
        .collect::<Vec<_>>();
    assert_eq!(repair_counts, vec![0, 0, 0]);
}

#[tokio::test]
async fn cancel_during_retry_backoff_prevents_next_attempt() {
    let model = SequenceModel {
        outcomes: vec![transient(
            "provider_http_429",
            ModelRetryClass::Rejected,
            true,
        )],
        calls: Arc::new(AtomicUsize::new(0)),
        attempt_ids: Arc::new(std::sync::Mutex::new(Vec::new())),
        entered: Arc::new(Notify::new()),
        rejected: true,
        retry_after_ms: Some(5_000),
        emit_delta_before_error: false,
    };
    let (harness, entered, calls, _) = harness(model, std::time::Duration::from_secs(15));
    let run_id = RunId::new();
    let running = {
        let harness = harness.clone();
        tokio::spawn(async move {
            harness
                .send(RunnerCommand::start_in(
                    run_id,
                    "cancel retry wait",
                    "/p4-j7-23-cancel",
                    "read-only",
                ))
                .await
                .unwrap()
        })
    };
    entered.notified().await;
    tokio::time::sleep(std::time::Duration::from_millis(20)).await;
    let cancelled = harness
        .send(RunnerCommand::Cancel {
            run_id,
            reason: "user".to_owned(),
        })
        .await
        .unwrap();
    assert!(cancelled.iter().any(|event| {
        matches!(event, RunnerEvent::Failed { error, .. } if error == "cancelled:user")
    }));
    let events = running.await.unwrap();
    assert_eq!(harness_model_calls(&events), 1);
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    assert!(!events
        .iter()
        .any(|event| matches!(event, RunnerEvent::Completed { .. })));
}

#[tokio::test]
async fn oversized_retry_after_does_not_retry_early() {
    let model = SequenceModel {
        outcomes: vec![transient(
            "provider_http_429",
            ModelRetryClass::Rejected,
            true,
        )],
        calls: Arc::new(AtomicUsize::new(0)),
        attempt_ids: Arc::new(std::sync::Mutex::new(Vec::new())),
        entered: Arc::new(Notify::new()),
        rejected: true,
        retry_after_ms: Some(30_000),
        emit_delta_before_error: false,
    };
    let (harness, _, calls, _) = harness(model, std::time::Duration::from_millis(300));
    let events = harness
        .send(RunnerCommand::start_in(
            RunId::new(),
            "retry beyond deadline",
            "/p4-j7-23-deadline",
            "read-only",
        ))
        .await
        .unwrap();
    assert_eq!(harness_model_calls(&events), 1);
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    assert!(!events
        .iter()
        .any(|event| matches!(event, RunnerEvent::Completed { .. })));
}

#[tokio::test]
async fn observed_delta_prevents_retry_even_for_retryable_rejection() {
    let model = SequenceModel {
        outcomes: vec![transient(
            "provider_http_429",
            ModelRetryClass::Rejected,
            true,
        )],
        calls: Arc::new(AtomicUsize::new(0)),
        attempt_ids: Arc::new(std::sync::Mutex::new(Vec::new())),
        entered: Arc::new(Notify::new()),
        rejected: true,
        retry_after_ms: Some(1),
        emit_delta_before_error: true,
    };
    let (harness, _, calls, _) = harness(model, std::time::Duration::from_secs(2));
    let events = harness
        .send(RunnerCommand::start_in(
            RunId::new(),
            "partial then reject",
            "/p4-j7-23-partial",
            "read-only",
        ))
        .await
        .unwrap();
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    assert!(events.iter().any(
        |event| matches!(event, RunnerEvent::Delta { text, .. } if text == "partial response")
    ));
    assert!(!events
        .iter()
        .any(|event| matches!(event, RunnerEvent::Completed { .. })));
}

fn harness_model_calls(events: &[RunnerEvent]) -> usize {
    events
        .iter()
        .filter(|event| matches!(event, RunnerEvent::ModelTurn { .. }))
        .count()
}

/// 记录每一次准入与结算的假 `ModelBudgetPort`。
///
/// 它不做授权决策：只把 runner 的准入/消费/结算调用记录下来，供测试断言
/// “每次重试都重新准入”、“队列阶段可取消”以及“许可最终被结算释放”。
/// `hold_queue` 为真时 `reserve_prepared` 永不返回，用来把 run 停在排队阶段。
struct RecordingBudget {
    entered: Arc<Notify>,
    reservations: Arc<AtomicUsize>,
    consumes: Arc<AtomicUsize>,
    settlements: Arc<AtomicUsize>,
    admitted_attempts: Arc<std::sync::Mutex<Vec<RequestId>>>,
    hold_queue: bool,
}

impl RecordingBudget {
    fn new(hold_queue: bool) -> Self {
        Self {
            entered: Arc::new(Notify::new()),
            reservations: Arc::new(AtomicUsize::new(0)),
            consumes: Arc::new(AtomicUsize::new(0)),
            settlements: Arc::new(AtomicUsize::new(0)),
            admitted_attempts: Arc::new(std::sync::Mutex::new(Vec::new())),
            hold_queue,
        }
    }
}

#[async_trait]
impl kiana_ports::ModelBudgetPort for RecordingBudget {
    async fn reserve_prepared(
        &self,
        prepared: &kiana_domain::PreparedModelCall,
    ) -> Result<kiana_domain::ModelCallPermit, kiana_ports::PortError> {
        self.reservations.fetch_add(1, Ordering::SeqCst);
        self.admitted_attempts
            .lock()
            .unwrap()
            .push(prepared.spec.attempt_id);
        self.entered.notify_one();
        if self.hold_queue {
            // 排队阶段的代表：准入永不返回，只有取消能让 run 退出。
            return std::future::pending::<
                Result<kiana_domain::ModelCallPermit, kiana_ports::PortError>,
            >()
            .await;
        }
        Ok(kiana_domain::ModelCallPermit {
            schema: "kiana.model-call-permit.v1".to_owned(),
            permit_id: RequestId::new(),
            run_id: RunId::new(),
            attempt_id: prepared.spec.attempt_id,
            request_hash: prepared.request_hash.clone(),
            expires_at_unix_ms: u64::MAX,
            route_digest: None,
            configuration_revision: None,
            authority_revision: None,
            credential_revision: prepared.credential_revision.clone(),
            provider_account: prepared.provider_account.clone(),
        })
    }

    async fn consume_prepared(
        &self,
        _prepared: &kiana_domain::PreparedModelCall,
        _permit: &kiana_domain::ModelCallPermit,
    ) -> Result<(), kiana_ports::PortError> {
        self.consumes.fetch_add(1, Ordering::SeqCst);
        Ok(())
    }

    async fn reserve(
        &self,
        _run_id: RunId,
        _request_id: RequestId,
        _tokens: u64,
    ) -> Result<(), kiana_ports::PortError> {
        Ok(())
    }

    async fn settle(
        &self,
        _run_id: RunId,
        _request_id: RequestId,
        _tokens: Option<u64>,
    ) -> Result<(), kiana_ports::PortError> {
        self.settlements.fetch_add(1, Ordering::SeqCst);
        Ok(())
    }
}

fn install_recording_budget(harness: &Arc<KianaHarness>, budget: Arc<RecordingBudget>) {
    kiana_ports::RunnerPort::install_model_budget(harness.as_ref(), budget)
        .expect("install model budget port");
}

/// 阻塞在释放信号上的模型：被取消时其 future 会被丢弃，不会返回“完成”。
struct LateCompletionModel {
    entered: Arc<Notify>,
    release: Arc<Notify>,
    calls: Arc<AtomicUsize>,
}

#[async_trait]
impl ModelClient for LateCompletionModel {
    async fn complete(&self, _request: ModelRequest) -> Result<ModelOutput, String> {
        Err("p4_j7_23_prepared_path_required".to_owned())
    }

    async fn complete_prepared(
        &self,
        _prepared: kiana_domain::PreparedModelCall,
        _on_delta: &mut (dyn FnMut(kiana_domain::ModelDelta) -> Result<(), String> + Send),
    ) -> Result<ModelReply, ModelError> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        self.entered.notify_one();
        self.release.notified().await;
        ModelReply::legacy(ModelOutput::text("finished after the cancel race"))
    }
}

fn is_terminal_event(event: &RunnerEvent) -> bool {
    matches!(
        event,
        RunnerEvent::Completed { .. } | RunnerEvent::Failed { .. }
    )
}

fn model_turn_metadata(events: &[RunnerEvent]) -> Vec<serde_json::Value> {
    events
        .iter()
        .filter_map(|event| match event {
            RunnerEvent::ModelTurn { metadata, .. } => Some(metadata.clone()),
            _ => None,
        })
        .collect()
}

#[tokio::test]
async fn cancel_during_model_admission_wait_prevents_dispatch() {
    let model = SequenceModel {
        outcomes: vec![Ok(ModelOutput::text("must never be dispatched"))],
        calls: Arc::new(AtomicUsize::new(0)),
        attempt_ids: Arc::new(std::sync::Mutex::new(Vec::new())),
        entered: Arc::new(Notify::new()),
        rejected: false,
        retry_after_ms: None,
        emit_delta_before_error: false,
    };
    let (harness, _, calls, attempt_ids) = harness(model, std::time::Duration::from_secs(5));
    let budget = Arc::new(RecordingBudget::new(true));
    install_recording_budget(&harness, budget.clone());
    let run_id = RunId::new();
    let running = {
        let harness = harness.clone();
        tokio::spawn(async move {
            harness
                .send(RunnerCommand::start_in(
                    run_id,
                    "cancel while waiting for admission",
                    "/p4-j7-23-admission-wait",
                    "read-only",
                ))
                .await
                .unwrap()
        })
    };
    budget.entered.notified().await;
    let cancelled = harness
        .send(RunnerCommand::Cancel {
            run_id,
            reason: "user".to_owned(),
        })
        .await
        .unwrap();
    assert!(cancelled.iter().any(|event| {
        matches!(event, RunnerEvent::Failed { error, .. } if error == "cancelled:user")
    }));
    let events = running.await.unwrap();
    let terminals = events
        .iter()
        .filter(|event| is_terminal_event(event))
        .count();
    assert_eq!(
        terminals, 1,
        "cancel during admission must end the run once"
    );
    assert!(events.iter().any(|event| {
        matches!(event, RunnerEvent::Failed { error, .. } if error == "cancelled:user")
    }));
    assert_eq!(budget.reservations.load(Ordering::SeqCst), 1);
    assert_eq!(budget.consumes.load(Ordering::SeqCst), 0);
    assert_eq!(budget.settlements.load(Ordering::SeqCst), 0);
    assert_eq!(calls.load(Ordering::SeqCst), 0);
    assert!(attempt_ids.lock().unwrap().is_empty());
    assert!(!events.iter().any(|event| matches!(
        event,
        RunnerEvent::ModelTurn { .. }
            | RunnerEvent::Completed { .. }
            | RunnerEvent::CapabilityRequested { .. }
    )));
}

#[tokio::test]
async fn each_retry_is_admitted_again_with_a_fresh_permit() {
    let model = SequenceModel {
        outcomes: vec![
            transient("provider_http_429", ModelRetryClass::Rejected, true),
            Ok(ModelOutput::text("admitted twice")),
        ],
        calls: Arc::new(AtomicUsize::new(0)),
        attempt_ids: Arc::new(std::sync::Mutex::new(Vec::new())),
        entered: Arc::new(Notify::new()),
        rejected: true,
        retry_after_ms: Some(1),
        emit_delta_before_error: false,
    };
    let (harness, _, calls, attempt_ids) = harness(model, std::time::Duration::from_secs(5));
    let budget = Arc::new(RecordingBudget::new(false));
    install_recording_budget(&harness, budget.clone());
    let events = harness
        .send(RunnerCommand::start_in(
            RunId::new(),
            "re-admit the retry",
            "/p4-j7-23-readmission",
            "read-only",
        ))
        .await
        .unwrap();
    assert!(events
        .iter()
        .any(|event| matches!(event, RunnerEvent::Completed { .. })));
    assert_eq!(calls.load(Ordering::SeqCst), 2);
    assert_eq!(harness_model_calls(&events), 2);
    assert_eq!(
        budget.reservations.load(Ordering::SeqCst),
        2,
        "every retry must pass admission again"
    );
    assert_eq!(budget.consumes.load(Ordering::SeqCst), 2);
    assert_eq!(budget.settlements.load(Ordering::SeqCst), 2);
    let admitted = budget.admitted_attempts.lock().unwrap().clone();
    let dispatched = attempt_ids.lock().unwrap().clone();
    assert_eq!(admitted.len(), 2);
    assert_eq!(admitted, dispatched);
    assert_ne!(admitted[0], admitted[1]);
    let leases = model_turn_metadata(&events)
        .iter()
        .filter_map(|metadata| {
            metadata["retry_reservation"]["capacity_lease_digest"]
                .as_str()
                .map(str::to_owned)
        })
        .collect::<Vec<_>>();
    assert_eq!(leases.len(), 2, "each attempt must carry its own lease");
    assert_ne!(leases[0], leases[1]);
}

#[tokio::test]
async fn attempt_reservations_settle_or_mark_unknown_and_never_dangle() {
    // 429 被拒（请求已发送）→ 成功：第一次保守记为 unknown，第二次结算为 settled，
    // 且账本里的预留全部释放，不留下占着额度的 attempt。
    let model = SequenceModel {
        outcomes: vec![
            transient("provider_http_429", ModelRetryClass::Rejected, true),
            Ok(ModelOutput::text("settled after rejection")),
        ],
        calls: Arc::new(AtomicUsize::new(0)),
        attempt_ids: Arc::new(std::sync::Mutex::new(Vec::new())),
        entered: Arc::new(Notify::new()),
        rejected: true,
        retry_after_ms: Some(1),
        emit_delta_before_error: false,
    };
    let (reservation_harness, _, _, _) = harness(model, std::time::Duration::from_secs(5));
    let events = reservation_harness
        .send(RunnerCommand::start_in(
            RunId::new(),
            "reservation states",
            "/p4-j7-23-reservation",
            "read-only",
        ))
        .await
        .unwrap();
    let turns = model_turn_metadata(&events);
    assert_eq!(turns.len(), 2);
    let ordinals = turns
        .iter()
        .map(|metadata| metadata["retry_reservation"]["retry_ordinal"].as_u64())
        .collect::<Vec<_>>();
    assert_eq!(ordinals, vec![Some(1), Some(2)]);
    for metadata in &turns {
        assert_eq!(
            metadata["retry_reservation"]["request_count"].as_u64(),
            Some(1)
        );
        assert!(
            metadata["retry_reservation"]["state"].is_string(),
            "every dispatched attempt must record a reservation state"
        );
    }
    assert_eq!(
        turns[0]["retry_reservation"]["state"].as_str(),
        Some("unknown"),
        "a sent rejection must not claim a settled zero-usage attempt"
    );
    assert_eq!(
        turns[1]["retry_reservation"]["state"].as_str(),
        Some("settled")
    );
    let last = &turns[1];
    let per_attempt = last["reserved_tokens"].as_u64().unwrap();
    assert_eq!(
        last["harness_budget"]["reserved_tokens"].as_u64(),
        Some(0),
        "settlement must release the whole reservation"
    );
    assert_eq!(
        last["harness_budget"]["charged_tokens"].as_u64(),
        Some(per_attempt * 2),
        "attempts without usage reports are charged the reserved amount"
    );
    assert_eq!(last["harness_budget"]["unknown_attempts"].as_u64(), Some(2));

    // 发送结果未知的 attempt 保持“模型侧未知”，runner 不会据此编造工具结果，
    // 也不会在未知之后自动派发或完成。
    let model = SequenceModel {
        outcomes: vec![transient("post_send_unknown", ModelRetryClass::Never, true)],
        calls: Arc::new(AtomicUsize::new(0)),
        attempt_ids: Arc::new(std::sync::Mutex::new(Vec::new())),
        entered: Arc::new(Notify::new()),
        rejected: false,
        retry_after_ms: None,
        emit_delta_before_error: false,
    };
    let (harness, _, calls, _) = harness(model, std::time::Duration::from_secs(5));
    let events = harness
        .send(RunnerCommand::start_in(
            RunId::new(),
            "unknown reservation",
            "/p4-j7-23-unknown-reservation",
            "read-only",
        ))
        .await
        .unwrap();
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    let turns = model_turn_metadata(&events);
    assert_eq!(turns.len(), 1);
    assert_eq!(
        turns[0]["retry_reservation"]["state"].as_str(),
        Some("unknown")
    );
    assert_eq!(
        turns[0]["harness_budget"]["unknown_attempts"].as_u64(),
        Some(1)
    );
    assert!(events.iter().any(|event| {
        matches!(event, RunnerEvent::Failed { error, .. } if error == "post_send_unknown")
    }));
    assert!(!events.iter().any(|event| matches!(
        event,
        RunnerEvent::CapabilityRequested { .. }
            | RunnerEvent::ToolCancelled { .. }
            | RunnerEvent::Completed { .. }
    )));
}

#[tokio::test]
async fn cancel_while_the_model_turn_is_pending_emits_one_terminal_state() {
    let model = Arc::new(LateCompletionModel {
        entered: Arc::new(Notify::new()),
        release: Arc::new(Notify::new()),
        calls: Arc::new(AtomicUsize::new(0)),
    });
    let harness = Arc::new(KianaHarness::with_config(
        model.clone(),
        RuntimeConfig {
            wall_time_budget: Some(std::time::Duration::from_secs(5)),
            ..RuntimeConfig::default()
        },
    ));
    let run_id = RunId::new();
    let running = {
        let harness = harness.clone();
        tokio::spawn(async move {
            harness
                .send(RunnerCommand::start_in(
                    run_id,
                    "cancel the pending turn",
                    "/p4-j7-23-pending-turn",
                    "read-only",
                ))
                .await
                .unwrap()
        })
    };
    model.entered.notified().await;
    let cancelled = harness
        .send(RunnerCommand::Cancel {
            run_id,
            reason: "user".to_owned(),
        })
        .await
        .unwrap();
    assert!(cancelled.iter().any(|event| {
        matches!(event, RunnerEvent::Failed { error, .. } if error == "cancelled:user")
    }));
    let events = running.await.unwrap();
    let terminals = events
        .iter()
        .filter(|event| is_terminal_event(event))
        .count();
    assert_eq!(terminals, 1, "a cancelled model turn must end once");
    assert!(events.iter().any(|event| {
        matches!(event, RunnerEvent::Failed { error, .. } if error == "cancelled:user")
    }));
    assert!(!events
        .iter()
        .any(|event| matches!(event, RunnerEvent::Completed { .. })));
    assert!(!events
        .iter()
        .any(|event| matches!(event, RunnerEvent::Delta { .. })));
    assert_eq!(model.calls.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn cancel_racing_with_model_completion_keeps_one_terminal_state_per_observer() {
    for iteration in 0..8 {
        let model = SequenceModel {
            outcomes: vec![Ok(ModelOutput::text("completed before or after cancel"))],
            calls: Arc::new(AtomicUsize::new(0)),
            attempt_ids: Arc::new(std::sync::Mutex::new(Vec::new())),
            entered: Arc::new(Notify::new()),
            rejected: false,
            retry_after_ms: None,
            emit_delta_before_error: false,
        };
        let (harness, _, _, _) = harness(model, std::time::Duration::from_secs(5));
        let run_id = RunId::new();
        let run = {
            let harness = harness.clone();
            tokio::spawn(async move {
                harness
                    .send(RunnerCommand::start_in(
                        run_id,
                        "race the completion",
                        "/p4-j7-23-terminal-race",
                        "read-only",
                    ))
                    .await
            })
        };
        let cancel = {
            let harness = harness.clone();
            tokio::spawn(async move {
                harness
                    .send(RunnerCommand::Cancel {
                        run_id,
                        reason: "user".to_owned(),
                    })
                    .await
            })
        };
        let run_events = run
            .await
            .expect("start task")
            .expect("start command must not fail in the harness");
        let cancel_events = cancel
            .await
            .expect("cancel task")
            .expect("cancel command must not fail in the harness");
        let run_terminals = run_events
            .iter()
            .filter(|event| is_terminal_event(event))
            .count();
        assert_eq!(
            run_terminals, 1,
            "iteration {iteration}: the run itself must end in exactly one terminal state"
        );
        let run_completed = run_events
            .iter()
            .any(|event| matches!(event, RunnerEvent::Completed { .. }));
        let run_cancelled = run_events.iter().any(
            |event| matches!(event, RunnerEvent::Failed { error, .. } if error == "cancelled:user"),
        );
        assert!(
            !(run_completed && run_cancelled),
            "iteration {iteration}: a run cannot both complete and cancel"
        );
        assert!(
            !cancel_events
                .iter()
                .any(|event| matches!(event, RunnerEvent::Completed { .. })),
            "iteration {iteration}: cancellation must never report a completion"
        );
        let cancel_terminals = cancel_events
            .iter()
            .filter(|event| is_terminal_event(event))
            .count();
        assert!(cancel_terminals <= 1);
    }
}

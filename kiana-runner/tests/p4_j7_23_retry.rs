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
async fn provider_retries_do_not_consume_the_separate_repair_budget() {
    let model = SequenceModel {
        outcomes: vec![
            transient("provider_http_429", ModelRetryClass::Rejected, true),
            transient("provider_http_503", ModelRetryClass::Rejected, true),
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

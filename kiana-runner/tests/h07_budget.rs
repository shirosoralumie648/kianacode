use async_trait::async_trait;
use kiana_domain::{ModelError, ModelOutput, ModelReply, ModelRequest, ModelRetryClass, RunId};
use kiana_runner::{HarnessBudgetConfig, KianaHarness, ModelClient, RuntimeConfig, ScriptedModel};
use kiana_runner_protocol::{RunnerCommand, RunnerEvent};
use std::sync::{
    atomic::{AtomicUsize, Ordering},
    Arc,
};

#[derive(Clone)]
struct CountingModel {
    inner: Arc<ScriptedModel>,
    calls: Arc<AtomicUsize>,
}

impl CountingModel {
    fn new(outputs: Vec<ModelOutput>) -> (Self, Arc<AtomicUsize>) {
        let calls = Arc::new(AtomicUsize::new(0));
        (
            Self {
                inner: Arc::new(ScriptedModel::new(outputs)),
                calls: calls.clone(),
            },
            calls,
        )
    }
}

#[async_trait]
impl ModelClient for CountingModel {
    async fn complete(&self, request: ModelRequest) -> Result<ModelOutput, String> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        self.inner.complete(request).await
    }
}

struct RetryModel {
    calls: Arc<AtomicUsize>,
}

#[async_trait]
impl ModelClient for RetryModel {
    async fn complete(&self, _request: ModelRequest) -> Result<ModelOutput, String> {
        Err("retry_model_complete_not_used".to_owned())
    }

    async fn complete_prepared(
        &self,
        _prepared: kiana_domain::PreparedModelCall,
        _on_delta: &mut (dyn FnMut(kiana_domain::ModelDelta) -> Result<(), String> + Send),
    ) -> Result<ModelReply, ModelError> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        Err(ModelError::transport(
            "temporary_provider_failure",
            ModelRetryClass::BeforeSend,
            false,
        ))
    }
}

fn budget_config(mut update: impl FnMut(&mut HarnessBudgetConfig)) -> RuntimeConfig {
    let mut budget = HarnessBudgetConfig::default();
    update(&mut budget);
    RuntimeConfig {
        budget,
        max_steps_per_turn: 4,
        ..RuntimeConfig::default()
    }
}

#[tokio::test]
async fn continue_cannot_reset_task_chain_total_budget() {
    let (model, calls) = CountingModel::new(vec![
        ModelOutput::text("first turn"),
        ModelOutput::text("must not reach provider"),
    ]);
    let harness = KianaHarness::with_config(
        Arc::new(model),
        budget_config(|budget| budget.max_attempts_per_task = 1),
    );
    let run_id = RunId::new();
    let first = harness
        .send(RunnerCommand::start_in(
            run_id,
            "first",
            "/h07-project",
            "read-only",
        ))
        .await
        .unwrap();
    assert!(first
        .iter()
        .any(|event| matches!(event, RunnerEvent::Completed { .. })));
    let continued = harness
        .send(RunnerCommand::continue_run(run_id, "continue"))
        .await
        .unwrap();
    assert!(continued.iter().any(|event| {
        matches!(event, RunnerEvent::Failed { error, .. } if error == "budget_exceeded:model_attempts")
    }));
    assert_eq!(calls.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn provider_retries_consume_attempt_budget() {
    let calls = Arc::new(AtomicUsize::new(0));
    let harness = KianaHarness::with_config(
        Arc::new(RetryModel {
            calls: calls.clone(),
        }),
        budget_config(|budget| budget.max_attempts_per_task = 2),
    );
    let events = harness
        .send(RunnerCommand::start_in(
            RunId::new(),
            "retry",
            "/h07-retry",
            "read-only",
        ))
        .await
        .unwrap();
    assert!(events.iter().any(|event| {
        matches!(event, RunnerEvent::Failed { error, .. } if error == "budget_exceeded:model_attempts")
    }));
    assert_eq!(calls.load(Ordering::SeqCst), 2);
}

#[tokio::test]
async fn invalid_effective_budget_never_calls_model() {
    let (model, calls) = CountingModel::new(vec![ModelOutput::text("must not reach provider")]);
    let harness = KianaHarness::with_config(
        Arc::new(model),
        budget_config(|budget| budget.max_tokens_per_task = 0),
    );
    let events = harness
        .send(RunnerCommand::start_in(
            RunId::new(),
            "invalid",
            "/h07-invalid",
            "read-only",
        ))
        .await
        .unwrap();
    assert!(events.iter().any(|event| {
        matches!(event, RunnerEvent::Failed { error, .. } if error == "runtime_config_invalid:budget:harness_budget_empty")
    }));
    assert_eq!(calls.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn role_limits_reach_two_product_runs_without_leakage() {
    let (model, calls) = CountingModel::new(vec![
        ModelOutput::text("project one"),
        ModelOutput::text("project two"),
    ]);
    let harness = KianaHarness::with_config(
        Arc::new(model),
        budget_config(|budget| budget.max_attempts_per_task = 1),
    );
    for (run_id, project_root) in [
        (RunId::new(), "/h07-project-one"),
        (RunId::new(), "/h07-project-two"),
    ] {
        let events = harness
            .send(RunnerCommand::start_in(
                run_id,
                "role-limited",
                project_root,
                "read-only",
            ))
            .await
            .unwrap();
        assert!(events
            .iter()
            .any(|event| matches!(event, RunnerEvent::Completed { .. })));
    }
    assert_eq!(calls.load(Ordering::SeqCst), 2);
}

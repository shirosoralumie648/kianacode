use async_trait::async_trait;
use kiana_domain::{ModelError, ModelOutput, ModelReply, ModelRequest, ModelRetryClass, RunId};
use kiana_runner::{KianaHarness, ModelClient, RuntimeConfig};
use kiana_runner_protocol::{RunnerCommand, RunnerEvent};
use std::sync::{
    atomic::{AtomicUsize, Ordering},
    Arc,
};
use tokio::sync::Notify;

struct SilentModel {
    calls: Arc<AtomicUsize>,
}

#[async_trait]
impl ModelClient for SilentModel {
    async fn complete(&self, _request: ModelRequest) -> Result<ModelOutput, String> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        tokio::time::sleep(std::time::Duration::from_secs(5)).await;
        Ok(ModelOutput::text("late model output"))
    }
}

struct RetryModel {
    calls: Arc<AtomicUsize>,
    first_attempt: Arc<Notify>,
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
        let attempt = self.calls.fetch_add(1, Ordering::SeqCst) + 1;
        if attempt == 1 {
            self.first_attempt.notify_one();
        }
        Err(ModelError::transport(
            "transient_provider_failure",
            ModelRetryClass::BeforeSend,
            false,
        ))
    }
}

#[tokio::test]
async fn silent_model_is_interrupted_by_deadline() {
    let calls = Arc::new(AtomicUsize::new(0));
    let harness = KianaHarness::new(Arc::new(SilentModel {
        calls: calls.clone(),
    }))
    .with_wall_time_budget(std::time::Duration::from_millis(20));
    let events = harness
        .send(RunnerCommand::start_in(
            RunId::new(),
            "silent",
            "/h08-silent",
            "read-only",
        ))
        .await
        .unwrap();
    assert!(events.iter().any(|event| {
        matches!(event, RunnerEvent::Failed { error, .. } if error.contains("model_attempt_deadline"))
    }));
    assert!(!events
        .iter()
        .any(|event| matches!(event, RunnerEvent::Completed { .. })));
    assert_eq!(calls.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn cancel_during_retry_wait_prevents_next_attempt() {
    let calls = Arc::new(AtomicUsize::new(0));
    let first_attempt = Arc::new(Notify::new());
    let harness = Arc::new(KianaHarness::with_config(
        Arc::new(RetryModel {
            calls: calls.clone(),
            first_attempt: first_attempt.clone(),
        }),
        RuntimeConfig::default(),
    ));
    let run_id = RunId::new();
    let running = {
        let harness = harness.clone();
        tokio::spawn(async move {
            harness
                .send(RunnerCommand::start_in(
                    run_id,
                    "retry then cancel",
                    "/h08-retry",
                    "read-only",
                ))
                .await
                .unwrap()
        })
    };
    first_attempt.notified().await;
    let cancel = harness
        .send(RunnerCommand::Cancel {
            run_id,
            reason: "user".to_owned(),
        })
        .await
        .unwrap();
    assert!(cancel.iter().any(|event| {
        matches!(event, RunnerEvent::Failed { error, .. } if error == "cancelled:user")
    }));
    let events = running.await.unwrap();
    assert!(!events
        .iter()
        .any(|event| matches!(event, RunnerEvent::Completed { .. })));
    assert_eq!(calls.load(Ordering::SeqCst), 1);
}

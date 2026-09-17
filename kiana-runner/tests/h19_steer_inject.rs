use async_trait::async_trait;
use kiana_domain::{ModelOutput, ModelRequest, RunId};
use kiana_runner::{KianaHarness, ModelClient, RuntimeConfig};
use kiana_runner_protocol::{RunnerCommand, RunnerEvent};
use std::sync::{
    atomic::{AtomicUsize, Ordering},
    Arc, Mutex,
};
use tokio::sync::Notify;

struct StreamingSteerModel {
    calls: AtomicUsize,
    entered: Notify,
    release: Notify,
    requests: Mutex<Vec<ModelRequest>>,
}

#[async_trait]
impl ModelClient for StreamingSteerModel {
    async fn complete(&self, request: ModelRequest) -> Result<ModelOutput, String> {
        let call = self.calls.fetch_add(1, Ordering::SeqCst);
        self.requests.lock().unwrap().push(request);
        if call == 0 {
            self.entered.notify_one();
            self.release.notified().await;
            Ok(ModelOutput::text("first"))
        } else {
            Ok(ModelOutput::text("finished"))
        }
    }
}

#[tokio::test]
async fn steer_during_stream_is_seen_by_next_step_without_duplicate_prompt() {
    let model = Arc::new(StreamingSteerModel {
        calls: AtomicUsize::new(0),
        entered: Notify::new(),
        release: Notify::new(),
        requests: Mutex::new(Vec::new()),
    });
    let harness = Arc::new(KianaHarness::with_config(
        model.clone(),
        RuntimeConfig::default(),
    ));
    let run_id = RunId::new();
    let running = {
        let harness = harness.clone();
        tokio::spawn(async move {
            harness
                .send(RunnerCommand::start_in(
                    run_id,
                    "initial prompt",
                    "/h19",
                    "read-only",
                ))
                .await
                .unwrap()
        })
    };

    model.entered.notified().await;
    let queued = harness
        .send(RunnerCommand::Inject {
            run_id,
            input_id: kiana_domain::InputId::new(),
            source: "tty".to_owned(),
            target: "next-step".to_owned(),
            target_turn_id: None,
            text: "steer once".to_owned(),
        })
        .await
        .unwrap();
    assert!(queued.is_empty());
    model.release.notify_one();

    let events = running.await.unwrap();
    assert!(events
        .iter()
        .any(|event| matches!(event, RunnerEvent::Completed { .. })));
    assert_eq!(model.calls.load(Ordering::SeqCst), 2);
    let requests = model.requests.lock().unwrap();
    assert_eq!(requests.len(), 2);
    assert_eq!(
        requests[1]
            .messages
            .iter()
            .filter(|message| message.text == "steer once")
            .count(),
        1
    );
}

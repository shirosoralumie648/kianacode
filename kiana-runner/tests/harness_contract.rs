use async_trait::async_trait;
use kiana_domain::{CapabilityResult, RunId};
use kiana_runner::{KianaHarness, ModelClient, ModelOutput, ModelRequest, ScriptedModel};
use kiana_runner_protocol::{RunnerCommand, RunnerEvent};
use serde_json::json;
use std::sync::{
    atomic::{AtomicU64, AtomicUsize, Ordering},
    Arc,
};
use tokio::sync::Notify;

#[derive(Clone)]
struct CountingModel {
    inner: Arc<ScriptedModel>,
    calls: Arc<AtomicUsize>,
}

impl CountingModel {
    fn from_json(cassette: serde_json::Value) -> (Self, Arc<AtomicUsize>) {
        let calls = Arc::new(AtomicUsize::new(0));
        let model = Self {
            inner: Arc::new(ScriptedModel::from_json(&cassette).unwrap()),
            calls: calls.clone(),
        };
        (model, calls)
    }
}

#[async_trait]
impl ModelClient for CountingModel {
    async fn complete(&self, request: ModelRequest) -> Result<ModelOutput, String> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        self.inner.complete(request).await
    }
}

struct BlockingModel {
    entered: Arc<Notify>,
    release: Arc<Notify>,
    calls: Arc<AtomicUsize>,
}

#[async_trait]
impl ModelClient for BlockingModel {
    async fn complete(&self, _request: ModelRequest) -> Result<ModelOutput, String> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        self.entered.notify_one();
        self.release.notified().await;
        Ok(ModelOutput::text("released"))
    }
}

struct EventWriteFault {
    writes: usize,
    fail_after: usize,
}

impl EventWriteFault {
    fn sink(&mut self, _event: RunnerEvent) -> Result<(), String> {
        self.writes += 1;
        if self.writes > self.fail_after {
            Err("event_write_fault".to_owned())
        } else {
            Ok(())
        }
    }
}

#[derive(Default)]
struct InjectedClock {
    now_ms: AtomicU64,
}

impl InjectedClock {
    fn now_ms(&self) -> u64 {
        self.now_ms.load(Ordering::SeqCst)
    }

    fn advance(&self, delta_ms: u64) {
        self.now_ms.fetch_add(delta_ms, Ordering::SeqCst);
    }
}

#[tokio::test]
async fn runner_event_for_other_run_is_rejected() {
    let (model, calls) = CountingModel::from_json(json!([
        {
            "text": "requesting a read",
            "tool_calls": [{
                "id": "h01-shell",
                "name": "shell",
                "arguments": {"command": "printf h01"}
            }]
        },
        {"text": "done"}
    ]));
    let harness = KianaHarness::new(Arc::new(model));
    let run_id = RunId::new();
    let started = harness
        .send(RunnerCommand::start_in(
            run_id,
            "start h01",
            "/repo",
            "read-only",
        ))
        .await
        .unwrap();
    let request = started
        .iter()
        .find_map(|event| match event {
            RunnerEvent::CapabilityRequested { request, .. } => Some(request.clone()),
            _ => None,
        })
        .expect("the current run must retain its pending capability");

    let error = harness
        .send(RunnerCommand::CapabilityResult {
            run_id: RunId::new(),
            result: CapabilityResult::success(request.request_id, json!({"forged": true})),
        })
        .await
        .expect_err("a result for another run must be rejected");
    assert_eq!(error.to_string(), "kiana_harness_failed:run_not_found");
    assert_eq!(calls.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn event_write_fault_stops_before_model_call() {
    let (model, calls) = CountingModel::from_json(json!([{"text": "must not run"}]));
    let harness = KianaHarness::new(Arc::new(model));
    let run_id = RunId::new();
    let mut fault = EventWriteFault {
        writes: 0,
        fail_after: 0,
    };
    let error = harness
        .send_with_events(
            RunnerCommand::start_in(run_id, "fault", "/repo", "read-only"),
            &mut |event| fault.sink(event),
        )
        .await
        .expect_err("failed event delivery must stop the runner");
    assert_eq!(
        error.to_string(),
        "kiana_harness_failed:event_sink_failed:event_write_fault"
    );
    assert_eq!(fault.writes, 1);
    assert_eq!(calls.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn blocking_model_fixture_releases_deterministically() {
    let entered = Arc::new(Notify::new());
    let release = Arc::new(Notify::new());
    let calls = Arc::new(AtomicUsize::new(0));
    let model = Arc::new(BlockingModel {
        entered: entered.clone(),
        release: release.clone(),
        calls: calls.clone(),
    });
    let harness = Arc::new(KianaHarness::new(model));
    let run_id = RunId::new();
    let task_harness = harness.clone();
    let task = tokio::spawn(async move {
        task_harness
            .send(RunnerCommand::start_in(
                run_id,
                "blocked",
                "/repo",
                "read-only",
            ))
            .await
    });
    entered.notified().await;
    release.notify_one();
    let events = task.await.unwrap().unwrap();
    assert!(events.iter().any(|event| {
        matches!(event, RunnerEvent::Completed { run_id: event_run, .. } if *event_run == run_id)
    }));
    assert_eq!(calls.load(Ordering::SeqCst), 1);
}

#[test]
fn injected_clock_fixture_is_monotonic() {
    let clock = InjectedClock::default();
    assert_eq!(clock.now_ms(), 0);
    clock.advance(25);
    clock.advance(17);
    assert_eq!(clock.now_ms(), 42);
}

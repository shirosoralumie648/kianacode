//! In-process run event fan-out for additive streaming clients.
//!
//! The bus is a display projection only. It never writes the event ledger and it does not
//! replace receipts. A runner is switched to the real-time sink path only when at least one
//! subscriber already exists for the run; otherwise the original `RunnerPort::send` path is
//! preserved.

use async_trait::async_trait;
use kiana_ports::{PortError, RunnerPort};
use kiana_protocol::{ResponseEnvelope, RunId, RunStreamEnvelope, RunStreamEvent};
use kiana_runner_protocol::{RunnerCommand, RunnerEvent};
use std::collections::HashMap;
use std::sync::{Arc, Mutex, PoisonError};
use tokio::sync::broadcast;

const RUN_STREAM_CAPACITY: usize = 256;

#[derive(Default)]
pub(crate) struct RunStreamBus {
    channels: Mutex<HashMap<RunId, broadcast::Sender<RunStreamEnvelope>>>,
}

impl RunStreamBus {
    fn has_subscribers(&self, run_id: RunId) -> bool {
        let channels = self.channels.lock().unwrap_or_else(PoisonError::into_inner);
        channels
            .get(&run_id)
            .is_some_and(|sender| sender.receiver_count() > 0)
    }

    pub(crate) fn subscribe(&self, run_id: RunId) -> RunStreamSubscription {
        let mut channels = self.channels.lock().unwrap_or_else(PoisonError::into_inner);
        channels.retain(|_, sender| sender.receiver_count() > 0);
        let sender = channels
            .entry(run_id)
            .or_insert_with(|| broadcast::channel(RUN_STREAM_CAPACITY).0)
            .clone();
        RunStreamSubscription {
            run_id,
            receiver: sender.subscribe(),
        }
    }

    pub(crate) fn publish_delta(&self, run_id: RunId, text: String) {
        self.publish(run_id, RunStreamEvent::Delta { run_id, text }, false);
    }

    pub(crate) fn publish_terminal(&self, run_id: RunId, response: ResponseEnvelope) {
        self.publish(run_id, RunStreamEvent::Terminal { run_id, response }, true);
    }

    fn publish(&self, run_id: RunId, event: RunStreamEvent, terminal: bool) {
        let sender = {
            let mut channels = self.channels.lock().unwrap_or_else(PoisonError::into_inner);
            channels.retain(|_, sender| sender.receiver_count() > 0);
            channels.get(&run_id).cloned()
        };
        let Some(sender) = sender else {
            return;
        };
        let _ = sender.send(RunStreamEnvelope::new(event));
        if terminal {
            self.channels
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .remove(&run_id);
        }
    }
}

/// 一个按 run ID 过滤的进程内订阅。
///
/// 订阅必须在该 run 开始前建立才能观察到运行中的 `delta`。终态事件在现有订阅者上发送；
/// 订阅者掉线或落后时，调用方应以 Receipt 重新对账，不能把已收到的增量当作完成事实。
#[derive(Debug)]
pub struct RunStreamSubscription {
    run_id: RunId,
    receiver: broadcast::Receiver<RunStreamEnvelope>,
}

impl RunStreamSubscription {
    /// 返回订阅对应的 run ID。
    pub const fn run_id(&self) -> RunId {
        self.run_id
    }

    /// 等待下一条运行中事件。
    pub async fn recv(&mut self) -> Result<RunStreamEnvelope, broadcast::error::RecvError> {
        self.receiver.recv().await
    }
}

pub(crate) struct RunStreamRunner {
    inner: Arc<dyn RunnerPort>,
    bus: Arc<RunStreamBus>,
}

impl RunStreamRunner {
    pub(crate) fn wrap(inner: Arc<dyn RunnerPort>, bus: Arc<RunStreamBus>) -> Arc<dyn RunnerPort> {
        Arc::new(Self { inner, bus })
    }

    fn publish_runner_delta(&self, event: &RunnerEvent) {
        if let RunnerEvent::Delta { run_id, text } = event {
            self.bus.publish_delta(*run_id, text.clone());
        }
    }
}

#[async_trait]
impl RunnerPort for RunStreamRunner {
    async fn send(&self, command: RunnerCommand) -> Result<Vec<RunnerEvent>, PortError> {
        let run_id = command.run_id();
        if !self.bus.has_subscribers(run_id) {
            return self.inner.send(command).await;
        }
        self.inner
            .send_with_events(command, &mut |event| {
                self.publish_runner_delta(&event);
                Ok(())
            })
            .await
    }

    async fn send_with_events(
        &self,
        command: RunnerCommand,
        on_event: &mut (dyn FnMut(RunnerEvent) -> Result<(), String> + Send),
    ) -> Result<Vec<RunnerEvent>, PortError> {
        self.inner
            .send_with_events(command, &mut |event| {
                self.publish_runner_delta(&event);
                on_event(event)
            })
            .await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    #[derive(Default)]
    struct CountingRunner {
        send_calls: AtomicUsize,
        streaming_calls: AtomicUsize,
    }

    #[async_trait]
    impl RunnerPort for CountingRunner {
        async fn send(&self, command: RunnerCommand) -> Result<Vec<RunnerEvent>, PortError> {
            self.send_calls.fetch_add(1, Ordering::SeqCst);
            Ok(vec![RunnerEvent::Started {
                run_id: command.run_id(),
            }])
        }

        async fn send_with_events(
            &self,
            command: RunnerCommand,
            on_event: &mut (dyn FnMut(RunnerEvent) -> Result<(), String> + Send),
        ) -> Result<Vec<RunnerEvent>, PortError> {
            self.streaming_calls.fetch_add(1, Ordering::SeqCst);
            let events = vec![
                RunnerEvent::Started {
                    run_id: command.run_id(),
                },
                RunnerEvent::Delta {
                    run_id: command.run_id(),
                    text: "alpha".to_owned(),
                },
            ];
            for event in &events {
                on_event(event.clone())
                    .map_err(|error| PortError::Failed(format!("test_sink_failed:{error}")))?;
            }
            Ok(events)
        }
    }

    struct BackpressureRunner;

    #[async_trait]
    impl RunnerPort for BackpressureRunner {
        async fn send(&self, command: RunnerCommand) -> Result<Vec<RunnerEvent>, PortError> {
            Ok(vec![RunnerEvent::Started {
                run_id: command.run_id(),
            }])
        }

        async fn send_with_events(
            &self,
            command: RunnerCommand,
            on_event: &mut (dyn FnMut(RunnerEvent) -> Result<(), String> + Send),
        ) -> Result<Vec<RunnerEvent>, PortError> {
            let events = vec![
                RunnerEvent::Started {
                    run_id: command.run_id(),
                },
                RunnerEvent::Delta {
                    run_id: command.run_id(),
                    text: "alpha".to_owned(),
                },
                RunnerEvent::Completed {
                    run_id: command.run_id(),
                    output: serde_json::json!({"text": "alpha"}),
                },
            ];
            for event in &events {
                on_event(event.clone())
                    .map_err(|error| PortError::Failed(format!("test_sink_failed:{error}")))?;
            }
            Ok(events)
        }
    }

    #[tokio::test]
    async fn no_subscriber_keeps_the_plain_runner_send_path() {
        let inner = Arc::new(CountingRunner::default());
        let runner = RunStreamRunner::wrap(inner.clone(), Arc::new(RunStreamBus::default()));
        let run_id = RunId::new();
        let events = runner
            .send(RunnerCommand::start(run_id, "hello"))
            .await
            .unwrap();

        assert_eq!(events.len(), 1);
        assert_eq!(inner.send_calls.load(Ordering::SeqCst), 1);
        assert_eq!(inner.streaming_calls.load(Ordering::SeqCst), 0);
    }

    #[tokio::test]
    async fn subscriber_switches_the_runner_to_the_sink_path() {
        let inner = Arc::new(CountingRunner::default());
        let bus = Arc::new(RunStreamBus::default());
        let run_id = RunId::new();
        let mut subscription = bus.subscribe(run_id);
        let runner = RunStreamRunner::wrap(inner.clone(), bus);
        let events = runner
            .send(RunnerCommand::start(run_id, "hello"))
            .await
            .unwrap();

        assert_eq!(events.len(), 2);
        assert_eq!(inner.send_calls.load(Ordering::SeqCst), 0);
        assert_eq!(inner.streaming_calls.load(Ordering::SeqCst), 1);
        assert_eq!(
            subscription.recv().await.unwrap().event,
            RunStreamEvent::Delta {
                run_id,
                text: "alpha".to_owned()
            }
        );
    }

    #[tokio::test]
    async fn lagged_subscriber_is_reported_instead_of_implying_completion() {
        let bus = Arc::new(RunStreamBus::default());
        let run_id = RunId::new();
        let mut subscription = bus.subscribe(run_id);

        for index in 0..=RUN_STREAM_CAPACITY {
            bus.publish_delta(run_id, format!("chunk-{index}"));
        }

        let error = subscription
            .recv()
            .await
            .expect_err("a lagged subscriber must fail closed");
        assert!(
            matches!(error, broadcast::error::RecvError::Lagged(skipped) if skipped > 0),
            "{error:?}"
        );
    }

    #[tokio::test]
    async fn sink_backpressure_stops_before_completed_and_publishes_no_terminal() {
        let bus = Arc::new(RunStreamBus::default());
        let run_id = RunId::new();
        let mut subscription = bus.subscribe(run_id);
        let runner = RunStreamRunner::wrap(Arc::new(BackpressureRunner), bus);
        let mut delivered = Vec::new();

        let error = runner
            .send_with_events(RunnerCommand::start(run_id, "stream it"), &mut |event| {
                delivered.push(event.clone());
                if matches!(event, RunnerEvent::Delta { .. }) {
                    Err("backpressure".to_owned())
                } else {
                    Ok(())
                }
            })
            .await
            .expect_err("sink errors must fail the run");

        assert!(
            matches!(
                error,
                PortError::Failed(ref message)
                    if message == "test_sink_failed:backpressure"
            ),
            "{error:?}"
        );
        assert!(!delivered
            .iter()
            .any(|event| matches!(event, RunnerEvent::Completed { .. })));

        let published = subscription.recv().await.expect("published delta");
        assert_eq!(
            published.event,
            RunStreamEvent::Delta {
                run_id,
                text: "alpha".to_owned(),
            }
        );
        assert!(
            tokio::time::timeout(std::time::Duration::from_millis(50), subscription.recv())
                .await
                .is_err(),
            "a failed sink must not publish a terminal completion"
        );
    }
}

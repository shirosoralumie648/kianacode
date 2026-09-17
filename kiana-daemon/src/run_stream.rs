//! Disposable, bounded projections of committed events and live model deltas.
//! Terminal replay and cursors never authorize or repeat an execution.

use async_trait::async_trait;
use kiana_domain::{RequestId, RuntimeEvent};
use kiana_ports::{EventAppendResult, EventStorePort, PortError, RunnerPort};
use kiana_protocol::{
    ResponseEnvelope, RunId, RunStreamEnvelope, RunStreamEvent, UiAction, UiCursor,
};
use kiana_runner_protocol::{RunnerCommand, RunnerEvent};
use std::collections::{HashMap, VecDeque};
use std::sync::{Arc, Mutex, PoisonError};
use std::time::{Duration, Instant};
use tokio::sync::broadcast;

#[cfg(test)]
use kiana_protocol::ExecutionStatus;

const RUN_STREAM_CAPACITY: usize = 256;
const MAX_RETAINED_RUNS: usize = 128;
const TERMINAL_RETENTION: Duration = Duration::from_secs(600);
const MAX_TERMINAL_BYTES: usize = 256 * 1024;
const MAX_ACTION_KEYS: usize = 1024;

struct RunChannel {
    sender: broadcast::Sender<RunStreamEnvelope>,
    sequence: u64,
    terminal: Option<RunStreamEnvelope>,
    touched: Instant,
}

impl RunChannel {
    fn new() -> Self {
        Self {
            sender: broadcast::channel(RUN_STREAM_CAPACITY).0,
            sequence: 0,
            terminal: None,
            touched: Instant::now(),
        }
    }
}

struct BusState {
    channels: HashMap<RunId, RunChannel>,
    ui_sequence: u64,
    action_keys: VecDeque<String>,
}

pub(crate) struct RunStreamBus {
    epoch: String,
    state: Mutex<BusState>,
}

impl Default for RunStreamBus {
    fn default() -> Self {
        Self {
            epoch: RunId::new().to_string(),
            state: Mutex::new(BusState {
                channels: HashMap::new(),
                ui_sequence: 0,
                action_keys: VecDeque::new(),
            }),
        }
    }
}

impl RunStreamBus {
    fn prune(state: &mut BusState, keep: RunId) {
        state.channels.retain(|id, channel| {
            *id == keep
                || channel.sender.receiver_count() > 0
                || channel.touched.elapsed() < TERMINAL_RETENTION
        });
        while state.channels.len() >= MAX_RETAINED_RUNS && !state.channels.contains_key(&keep) {
            let oldest = state
                .channels
                .iter()
                .min_by_key(|(_, channel)| (channel.sender.receiver_count() > 0, channel.touched))
                .map(|(id, _)| *id);
            if let Some(id) = oldest {
                state.channels.remove(&id);
            } else {
                break;
            }
        }
    }

    pub(crate) fn ui_cursor(&self) -> UiCursor {
        let state = self.state.lock().unwrap_or_else(PoisonError::into_inner);
        UiCursor {
            epoch: self.epoch.clone(),
            sequence: state.ui_sequence,
        }
    }

    pub(crate) fn run_cursor(&self, run_id: RunId) -> UiCursor {
        let state = self.state.lock().unwrap_or_else(PoisonError::into_inner);
        UiCursor {
            epoch: self.epoch.clone(),
            sequence: state
                .channels
                .get(&run_id)
                .map_or(0, |channel| channel.sequence),
        }
    }

    /// Atomically reject stale and repeated UI mutations before handing them to core.
    pub(crate) fn claim_ui_action(&self, action: &UiAction) -> Result<UiCursor, PortError> {
        if action.target_id.trim().is_empty()
            || action.target_id.len() > 256
            || action.idempotency_key.trim().is_empty()
            || action.idempotency_key.len() > 256
        {
            return Err(PortError::Failed("ui_action_invalid".to_owned()));
        }
        let mut state = self.state.lock().unwrap_or_else(PoisonError::into_inner);
        if action.expected_epoch != self.epoch || action.expected_cursor != state.ui_sequence {
            return Err(PortError::Conflict("ui_action_stale".to_owned()));
        }
        if state.action_keys.contains(&action.idempotency_key) {
            return Err(PortError::Conflict("ui_action_replayed".to_owned()));
        }
        state.ui_sequence = state
            .ui_sequence
            .checked_add(1)
            .ok_or_else(|| PortError::Failed("ui_cursor_exhausted".to_owned()))?;
        state.action_keys.push_back(action.idempotency_key.clone());
        if state.action_keys.len() > MAX_ACTION_KEYS {
            state.action_keys.pop_front();
        }
        Ok(UiCursor {
            epoch: self.epoch.clone(),
            sequence: state.ui_sequence,
        })
    }

    fn has_subscribers(&self, run_id: RunId) -> bool {
        self.state
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .channels
            .get(&run_id)
            .is_some_and(|channel| channel.sender.receiver_count() > 0)
    }

    pub(crate) fn begin_turn(&self, run_id: RunId) {
        let mut state = self.state.lock().unwrap_or_else(PoisonError::into_inner);
        Self::prune(&mut state, run_id);
        let channel = state.channels.entry(run_id).or_insert_with(RunChannel::new);
        channel.terminal = None;
        channel.touched = Instant::now();
    }

    pub(crate) fn subscribe(&self, run_id: RunId) -> RunStreamSubscription {
        self.subscribe_after(run_id, None)
    }

    pub(crate) fn subscribe_after(
        &self,
        run_id: RunId,
        after: Option<&UiCursor>,
    ) -> RunStreamSubscription {
        let mut state = self.state.lock().unwrap_or_else(PoisonError::into_inner);
        Self::prune(&mut state, run_id);
        let channel = state.channels.entry(run_id).or_insert_with(RunChannel::new);
        let mut replay = VecDeque::new();
        let same_epoch = after.is_none_or(|cursor| cursor.epoch == self.epoch);
        let last_sequence = after
            .filter(|_| same_epoch)
            .map_or(0, |cursor| cursor.sequence);
        let gap = !same_epoch
            || after.is_some_and(|cursor| cursor.sequence > channel.sequence)
            || channel.sequence > last_sequence;
        if let Some(terminal) = &channel.terminal {
            if !same_epoch || terminal.sequence > last_sequence {
                replay.push_back(terminal.clone());
            }
        }
        RunStreamSubscription {
            run_id,
            receiver: channel.sender.subscribe(),
            replay,
            gap,
            cursor: UiCursor {
                epoch: self.epoch.clone(),
                sequence: channel.sequence,
            },
        }
    }

    pub(crate) fn publish_delta(&self, run_id: RunId, text: String) {
        self.publish(run_id, RunStreamEvent::Delta { run_id, text });
    }

    pub(crate) fn publish_terminal(&self, run_id: RunId, mut response: ResponseEnvelope) {
        if serde_json::to_vec(&response).map_or(true, |bytes| bytes.len() > MAX_TERMINAL_BYTES) {
            response.output =
                serde_json::json!({"run_id": run_id, "stream_projection_truncated": true});
            if let Some(error) = response.error.as_mut() {
                *error = error.chars().take(4096).collect();
            }
        }
        self.publish(run_id, RunStreamEvent::Terminal { run_id, response });
    }

    fn publish(&self, run_id: RunId, event: RunStreamEvent) {
        let mut state = self.state.lock().unwrap_or_else(PoisonError::into_inner);
        Self::prune(&mut state, run_id);
        if state
            .channels
            .get(&run_id)
            .is_some_and(|channel| channel.terminal.is_some())
        {
            return;
        }
        let terminal = matches!(event, RunStreamEvent::Terminal { .. });
        let advances_ui = terminal || matches!(event, RunStreamEvent::ApprovalRequested { .. });
        if advances_ui {
            state.ui_sequence = state.ui_sequence.saturating_add(1);
        }
        let ui_cursor = state.ui_sequence;
        let channel = state.channels.entry(run_id).or_insert_with(RunChannel::new);
        let Some(sequence) = channel.sequence.checked_add(1) else {
            return;
        };
        channel.sequence = sequence;
        channel.touched = Instant::now();
        let envelope = RunStreamEnvelope {
            schema: kiana_protocol::PROTOCOL_SCHEMA.to_owned(),
            epoch: self.epoch.clone(),
            sequence,
            ui_cursor,
            event,
        };
        if terminal {
            channel.terminal = Some(envelope.clone());
        }
        let _ = channel.sender.send(envelope);
    }

    fn project_committed(&self, event: &RuntimeEvent) {
        // CLI and other clients also change the facts represented by a Web snapshot.
        // Advance the projection cursor even when this fact has no token-stream payload.
        {
            let mut state = self.state.lock().unwrap_or_else(PoisonError::into_inner);
            state.ui_sequence = state.ui_sequence.saturating_add(1);
        }
        let Some(run_id) = event
            .data
            .get("run_id")
            .and_then(serde_json::Value::as_str)
            .and_then(RunId::parse_str)
        else {
            return;
        };
        let data = event.data.clone();
        let projection = match event.kind.as_str() {
            "run.usage" | "model.usage" | "provider.usage" | "usage.recorded"
            | "run.model_turn" => RunStreamEvent::Usage { run_id, data },
            "run.capability_requested" | "run.tool_result" => {
                RunStreamEvent::ToolCall { run_id, data }
            }
            "approval.requested" => RunStreamEvent::ApprovalRequested { run_id, data },
            "run.failed"
            | "run.result_unknown"
            | "capability.failed"
            | "capability.result_unknown" => RunStreamEvent::Error { run_id, data },
            _ => return,
        };
        self.publish(run_id, projection);
    }
}

/// Subscriptions atomically attach a receiver and capture a bounded terminal replay.
/// Missing deltas are signalled as a gap; they are never silently replayed.
#[derive(Debug)]
pub struct RunStreamSubscription {
    run_id: RunId,
    receiver: broadcast::Receiver<RunStreamEnvelope>,
    replay: VecDeque<RunStreamEnvelope>,
    gap: bool,
    cursor: UiCursor,
}

impl RunStreamSubscription {
    pub const fn run_id(&self) -> RunId {
        self.run_id
    }
    pub const fn has_gap(&self) -> bool {
        self.gap
    }
    pub fn cursor(&self) -> &UiCursor {
        &self.cursor
    }
    pub async fn recv(&mut self) -> Result<RunStreamEnvelope, broadcast::error::RecvError> {
        if let Some(envelope) = self.replay.pop_front() {
            return Ok(envelope);
        }
        self.receiver.recv().await
    }
}

/// Transparently forwards every event-store guarantee; only committed fresh appends fan out.
pub(crate) struct StreamEventStore {
    inner: Arc<dyn EventStorePort>,
    bus: Arc<RunStreamBus>,
}
impl StreamEventStore {
    pub(crate) fn wrap(
        inner: Arc<dyn EventStorePort>,
        bus: Arc<RunStreamBus>,
    ) -> Arc<dyn EventStorePort> {
        Arc::new(Self { inner, bus })
    }
}
#[async_trait]
impl EventStorePort for StreamEventStore {
    fn supports_atomic_transitions(&self) -> bool {
        self.inner.supports_atomic_transitions()
    }
    fn capabilities(&self) -> kiana_domain::EventStoreCapabilities {
        self.inner.capabilities()
    }

    async fn flush(&self) -> Result<kiana_domain::EventStoreHealth, PortError> {
        self.inner.flush().await
    }

    async fn health(&self) -> Result<kiana_domain::EventStoreHealth, PortError> {
        self.inner.health().await
    }

    async fn last_durable_cursor(&self) -> Result<kiana_domain::EventCursor, PortError> {
        self.inner.last_durable_cursor().await
    }

    async fn close(&self) -> Result<kiana_domain::EventStoreHealth, PortError> {
        self.inner.close().await
    }
    async fn commit_transition(
        &self,
        batch: kiana_domain::TransitionBatch,
    ) -> Result<kiana_domain::CommitOutcome, PortError> {
        let events = batch.events.clone();
        let outcome = self.inner.commit_transition(batch).await?;
        if matches!(&outcome, kiana_domain::CommitOutcome::Committed { .. }) {
            for event in events {
                self.bus.project_committed(&event);
            }
        }
        Ok(outcome)
    }
    async fn read_command(
        &self,
        id: &kiana_domain::RequestId,
    ) -> Result<Option<kiana_domain::CommandReceipt>, PortError> {
        self.inner.read_command(id).await
    }
    async fn read_from(
        &self,
        cursor: u64,
        limit: usize,
    ) -> Result<kiana_domain::JournalPage, PortError> {
        self.inner.read_from(cursor, limit).await
    }

    async fn append(&self, event: RuntimeEvent) -> Result<(), PortError> {
        self.inner.append(event.clone()).await?;
        self.bus.project_committed(&event);
        Ok(())
    }
    async fn append_expected(
        &self,
        event: RuntimeEvent,
        expected: Option<u64>,
    ) -> Result<(), PortError> {
        self.inner.append_expected(event.clone(), expected).await?;
        self.bus.project_committed(&event);
        Ok(())
    }
    async fn append_idempotent(&self, event: RuntimeEvent) -> Result<EventAppendResult, PortError> {
        let result = self.inner.append_idempotent(event).await?;
        if !result.replayed {
            self.bus.project_committed(&result.event);
        }
        Ok(result)
    }
    async fn append_idempotent_expected(
        &self,
        event: RuntimeEvent,
        expected: Option<u64>,
    ) -> Result<EventAppendResult, PortError> {
        let result = self
            .inner
            .append_idempotent_expected(event, expected)
            .await?;
        if !result.replayed {
            self.bus.project_committed(&result.event);
        }
        Ok(result)
    }
    async fn read_request(&self, request_id: &RequestId) -> Result<Vec<RuntimeEvent>, PortError> {
        self.inner.read_request(request_id).await
    }
    async fn read_all(&self) -> Result<Vec<RuntimeEvent>, PortError> {
        self.inner.read_all().await
    }
    async fn read_stream(
        &self,
        aggregate_type: &str,
        aggregate_id: &str,
    ) -> Result<Vec<RuntimeEvent>, PortError> {
        self.inner.read_stream(aggregate_type, aggregate_id).await
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
    fn prepare(&self, command: &RunnerCommand) {
        if matches!(
            command,
            RunnerCommand::Start { .. } | RunnerCommand::Continue { .. }
        ) {
            self.bus.begin_turn(command.run_id());
        }
    }
    fn publish_runner_delta(&self, event: &RunnerEvent) {
        if let RunnerEvent::Delta { run_id, text } = event {
            self.bus.publish_delta(*run_id, text.clone());
        }
    }
}
#[async_trait]
impl RunnerPort for RunStreamRunner {
    fn bind_model_history(
        &self,
        run_id: kiana_domain::RunId,
        history: Vec<kiana_domain::ModelMessage>,
    ) -> Result<(), PortError> {
        self.inner.bind_model_history(run_id, history)
    }
    fn bind_model_assignment(
        &self,
        run_id: kiana_domain::RunId,
        assignment: kiana_domain::ModelAssignment,
    ) -> Result<(), PortError> {
        self.inner.bind_model_assignment(run_id, assignment)
    }
    fn install_model_budget(
        &self,
        budget: Arc<dyn kiana_ports::ModelBudgetPort>,
    ) -> Result<(), PortError> {
        self.inner.install_model_budget(budget)
    }

    async fn checkpoint(&self, run_id: RunId) -> Result<serde_json::Value, PortError> {
        self.inner.checkpoint(run_id).await
    }
    async fn restore(&self, run_id: RunId, checkpoint: serde_json::Value) -> Result<(), PortError> {
        self.inner.restore(run_id, checkpoint).await
    }

    async fn send(&self, command: RunnerCommand) -> Result<Vec<RunnerEvent>, PortError> {
        self.prepare(&command);
        if !self.bus.has_subscribers(command.run_id()) {
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
        self.prepare(&command);
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

    #[tokio::test]
    async fn run_stream_sequence_is_monotonic() {
        let bus = RunStreamBus::default();
        let run_id = RunId::new();
        let mut subscription = bus.subscribe(run_id);
        bus.publish_delta(run_id, "one".to_owned());
        bus.publish_delta(run_id, "two".to_owned());
        let first = subscription.recv().await.expect("first delta");
        let second = subscription.recv().await.expect("second delta");
        assert_eq!(first.epoch, second.epoch);
        assert_eq!(first.sequence, 1);
        assert_eq!(second.sequence, 2);
        assert_eq!(first.ui_cursor, 0);
        assert_eq!(second.ui_cursor, 0);

        bus.publish_terminal(
            run_id,
            ResponseEnvelope {
                schema: kiana_protocol::PROTOCOL_SCHEMA.to_owned(),
                request_id: RequestId::new(),
                status: ExecutionStatus::Completed,
                output: serde_json::json!({"ok": true}),
                error: None,
            },
        );
        let terminal = subscription.recv().await.expect("terminal");
        assert_eq!(terminal.sequence, 3);
        assert!(matches!(terminal.event, RunStreamEvent::Terminal { .. }));
        assert!(terminal.ui_cursor > second.ui_cursor);

        let cursor = UiCursor {
            epoch: first.epoch.clone(),
            sequence: second.sequence,
        };
        let mut late = bus.subscribe_after(run_id, Some(&cursor));
        assert!(late.has_gap());
        let replay = late.recv().await.expect("late terminal replay");
        assert_eq!(replay.epoch, first.epoch);
        assert_eq!(replay.sequence, terminal.sequence);
        assert!(matches!(replay.event, RunStreamEvent::Terminal { .. }));
    }

    #[test]
    fn stale_ui_action_is_rejected_by_epoch() {
        let bus = RunStreamBus::default();
        let cursor = bus.ui_cursor();
        let action = UiAction {
            target_id: "approval:one".to_owned(),
            expected_epoch: cursor.epoch.clone(),
            expected_cursor: cursor.sequence,
            idempotency_key: "action-one".to_owned(),
        };
        let next = bus.claim_ui_action(&action).expect("fresh action accepted");
        assert_eq!(next.sequence, cursor.sequence + 1);
        assert!(matches!(
            bus.claim_ui_action(&action),
            Err(PortError::Conflict(reason)) if reason == "ui_action_stale"
        ));
        let wrong_epoch = UiAction {
            expected_epoch: "stale-epoch".to_owned(),
            expected_cursor: next.sequence,
            idempotency_key: "action-two".to_owned(),
            ..action
        };
        assert!(matches!(
            bus.claim_ui_action(&wrong_epoch),
            Err(PortError::Conflict(reason)) if reason == "ui_action_stale"
        ));
    }
}

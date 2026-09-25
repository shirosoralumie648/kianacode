//! Disposable, bounded projections of committed events and live model deltas.
//! Terminal replay and cursors never authorize or repeat an execution.

use async_trait::async_trait;
use kiana_domain::{json_digest, RequestId, RuntimeEvent};
use kiana_ports::{EventAppendResult, EventStorePort, PortError, RunnerPort};
use kiana_protocol::{
    ResponseEnvelope, RunId, RunStreamEnvelope, RunStreamEvent, UiAction, UiCursor, UiFeedCursorV1,
    UiFeedFrameKind, UiFeedFrameV1, UiFeedGapReason, UiFeedGapV1,
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
const FEED_REPLAY_WINDOW: usize = 128;

/// The feed is deliberately bounded. A slow consumer receives an explicit gap and must
/// rehydrate a snapshot; it can never make the EventStore append path wait on an unbounded queue.
pub const UI_FEED_QUEUE_CAPACITY: usize = RUN_STREAM_CAPACITY;

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct UiFeedBackpressureMetrics {
    pub queue_capacity: usize,
    pub replay_window: usize,
    pub lagged_receives: u64,
    pub gap_frames: u64,
    pub terminal_delta_rejections: u64,
}

#[derive(Debug)]
pub enum RunStreamFeedError {
    Gap(UiFeedGapV1),
    Closed,
    Invalid(String),
}

impl std::fmt::Display for RunStreamFeedError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Gap(gap) => write!(formatter, "feed_gap:{:?}", gap.reason),
            Self::Closed => formatter.write_str("feed_closed"),
            Self::Invalid(reason) => formatter.write_str(reason),
        }
    }
}

impl std::error::Error for RunStreamFeedError {}

struct RunChannel {
    sender: broadcast::Sender<RunStreamEnvelope>,
    sequence: u64,
    terminal: Option<RunStreamEnvelope>,
    history: VecDeque<RunStreamEnvelope>,
    touched: Instant,
}

impl RunChannel {
    fn new() -> Self {
        Self {
            sender: broadcast::channel(RUN_STREAM_CAPACITY).0,
            sequence: 0,
            terminal: None,
            history: VecDeque::new(),
            touched: Instant::now(),
        }
    }
}

struct BusState {
    channels: HashMap<RunId, RunChannel>,
    ui_sequence: u64,
    action_keys: VecDeque<String>,
    lagged_receives: u64,
    gap_frames: u64,
    terminal_delta_rejections: u64,
}

pub(crate) struct RunStreamBus {
    instance_id: String,
    epoch: String,
    state: Mutex<BusState>,
}

impl Default for RunStreamBus {
    fn default() -> Self {
        Self {
            instance_id: RunId::new().to_string(),
            epoch: RunId::new().to_string(),
            state: Mutex::new(BusState {
                channels: HashMap::new(),
                ui_sequence: 0,
                action_keys: VecDeque::new(),
                lagged_receives: 0,
                gap_frames: 0,
                terminal_delta_rejections: 0,
            }),
        }
    }
}

impl RunStreamBus {
    pub(crate) fn instance_id(&self) -> &str {
        &self.instance_id
    }

    pub(crate) fn feed_metrics(&self) -> UiFeedBackpressureMetrics {
        let state = self.state.lock().unwrap_or_else(PoisonError::into_inner);
        UiFeedBackpressureMetrics {
            queue_capacity: UI_FEED_QUEUE_CAPACITY,
            replay_window: FEED_REPLAY_WINDOW,
            lagged_receives: state.lagged_receives,
            gap_frames: state.gap_frames,
            terminal_delta_rejections: state.terminal_delta_rejections,
        }
    }

    fn feed_cursor(&self, sequence: u64, ui_cursor: u64) -> Result<UiFeedCursorV1, String> {
        UiFeedCursorV1::new(
            self.instance_id.clone(),
            self.epoch.clone(),
            sequence,
            UiCursor {
                epoch: if ui_cursor == 0 {
                    String::new()
                } else {
                    self.epoch.clone()
                },
                sequence: ui_cursor,
            },
        )
    }

    fn feed_event_id(&self, envelope: &RunStreamEnvelope) -> String {
        json_digest(&serde_json::json!({
            "instance_id": &self.instance_id,
            "epoch": &envelope.epoch,
            "sequence": envelope.sequence,
            "event": &envelope.event,
        }))
    }

    fn feed_frame(
        &self,
        envelope: &RunStreamEnvelope,
        replay: bool,
    ) -> Result<UiFeedFrameV1, String> {
        let kind = match &envelope.event {
            RunStreamEvent::Terminal { .. } => UiFeedFrameKind::Terminal,
            RunStreamEvent::Unknown => UiFeedFrameKind::Unknown,
            _ => UiFeedFrameKind::Delta,
        };
        let frame = UiFeedFrameV1 {
            schema: kiana_protocol::UI_FEED_FRAME_SCHEMA.to_owned(),
            kind,
            cursor: self.feed_cursor(envelope.sequence, envelope.ui_cursor)?,
            event_id: self.feed_event_id(envelope),
            replay,
            terminal: matches!(&envelope.event, RunStreamEvent::Terminal { .. }),
            event: Some(
                serde_json::to_value(&envelope.event)
                    .map_err(|_| "feed_event_encode_failed".to_owned())?,
            ),
            gap: None,
        };
        frame.validate()?;
        Ok(frame)
    }

    fn gap_frame(
        &self,
        reason: UiFeedGapReason,
        from: Option<UiFeedCursorV1>,
        sequence: u64,
        ui_cursor: u64,
    ) -> Result<UiFeedFrameV1, String> {
        let to = self.feed_cursor(sequence, ui_cursor)?;
        let gap = UiFeedGapV1 {
            schema: kiana_protocol::UI_FEED_GAP_SCHEMA.to_owned(),
            reason,
            from,
            to: to.clone(),
            snapshot_required: true,
        };
        gap.validate()?;
        let frame = UiFeedFrameV1 {
            schema: kiana_protocol::UI_FEED_FRAME_SCHEMA.to_owned(),
            kind: UiFeedFrameKind::Gap,
            cursor: to,
            event_id: format!("gap:{}:{}", self.instance_id, sequence),
            replay: false,
            terminal: false,
            event: None,
            gap: Some(gap),
        };
        frame.validate()?;
        Ok(frame)
    }

    fn boundary_frame(&self, sequence: u64, ui_cursor: u64) -> Result<UiFeedFrameV1, String> {
        let cursor = self.feed_cursor(sequence, ui_cursor)?;
        let frame = UiFeedFrameV1 {
            schema: kiana_protocol::UI_FEED_FRAME_SCHEMA.to_owned(),
            kind: UiFeedFrameKind::SnapshotBoundary,
            cursor,
            event_id: format!("boundary:{}:{}", self.instance_id, sequence),
            replay: false,
            terminal: false,
            event: None,
            gap: None,
        };
        frame.validate()?;
        Ok(frame)
    }

    fn heartbeat_frame(&self, sequence: u64, ui_cursor: u64) -> Result<UiFeedFrameV1, String> {
        let cursor = self.feed_cursor(sequence, ui_cursor)?;
        let frame = UiFeedFrameV1 {
            schema: kiana_protocol::UI_FEED_FRAME_SCHEMA.to_owned(),
            kind: UiFeedFrameKind::Heartbeat,
            cursor,
            event_id: format!("heartbeat:{}:{}", self.instance_id, sequence),
            replay: false,
            terminal: false,
            event: None,
            gap: None,
        };
        frame.validate()?;
        Ok(frame)
    }

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
        channel
            .history
            .retain(|event| !matches!(&event.event, RunStreamEvent::Terminal { .. }));
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

    /// Subscribe to the versioned UI feed.  Replay is served only from the bounded history; an
    /// old epoch, expired window, sequence jump or instance mismatch yields an explicit gap frame
    /// that requires snapshot hydration.  No feed subscription can block EventStore commits.
    pub(crate) fn subscribe_feed_after(
        self: &Arc<Self>,
        run_id: RunId,
        after: Option<&UiFeedCursorV1>,
    ) -> RunStreamFeedSubscription {
        let mut state = self.state.lock().unwrap_or_else(PoisonError::into_inner);
        Self::prune(&mut state, run_id);
        let (current_sequence, history, terminal, sender) = {
            let channel = state.channels.entry(run_id).or_insert_with(RunChannel::new);
            (
                channel.sequence,
                channel.history.iter().cloned().collect::<Vec<_>>(),
                channel.terminal.clone(),
                channel.sender.clone(),
            )
        };
        let current_ui_cursor = state.ui_sequence;
        let mut frames = VecDeque::new();
        let mut gap_reason = None;
        let mut replay = Vec::new();

        if let Some(cursor) = after {
            if cursor.instance_id != self.instance_id {
                gap_reason = Some(UiFeedGapReason::InstanceChanged);
            } else if cursor.authority_epoch != self.epoch {
                gap_reason = Some(UiFeedGapReason::OldEpoch);
            } else if cursor.feed_sequence > current_sequence {
                gap_reason = Some(UiFeedGapReason::SequenceAhead);
            } else if cursor.feed_sequence < current_sequence {
                let first = history.first().map(|event| event.sequence);
                if first.is_none_or(|sequence| sequence > cursor.feed_sequence.saturating_add(1)) {
                    gap_reason = Some(UiFeedGapReason::ReplayExpired);
                } else {
                    replay.extend(
                        history
                            .iter()
                            .filter(|event| event.sequence > cursor.feed_sequence)
                            .cloned(),
                    );
                }
            }
        } else if let Some(terminal) = terminal.clone() {
            replay.push(terminal);
        }

        let from = after.cloned();
        let snapshot_boundary_after_gap = gap_reason.is_some();
        if let Some(reason) = gap_reason {
            state.gap_frames = state.gap_frames.saturating_add(1);
            if let Ok(frame) = self.gap_frame(reason, from, current_sequence, current_ui_cursor) {
                frames.push_back(frame);
            }
        }
        let (boundary_sequence, boundary_ui_cursor) = if snapshot_boundary_after_gap {
            (current_sequence, current_ui_cursor)
        } else {
            (
                after.map_or(current_sequence, |cursor| cursor.feed_sequence),
                after.map_or(current_ui_cursor, |cursor| cursor.snapshot_cursor.sequence),
            )
        };
        if let Ok(frame) = self.boundary_frame(boundary_sequence, boundary_ui_cursor) {
            frames.push_back(frame);
        }
        for envelope in replay {
            if let Ok(frame) = self.feed_frame(&envelope, true) {
                frames.push_back(frame);
            }
        }
        let initial_cursor = after
            .cloned()
            .unwrap_or_else(|| self.feed_cursor(0, 0).expect("zero feed cursor is valid"));
        RunStreamFeedSubscription {
            run_id,
            receiver: sender.subscribe(),
            bus: Arc::clone(self),
            frames,
            cursor: initial_cursor,
            terminal: terminal.is_some(),
        }
    }

    pub(crate) fn heartbeat(&self, run_id: RunId) -> Result<UiFeedFrameV1, String> {
        let state = self.state.lock().unwrap_or_else(PoisonError::into_inner);
        let sequence = state
            .channels
            .get(&run_id)
            .map_or(0, |channel| channel.sequence);
        self.heartbeat_frame(sequence, state.ui_sequence)
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
        let _ = self.publish_checked(run_id, event);
    }

    fn publish_checked(&self, run_id: RunId, event: RunStreamEvent) -> Result<(), PortError> {
        let mut state = self.state.lock().unwrap_or_else(PoisonError::into_inner);
        Self::prune(&mut state, run_id);
        let terminal_event = matches!(&event, RunStreamEvent::Terminal { .. });
        if state
            .channels
            .get(&run_id)
            .is_some_and(|channel| channel.terminal.is_some())
        {
            if !terminal_event {
                state.terminal_delta_rejections = state.terminal_delta_rejections.saturating_add(1);
                return Err(PortError::Conflict(
                    "feed_terminal_delta_forbidden".to_owned(),
                ));
            }
            return Err(PortError::Conflict("feed_terminal_duplicate".to_owned()));
        }
        let terminal = terminal_event;
        let advances_ui = terminal || matches!(&event, RunStreamEvent::ApprovalRequested { .. });
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
        channel.history.push_back(envelope.clone());
        while channel.history.len() > FEED_REPLAY_WINDOW {
            channel.history.pop_front();
        }
        let _ = channel.sender.send(envelope);
        Ok(())
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

/// Versioned feed subscription used by surfaces that need replay/gap semantics.  It keeps the
/// legacy `RunStreamSubscription` API intact while exposing an explicit snapshot boundary and
/// bounded backpressure result for new clients.
pub struct RunStreamFeedSubscription {
    run_id: RunId,
    receiver: broadcast::Receiver<RunStreamEnvelope>,
    bus: Arc<RunStreamBus>,
    frames: VecDeque<UiFeedFrameV1>,
    cursor: UiFeedCursorV1,
    terminal: bool,
}

impl RunStreamFeedSubscription {
    pub const fn run_id(&self) -> RunId {
        self.run_id
    }

    pub fn cursor(&self) -> &UiFeedCursorV1 {
        &self.cursor
    }

    pub fn has_pending_gap(&self) -> bool {
        self.frames
            .iter()
            .any(|frame| frame.kind == UiFeedFrameKind::Gap)
    }

    pub fn heartbeat(&self) -> Result<UiFeedFrameV1, String> {
        self.bus.heartbeat(self.run_id).map(|frame| frame)
    }

    pub async fn recv_frame(&mut self) -> Result<UiFeedFrameV1, RunStreamFeedError> {
        loop {
            if let Some(frame) = self.frames.pop_front() {
                if frame.kind == UiFeedFrameKind::Terminal {
                    self.terminal = true;
                }
                if frame.kind != UiFeedFrameKind::SnapshotBoundary {
                    self.cursor = frame.cursor.clone();
                }
                return Ok(frame);
            }
            let envelope = match self.receiver.recv().await {
                Ok(envelope) => envelope,
                Err(broadcast::error::RecvError::Lagged(skipped)) => {
                    let mut state = self
                        .bus
                        .state
                        .lock()
                        .unwrap_or_else(PoisonError::into_inner);
                    state.lagged_receives = state.lagged_receives.saturating_add(1);
                    state.gap_frames = state.gap_frames.saturating_add(1);
                    return Err(RunStreamFeedError::Gap(
                        self.bus
                            .gap_frame(
                                UiFeedGapReason::Backpressure,
                                Some(self.cursor.clone()),
                                self.cursor.feed_sequence.saturating_add(skipped as u64),
                                self.cursor.snapshot_cursor.sequence,
                            )
                            .map_err(RunStreamFeedError::Invalid)?
                            .gap
                            .expect("gap frame contains gap"),
                    ));
                }
                Err(broadcast::error::RecvError::Closed) => {
                    return Err(RunStreamFeedError::Closed);
                }
            };
            if envelope.epoch != self.cursor.authority_epoch {
                return self
                    .bus
                    .gap_frame(
                        UiFeedGapReason::OldEpoch,
                        Some(self.cursor.clone()),
                        envelope.sequence,
                        envelope.ui_cursor,
                    )
                    .map_err(RunStreamFeedError::Invalid);
            }
            if envelope.sequence <= self.cursor.feed_sequence {
                continue;
            }
            if self.cursor.feed_sequence != 0
                && envelope.sequence != self.cursor.feed_sequence.saturating_add(1)
            {
                return self
                    .bus
                    .gap_frame(
                        UiFeedGapReason::SequenceGap,
                        Some(self.cursor.clone()),
                        envelope.sequence,
                        envelope.ui_cursor,
                    )
                    .map_err(RunStreamFeedError::Invalid);
            }
            if self.terminal && !matches!(&envelope.event, RunStreamEvent::Terminal { .. }) {
                return Err(RunStreamFeedError::Invalid(
                    "feed_terminal_delta_forbidden".to_owned(),
                ));
            }
            let frame = self
                .bus
                .feed_frame(&envelope, false)
                .map_err(RunStreamFeedError::Invalid)?;
            self.cursor = frame.cursor.clone();
            if frame.terminal {
                self.terminal = true;
            }
            return Ok(frame);
        }
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

    #[tokio::test]
    async fn terminal_is_replayed_to_late_subscriber() {
        let bus = RunStreamBus::default();
        let run_id = RunId::new();
        let mut live = bus.subscribe(run_id);
        for kind in [
            "run.usage",
            "run.capability_requested",
            "approval.requested",
            "run.failed",
        ] {
            bus.project_committed(
                &RuntimeEvent::new(
                    RequestId::new(),
                    1,
                    kind,
                    serde_json::json!({"run_id": run_id}),
                )
                .expect("projection event"),
            );
            let _ = live.recv().await.expect("projected stream event");
        }
        bus.publish_terminal(
            run_id,
            ResponseEnvelope {
                schema: kiana_protocol::PROTOCOL_SCHEMA.to_owned(),
                request_id: RequestId::new(),
                status: ExecutionStatus::Completed,
                output: serde_json::json!({"terminal": true}),
                error: None,
            },
        );
        let mut late = bus.subscribe_after(run_id, None);
        assert!(late.has_gap());
        let replay = late.recv().await.expect("terminal replay");
        assert_eq!(replay.sequence, 5);
        assert!(matches!(replay.event, RunStreamEvent::Terminal { .. }));
        let mut caught_up = bus.subscribe_after(run_id, Some(late.cursor()));
        assert!(!caught_up.has_gap());
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

    #[tokio::test]
    async fn feed_replays_only_the_bounded_window_and_marks_boundaries() {
        let bus = Arc::new(RunStreamBus::default());
        let run_id = RunId::new();
        let mut live = bus.subscribe_feed_after(run_id, None);
        assert_eq!(
            live.recv_frame().await.unwrap().kind,
            UiFeedFrameKind::SnapshotBoundary
        );
        bus.publish_delta(run_id, "one".to_owned());
        let first = live.recv_frame().await.unwrap();
        assert_eq!(first.kind, UiFeedFrameKind::Delta);
        let cursor = first.cursor.clone();
        bus.publish_delta(run_id, "two".to_owned());
        drop(live);

        let mut resumed = bus.subscribe_feed_after(run_id, Some(&cursor));
        assert_eq!(
            resumed.recv_frame().await.unwrap().kind,
            UiFeedFrameKind::SnapshotBoundary
        );
        let replay = resumed.recv_frame().await.unwrap();
        assert!(replay.replay);
        assert_eq!(replay.cursor.feed_sequence, cursor.feed_sequence + 1);
        assert!(!resumed.has_pending_gap());
    }

    #[tokio::test]
    async fn feed_rejects_foreign_epoch_and_expired_replay() {
        let bus = Arc::new(RunStreamBus::default());
        let run_id = RunId::new();
        bus.publish_delta(run_id, "one".to_owned());
        let mut foreign = bus.subscribe_feed_after(
            run_id,
            Some(
                &UiFeedCursorV1::new(
                    bus.instance_id().to_owned(),
                    "old-epoch",
                    1,
                    UiCursor {
                        epoch: "old-epoch".to_owned(),
                        sequence: 1,
                    },
                )
                .unwrap(),
            ),
        );
        let frame = foreign.recv_frame().await.unwrap();
        assert_eq!(frame.kind, UiFeedFrameKind::Gap);
        assert_eq!(
            frame.gap.as_ref().unwrap().reason,
            UiFeedGapReason::OldEpoch
        );

        for index in 0..(FEED_REPLAY_WINDOW + 2) {
            bus.publish_delta(run_id, format!("delta-{index}"));
        }
        let stale = bus.feed_cursor(1, 1).unwrap();
        let mut expired = bus.subscribe_feed_after(run_id, Some(&stale));
        let frame = expired.recv_frame().await.unwrap();
        assert_eq!(frame.kind, UiFeedFrameKind::Gap);
        assert_eq!(
            frame.gap.as_ref().unwrap().reason,
            UiFeedGapReason::ReplayExpired
        );
        let boundary = expired.recv_frame().await.unwrap();
        assert_eq!(boundary.kind, UiFeedFrameKind::SnapshotBoundary);
        assert_eq!(boundary.cursor.feed_sequence, frame.cursor.feed_sequence);
    }

    #[tokio::test]
    async fn slow_consumer_gets_backpressure_gap_and_terminal_delta_is_denied() {
        let bus = Arc::new(RunStreamBus::default());
        let run_id = RunId::new();
        let mut subscription = bus.subscribe_feed_after(run_id, None);
        let _ = subscription.recv_frame().await.unwrap();
        for index in 0..=RUN_STREAM_CAPACITY {
            bus.publish_delta(run_id, format!("chunk-{index}"));
        }
        assert!(matches!(
            subscription.recv_frame().await,
            Err(RunStreamFeedError::Gap(gap))
                if gap.reason == UiFeedGapReason::Backpressure
        ));
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
        assert!(matches!(
            bus.publish_checked(run_id, RunStreamEvent::Delta {
                run_id,
                text: "late".to_owned(),
            }),
            Err(PortError::Conflict(reason)) if reason == "feed_terminal_delta_forbidden"
        ));
        let metrics = bus.feed_metrics();
        assert!(metrics.lagged_receives > 0);
        assert!(metrics.terminal_delta_rejections > 0);
    }

    #[test]
    fn heartbeat_is_bounded_and_explicit() {
        let bus = RunStreamBus::default();
        let frame = bus.heartbeat(RunId::new()).unwrap();
        assert_eq!(frame.kind, UiFeedFrameKind::Heartbeat);
        frame.validate().unwrap();
        assert_eq!(bus.feed_metrics().queue_capacity, UI_FEED_QUEUE_CAPACITY);
    }
}

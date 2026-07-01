use crate::types::SDKMessage;
use anyhow::{anyhow, Context, Result};
use futures_util::{SinkExt, StreamExt};
use kiana_remote::{
    CcrV2ClientEvent, CcrV2DeliveryStatus, CcrV2DeliveryUpdate, CcrV2ReconnectPolicy,
    CcrV2WorkerClient, CcrV2WorkerEvent, CcrV2WorkerEventStream,
};
use serde_json::{json, Value};
use std::collections::{BTreeMap, HashMap, HashSet, VecDeque};
use std::time::{Duration, Instant};
use tokio_tungstenite::{
    connect_async,
    tungstenite::{
        client::IntoClientRequest, http::header::AUTHORIZATION, http::HeaderValue, Message,
    },
};
use uuid::Uuid;

const STREAM_EVENT_FLUSH_INTERVAL: Duration = Duration::from_millis(100);
const CLIENT_EVENT_MAX_BATCH_SIZE: usize = 100;
const CLIENT_EVENT_MAX_BATCH_BYTES: usize = 10 * 1024 * 1024;
const CLIENT_EVENT_MAX_QUEUE_SIZE: usize = 100_000;
const DELIVERY_UPDATE_MAX_BATCH_SIZE: usize = 64;
const DELIVERY_UPDATE_MAX_QUEUE_SIZE: usize = 64;
const SESSION_INGRESS_UUID_DEDUP_CAPACITY: usize = 1024;

pub enum BridgeTransport {
    SessionIngress(Transport),
    CcrV2(CcrV2Transport),
}

impl BridgeTransport {
    pub async fn connect(&self) -> Result<BridgeTransportHandle> {
        match self {
            Self::SessionIngress(transport) => Ok(BridgeTransportHandle::SessionIngress(
                transport.connect().await?,
            )),
            Self::CcrV2(transport) => Ok(BridgeTransportHandle::CcrV2(transport.connect().await?)),
        }
    }
}

pub struct Transport {
    ws_url: String,
    access_token: String,
}

impl Transport {
    pub fn new(ws_url: String, access_token: String) -> Self {
        Self {
            ws_url,
            access_token,
        }
    }

    pub async fn connect(&self) -> Result<TransportHandle> {
        let mut request = self.ws_url.clone().into_client_request()?;
        let auth_value = HeaderValue::from_str(&format!("Bearer {}", self.access_token))
            .context("failed to build bridge authorization header")?;
        request.headers_mut().insert(AUTHORIZATION, auth_value);

        let (ws_stream, _) = connect_async(request).await?;
        let (write, read) = ws_stream.split();

        Ok(TransportHandle {
            write,
            read,
            recent_posted_uuids: BoundedUuidSet::new(SESSION_INGRESS_UUID_DEDUP_CAPACITY),
            recent_inbound_uuids: BoundedUuidSet::new(SESSION_INGRESS_UUID_DEDUP_CAPACITY),
        })
    }
}

pub struct CcrV2Transport {
    client: CcrV2WorkerClient,
    reconnect_policy: CcrV2ReconnectPolicy,
}

impl CcrV2Transport {
    pub fn new(client: CcrV2WorkerClient) -> Self {
        Self::new_with_reconnect_policy(client, CcrV2ReconnectPolicy::default())
    }

    pub fn new_with_reconnect_policy(
        client: CcrV2WorkerClient,
        reconnect_policy: CcrV2ReconnectPolicy,
    ) -> Self {
        Self {
            client,
            reconnect_policy,
        }
    }

    pub async fn connect(&self) -> Result<CcrV2TransportHandle> {
        self.client.initialize_worker().await?;
        let stream = self
            .client
            .connect_event_stream_with_reconnect_policy(self.reconnect_policy)
            .await?;
        Ok(CcrV2TransportHandle {
            client: self.client.clone(),
            stream,
            stream_events: StreamEventAccumulator::default(),
            client_events: ClientEventUploader::default(),
            delivery_updates: DeliveryUpdateUploader::default(),
            worker_status: Some("idle"),
        })
    }
}

pub enum BridgeTransportHandle {
    SessionIngress(TransportHandle),
    CcrV2(CcrV2TransportHandle),
}

impl BridgeTransportHandle {
    pub async fn send(&mut self, msg: &SDKMessage) -> Result<()> {
        match self {
            Self::SessionIngress(handle) => handle.send(msg).await,
            Self::CcrV2(handle) => handle.send(msg).await,
        }
    }

    pub async fn recv(&mut self) -> Result<Option<SDKMessage>> {
        match self {
            Self::SessionIngress(handle) => handle.recv().await,
            Self::CcrV2(handle) => handle.recv().await,
        }
    }

    pub async fn heartbeat(&mut self) -> Result<()> {
        match self {
            Self::SessionIngress(_) => Ok(()),
            Self::CcrV2(handle) => handle.heartbeat().await,
        }
    }

    pub fn maintenance_interval(&self) -> Option<Duration> {
        match self {
            Self::SessionIngress(_) => None,
            Self::CcrV2(handle) => handle.maintenance_interval(),
        }
    }

    pub async fn maintenance(&mut self) -> Result<()> {
        match self {
            Self::SessionIngress(_) => Ok(()),
            Self::CcrV2(handle) => handle.maintenance().await,
        }
    }

    pub async fn flush_pending(&mut self) -> Result<()> {
        match self {
            Self::SessionIngress(_) => Ok(()),
            Self::CcrV2(handle) => handle.flush_pending().await,
        }
    }

    pub fn reconnect_on_eof(&self) -> bool {
        matches!(self, Self::CcrV2(_))
    }

    pub async fn close(self) -> Result<()> {
        match self {
            Self::SessionIngress(handle) => handle.close().await,
            Self::CcrV2(handle) => handle.close().await,
        }
    }
}

pub struct TransportHandle {
    write: futures_util::stream::SplitSink<
        tokio_tungstenite::WebSocketStream<
            tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>,
        >,
        Message,
    >,
    read: futures_util::stream::SplitStream<
        tokio_tungstenite::WebSocketStream<
            tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>,
        >,
    >,
    recent_posted_uuids: BoundedUuidSet,
    recent_inbound_uuids: BoundedUuidSet,
}

impl TransportHandle {
    pub async fn send(&mut self, msg: &SDKMessage) -> Result<()> {
        if let Some(uuid) = sdk_message_uuid(msg) {
            self.recent_posted_uuids.add(uuid);
        }
        let json = serde_json::to_string(msg)?;
        self.write.send(Message::Text(json.into())).await?;
        Ok(())
    }

    pub async fn recv(&mut self) -> Result<Option<SDKMessage>> {
        loop {
            match self.read.next().await {
                Some(Ok(Message::Text(text))) => {
                    let msg: SDKMessage = serde_json::from_str(&text)?;
                    if let Some(uuid) = sdk_message_uuid(&msg) {
                        if self.recent_posted_uuids.has(uuid) {
                            continue;
                        }
                        if matches!(msg, SDKMessage::User { .. }) {
                            if self.recent_inbound_uuids.has(uuid) {
                                continue;
                            }
                            self.recent_inbound_uuids.add(uuid);
                        }
                    }
                    return Ok(Some(msg));
                }
                Some(Ok(Message::Close(_))) | None => return Ok(None),
                Some(Ok(_)) => continue,
                Some(Err(e)) => return Err(e.into()),
            }
        }
    }

    pub async fn close(mut self) -> Result<()> {
        self.write.close().await?;
        Ok(())
    }
}

pub struct CcrV2TransportHandle {
    client: CcrV2WorkerClient,
    stream: CcrV2WorkerEventStream,
    stream_events: StreamEventAccumulator,
    client_events: ClientEventUploader,
    delivery_updates: DeliveryUpdateUploader,
    worker_status: Option<&'static str>,
}

impl CcrV2TransportHandle {
    pub async fn send(&mut self, msg: &SDKMessage) -> Result<()> {
        if let Some((worker_status, requires_action_details)) =
            ccr_v2_worker_state_for_outbound_message(msg)
        {
            self.report_worker_state(worker_status, requires_action_details)
                .await?;
        }
        let payload = normalized_ccr_v2_payload(msg)?;
        if is_ccr_v2_stream_event_payload(&payload) {
            self.flush_expired_stream_events().await?;
            self.stream_events.push(payload);
            return Ok(());
        }

        let mut events = self.drain_stream_event_buffer();
        events.push(ccr_v2_client_event_from_payload(payload.clone()));
        self.client_events
            .enqueue_and_flush(events, &self.client)
            .await?;
        self.stream_events
            .clear_for_completed_assistant_payload(&payload);
        persist_internal_transcript_event(self.client.clone(), msg).await;
        Ok(())
    }

    pub async fn recv(&mut self) -> Result<Option<SDKMessage>> {
        let Some(event) = self.stream.next_event().await? else {
            return Ok(None);
        };
        if let Err(error) = self
            .delivery_updates
            .enqueue_and_flush_if_full(
                vec![
                    CcrV2DeliveryUpdate {
                        event_id: event.event_id.clone(),
                        status: CcrV2DeliveryStatus::Received,
                    },
                    CcrV2DeliveryUpdate {
                        event_id: event.event_id.clone(),
                        status: CcrV2DeliveryStatus::Processed,
                    },
                ],
                &self.client,
            )
            .await
        {
            eprintln!(
                "CCR v2 delivery acknowledgement failed for {}: {}",
                event.event_id, error
            );
        }
        let message = serde_json::from_value(event.payload).with_context(|| {
            format!(
                "failed to parse CCR v2 client event {} as SDK message",
                event.event_id
            )
        })?;
        if matches!(message, SDKMessage::User { .. }) {
            self.report_worker_state("running", None).await?;
        }
        persist_internal_transcript_event(self.client.clone(), &message).await;
        Ok(Some(message))
    }

    async fn report_worker_state(
        &mut self,
        worker_status: &'static str,
        requires_action_details: Option<Value>,
    ) -> Result<()> {
        if self.worker_status == Some(worker_status) && requires_action_details.is_none() {
            return Ok(());
        }
        self.client
            .report_worker_state(worker_status, requires_action_details)
            .await?;
        self.worker_status = Some(worker_status);
        Ok(())
    }

    pub async fn heartbeat(&mut self) -> Result<()> {
        self.flush_expired_stream_events().await?;
        if let Err(error) = self.delivery_updates.flush(&self.client).await {
            eprintln!("CCR v2 delivery acknowledgement failed before heartbeat: {error}");
        }
        if let Err(error) = self.client.flush_internal_events().await {
            eprintln!("CCR v2 internal transcript flush failed before heartbeat: {error}");
        }
        self.client.send_heartbeat().await?;
        Ok(())
    }

    fn maintenance_interval(&self) -> Option<Duration> {
        Some(STREAM_EVENT_FLUSH_INTERVAL)
    }

    async fn maintenance(&mut self) -> Result<()> {
        self.flush_expired_stream_events().await?;
        if let Err(error) = self.delivery_updates.flush(&self.client).await {
            eprintln!("CCR v2 delivery acknowledgement maintenance flush failed: {error}");
        }
        if let Err(error) = self.client.flush_internal_events().await {
            eprintln!("CCR v2 internal transcript maintenance flush failed: {error}");
        }
        Ok(())
    }

    async fn flush_pending(&mut self) -> Result<()> {
        self.flush_stream_event_buffer().await?;
        self.client_events.flush(&self.client).await?;
        if let Err(error) = self.delivery_updates.flush(&self.client).await {
            eprintln!("CCR v2 delivery acknowledgement final flush failed: {error}");
        }
        if let Err(error) = self.client.flush_internal_events().await {
            eprintln!("CCR v2 internal transcript final flush failed: {error}");
        }
        Ok(())
    }

    pub async fn close(mut self) -> Result<()> {
        self.flush_pending().await?;
        Ok(())
    }

    async fn flush_expired_stream_events(&mut self) -> Result<()> {
        if self.stream_events.should_flush(Instant::now()) {
            self.flush_stream_event_buffer().await?;
        }
        Ok(())
    }

    async fn flush_stream_event_buffer(&mut self) -> Result<()> {
        let events = self.drain_stream_event_buffer();
        if !events.is_empty() {
            self.client_events
                .enqueue_and_flush(events, &self.client)
                .await?;
        }
        Ok(())
    }

    fn drain_stream_event_buffer(&mut self) -> Vec<CcrV2ClientEvent> {
        self.stream_events
            .flush()
            .into_iter()
            .map(ccr_v2_client_event_from_payload)
            .collect()
    }
}

#[derive(Debug)]
struct DeliveryUpdateUploader {
    pending: VecDeque<CcrV2DeliveryUpdate>,
    max_batch_size: usize,
    max_queue_size: usize,
}

impl Default for DeliveryUpdateUploader {
    fn default() -> Self {
        Self {
            pending: VecDeque::new(),
            max_batch_size: DELIVERY_UPDATE_MAX_BATCH_SIZE,
            max_queue_size: DELIVERY_UPDATE_MAX_QUEUE_SIZE,
        }
    }
}

impl DeliveryUpdateUploader {
    #[cfg(test)]
    fn with_limits(max_batch_size: usize, max_queue_size: usize) -> Self {
        Self {
            pending: VecDeque::new(),
            max_batch_size: max_batch_size.max(1),
            max_queue_size: max_queue_size.max(1),
        }
    }

    async fn enqueue_and_flush_if_full(
        &mut self,
        updates: Vec<CcrV2DeliveryUpdate>,
        client: &CcrV2WorkerClient,
    ) -> Result<()> {
        if updates.is_empty() {
            return Ok(());
        }

        if self.pending.len() + updates.len() > self.max_queue_size {
            self.flush(client).await?;
        }
        self.enqueue(updates)?;
        if self.pending.len() >= self.max_batch_size {
            self.flush(client).await?;
        }
        Ok(())
    }

    fn enqueue<I>(&mut self, updates: I) -> Result<()>
    where
        I: IntoIterator<Item = CcrV2DeliveryUpdate>,
    {
        let updates = updates.into_iter().collect::<Vec<_>>();
        if updates.is_empty() {
            return Ok(());
        }
        if self.pending.len() + updates.len() > self.max_queue_size {
            return Err(anyhow!(
                "CCR v2 delivery update queue backpressure exceeded: pending={} incoming={} max={}",
                self.pending.len(),
                updates.len(),
                self.max_queue_size
            ));
        }
        self.pending.extend(updates);
        Ok(())
    }

    async fn flush(&mut self, client: &CcrV2WorkerClient) -> Result<()> {
        while !self.pending.is_empty() {
            let batch = self.take_batch();
            if !batch.is_empty() {
                client.report_delivery(batch).await?;
            }
        }
        Ok(())
    }

    fn take_batch(&mut self) -> Vec<CcrV2DeliveryUpdate> {
        let count = self.pending.len().min(self.max_batch_size);
        self.pending.drain(..count).collect()
    }
}

#[derive(Debug)]
struct ClientEventUploader {
    pending: VecDeque<CcrV2ClientEvent>,
    max_batch_size: usize,
    max_batch_bytes: usize,
    max_queue_size: usize,
}

impl Default for ClientEventUploader {
    fn default() -> Self {
        Self {
            pending: VecDeque::new(),
            max_batch_size: CLIENT_EVENT_MAX_BATCH_SIZE,
            max_batch_bytes: CLIENT_EVENT_MAX_BATCH_BYTES,
            max_queue_size: CLIENT_EVENT_MAX_QUEUE_SIZE,
        }
    }
}

impl ClientEventUploader {
    #[cfg(test)]
    fn with_limits(max_batch_size: usize, max_batch_bytes: usize, max_queue_size: usize) -> Self {
        Self {
            pending: VecDeque::new(),
            max_batch_size: max_batch_size.max(1),
            max_batch_bytes: max_batch_bytes.max(1),
            max_queue_size: max_queue_size.max(1),
        }
    }

    async fn enqueue_and_flush(
        &mut self,
        events: Vec<CcrV2ClientEvent>,
        client: &CcrV2WorkerClient,
    ) -> Result<()> {
        if events.is_empty() {
            return Ok(());
        }

        for chunk in events.chunks(self.max_queue_size) {
            self.enqueue(chunk.iter().cloned())?;
            self.flush(client).await?;
        }
        Ok(())
    }

    fn enqueue<I>(&mut self, events: I) -> Result<()>
    where
        I: IntoIterator<Item = CcrV2ClientEvent>,
    {
        let events = events.into_iter().collect::<Vec<_>>();
        if events.is_empty() {
            return Ok(());
        }
        if self.pending.len() + events.len() > self.max_queue_size {
            return Err(anyhow!(
                "CCR v2 client event queue backpressure exceeded: pending={} incoming={} max={}",
                self.pending.len(),
                events.len(),
                self.max_queue_size
            ));
        }
        self.pending.extend(events);
        Ok(())
    }

    async fn flush(&mut self, client: &CcrV2WorkerClient) -> Result<()> {
        while !self.pending.is_empty() {
            let batch = self.take_batch()?;
            if !batch.is_empty() {
                client.write_client_events(batch).await?;
            }
        }
        Ok(())
    }

    fn take_batch(&mut self) -> Result<Vec<CcrV2ClientEvent>> {
        let mut batch = Vec::new();
        let mut bytes = 0_usize;

        while batch.len() < self.max_batch_size {
            let Some(next) = self.pending.front() else {
                break;
            };
            let next_bytes = serde_json::to_vec(next)?.len();
            if !batch.is_empty() && bytes + next_bytes > self.max_batch_bytes {
                break;
            }
            let next = self.pending.pop_front().expect("front item exists");
            bytes = bytes.saturating_add(next_bytes);
            batch.push(next);
        }

        Ok(batch)
    }
}

#[cfg(test)]
fn ccr_v2_client_event(msg: &SDKMessage) -> Result<CcrV2ClientEvent> {
    let payload = normalized_ccr_v2_payload(msg)?;
    Ok(ccr_v2_client_event_from_payload(payload))
}

fn ccr_v2_client_event_from_payload(mut payload: Value) -> CcrV2ClientEvent {
    ensure_ccr_v2_payload_uuid(&mut payload);
    if is_ccr_v2_stream_event_payload(&payload) {
        CcrV2ClientEvent::ephemeral(payload)
    } else {
        CcrV2ClientEvent::new(payload)
    }
}

fn normalized_ccr_v2_payload(msg: &SDKMessage) -> Result<serde_json::Value> {
    let mut payload = serde_json::to_value(msg)?;
    ensure_ccr_v2_payload_uuid(&mut payload);
    Ok(payload)
}

fn ccr_v2_internal_transcript_event(msg: &SDKMessage) -> Result<Option<CcrV2WorkerEvent>> {
    let (role, uuid, message) = match msg {
        SDKMessage::User { uuid, message } => ("user", uuid, message),
        SDKMessage::Assistant { uuid, message } => ("assistant", uuid, message),
        _ => return Ok(None),
    };
    let uuid = if uuid.trim().is_empty() {
        Uuid::new_v4().to_string()
    } else {
        uuid.clone()
    };

    Ok(Some(CcrV2WorkerEvent {
        payload: json!({
            "type": "transcript",
            "uuid": uuid,
            "message": {
                "role": role,
                "content": serde_json::to_value(&message.content)?,
            },
        }),
        is_compaction: None,
        agent_id: None,
    }))
}

async fn persist_internal_transcript_event(client: CcrV2WorkerClient, msg: &SDKMessage) {
    let event = match ccr_v2_internal_transcript_event(msg) {
        Ok(Some(event)) => event,
        Ok(None) => return,
        Err(error) => {
            eprintln!("CCR v2 internal transcript serialization failed: {error}");
            return;
        }
    };

    if let Err(error) = client.write_internal_events(vec![event]).await {
        eprintln!("CCR v2 internal transcript persistence failed: {error}");
    }
}

fn ensure_ccr_v2_payload_uuid(payload: &mut Value) {
    let Some(object) = payload.as_object_mut() else {
        return;
    };
    let has_uuid = object
        .get("uuid")
        .and_then(|value| value.as_str())
        .is_some();
    if !has_uuid {
        object.insert(
            "uuid".to_string(),
            serde_json::Value::String(Uuid::new_v4().to_string()),
        );
    }
}

fn is_ccr_v2_stream_event_payload(payload: &Value) -> bool {
    payload.get("type").and_then(Value::as_str) == Some("stream_event")
}

fn ccr_v2_worker_state_for_outbound_message(
    msg: &SDKMessage,
) -> Option<(&'static str, Option<Value>)> {
    match msg {
        SDKMessage::ControlRequest {
            request_id,
            request:
                crate::types::ControlRequestType::CanUseTool {
                    tool_name, input, ..
                },
        } => Some((
            "requires_action",
            Some(ccr_v2_requires_action_details(request_id, tool_name, input)),
        )),
        SDKMessage::ControlResponse { .. } | SDKMessage::ControlCancelRequest { .. } => {
            Some(("running", None))
        }
        SDKMessage::Result { .. } => Some(("idle", None)),
        _ => None,
    }
}

fn ccr_v2_requires_action_details(
    request_id: &str,
    tool_name: &str,
    input: &HashMap<String, Value>,
) -> Value {
    json!({
        "tool_name": tool_name,
        "action_description": ccr_v2_action_description(tool_name, input),
        "request_id": request_id,
    })
}

fn ccr_v2_action_description(tool_name: &str, input: &HashMap<String, Value>) -> String {
    let verb = match tool_name {
        "Read" | "FileReadTool" => "Reading",
        "Write" | "FileWriteTool" => "Writing",
        "Edit" | "MultiEdit" | "FileEditTool" => "Editing",
        "Bash" | "BashTool" => "Running",
        "Glob" | "GlobTool" | "Grep" | "GrepTool" => "Searching",
        "WebFetch" => "Fetching",
        "WebSearch" => "Searching",
        "Task" => "Running task",
        "NotebookEditTool" => "Editing notebook",
        "LSP" => "LSP",
        other => other,
    };
    let target = ccr_v2_action_target(input);
    if target.is_empty() {
        verb.to_string()
    } else {
        format!("{verb} {target}")
    }
}

fn ccr_v2_action_target(input: &HashMap<String, Value>) -> String {
    for key in [
        "file_path",
        "filePath",
        "pattern",
        "command",
        "url",
        "query",
    ] {
        if let Some(value) = input.get(key).and_then(Value::as_str) {
            return if key == "command" {
                value.chars().take(60).collect()
            } else {
                value.to_string()
            };
        }
    }
    String::new()
}

fn sdk_message_uuid(msg: &SDKMessage) -> Option<&str> {
    match msg {
        SDKMessage::User { uuid, .. } | SDKMessage::Assistant { uuid, .. } => Some(uuid.as_str()),
        _ => None,
    }
}

#[derive(Debug)]
struct BoundedUuidSet {
    capacity: usize,
    ring: VecDeque<String>,
    set: HashSet<String>,
}

impl BoundedUuidSet {
    fn new(capacity: usize) -> Self {
        let capacity = capacity.max(1);
        Self {
            capacity,
            ring: VecDeque::with_capacity(capacity),
            set: HashSet::with_capacity(capacity),
        }
    }

    fn add(&mut self, uuid: impl Into<String>) {
        let uuid = uuid.into();
        if self.set.contains(&uuid) {
            return;
        }
        if self.ring.len() >= self.capacity {
            if let Some(evicted) = self.ring.pop_front() {
                self.set.remove(&evicted);
            }
        }
        self.set.insert(uuid.clone());
        self.ring.push_back(uuid);
    }

    fn has(&self, uuid: &str) -> bool {
        self.set.contains(uuid)
    }
}

#[derive(Debug, Default)]
struct StreamEventAccumulator {
    buffer: Vec<Value>,
    first_buffered_at: Option<Instant>,
    by_message: HashMap<String, StreamMessageState>,
    scope_to_message: HashMap<String, String>,
}

#[derive(Debug, Default)]
struct StreamMessageState {
    blocks: BTreeMap<usize, String>,
}

impl StreamEventAccumulator {
    fn push(&mut self, payload: Value) {
        if self.buffer.is_empty() {
            self.first_buffered_at = Some(Instant::now());
        }
        self.buffer.push(payload);
    }

    fn should_flush(&self, now: Instant) -> bool {
        self.first_buffered_at
            .map(|started| now.duration_since(started) >= STREAM_EVENT_FLUSH_INTERVAL)
            .unwrap_or(false)
    }

    fn flush(&mut self) -> Vec<Value> {
        self.first_buffered_at = None;
        let buffer = std::mem::take(&mut self.buffer);
        self.accumulate(buffer)
    }

    fn accumulate(&mut self, buffer: Vec<Value>) -> Vec<Value> {
        let mut out = Vec::new();
        let mut touched: HashMap<(String, usize), usize> = HashMap::new();

        for mut payload in buffer {
            match stream_event_type(&payload) {
                Some("message_start") => {
                    if let Some((scope, message_id)) = stream_event_message_start(&payload) {
                        if let Some(previous_id) =
                            self.scope_to_message.insert(scope, message_id.clone())
                        {
                            self.by_message.remove(&previous_id);
                        }
                        self.by_message
                            .insert(message_id, StreamMessageState::default());
                    }
                    out.push(payload);
                }
                Some("content_block_delta") if is_text_delta_stream_event(&payload) => {
                    let Some(scope) = payload_scope_key(&payload) else {
                        out.push(payload);
                        continue;
                    };
                    let Some(message_id) = self.scope_to_message.get(&scope).cloned() else {
                        out.push(payload);
                        continue;
                    };
                    let Some(index) = stream_event_index(&payload) else {
                        out.push(payload);
                        continue;
                    };
                    let Some(text) = stream_event_text_delta(&payload).map(str::to_string) else {
                        out.push(payload);
                        continue;
                    };
                    let Some(message_state) = self.by_message.get_mut(&message_id) else {
                        out.push(payload);
                        continue;
                    };

                    let full_text = {
                        let block_text = message_state.blocks.entry(index).or_default();
                        block_text.push_str(&text);
                        block_text.clone()
                    };
                    let touch_key = (message_id, index);
                    if let Some(out_index) = touched.get(&touch_key).copied() {
                        set_stream_event_text_delta(&mut out[out_index], full_text);
                    } else {
                        set_stream_event_text_delta(&mut payload, full_text);
                        touched.insert(touch_key, out.len());
                        out.push(payload);
                    }
                }
                _ => out.push(payload),
            }
        }

        out
    }

    fn clear_for_completed_assistant_payload(&mut self, payload: &Value) {
        if payload.get("type").and_then(Value::as_str) != Some("assistant") {
            return;
        }
        let Some(message_id) = payload
            .get("message")
            .and_then(|message| message.get("id"))
            .and_then(Value::as_str)
        else {
            return;
        };
        self.by_message.remove(message_id);
        if let Some(scope) = payload_scope_key(payload) {
            if self.scope_to_message.get(&scope).map(String::as_str) == Some(message_id) {
                self.scope_to_message.remove(&scope);
            }
        } else {
            self.scope_to_message
                .retain(|_, active_message_id| active_message_id != message_id);
        }
    }
}

fn stream_event_type(payload: &Value) -> Option<&str> {
    payload
        .get("event")
        .and_then(|event| event.get("type"))
        .and_then(Value::as_str)
}

fn stream_event_message_start(payload: &Value) -> Option<(String, String)> {
    let scope = payload_scope_key(payload)?;
    let message_id = payload
        .get("event")?
        .get("message")?
        .get("id")?
        .as_str()?
        .to_string();
    Some((scope, message_id))
}

fn payload_scope_key(payload: &Value) -> Option<String> {
    let session_id = payload.get("session_id")?.as_str()?;
    let parent_tool_use_id = match payload.get("parent_tool_use_id") {
        Some(Value::String(value)) => value.as_str(),
        Some(Value::Null) | None => "",
        _ => return None,
    };
    Some(format!("{session_id}:{parent_tool_use_id}"))
}

fn is_text_delta_stream_event(payload: &Value) -> bool {
    payload
        .get("event")
        .and_then(|event| event.get("delta"))
        .and_then(|delta| delta.get("type"))
        .and_then(Value::as_str)
        == Some("text_delta")
}

fn stream_event_index(payload: &Value) -> Option<usize> {
    payload
        .get("event")?
        .get("index")?
        .as_u64()
        .and_then(|index| usize::try_from(index).ok())
}

fn stream_event_text_delta(payload: &Value) -> Option<&str> {
    payload.get("event")?.get("delta")?.get("text")?.as_str()
}

fn set_stream_event_text_delta(payload: &mut Value, text: String) {
    if let Some(delta) = payload
        .get_mut("event")
        .and_then(|event| event.get_mut("delta"))
        .and_then(Value::as_object_mut)
    {
        delta.insert("text".to_string(), Value::String(text));
    }
}

pub fn build_sdk_url(api_base_url: &str, session_id: &str) -> Result<String> {
    let base = api_base_url.trim();
    if base.is_empty() {
        return Err(anyhow!("bridge session ingress URL cannot be empty"));
    }

    let mut url = url::Url::parse(base)
        .with_context(|| format!("invalid bridge session ingress URL: {api_base_url}"))?;
    let scheme = match url.scheme() {
        "http" => "ws",
        "https" => "wss",
        "ws" => "ws",
        "wss" => "wss",
        other => {
            return Err(anyhow!(
                "bridge session ingress URL must start with http://, https://, ws://, or wss://, got {other}://"
            ));
        }
    };
    url.set_scheme(scheme)
        .map_err(|_| anyhow!("failed to set bridge session ingress WebSocket scheme"))?;

    let is_localhost = matches!(
        url.host_str(),
        Some("localhost") | Some("127.0.0.1") | Some("::1")
    );
    let version = if is_localhost { "v2" } else { "v1" };
    let existing_segments = url
        .path_segments()
        .map(|segments| {
            segments
                .filter(|segment| !segment.is_empty())
                .map(str::to_string)
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    {
        let mut segments = url
            .path_segments_mut()
            .map_err(|_| anyhow!("bridge session ingress URL cannot be a base URL"))?;
        segments.clear();
        for segment in existing_segments {
            segments.push(&segment);
        }
        segments.push(version);
        segments.push("session_ingress");
        segments.push("ws");
        segments.push(session_id);
    }
    url.set_query(None);
    url.set_fragment(None);
    Ok(url.to_string())
}

#[cfg(test)]
mod tests {
    use super::{
        build_sdk_url, ccr_v2_client_event, ccr_v2_client_event_from_payload,
        ccr_v2_internal_transcript_event, CcrV2Transport, ClientEventUploader,
        DeliveryUpdateUploader, StreamEventAccumulator, Transport,
    };
    use crate::types::{
        ContentBlock, ControlRequestType, ControlResponseType, MessageContent, SDKMessage,
    };
    use futures_util::{SinkExt, StreamExt};
    use kiana_remote::{
        CcrV2ClientEvent, CcrV2DeliveryStatus, CcrV2DeliveryUpdate, CcrV2ReconnectPolicy,
        CcrV2WorkerClient,
    };
    use serde_json::{json, Value};
    use std::collections::{HashMap, VecDeque};
    use std::sync::{Arc, Mutex};
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::TcpListener;
    use tokio::time::{timeout, Duration};
    use tokio_tungstenite::{accept_async, tungstenite::Message};

    #[derive(Debug, Clone)]
    struct RecordedHttpRequest {
        method: String,
        path: String,
        body: String,
    }

    #[derive(Debug)]
    struct MockHttpResponse {
        status: u16,
        content_type: &'static str,
        body: String,
    }

    #[test]
    fn sdk_url_accepts_http_https_and_ws_bases() {
        assert_eq!(
            build_sdk_url("https://api.anthropic.com", "session-1").unwrap(),
            "wss://api.anthropic.com/v1/session_ingress/ws/session-1"
        );
        assert_eq!(
            build_sdk_url("http://localhost:8080", "session-1").unwrap(),
            "ws://localhost:8080/v2/session_ingress/ws/session-1"
        );
        assert_eq!(
            build_sdk_url("wss://ingress.example.test/proxy/", "session-1").unwrap(),
            "wss://ingress.example.test/proxy/v1/session_ingress/ws/session-1"
        );
        assert_eq!(
            build_sdk_url("ws://127.0.0.1:9000/base?ignored=true", "session/1").unwrap(),
            "ws://127.0.0.1:9000/base/v2/session_ingress/ws/session%2F1"
        );
    }

    #[test]
    fn sdk_url_rejects_empty_or_unsupported_bases() {
        let empty = build_sdk_url("", "session-1").unwrap_err().to_string();
        assert!(empty.contains("cannot be empty"));
        let invalid = build_sdk_url("ftp://localhost", "session-1")
            .unwrap_err()
            .to_string();
        assert!(invalid.contains("must start with"));
    }

    #[tokio::test]
    async fn session_ingress_transport_ignores_echo_and_duplicate_inbound_user_messages() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let server = tokio::spawn(async move {
            let (stream, _) = listener.accept().await.unwrap();
            let mut websocket = accept_async(stream).await.unwrap();
            let outbound = match websocket.next().await {
                Some(Ok(Message::Text(text))) => text,
                other => panic!("expected outbound text message, got {other:?}"),
            };
            websocket.send(Message::Text(outbound)).await.unwrap();
            let inbound = serde_json::json!({
                "type": "user",
                "uuid": "inbound-1",
                "message": {"content": "run once"}
            })
            .to_string();
            websocket
                .send(Message::Text(inbound.clone().into()))
                .await
                .unwrap();
            websocket.send(Message::Text(inbound.into())).await.unwrap();
            websocket.close(None).await.unwrap();
        });

        let transport = Transport::new(format!("ws://{addr}"), "token".to_string());
        let mut handle = transport.connect().await.unwrap();
        handle
            .send(&SDKMessage::User {
                uuid: "outbound-1".to_string(),
                message: MessageContent {
                    content: ContentBlock::Text("already sent".to_string()),
                },
            })
            .await
            .unwrap();

        let delivered = timeout(Duration::from_secs(1), handle.recv())
            .await
            .unwrap()
            .unwrap()
            .expect("inbound user should be delivered");
        match delivered {
            SDKMessage::User { uuid, .. } => assert_eq!(uuid, "inbound-1"),
            other => panic!("expected inbound user, got {other:?}"),
        }

        let duplicate = timeout(Duration::from_secs(1), handle.recv())
            .await
            .unwrap()
            .unwrap();
        assert!(
            duplicate.is_none(),
            "duplicate inbound user should be dropped, got {duplicate:?}"
        );
        server.await.unwrap();
    }

    #[test]
    fn ccr_v2_client_event_preserves_existing_uuid() {
        let event = ccr_v2_client_event(&SDKMessage::Assistant {
            uuid: "assistant-1".to_string(),
            message: MessageContent {
                content: ContentBlock::Text("done".to_string()),
            },
        })
        .unwrap();

        assert_eq!(event.payload["type"], json!("assistant"));
        assert_eq!(event.payload["uuid"], json!("assistant-1"));
        assert_eq!(event.ephemeral, None);
    }

    #[test]
    fn ccr_v2_client_event_injects_uuid_for_payload_without_one() {
        let event = ccr_v2_client_event(&SDKMessage::Result {
            data: HashMap::from([("subtype".to_string(), json!("success"))]),
        })
        .unwrap();

        assert_eq!(event.payload["type"], json!("result"));
        assert_eq!(event.payload["subtype"], json!("success"));
        let uuid = event.payload["uuid"].as_str().unwrap();
        assert!(!uuid.is_empty());
        assert!(uuid::Uuid::parse_str(uuid).is_ok());
        assert_eq!(event.ephemeral, None);
    }

    #[test]
    fn ccr_v2_client_event_marks_stream_events_ephemeral() {
        let event = ccr_v2_client_event(&SDKMessage::StreamEvent {
            data: HashMap::from([(
                "event".to_string(),
                json!({"type": "content_block_delta", "delta": {"text": "partial"}}),
            )]),
        })
        .unwrap();

        assert_eq!(event.payload["type"], json!("stream_event"));
        assert_eq!(
            event.payload["event"]["type"],
            Value::String("content_block_delta".to_string())
        );
        assert_eq!(event.ephemeral, Some(true));
        let uuid = event.payload["uuid"].as_str().unwrap();
        assert!(uuid::Uuid::parse_str(uuid).is_ok());
    }

    #[tokio::test]
    async fn ccr_v2_transport_reports_worker_state_for_permission_lifecycle() {
        let (base_url, requests) = spawn_ccr_v2_mock_worker_server(vec![
            MockHttpResponse::json(200, r#"{}"#),
            MockHttpResponse::sse(200, ""),
            MockHttpResponse::json(200, r#"{}"#),
            MockHttpResponse::json(200, r#"{}"#),
            MockHttpResponse::json(200, r#"{}"#),
            MockHttpResponse::json(200, r#"{}"#),
            MockHttpResponse::json(200, r#"{}"#),
            MockHttpResponse::json(200, r#"{}"#),
            MockHttpResponse::json(200, r#"{}"#),
            MockHttpResponse::json(200, r#"{}"#),
        ])
        .await;
        let client = CcrV2WorkerClient::new(
            format!("{base_url}/v1/code/sessions/cse_session_1"),
            "cse_session_1",
            "worker-token",
            42,
        )
        .unwrap();
        let transport =
            CcrV2Transport::new_with_reconnect_policy(client, CcrV2ReconnectPolicy::disabled());
        let mut handle = transport.connect().await.unwrap();

        handle
            .send(&SDKMessage::ControlRequest {
                request_id: "perm-1".to_string(),
                request: ControlRequestType::CanUseTool {
                    tool_name: "Bash".to_string(),
                    input: HashMap::from([("command".to_string(), json!("git status"))]),
                    tool_use_id: "toolu_1".to_string(),
                },
            })
            .await
            .unwrap();
        handle
            .send(&SDKMessage::ControlResponse {
                response: ControlResponseType::Success {
                    request_id: "perm-1".to_string(),
                    response: Some(HashMap::from([("allowed".to_string(), json!(true))])),
                },
            })
            .await
            .unwrap();
        handle
            .send(&SDKMessage::ControlCancelRequest {
                request_id: "perm-2".to_string(),
                tool_use_id: Some("toolu_2".to_string()),
            })
            .await
            .unwrap();
        handle
            .send(&SDKMessage::Result {
                data: HashMap::from([("subtype".to_string(), json!("success"))]),
            })
            .await
            .unwrap();

        let requests = requests.lock().unwrap().clone();
        let state_updates = requests
            .iter()
            .filter(|request| {
                request.method == "PUT" && request.path == "/v1/code/sessions/cse_session_1/worker"
            })
            .map(|request| serde_json::from_str::<Value>(&request.body).unwrap())
            .collect::<Vec<_>>();
        assert_eq!(state_updates.len(), 4);
        assert_eq!(state_updates[0]["worker_status"], json!("idle"));
        assert_eq!(state_updates[1]["worker_status"], json!("requires_action"));
        assert_eq!(
            state_updates[1]["requires_action_details"]["request_id"],
            json!("perm-1")
        );
        assert_eq!(
            state_updates[1]["requires_action_details"]["tool_name"],
            json!("Bash")
        );
        assert_eq!(state_updates[2]["worker_status"], json!("running"));
        assert_eq!(state_updates[3]["worker_status"], json!("idle"));
    }

    #[tokio::test]
    async fn ccr_v2_transport_reports_running_when_receiving_user_event() {
        let user_event = json!({
            "event_id": "evt-user-1",
            "sequence_num": 1,
            "event_type": "user",
            "source": "frontend",
            "payload": {
                "type": "user",
                "uuid": "user-1",
                "message": {
                    "content": "run tests"
                }
            },
            "created_at": "2026-06-17T00:00:00Z"
        });
        let sse_body = format!("id: 1\nevent: client_event\ndata: {}\n\n", user_event);
        let (base_url, requests) = spawn_ccr_v2_mock_worker_server(vec![
            MockHttpResponse::json(200, r#"{}"#),
            MockHttpResponse::sse(200, &sse_body),
            MockHttpResponse::json(200, r#"{}"#),
            MockHttpResponse::json(200, r#"{}"#),
            MockHttpResponse::json(200, r#"{}"#),
        ])
        .await;
        let client = CcrV2WorkerClient::new(
            format!("{base_url}/v1/code/sessions/cse_session_1"),
            "cse_session_1",
            "worker-token",
            42,
        )
        .unwrap();
        let transport =
            CcrV2Transport::new_with_reconnect_policy(client, CcrV2ReconnectPolicy::disabled());
        let mut handle = transport.connect().await.unwrap();

        let received = handle.recv().await.unwrap().expect("expected user event");
        match received {
            SDKMessage::User { uuid, message } => {
                assert_eq!(uuid, "user-1");
                assert_eq!(
                    serde_json::to_value(message.content).unwrap(),
                    json!("run tests")
                );
            }
            other => panic!("expected user event, got {other:?}"),
        }

        let requests = requests.lock().unwrap().clone();
        let state_updates = requests
            .iter()
            .filter(|request| {
                request.method == "PUT" && request.path == "/v1/code/sessions/cse_session_1/worker"
            })
            .map(|request| serde_json::from_str::<Value>(&request.body).unwrap())
            .collect::<Vec<_>>();
        assert_eq!(state_updates.len(), 2);
        assert_eq!(state_updates[0]["worker_status"], json!("idle"));
        assert_eq!(state_updates[1]["worker_status"], json!("running"));
    }

    #[test]
    fn ccr_v2_internal_transcript_event_converts_user_for_local_hydrate() {
        let event = ccr_v2_internal_transcript_event(&SDKMessage::User {
            uuid: "user-1".to_string(),
            message: MessageContent {
                content: ContentBlock::Text("restore this task".to_string()),
            },
        })
        .unwrap()
        .expect("user messages should be persisted");

        assert_eq!(event.payload["type"], json!("transcript"));
        assert_eq!(event.payload["uuid"], json!("user-1"));
        assert_eq!(event.payload["message"]["role"], json!("user"));
        assert_eq!(
            event.payload["message"]["content"],
            json!("restore this task")
        );
        assert_eq!(event.is_compaction, None);
        assert_eq!(event.agent_id, None);
    }

    #[test]
    fn ccr_v2_internal_transcript_event_converts_assistant_blocks_for_local_hydrate() {
        let event = ccr_v2_internal_transcript_event(&SDKMessage::Assistant {
            uuid: "assistant-1".to_string(),
            message: MessageContent {
                content: ContentBlock::Blocks(vec![HashMap::from([
                    ("type".to_string(), json!("text")),
                    ("text".to_string(), json!("restored answer")),
                ])]),
            },
        })
        .unwrap()
        .expect("assistant messages should be persisted");

        assert_eq!(event.payload["type"], json!("transcript"));
        assert_eq!(event.payload["uuid"], json!("assistant-1"));
        assert_eq!(event.payload["message"]["role"], json!("assistant"));
        assert_eq!(
            event.payload["message"]["content"][0]["text"],
            json!("restored answer")
        );
    }

    #[test]
    fn ccr_v2_internal_transcript_event_ignores_non_transcript_messages() {
        let event = ccr_v2_internal_transcript_event(&SDKMessage::Result {
            data: HashMap::from([("subtype".to_string(), json!("success"))]),
        })
        .unwrap();

        assert_eq!(event, None);
    }

    #[test]
    fn client_event_uploader_splits_batches_by_count() {
        let mut uploader = ClientEventUploader::with_limits(2, 10_000, 10);
        uploader
            .enqueue((0..5).map(|index| test_client_event(&format!("event-{index}"), "x")))
            .unwrap();

        let first = uploader.take_batch().unwrap();
        let second = uploader.take_batch().unwrap();
        let third = uploader.take_batch().unwrap();

        assert_eq!(
            first
                .iter()
                .map(|event| event.payload["uuid"].as_str().unwrap())
                .collect::<Vec<_>>(),
            ["event-0", "event-1"]
        );
        assert_eq!(
            second
                .iter()
                .map(|event| event.payload["uuid"].as_str().unwrap())
                .collect::<Vec<_>>(),
            ["event-2", "event-3"]
        );
        assert_eq!(third[0].payload["uuid"], json!("event-4"));
    }

    #[test]
    fn client_event_uploader_splits_batches_by_serialized_bytes() {
        let first = test_client_event("small-1", "abc");
        let second = test_client_event("small-2", "def");
        let byte_limit = serde_json::to_vec(&first).unwrap().len()
            + serde_json::to_vec(&second).unwrap().len()
            - 1;
        let mut uploader = ClientEventUploader::with_limits(10, byte_limit, 10);
        uploader
            .enqueue([
                first.clone(),
                second.clone(),
                test_client_event("small-3", "ghi"),
            ])
            .unwrap();

        let first_batch = uploader.take_batch().unwrap();
        let second_batch = uploader.take_batch().unwrap();

        assert_eq!(first_batch.len(), 1);
        assert_eq!(first_batch[0].payload["uuid"], first.payload["uuid"]);
        assert_eq!(second_batch.len(), 1);
        assert_eq!(second_batch[0].payload["uuid"], second.payload["uuid"]);
    }

    #[test]
    fn client_event_uploader_reports_backpressure_when_queue_limit_is_exceeded() {
        let mut uploader = ClientEventUploader::with_limits(10, 10_000, 2);
        uploader
            .enqueue([
                test_client_event("event-1", "x"),
                test_client_event("event-2", "x"),
            ])
            .unwrap();

        let error = uploader
            .enqueue([test_client_event("event-3", "x")])
            .unwrap_err()
            .to_string();

        assert!(error.contains("backpressure exceeded"));
        assert!(error.contains("pending=2"));
        assert!(error.contains("max=2"));
    }

    #[test]
    fn delivery_update_uploader_splits_batches_by_count() {
        let mut uploader = DeliveryUpdateUploader::with_limits(3, 10);
        uploader
            .enqueue((0..7).map(|index| test_delivery_update(&format!("evt-{index}"))))
            .unwrap();

        let first = uploader.take_batch();
        let second = uploader.take_batch();
        let third = uploader.take_batch();

        assert_eq!(
            first
                .iter()
                .map(|update| update.event_id.as_str())
                .collect::<Vec<_>>(),
            ["evt-0", "evt-1", "evt-2"]
        );
        assert_eq!(
            second
                .iter()
                .map(|update| update.event_id.as_str())
                .collect::<Vec<_>>(),
            ["evt-3", "evt-4", "evt-5"]
        );
        assert_eq!(third[0].event_id, "evt-6");
    }

    #[test]
    fn delivery_update_uploader_reports_backpressure_when_queue_limit_is_exceeded() {
        let mut uploader = DeliveryUpdateUploader::with_limits(10, 2);
        uploader
            .enqueue([test_delivery_update("evt-1"), test_delivery_update("evt-2")])
            .unwrap();

        let error = uploader
            .enqueue([test_delivery_update("evt-3")])
            .unwrap_err()
            .to_string();

        assert!(error.contains("backpressure exceeded"));
        assert!(error.contains("pending=2"));
        assert!(error.contains("max=2"));
    }

    #[test]
    fn stream_event_accumulator_coalesces_text_deltas_into_one_snapshot_per_flush() {
        let mut accumulator = StreamEventAccumulator::default();
        accumulator.push(stream_payload(
            "start-1",
            json!({
                "type": "message_start",
                "message": {"id": "msg_1", "role": "assistant"}
            }),
        ));
        accumulator.push(stream_payload(
            "delta-1",
            json!({
                "type": "content_block_delta",
                "index": 0,
                "delta": {"type": "text_delta", "text": "hello "}
            }),
        ));
        accumulator.push(stream_payload(
            "delta-2",
            json!({
                "type": "content_block_delta",
                "index": 0,
                "delta": {"type": "text_delta", "text": "world"}
            }),
        ));

        let events = accumulator.flush();

        assert_eq!(events.len(), 2);
        assert_eq!(events[0]["event"]["type"], json!("message_start"));
        assert_eq!(events[1]["uuid"], json!("delta-1"));
        assert_eq!(events[1]["event"]["type"], json!("content_block_delta"));
        assert_eq!(events[1]["event"]["delta"]["text"], json!("hello world"));
        let client_event = ccr_v2_client_event_from_payload(events[1].clone());
        assert_eq!(client_event.ephemeral, Some(true));
    }

    #[test]
    fn stream_event_accumulator_emits_full_text_on_later_flush() {
        let mut accumulator = StreamEventAccumulator::default();
        accumulator.push(stream_payload(
            "start-1",
            json!({
                "type": "message_start",
                "message": {"id": "msg_1", "role": "assistant"}
            }),
        ));
        accumulator.push(stream_payload(
            "delta-1",
            json!({
                "type": "content_block_delta",
                "index": 0,
                "delta": {"type": "text_delta", "text": "hello "}
            }),
        ));
        let first = accumulator.flush();
        assert_eq!(first.len(), 2);
        assert_eq!(first[1]["event"]["delta"]["text"], json!("hello "));

        accumulator.push(stream_payload(
            "delta-2",
            json!({
                "type": "content_block_delta",
                "index": 0,
                "delta": {"type": "text_delta", "text": "world"}
            }),
        ));
        let second = accumulator.flush();

        assert_eq!(second.len(), 1);
        assert_eq!(second[0]["uuid"], json!("delta-2"));
        assert_eq!(second[0]["event"]["delta"]["text"], json!("hello world"));
    }

    #[test]
    fn stream_event_accumulator_passes_non_text_delta_through() {
        let mut accumulator = StreamEventAccumulator::default();
        let raw = stream_payload(
            "json-1",
            json!({
                "type": "content_block_delta",
                "index": 0,
                "delta": {"type": "input_json_delta", "partial_json": "{\"a\""}
            }),
        );

        accumulator.push(raw.clone());
        let events = accumulator.flush();

        assert_eq!(events, vec![raw]);
    }

    #[test]
    fn stream_event_accumulator_passes_text_delta_without_message_start_through() {
        let mut accumulator = StreamEventAccumulator::default();
        let raw = stream_payload(
            "orphan-1",
            json!({
                "type": "content_block_delta",
                "index": 0,
                "delta": {"type": "text_delta", "text": "orphan"}
            }),
        );

        accumulator.push(raw.clone());
        let events = accumulator.flush();

        assert_eq!(events, vec![raw]);
    }

    #[test]
    fn stream_event_accumulator_clears_completed_assistant_message() {
        let mut accumulator = StreamEventAccumulator::default();
        accumulator.push(stream_payload(
            "start-1",
            json!({
                "type": "message_start",
                "message": {"id": "msg_1", "role": "assistant"}
            }),
        ));
        accumulator.push(stream_payload(
            "delta-1",
            json!({
                "type": "content_block_delta",
                "index": 0,
                "delta": {"type": "text_delta", "text": "hello "}
            }),
        ));
        let first = accumulator.flush();
        assert_eq!(first[1]["event"]["delta"]["text"], json!("hello "));

        accumulator.clear_for_completed_assistant_payload(&json!({
            "type": "assistant",
            "uuid": "assistant-1",
            "session_id": "session-1",
            "parent_tool_use_id": null,
            "message": {"id": "msg_1", "role": "assistant", "content": []}
        }));
        accumulator.push(stream_payload(
            "delta-2",
            json!({
                "type": "content_block_delta",
                "index": 0,
                "delta": {"type": "text_delta", "text": "world"}
            }),
        ));
        let second = accumulator.flush();

        assert_eq!(second.len(), 1);
        assert_eq!(second[0]["event"]["delta"]["text"], json!("world"));
    }

    fn stream_payload(uuid: &str, event: Value) -> Value {
        json!({
            "type": "stream_event",
            "uuid": uuid,
            "session_id": "session-1",
            "parent_tool_use_id": null,
            "event": event,
        })
    }

    fn test_client_event(uuid: &str, text: &str) -> CcrV2ClientEvent {
        CcrV2ClientEvent::new(json!({
            "type": "assistant",
            "uuid": uuid,
            "message": {
                "content": text
            }
        }))
    }

    fn test_delivery_update(event_id: &str) -> CcrV2DeliveryUpdate {
        CcrV2DeliveryUpdate {
            event_id: event_id.to_string(),
            status: CcrV2DeliveryStatus::Processed,
        }
    }

    async fn spawn_ccr_v2_mock_worker_server(
        responses: Vec<MockHttpResponse>,
    ) -> (String, Arc<Mutex<Vec<RecordedHttpRequest>>>) {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let requests = Arc::new(Mutex::new(Vec::new()));
        let shared_requests = requests.clone();
        let responses = Arc::new(Mutex::new(VecDeque::from(responses)));
        let shared_responses = responses.clone();

        tokio::spawn(async move {
            loop {
                let response = {
                    let mut responses = shared_responses.lock().unwrap();
                    responses.pop_front()
                };
                let Some(response) = response else {
                    break;
                };
                let Ok((mut stream, _)) = listener.accept().await else {
                    break;
                };
                let Ok(request) = read_http_request(&mut stream).await else {
                    break;
                };
                shared_requests.lock().unwrap().push(request);
                let _ = write_http_response(&mut stream, response).await;
            }
        });

        (format!("http://{}", address), requests)
    }

    async fn read_http_request(
        stream: &mut tokio::net::TcpStream,
    ) -> std::io::Result<RecordedHttpRequest> {
        let mut buffer = Vec::new();
        let mut temp = [0_u8; 1024];
        let mut header_end = None;
        loop {
            let n = stream.read(&mut temp).await?;
            if n == 0 {
                break;
            }
            buffer.extend_from_slice(&temp[..n]);
            if let Some(pos) = find_header_end(&buffer) {
                header_end = Some(pos);
                break;
            }
        }
        let header_end = header_end.unwrap_or(buffer.len());
        let headers = String::from_utf8_lossy(&buffer[..header_end]).to_string();
        let mut lines = headers.lines();
        let request_line = lines.next().unwrap_or_default();
        let mut parts = request_line.split_whitespace();
        let method = parts.next().unwrap_or_default().to_string();
        let path = parts.next().unwrap_or_default().to_string();
        let content_length = headers
            .lines()
            .find_map(|line| {
                let (name, value) = line.split_once(':')?;
                name.eq_ignore_ascii_case("content-length")
                    .then(|| value.trim().parse::<usize>().ok())
                    .flatten()
            })
            .unwrap_or(0);
        let body_start = header_end.saturating_add(4);
        while buffer.len().saturating_sub(body_start) < content_length {
            let n = stream.read(&mut temp).await?;
            if n == 0 {
                break;
            }
            buffer.extend_from_slice(&temp[..n]);
        }
        let body_end = body_start.saturating_add(content_length).min(buffer.len());
        let body = String::from_utf8_lossy(&buffer[body_start..body_end]).to_string();

        Ok(RecordedHttpRequest { method, path, body })
    }

    fn find_header_end(buffer: &[u8]) -> Option<usize> {
        buffer.windows(4).position(|window| window == b"\r\n\r\n")
    }

    async fn write_http_response(
        stream: &mut tokio::net::TcpStream,
        response: MockHttpResponse,
    ) -> std::io::Result<()> {
        let payload = format!(
            "HTTP/1.1 {} OK\r\nContent-Type: {}\r\nConnection: close\r\nContent-Length: {}\r\n\r\n{}",
            response.status,
            response.content_type,
            response.body.len(),
            response.body
        );
        stream.write_all(payload.as_bytes()).await?;
        stream.flush().await
    }

    impl MockHttpResponse {
        fn json(status: u16, body: &str) -> Self {
            Self {
                status,
                content_type: "application/json",
                body: body.to_string(),
            }
        }

        fn sse(status: u16, body: &str) -> Self {
            Self {
                status,
                content_type: "text/event-stream",
                body: body.to_string(),
            }
        }
    }
}

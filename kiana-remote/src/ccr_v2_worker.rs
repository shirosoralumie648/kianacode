use bytes::Bytes;
use futures_util::{stream::BoxStream, StreamExt};
use reqwest::{Method, StatusCode};
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::VecDeque;
use std::sync::{Arc, RwLock};
use std::time::{Duration, Instant};
use thiserror::Error;

const ANTHROPIC_VERSION: &str = "2023-06-01";
const WORKER_API_TIMEOUT: Duration = Duration::from_secs(15);
const INTERNAL_EVENT_MAX_BATCH_SIZE: usize = 100;
const INTERNAL_EVENT_MAX_BATCH_BYTES: usize = 10 * 1024 * 1024;
const INTERNAL_EVENT_MAX_QUEUE_SIZE: usize = 200;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CcrV2RequestRetryPolicy {
    pub max_attempts: usize,
    pub base_delay: Duration,
    pub max_delay: Duration,
}

impl CcrV2RequestRetryPolicy {
    pub fn new(max_attempts: usize, base_delay: Duration, max_delay: Duration) -> Self {
        Self {
            max_attempts: max_attempts.max(1),
            base_delay,
            max_delay,
        }
    }

    pub fn no_delay(max_attempts: usize) -> Self {
        Self::new(max_attempts, Duration::ZERO, Duration::ZERO)
    }

    fn delay_for_attempt(&self, attempt: usize, retry_after: Option<Duration>) -> Duration {
        if let Some(retry_after) = retry_after {
            return if self.max_delay.is_zero() {
                retry_after
            } else {
                retry_after.min(self.max_delay)
            };
        }
        if self.base_delay.is_zero() || self.max_delay.is_zero() {
            return Duration::ZERO;
        }

        let mut delay = self.base_delay;
        for _ in 1..attempt {
            delay = delay.saturating_mul(2);
            if delay >= self.max_delay {
                return self.max_delay;
            }
        }
        delay.min(self.max_delay)
    }
}

impl Default for CcrV2RequestRetryPolicy {
    fn default() -> Self {
        Self::new(10, Duration::from_millis(500), Duration::from_secs(8))
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct CcrV2ClientEvent {
    pub payload: Value,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ephemeral: Option<bool>,
}

impl CcrV2ClientEvent {
    pub fn new(payload: Value) -> Self {
        Self {
            payload,
            ephemeral: None,
        }
    }

    pub fn ephemeral(payload: Value) -> Self {
        Self {
            payload,
            ephemeral: Some(true),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct CcrV2WorkerEvent {
    pub payload: Value,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub is_compaction: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub agent_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct CcrV2DeliveryUpdate {
    pub event_id: String,
    pub status: CcrV2DeliveryStatus,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum CcrV2DeliveryStatus {
    Received,
    Processing,
    Processed,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct CcrV2StreamClientEvent {
    pub event_id: String,
    pub sequence_num: u64,
    pub event_type: String,
    pub source: String,
    pub payload: Value,
    pub created_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct CcrV2InternalEvent {
    pub event_id: String,
    pub event_type: String,
    pub payload: Value,
    #[serde(default)]
    pub event_metadata: Option<Value>,
    pub is_compaction: bool,
    pub created_at: String,
    #[serde(default)]
    pub agent_id: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
struct ListInternalEventsResponse {
    #[serde(default)]
    data: Vec<CcrV2InternalEvent>,
    #[serde(default)]
    next_cursor: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CcrV2SseFrame {
    pub event: Option<String>,
    pub id: Option<String>,
    pub data: Option<String>,
    pub is_comment: bool,
}

#[derive(Debug, Error)]
pub enum CcrV2WorkerError {
    #[error("{0}")]
    Message(String),
    #[error("{action} failed: {status}")]
    HttpStatus {
        action: String,
        status: StatusCode,
        message: Option<String>,
    },
    #[error("ccr v2 worker request failed: {0}")]
    Request(#[from] reqwest::Error),
    #[error("ccr v2 worker json parse failed: {0}")]
    Json(#[from] serde_json::Error),
    #[error("ccr v2 worker epoch superseded")]
    EpochMismatch,
}

impl CcrV2WorkerError {
    fn http_status(&self) -> Option<StatusCode> {
        match self {
            Self::HttpStatus { status, .. } => Some(*status),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CcrV2ReconnectPolicy {
    pub base_delay: Duration,
    pub max_delay: Duration,
    pub give_up_after: Duration,
    pub liveness_timeout: Option<Duration>,
}

impl CcrV2ReconnectPolicy {
    pub fn new(base_delay: Duration, max_delay: Duration, give_up_after: Duration) -> Self {
        Self {
            base_delay,
            max_delay,
            give_up_after,
            liveness_timeout: Some(Duration::from_secs(45)),
        }
    }

    pub fn disabled() -> Self {
        Self::new(Duration::ZERO, Duration::ZERO, Duration::ZERO).with_liveness_timeout(None)
    }

    pub fn with_liveness_timeout(mut self, liveness_timeout: Option<Duration>) -> Self {
        self.liveness_timeout = liveness_timeout;
        self
    }

    fn delay_for_attempt(&self, attempt: u32) -> Duration {
        if self.base_delay.is_zero() || self.max_delay.is_zero() {
            return Duration::ZERO;
        }

        let mut delay = self.base_delay;
        for _ in 1..attempt {
            delay = delay.saturating_mul(2);
            if delay >= self.max_delay {
                return self.max_delay;
            }
        }
        delay.min(self.max_delay)
    }
}

impl Default for CcrV2ReconnectPolicy {
    fn default() -> Self {
        Self {
            base_delay: Duration::from_secs(1),
            max_delay: Duration::from_secs(30),
            give_up_after: Duration::from_secs(10 * 60),
            liveness_timeout: Some(Duration::from_secs(45)),
        }
    }
}

#[derive(Debug, Clone)]
pub struct CcrV2WorkerClient {
    session_url: String,
    session_id: String,
    credentials: Arc<RwLock<CcrV2WorkerCredentials>>,
    last_sequence_num: u64,
    request_retry_policy: CcrV2RequestRetryPolicy,
    internal_events: Arc<tokio::sync::Mutex<InternalEventUploader>>,
    client: reqwest::Client,
}

#[derive(Debug, Clone)]
struct CcrV2WorkerCredentials {
    worker_jwt: String,
    worker_epoch: u64,
}

#[derive(Debug)]
struct InternalEventUploader {
    pending: VecDeque<CcrV2WorkerEvent>,
    max_batch_size: usize,
    max_batch_bytes: usize,
    max_queue_size: usize,
}

pub struct CcrV2WorkerEventStream {
    client: CcrV2WorkerClient,
    stream: BoxStream<'static, Result<Bytes, reqwest::Error>>,
    buffer: String,
    pending_events: VecDeque<CcrV2StreamClientEvent>,
    ended: bool,
    reconnect_policy: CcrV2ReconnectPolicy,
    reconnect_attempts: u32,
    reconnect_started_at: Option<Instant>,
    last_liveness_at: Instant,
}

pub async fn register_worker(session_url: &str, worker_jwt: &str) -> Result<u64, CcrV2WorkerError> {
    let session_url = normalized_session_url(session_url)?;
    if worker_jwt.trim().is_empty() {
        return Err(CcrV2WorkerError::Message(
            "register_worker requires a worker JWT".to_string(),
        ));
    }
    let response = add_worker_headers(
        reqwest::Client::new().post(worker_path_url(&session_url, "/worker/register")),
        worker_jwt,
    )
    .timeout(WORKER_API_TIMEOUT)
    .json(&json!({}))
    .send()
    .await?;
    if !response.status().is_success() {
        return Err(worker_status_error(response, "register worker").await);
    }
    let value = response.json::<Value>().await?;
    parse_worker_epoch(&value)
}

impl CcrV2WorkerClient {
    pub fn new(
        session_url: impl Into<String>,
        session_id: impl Into<String>,
        worker_jwt: impl Into<String>,
        worker_epoch: u64,
    ) -> Result<Self, CcrV2WorkerError> {
        let session_url = normalized_session_url(&session_url.into())?;
        let session_id = session_id.into();
        let worker_jwt = worker_jwt.into();
        if session_id.trim().is_empty() {
            return Err(CcrV2WorkerError::Message(
                "ccr v2 worker client requires a session ID".to_string(),
            ));
        }
        if worker_jwt.trim().is_empty() {
            return Err(CcrV2WorkerError::Message(
                "ccr v2 worker client requires a worker JWT".to_string(),
            ));
        }
        Ok(Self {
            session_url,
            session_id,
            credentials: Arc::new(RwLock::new(CcrV2WorkerCredentials {
                worker_jwt,
                worker_epoch,
            })),
            last_sequence_num: 0,
            request_retry_policy: CcrV2RequestRetryPolicy::default(),
            internal_events: Arc::new(tokio::sync::Mutex::new(InternalEventUploader::default())),
            client: reqwest::Client::new(),
        })
    }

    pub fn with_request_retry_policy(
        mut self,
        request_retry_policy: CcrV2RequestRetryPolicy,
    ) -> Self {
        self.request_retry_policy = request_retry_policy;
        self
    }

    pub fn with_last_sequence_num(mut self, sequence_num: u64) -> Self {
        self.last_sequence_num = sequence_num;
        self
    }

    pub fn last_sequence_num(&self) -> u64 {
        self.last_sequence_num
    }

    pub fn session_url(&self) -> &str {
        &self.session_url
    }

    pub fn worker_epoch(&self) -> u64 {
        self.credentials_snapshot().worker_epoch
    }

    pub fn update_credentials(
        &self,
        worker_jwt: impl Into<String>,
        worker_epoch: u64,
    ) -> Result<(), CcrV2WorkerError> {
        let worker_jwt = worker_jwt.into();
        if worker_jwt.trim().is_empty() {
            return Err(CcrV2WorkerError::Message(
                "ccr v2 worker client requires a worker JWT".to_string(),
            ));
        }
        let mut credentials = self
            .credentials
            .write()
            .map_err(|_| CcrV2WorkerError::Message("ccr v2 credentials lock poisoned".into()))?;
        credentials.worker_jwt = worker_jwt;
        credentials.worker_epoch = worker_epoch;
        Ok(())
    }

    pub fn stream_url(&self) -> String {
        worker_path_url(&self.session_url, "/worker/events/stream")
    }

    pub async fn initialize_worker(&self) -> Result<(), CcrV2WorkerError> {
        let credentials = self.credentials_snapshot();
        self.put_worker(json!({
            "worker_status": "idle",
            "worker_epoch": credentials.worker_epoch,
            "external_metadata": {
                "pending_action": null,
                "task_summary": null
            }
        }))
        .await
    }

    pub async fn fetch_worker_state(&self) -> Result<Value, CcrV2WorkerError> {
        let credentials = self.credentials_snapshot();
        let response = add_worker_headers(
            self.client
                .get(worker_path_url(&self.session_url, "/worker")),
            &credentials.worker_jwt,
        )
        .timeout(WORKER_API_TIMEOUT)
        .send()
        .await?;
        if !response.status().is_success() {
            return Err(worker_status_error(response, "GET worker").await);
        }
        Ok(response.json::<Value>().await?)
    }

    pub async fn report_worker_state(
        &self,
        worker_status: &str,
        requires_action_details: Option<Value>,
    ) -> Result<(), CcrV2WorkerError> {
        let credentials = self.credentials_snapshot();
        self.put_worker(json!({
            "worker_epoch": credentials.worker_epoch,
            "worker_status": worker_status,
            "requires_action_details": requires_action_details,
        }))
        .await
    }

    pub async fn report_metadata(&self, metadata: Value) -> Result<(), CcrV2WorkerError> {
        let credentials = self.credentials_snapshot();
        self.put_worker(json!({
            "worker_epoch": credentials.worker_epoch,
            "external_metadata": metadata,
        }))
        .await
    }

    pub async fn send_heartbeat(&self) -> Result<(), CcrV2WorkerError> {
        let credentials = self.credentials_snapshot();
        self.post_worker(
            "/worker/heartbeat",
            json!({
                "session_id": self.session_id,
                "worker_epoch": credentials.worker_epoch,
            }),
            "Heartbeat",
        )
        .await
    }

    pub async fn write_client_events(
        &self,
        events: Vec<CcrV2ClientEvent>,
    ) -> Result<(), CcrV2WorkerError> {
        let credentials = self.credentials_snapshot();
        self.post_worker(
            "/worker/events",
            json!({
                "worker_epoch": credentials.worker_epoch,
                "events": events,
            }),
            "client events",
        )
        .await
    }

    pub async fn write_internal_events(
        &self,
        events: Vec<CcrV2WorkerEvent>,
    ) -> Result<(), CcrV2WorkerError> {
        self.internal_events
            .lock()
            .await
            .enqueue_and_flush(events, self)
            .await
    }

    pub async fn flush_internal_events(&self) -> Result<(), CcrV2WorkerError> {
        self.internal_events.lock().await.flush(self).await
    }

    pub async fn internal_events_pending(&self) -> usize {
        self.internal_events.lock().await.pending_count()
    }

    async fn post_internal_event_batch(
        &self,
        events: Vec<CcrV2WorkerEvent>,
    ) -> Result<(), CcrV2WorkerError> {
        let credentials = self.credentials_snapshot();
        self.post_worker(
            "/worker/internal-events",
            json!({
                "worker_epoch": credentials.worker_epoch,
                "events": events,
            }),
            "internal events",
        )
        .await
    }

    pub async fn report_delivery(
        &self,
        updates: Vec<CcrV2DeliveryUpdate>,
    ) -> Result<(), CcrV2WorkerError> {
        let credentials = self.credentials_snapshot();
        self.post_worker(
            "/worker/events/delivery",
            json!({
                "worker_epoch": credentials.worker_epoch,
                "updates": updates,
            }),
            "delivery batch",
        )
        .await
    }

    pub async fn read_sse_events_once(
        &mut self,
    ) -> Result<Vec<CcrV2StreamClientEvent>, CcrV2WorkerError> {
        let response = self.open_sse_response().await?;

        let mut buffer = String::new();
        let mut events = Vec::new();
        let mut stream = response.bytes_stream();
        while let Some(chunk) = stream.next().await {
            let chunk = chunk?;
            buffer.push_str(&String::from_utf8_lossy(&chunk));
            let (frames, remaining) = parse_sse_frames(&buffer);
            buffer = remaining;
            self.handle_sse_frames(frames, &mut events)?;
        }

        let (frames, remaining) = parse_sse_frames(&buffer);
        if !remaining.trim().is_empty() {
            return Err(CcrV2WorkerError::Message(
                "incomplete trailing SSE frame".to_string(),
            ));
        }
        self.handle_sse_frames(frames, &mut events)?;
        Ok(events)
    }

    pub async fn read_internal_events(&self) -> Result<Vec<CcrV2InternalEvent>, CcrV2WorkerError> {
        self.read_internal_events_paginated(false).await
    }

    pub async fn read_subagent_internal_events(
        &self,
    ) -> Result<Vec<CcrV2InternalEvent>, CcrV2WorkerError> {
        self.read_internal_events_paginated(true).await
    }

    pub async fn connect_event_stream(&self) -> Result<CcrV2WorkerEventStream, CcrV2WorkerError> {
        self.connect_event_stream_with_reconnect_policy(CcrV2ReconnectPolicy::default())
            .await
    }

    pub async fn connect_event_stream_with_reconnect_policy(
        &self,
        reconnect_policy: CcrV2ReconnectPolicy,
    ) -> Result<CcrV2WorkerEventStream, CcrV2WorkerError> {
        let response = self.open_sse_response().await?;
        Ok(CcrV2WorkerEventStream {
            client: self.clone(),
            stream: response.bytes_stream().boxed(),
            buffer: String::new(),
            pending_events: VecDeque::new(),
            ended: false,
            reconnect_policy,
            reconnect_attempts: 0,
            reconnect_started_at: None,
            last_liveness_at: Instant::now(),
        })
    }

    async fn open_sse_response(&self) -> Result<reqwest::Response, CcrV2WorkerError> {
        let credentials = self.credentials_snapshot();
        let mut url = reqwest::Url::parse(&self.stream_url()).map_err(|error| {
            CcrV2WorkerError::Message(format!("invalid CCR v2 stream URL: {error}"))
        })?;
        if self.last_sequence_num > 0 {
            url.query_pairs_mut()
                .append_pair("from_sequence_num", &self.last_sequence_num.to_string());
        }

        let mut request = self
            .client
            .get(url)
            .bearer_auth(&credentials.worker_jwt)
            .header("accept", "text/event-stream")
            .header("anthropic-version", ANTHROPIC_VERSION);
        if self.last_sequence_num > 0 {
            request = request.header("Last-Event-ID", self.last_sequence_num.to_string());
        }

        let response = request.send().await?;
        if !response.status().is_success() {
            return Err(worker_status_error(response, "SSE stream").await);
        }
        Ok(response)
    }

    async fn read_internal_events_paginated(
        &self,
        include_subagents: bool,
    ) -> Result<Vec<CcrV2InternalEvent>, CcrV2WorkerError> {
        let mut events = Vec::new();
        let mut cursor: Option<String> = None;

        loop {
            let mut url = reqwest::Url::parse(&worker_path_url(
                &self.session_url,
                "/worker/internal-events",
            ))
            .map_err(|error| {
                CcrV2WorkerError::Message(format!("invalid CCR v2 internal events URL: {error}"))
            })?;
            if include_subagents || cursor.is_some() {
                let mut query = url.query_pairs_mut();
                if include_subagents {
                    query.append_pair("subagents", "true");
                }
                if let Some(cursor) = cursor.as_deref() {
                    query.append_pair("cursor", cursor);
                }
            }

            let page: ListInternalEventsResponse = self
                .get_worker_json_with_retry(
                    url,
                    if include_subagents {
                        "subagent internal events"
                    } else {
                        "internal events"
                    },
                )
                .await?;
            events.extend(page.data);
            cursor = page.next_cursor.filter(|cursor| !cursor.trim().is_empty());
            if cursor.is_none() {
                break;
            }
        }

        Ok(events)
    }

    fn handle_sse_frames(
        &mut self,
        frames: Vec<CcrV2SseFrame>,
        events: &mut Vec<CcrV2StreamClientEvent>,
    ) -> Result<(), CcrV2WorkerError> {
        for frame in frames {
            if let Some(id) = frame.id.as_deref() {
                if let Ok(sequence_num) = id.parse::<u64>() {
                    self.last_sequence_num = self.last_sequence_num.max(sequence_num);
                }
            }
            let Some(event_type) = frame.event.as_deref() else {
                continue;
            };
            if event_type != "client_event" {
                continue;
            }
            let Some(data) = frame.data.as_deref() else {
                continue;
            };
            let event = serde_json::from_str::<CcrV2StreamClientEvent>(data)?;
            self.last_sequence_num = self.last_sequence_num.max(event.sequence_num);
            events.push(event);
        }
        Ok(())
    }

    async fn put_worker(&self, body: Value) -> Result<(), CcrV2WorkerError> {
        self.send_worker_json_with_retry(Method::PUT, "/worker", body, "PUT worker")
            .await
    }

    async fn post_worker(
        &self,
        path: &str,
        body: Value,
        action: &str,
    ) -> Result<(), CcrV2WorkerError> {
        self.send_worker_json_with_retry(Method::POST, path, body, action)
            .await
    }

    async fn get_worker_json_with_retry<T>(
        &self,
        url: reqwest::Url,
        action: &str,
    ) -> Result<T, CcrV2WorkerError>
    where
        T: DeserializeOwned,
    {
        let max_attempts = self.request_retry_policy.max_attempts.max(1);
        for attempt in 1..=max_attempts {
            let credentials = self.credentials_snapshot();
            let response =
                add_worker_headers(self.client.get(url.clone()), &credentials.worker_jwt)
                    .timeout(WORKER_API_TIMEOUT)
                    .send()
                    .await;

            match response {
                Ok(response) if response.status().is_success() => {
                    return Ok(response.json::<T>().await?);
                }
                Ok(response) => {
                    let status = response.status();
                    let retry_after = retry_after_duration(response.headers());
                    if !is_retryable_worker_status(status) || attempt == max_attempts {
                        return Err(worker_status_error(response, action).await);
                    }
                    let delay = self
                        .request_retry_policy
                        .delay_for_attempt(attempt, retry_after);
                    if !delay.is_zero() {
                        tokio::time::sleep(delay).await;
                    }
                }
                Err(error) => {
                    if attempt == max_attempts {
                        return Err(error.into());
                    }
                    let delay = self.request_retry_policy.delay_for_attempt(attempt, None);
                    if !delay.is_zero() {
                        tokio::time::sleep(delay).await;
                    }
                }
            }
        }
        Err(CcrV2WorkerError::Message(format!(
            "{action} failed after {max_attempts} attempts"
        )))
    }

    async fn send_worker_json_with_retry(
        &self,
        method: Method,
        path: &str,
        body: Value,
        action: &str,
    ) -> Result<(), CcrV2WorkerError> {
        let max_attempts = self.request_retry_policy.max_attempts.max(1);
        for attempt in 1..=max_attempts {
            let credentials = self.credentials_snapshot();
            let response = add_worker_headers(
                self.client
                    .request(method.clone(), worker_path_url(&self.session_url, path)),
                &credentials.worker_jwt,
            )
            .timeout(WORKER_API_TIMEOUT)
            .json(&body)
            .send()
            .await;

            match response {
                Ok(response) if response.status().is_success() => return Ok(()),
                Ok(response) => {
                    let status = response.status();
                    let retry_after = retry_after_duration(response.headers());
                    if !is_retryable_worker_status(status) || attempt == max_attempts {
                        return Err(worker_status_error(response, action).await);
                    }
                    let delay = self
                        .request_retry_policy
                        .delay_for_attempt(attempt, retry_after);
                    if !delay.is_zero() {
                        tokio::time::sleep(delay).await;
                    }
                }
                Err(error) => {
                    if attempt == max_attempts {
                        return Err(error.into());
                    }
                    let delay = self.request_retry_policy.delay_for_attempt(attempt, None);
                    if !delay.is_zero() {
                        tokio::time::sleep(delay).await;
                    }
                }
            }
        }
        Err(CcrV2WorkerError::Message(format!(
            "{action} failed after {max_attempts} attempts"
        )))
    }

    fn credentials_snapshot(&self) -> CcrV2WorkerCredentials {
        self.credentials
            .read()
            .map(|credentials| credentials.clone())
            .unwrap_or_else(|poisoned| poisoned.into_inner().clone())
    }
}

impl Default for InternalEventUploader {
    fn default() -> Self {
        Self {
            pending: VecDeque::new(),
            max_batch_size: INTERNAL_EVENT_MAX_BATCH_SIZE,
            max_batch_bytes: INTERNAL_EVENT_MAX_BATCH_BYTES,
            max_queue_size: INTERNAL_EVENT_MAX_QUEUE_SIZE,
        }
    }
}

impl InternalEventUploader {
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
        events: Vec<CcrV2WorkerEvent>,
        client: &CcrV2WorkerClient,
    ) -> Result<(), CcrV2WorkerError> {
        if events.is_empty() {
            return Ok(());
        }

        for chunk in events.chunks(self.max_queue_size) {
            self.enqueue(chunk.iter().cloned())?;
            self.flush(client).await?;
        }
        Ok(())
    }

    fn enqueue<I>(&mut self, events: I) -> Result<(), CcrV2WorkerError>
    where
        I: IntoIterator<Item = CcrV2WorkerEvent>,
    {
        let events = events.into_iter().collect::<Vec<_>>();
        if events.is_empty() {
            return Ok(());
        }
        if self.pending.len() + events.len() > self.max_queue_size {
            return Err(CcrV2WorkerError::Message(format!(
                "CCR v2 internal event queue backpressure exceeded: pending={} incoming={} max={}",
                self.pending.len(),
                events.len(),
                self.max_queue_size
            )));
        }
        self.pending.extend(events);
        Ok(())
    }

    async fn flush(&mut self, client: &CcrV2WorkerClient) -> Result<(), CcrV2WorkerError> {
        while !self.pending.is_empty() {
            let batch = self.take_batch()?;
            if !batch.is_empty() {
                if let Err(error) = client.post_internal_event_batch(batch.clone()).await {
                    for event in batch.into_iter().rev() {
                        self.pending.push_front(event);
                    }
                    return Err(error);
                }
            }
        }
        Ok(())
    }

    fn pending_count(&self) -> usize {
        self.pending.len()
    }

    fn take_batch(&mut self) -> Result<Vec<CcrV2WorkerEvent>, CcrV2WorkerError> {
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

impl CcrV2WorkerEventStream {
    pub fn last_sequence_num(&self) -> u64 {
        self.client.last_sequence_num()
    }

    pub async fn next_event(&mut self) -> Result<Option<CcrV2StreamClientEvent>, CcrV2WorkerError> {
        if let Some(event) = self.pending_events.pop_front() {
            return Ok(Some(event));
        }
        if self.ended {
            return Ok(None);
        }

        loop {
            let next_chunk = match self.next_stream_chunk().await? {
                Some(next_chunk) => next_chunk,
                None => return Ok(None),
            };
            match next_chunk {
                Some(Ok(chunk)) => {
                    if chunk.is_empty() {
                        continue;
                    }
                    self.buffer.push_str(&String::from_utf8_lossy(&chunk));
                    if self.drain_complete_frames()? {
                        self.last_liveness_at = Instant::now();
                    }
                    if let Some(event) = self.pending_events.pop_front() {
                        return Ok(Some(event));
                    }
                }
                Some(Err(error)) => {
                    eprintln!("CCR v2 SSE stream read error: {error}");
                    if self.reconnect_after_disconnect().await? {
                        continue;
                    }
                    return Ok(None);
                }
                None => {
                    let (frames, remaining) = parse_sse_frames(&self.buffer);
                    self.buffer.clear();
                    if !remaining.trim().is_empty() {
                        eprintln!("CCR v2 SSE stream ended with an incomplete trailing frame");
                    }
                    let mut events = Vec::new();
                    if !frames.is_empty() {
                        self.last_liveness_at = Instant::now();
                    }
                    self.client.handle_sse_frames(frames, &mut events)?;
                    self.pending_events.extend(events);
                    if let Some(event) = self.pending_events.pop_front() {
                        return Ok(Some(event));
                    }
                    if self.reconnect_after_disconnect().await? {
                        continue;
                    }
                    return Ok(None);
                }
            }
        }
    }

    async fn next_stream_chunk(
        &mut self,
    ) -> Result<Option<Option<Result<Bytes, reqwest::Error>>>, CcrV2WorkerError> {
        let Some(timeout) = self.reconnect_policy.liveness_timeout else {
            return Ok(Some(self.stream.next().await));
        };

        loop {
            let elapsed = self.last_liveness_at.elapsed();
            if elapsed >= timeout {
                eprintln!(
                    "CCR v2 SSE stream liveness timeout after {} ms",
                    timeout.as_millis()
                );
                if self.reconnect_after_disconnect().await? {
                    continue;
                }
                return Ok(None);
            }

            let remaining = timeout.saturating_sub(elapsed);
            return match tokio::time::timeout(remaining, self.stream.next()).await {
                Ok(next_chunk) => Ok(Some(next_chunk)),
                Err(_) => {
                    eprintln!(
                        "CCR v2 SSE stream liveness timeout after {} ms",
                        timeout.as_millis()
                    );
                    if self.reconnect_after_disconnect().await? {
                        continue;
                    }
                    Ok(None)
                }
            };
        }
    }

    async fn reconnect_after_disconnect(&mut self) -> Result<bool, CcrV2WorkerError> {
        if self.reconnect_policy.give_up_after.is_zero() {
            self.ended = true;
            return Ok(false);
        }

        let started_at = *self.reconnect_started_at.get_or_insert_with(Instant::now);
        loop {
            let elapsed = started_at.elapsed();
            if elapsed >= self.reconnect_policy.give_up_after {
                self.ended = true;
                return Ok(false);
            }

            self.reconnect_attempts = self.reconnect_attempts.saturating_add(1);
            let remaining = self.reconnect_policy.give_up_after.saturating_sub(elapsed);
            let delay = self
                .reconnect_policy
                .delay_for_attempt(self.reconnect_attempts)
                .min(remaining);
            if !delay.is_zero() {
                tokio::time::sleep(delay).await;
            }

            match self.client.open_sse_response().await {
                Ok(response) => {
                    self.stream = response.bytes_stream().boxed();
                    self.buffer.clear();
                    self.reconnect_attempts = 0;
                    self.reconnect_started_at = None;
                    self.last_liveness_at = Instant::now();
                    return Ok(true);
                }
                Err(error) if is_retryable_sse_error(&error) => {
                    eprintln!("CCR v2 SSE reconnect attempt failed: {error}");
                }
                Err(error) => {
                    self.ended = true;
                    return Err(error);
                }
            }
        }
    }

    fn drain_complete_frames(&mut self) -> Result<bool, CcrV2WorkerError> {
        let (frames, remaining) = parse_sse_frames(&self.buffer);
        self.buffer = remaining;
        if frames.is_empty() {
            return Ok(false);
        }
        let saw_frames = true;
        let mut events = Vec::new();
        self.client.handle_sse_frames(frames, &mut events)?;
        self.pending_events.extend(events);
        Ok(saw_frames)
    }
}

fn is_retryable_sse_error(error: &CcrV2WorkerError) -> bool {
    !matches!(error, CcrV2WorkerError::EpochMismatch)
        && !matches!(
            error.http_status(),
            Some(StatusCode::UNAUTHORIZED | StatusCode::FORBIDDEN | StatusCode::NOT_FOUND)
        )
}

fn parse_worker_epoch(value: &Value) -> Result<u64, CcrV2WorkerError> {
    match value.get("worker_epoch") {
        Some(Value::Number(number)) => number.as_u64(),
        Some(Value::String(value)) => value.parse::<u64>().ok(),
        _ => None,
    }
    .ok_or_else(|| {
        CcrV2WorkerError::Message(format!(
            "invalid worker_epoch in response: {}",
            serde_json::to_string(value).unwrap_or_else(|_| "<unserializable>".to_string())
        ))
    })
}

pub fn worker_path_url(session_url: &str, path: &str) -> String {
    format!(
        "{}{}",
        session_url.trim_end_matches('/'),
        if path.starts_with('/') {
            path.to_string()
        } else {
            format!("/{path}")
        }
    )
}

pub fn parse_sse_frames(buffer: &str) -> (Vec<CcrV2SseFrame>, String) {
    let mut frames = Vec::new();
    let mut pos = 0;

    while let Some((idx, delimiter_len)) = find_sse_frame_end(buffer, pos) {
        let raw_frame = &buffer[pos..idx];
        pos = idx + delimiter_len;
        if raw_frame.trim().is_empty() {
            continue;
        }
        frames.push(parse_sse_frame(raw_frame));
    }

    (frames, buffer[pos..].to_string())
}

fn parse_sse_frame(raw_frame: &str) -> CcrV2SseFrame {
    let normalized = raw_frame.replace("\r\n", "\n");
    let mut frame = CcrV2SseFrame {
        event: None,
        id: None,
        data: None,
        is_comment: false,
    };

    for line in normalized.lines() {
        if line.starts_with(':') {
            frame.is_comment = true;
            continue;
        }
        let Some((field, value)) = line.split_once(':') else {
            continue;
        };
        let value = value.strip_prefix(' ').unwrap_or(value).to_string();
        match field {
            "event" => frame.event = Some(value),
            "id" => frame.id = Some(value),
            "data" => {
                frame.data = Some(match frame.data.take() {
                    Some(existing) => format!("{existing}\n{value}"),
                    None => value,
                });
            }
            _ => {}
        }
    }

    frame
}

fn find_sse_frame_end(buffer: &str, from: usize) -> Option<(usize, usize)> {
    let lf = buffer[from..].find("\n\n").map(|idx| (from + idx, 2));
    let crlf = buffer[from..].find("\r\n\r\n").map(|idx| (from + idx, 4));
    match (lf, crlf) {
        (Some(a), Some(b)) => Some(if a.0 <= b.0 { a } else { b }),
        (Some(value), None) | (None, Some(value)) => Some(value),
        (None, None) => None,
    }
}

fn normalized_session_url(session_url: &str) -> Result<String, CcrV2WorkerError> {
    let value = session_url.trim().trim_end_matches('/').to_string();
    if value.is_empty() {
        return Err(CcrV2WorkerError::Message(
            "CCR v2 session URL is empty".to_string(),
        ));
    }
    let url = reqwest::Url::parse(&value).map_err(|error| {
        CcrV2WorkerError::Message(format!("invalid CCR v2 session URL: {error}"))
    })?;
    if url.scheme() != "http" && url.scheme() != "https" {
        return Err(CcrV2WorkerError::Message(format!(
            "CCR v2 session URL must be http(s), got {}",
            url.scheme()
        )));
    }
    Ok(value)
}

fn add_worker_headers(
    builder: reqwest::RequestBuilder,
    worker_jwt: &str,
) -> reqwest::RequestBuilder {
    builder
        .bearer_auth(worker_jwt)
        .header("content-type", "application/json")
        .header("anthropic-version", ANTHROPIC_VERSION)
}

fn is_retryable_worker_status(status: StatusCode) -> bool {
    status == StatusCode::TOO_MANY_REQUESTS || status.is_server_error()
}

fn retry_after_duration(headers: &reqwest::header::HeaderMap) -> Option<Duration> {
    headers
        .get(reqwest::header::RETRY_AFTER)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.parse::<u64>().ok())
        .map(Duration::from_secs)
}

async fn worker_status_error(response: reqwest::Response, action: &str) -> CcrV2WorkerError {
    let status = response.status();
    if status == StatusCode::CONFLICT {
        return CcrV2WorkerError::EpochMismatch;
    }
    let api_message = response
        .json::<Value>()
        .await
        .ok()
        .and_then(|value| {
            value
                .get("error")
                .and_then(|error| error.get("message"))
                .and_then(Value::as_str)
                .map(str::to_string)
        })
        .filter(|message| !message.trim().is_empty());
    CcrV2WorkerError::HttpStatus {
        action: action.to_string(),
        status,
        message: api_message,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Arc, Mutex as StdMutex};
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::{TcpListener, TcpStream};
    use tokio::sync::oneshot;

    #[derive(Debug, Clone)]
    struct RecordedRequest {
        method: String,
        path: String,
        authorization: Option<String>,
        anthropic_version: Option<String>,
        accept: Option<String>,
        last_event_id: Option<String>,
        content_type: Option<String>,
        body: String,
    }

    #[tokio::test]
    async fn worker_client_posts_reference_worker_paths_and_bodies() {
        let (base_url, requests) = spawn_mock_worker_server(vec![
            (200, json!({"ok": true}).to_string(), "application/json"),
            (200, json!({"ok": true}).to_string(), "application/json"),
            (200, json!({"ok": true}).to_string(), "application/json"),
            (200, json!({"ok": true}).to_string(), "application/json"),
            (
                200,
                json!({"worker": {"external_metadata": {"task_summary": "done"}}}).to_string(),
                "application/json",
            ),
        ])
        .await;
        let client = CcrV2WorkerClient::new(
            format!("{base_url}/v1/code/sessions/cse_session_1"),
            "cse_session_1",
            "worker-token",
            42,
        )
        .unwrap();

        client.initialize_worker().await.unwrap();
        client.send_heartbeat().await.unwrap();
        client
            .write_client_events(vec![CcrV2ClientEvent::new(json!({
                "type": "assistant",
                "uuid": "msg-1"
            }))])
            .await
            .unwrap();
        client
            .report_delivery(vec![CcrV2DeliveryUpdate {
                event_id: "evt-1".to_string(),
                status: CcrV2DeliveryStatus::Processed,
            }])
            .await
            .unwrap();
        let state = client.fetch_worker_state().await.unwrap();
        assert_eq!(state["worker"]["external_metadata"]["task_summary"], "done");

        let requests = requests.lock().unwrap().clone();
        assert_eq!(requests.len(), 5);
        assert_eq!(requests[0].method, "PUT");
        assert_eq!(requests[0].path, "/v1/code/sessions/cse_session_1/worker");
        assert_eq!(
            requests[0].authorization.as_deref(),
            Some("Bearer worker-token")
        );
        assert_eq!(
            requests[0].anthropic_version.as_deref(),
            Some(ANTHROPIC_VERSION)
        );
        assert!(requests[0]
            .content_type
            .as_deref()
            .unwrap_or_default()
            .starts_with("application/json"));
        let init_body: Value = serde_json::from_str(&requests[0].body).unwrap();
        assert_eq!(init_body["worker_status"], "idle");
        assert_eq!(init_body["worker_epoch"], 42);
        assert_eq!(
            init_body["external_metadata"]["pending_action"],
            Value::Null
        );

        assert_eq!(requests[1].method, "POST");
        assert_eq!(
            requests[1].path,
            "/v1/code/sessions/cse_session_1/worker/heartbeat"
        );
        let heartbeat_body: Value = serde_json::from_str(&requests[1].body).unwrap();
        assert_eq!(heartbeat_body["session_id"], "cse_session_1");
        assert_eq!(heartbeat_body["worker_epoch"], 42);

        assert_eq!(requests[2].method, "POST");
        assert_eq!(
            requests[2].path,
            "/v1/code/sessions/cse_session_1/worker/events"
        );
        let events_body: Value = serde_json::from_str(&requests[2].body).unwrap();
        assert_eq!(events_body["worker_epoch"], 42);
        assert_eq!(events_body["events"][0]["payload"]["type"], "assistant");

        assert_eq!(requests[3].method, "POST");
        assert_eq!(
            requests[3].path,
            "/v1/code/sessions/cse_session_1/worker/events/delivery"
        );
        let delivery_body: Value = serde_json::from_str(&requests[3].body).unwrap();
        assert_eq!(delivery_body["updates"][0]["event_id"], "evt-1");
        assert_eq!(delivery_body["updates"][0]["status"], "processed");

        assert_eq!(requests[4].method, "GET");
        assert_eq!(requests[4].path, "/v1/code/sessions/cse_session_1/worker");
    }

    #[tokio::test]
    async fn worker_client_uses_updated_credentials_for_later_requests() {
        let (base_url, requests) = spawn_mock_worker_server(vec![
            (200, json!({"ok": true}).to_string(), "application/json"),
            (200, json!({"ok": true}).to_string(), "application/json"),
        ])
        .await;
        let client = CcrV2WorkerClient::new(
            format!("{base_url}/v1/code/sessions/cse_session_1"),
            "cse_session_1",
            "old-worker-token",
            1,
        )
        .unwrap();

        client.send_heartbeat().await.unwrap();
        client.update_credentials("fresh-worker-token", 77).unwrap();
        assert_eq!(client.worker_epoch(), 77);
        client.send_heartbeat().await.unwrap();

        let requests = requests.lock().unwrap().clone();
        assert_eq!(requests.len(), 2);
        assert_eq!(
            requests[0].authorization.as_deref(),
            Some("Bearer old-worker-token")
        );
        let old_body: Value = serde_json::from_str(&requests[0].body).unwrap();
        assert_eq!(old_body["worker_epoch"], 1);

        assert_eq!(
            requests[1].authorization.as_deref(),
            Some("Bearer fresh-worker-token")
        );
        let fresh_body: Value = serde_json::from_str(&requests[1].body).unwrap();
        assert_eq!(fresh_body["worker_epoch"], 77);
    }

    #[tokio::test]
    async fn worker_client_retries_retryable_worker_posts() {
        let (base_url, requests) = spawn_mock_worker_server(vec![
            (
                500,
                json!({"error": {"message": "transient"}}).to_string(),
                "application/json",
            ),
            (200, json!({"ok": true}).to_string(), "application/json"),
        ])
        .await;
        let client = CcrV2WorkerClient::new(
            format!("{base_url}/v1/code/sessions/cse_session_1"),
            "cse_session_1",
            "worker-token",
            42,
        )
        .unwrap()
        .with_request_retry_policy(CcrV2RequestRetryPolicy::no_delay(2));

        client
            .write_client_events(vec![CcrV2ClientEvent::new(json!({
                "type": "assistant",
                "uuid": "retry-msg-1"
            }))])
            .await
            .unwrap();

        let requests = requests.lock().unwrap().clone();
        assert_eq!(requests.len(), 2);
        assert_eq!(
            requests[0].path,
            "/v1/code/sessions/cse_session_1/worker/events"
        );
        assert_eq!(requests[0].path, requests[1].path);
        let first_body: Value = serde_json::from_str(&requests[0].body).unwrap();
        let second_body: Value = serde_json::from_str(&requests[1].body).unwrap();
        assert_eq!(first_body, second_body);
    }

    #[tokio::test]
    async fn worker_client_does_not_retry_epoch_mismatch_posts() {
        let (base_url, requests) = spawn_mock_worker_server(vec![
            (
                409,
                json!({"error": {"message": "epoch superseded"}}).to_string(),
                "application/json",
            ),
            (200, json!({"ok": true}).to_string(), "application/json"),
        ])
        .await;
        let client = CcrV2WorkerClient::new(
            format!("{base_url}/v1/code/sessions/cse_session_1"),
            "cse_session_1",
            "worker-token",
            42,
        )
        .unwrap()
        .with_request_retry_policy(CcrV2RequestRetryPolicy::no_delay(2));

        let error = client
            .write_client_events(vec![CcrV2ClientEvent::new(json!({
                "type": "assistant",
                "uuid": "epoch-msg-1"
            }))])
            .await
            .unwrap_err();

        assert!(matches!(error, CcrV2WorkerError::EpochMismatch));
        let requests = requests.lock().unwrap().clone();
        assert_eq!(requests.len(), 1);
        assert_eq!(
            requests[0].path,
            "/v1/code/sessions/cse_session_1/worker/events"
        );
    }

    #[tokio::test]
    async fn worker_client_reads_sse_client_events_and_resumes_from_sequence() {
        let event = json!({
            "event_id": "evt-1",
            "sequence_num": 7,
            "event_type": "user",
            "source": "frontend",
            "payload": {
                "type": "user",
                "uuid": "user-1",
                "message": {
                    "role": "user",
                    "content": "hello"
                }
            },
            "created_at": "2026-06-16T00:00:00Z"
        });
        let body = format!(
            ":keepalive\n\nid: 7\nevent: client_event\ndata: {}\n\n",
            event
        );
        let (base_url, requests) =
            spawn_mock_worker_server(vec![(200, body, "text/event-stream")]).await;
        let mut client = CcrV2WorkerClient::new(
            format!("{base_url}/v1/code/sessions/cse_session_1"),
            "cse_session_1",
            "worker-token",
            42,
        )
        .unwrap()
        .with_last_sequence_num(3);

        let events = client.read_sse_events_once().await.unwrap();
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].event_id, "evt-1");
        assert_eq!(events[0].payload["type"], "user");
        assert_eq!(client.last_sequence_num(), 7);

        let requests = requests.lock().unwrap().clone();
        assert_eq!(requests[0].method, "GET");
        assert_eq!(
            requests[0].path,
            "/v1/code/sessions/cse_session_1/worker/events/stream?from_sequence_num=3"
        );
        assert_eq!(
            requests[0].authorization.as_deref(),
            Some("Bearer worker-token")
        );
        assert_eq!(requests[0].accept.as_deref(), Some("text/event-stream"));
        assert_eq!(requests[0].last_event_id.as_deref(), Some("3"));
        assert!(requests[0].content_type.is_none());
    }

    #[tokio::test]
    async fn worker_event_stream_yields_events_before_connection_closes() {
        let event = json!({
            "event_id": "evt-live-1",
            "sequence_num": 8,
            "event_type": "user",
            "source": "frontend",
            "payload": {
                "type": "user",
                "uuid": "user-live-1",
                "message": {
                    "role": "user",
                    "content": "hello while open"
                }
            },
            "created_at": "2026-06-16T00:00:00Z"
        });
        let frame = format!("id: 8\nevent: client_event\ndata: {}\n\n", event);
        let (base_url, requests, close_tx) = spawn_open_sse_server(frame).await;
        let client = CcrV2WorkerClient::new(
            format!("{base_url}/v1/code/sessions/cse_session_1"),
            "cse_session_1",
            "worker-token",
            42,
        )
        .unwrap()
        .with_last_sequence_num(5);

        let mut stream = client
            .connect_event_stream_with_reconnect_policy(CcrV2ReconnectPolicy::disabled())
            .await
            .unwrap();
        let first = tokio::time::timeout(Duration::from_secs(1), stream.next_event())
            .await
            .unwrap()
            .unwrap()
            .unwrap();

        assert_eq!(first.event_id, "evt-live-1");
        assert_eq!(first.payload["type"], "user");
        assert_eq!(stream.last_sequence_num(), 8);

        let _ = close_tx.send(());
        assert!(stream.next_event().await.unwrap().is_none());

        let requests = requests.lock().unwrap().clone();
        assert_eq!(requests[0].method, "GET");
        assert_eq!(
            requests[0].path,
            "/v1/code/sessions/cse_session_1/worker/events/stream?from_sequence_num=5"
        );
        assert_eq!(requests[0].last_event_id.as_deref(), Some("5"));
    }

    #[tokio::test]
    async fn worker_event_stream_reconnects_and_resumes_from_last_sequence() {
        let first_event = json!({
            "event_id": "evt-reconnect-1",
            "sequence_num": 8,
            "event_type": "user",
            "source": "frontend",
            "payload": {
                "type": "user",
                "uuid": "user-reconnect-1",
                "message": {
                    "role": "user",
                    "content": "first"
                }
            },
            "created_at": "2026-06-16T00:00:00Z"
        });
        let second_event = json!({
            "event_id": "evt-reconnect-2",
            "sequence_num": 9,
            "event_type": "user",
            "source": "frontend",
            "payload": {
                "type": "user",
                "uuid": "user-reconnect-2",
                "message": {
                    "role": "user",
                    "content": "second"
                }
            },
            "created_at": "2026-06-16T00:00:01Z"
        });
        let first_body = format!("id: 8\nevent: client_event\ndata: {}\n\n", first_event);
        let second_body = format!("id: 9\nevent: client_event\ndata: {}\n\n", second_event);
        let (base_url, requests) = spawn_mock_worker_server(vec![
            (200, first_body, "text/event-stream"),
            (200, second_body, "text/event-stream"),
        ])
        .await;
        let client = CcrV2WorkerClient::new(
            format!("{base_url}/v1/code/sessions/cse_session_1"),
            "cse_session_1",
            "worker-token",
            42,
        )
        .unwrap()
        .with_last_sequence_num(5);
        let mut stream = client
            .connect_event_stream_with_reconnect_policy(CcrV2ReconnectPolicy::new(
                Duration::ZERO,
                Duration::ZERO,
                Duration::from_secs(1),
            ))
            .await
            .unwrap();

        let first = tokio::time::timeout(Duration::from_secs(1), stream.next_event())
            .await
            .unwrap()
            .unwrap()
            .unwrap();
        let second = tokio::time::timeout(Duration::from_secs(1), stream.next_event())
            .await
            .unwrap()
            .unwrap()
            .unwrap();

        assert_eq!(first.event_id, "evt-reconnect-1");
        assert_eq!(second.event_id, "evt-reconnect-2");
        assert_eq!(stream.last_sequence_num(), 9);

        let requests = requests.lock().unwrap().clone();
        assert_eq!(requests.len(), 2);
        assert_eq!(
            requests[0].path,
            "/v1/code/sessions/cse_session_1/worker/events/stream?from_sequence_num=5"
        );
        assert_eq!(requests[0].last_event_id.as_deref(), Some("5"));
        assert_eq!(
            requests[1].path,
            "/v1/code/sessions/cse_session_1/worker/events/stream?from_sequence_num=8"
        );
        assert_eq!(requests[1].last_event_id.as_deref(), Some("8"));
    }

    #[tokio::test]
    async fn worker_event_stream_liveness_timeout_reconnects_with_last_sequence() {
        let event = json!({
            "event_id": "evt-liveness-1",
            "sequence_num": 6,
            "event_type": "user",
            "source": "frontend",
            "payload": {
                "type": "user",
                "uuid": "user-liveness-1",
                "message": {
                    "role": "user",
                    "content": "after idle"
                }
            },
            "created_at": "2026-06-16T00:00:00Z"
        });
        let body = format!("id: 6\nevent: client_event\ndata: {}\n\n", event);
        let (base_url, requests) = spawn_idle_then_sse_event_server(body).await;
        let client = CcrV2WorkerClient::new(
            format!("{base_url}/v1/code/sessions/cse_session_1"),
            "cse_session_1",
            "worker-token",
            42,
        )
        .unwrap()
        .with_last_sequence_num(5);
        let mut stream = client
            .connect_event_stream_with_reconnect_policy(
                CcrV2ReconnectPolicy::new(Duration::ZERO, Duration::ZERO, Duration::from_secs(1))
                    .with_liveness_timeout(Some(Duration::from_millis(50))),
            )
            .await
            .unwrap();

        let received = tokio::time::timeout(Duration::from_secs(1), stream.next_event())
            .await
            .unwrap()
            .unwrap()
            .unwrap();

        assert_eq!(received.event_id, "evt-liveness-1");
        assert_eq!(stream.last_sequence_num(), 6);

        let requests = requests.lock().unwrap().clone();
        assert_eq!(requests.len(), 2);
        assert_eq!(
            requests[0].path,
            "/v1/code/sessions/cse_session_1/worker/events/stream?from_sequence_num=5"
        );
        assert_eq!(requests[0].last_event_id.as_deref(), Some("5"));
        assert_eq!(
            requests[1].path,
            "/v1/code/sessions/cse_session_1/worker/events/stream?from_sequence_num=5"
        );
        assert_eq!(requests[1].last_event_id.as_deref(), Some("5"));
    }

    #[tokio::test]
    async fn worker_event_stream_returns_none_after_reconnect_budget_exhausted() {
        let (base_url, requests) = spawn_mock_worker_server(vec![
            (200, String::new(), "text/event-stream"),
            (
                500,
                json!({"error": {"message": "temporary"}}).to_string(),
                "application/json",
            ),
            (
                500,
                json!({"error": {"message": "temporary"}}).to_string(),
                "application/json",
            ),
        ])
        .await;
        let client = CcrV2WorkerClient::new(
            format!("{base_url}/v1/code/sessions/cse_session_1"),
            "cse_session_1",
            "worker-token",
            42,
        )
        .unwrap()
        .with_last_sequence_num(5);

        let mut stream = client
            .connect_event_stream_with_reconnect_policy(
                CcrV2ReconnectPolicy::new(
                    Duration::from_millis(5),
                    Duration::from_millis(5),
                    Duration::from_millis(15),
                )
                .with_liveness_timeout(None),
            )
            .await
            .unwrap();

        assert!(stream.next_event().await.unwrap().is_none());
        assert_eq!(stream.last_sequence_num(), 5);
        let requests = requests.lock().unwrap().clone();
        assert!(requests.len() >= 2);
        for request in requests {
            assert_eq!(
                request.path,
                "/v1/code/sessions/cse_session_1/worker/events/stream?from_sequence_num=5"
            );
            assert_eq!(request.last_event_id.as_deref(), Some("5"));
        }
    }

    #[tokio::test]
    async fn worker_event_stream_keepalive_comment_resets_liveness_timeout() {
        let event = json!({
            "event_id": "evt-keepalive-1",
            "sequence_num": 6,
            "event_type": "user",
            "source": "frontend",
            "payload": {
                "type": "user",
                "uuid": "user-keepalive-1",
                "message": {
                    "role": "user",
                    "content": "after keepalive"
                }
            },
            "created_at": "2026-06-16T00:00:00Z"
        });
        let frame = format!("id: 6\nevent: client_event\ndata: {}\n\n", event);
        let (base_url, requests) = spawn_delayed_sse_server(vec![
            (Duration::from_millis(80), ":keepalive\n\n".to_string()),
            (Duration::from_millis(100), frame),
        ])
        .await;
        let client = CcrV2WorkerClient::new(
            format!("{base_url}/v1/code/sessions/cse_session_1"),
            "cse_session_1",
            "worker-token",
            42,
        )
        .unwrap()
        .with_last_sequence_num(5);
        let mut stream = client
            .connect_event_stream_with_reconnect_policy(
                CcrV2ReconnectPolicy::new(Duration::ZERO, Duration::ZERO, Duration::from_secs(1))
                    .with_liveness_timeout(Some(Duration::from_millis(150))),
            )
            .await
            .unwrap();

        let received = tokio::time::timeout(Duration::from_secs(1), stream.next_event())
            .await
            .unwrap()
            .unwrap()
            .unwrap();

        assert_eq!(received.event_id, "evt-keepalive-1");
        assert_eq!(stream.last_sequence_num(), 6);

        let requests = requests.lock().unwrap().clone();
        assert_eq!(requests.len(), 1);
        assert_eq!(
            requests[0].path,
            "/v1/code/sessions/cse_session_1/worker/events/stream?from_sequence_num=5"
        );
    }

    #[tokio::test]
    async fn register_worker_posts_reference_endpoint_and_parses_string_epoch() {
        let (base_url, requests) = spawn_mock_worker_server(vec![(
            200,
            json!({"worker_epoch": "77"}).to_string(),
            "application/json",
        )])
        .await;

        let epoch = register_worker(
            &format!("{base_url}/v1/code/sessions/cse_session_1"),
            "worker-token",
        )
        .await
        .unwrap();

        assert_eq!(epoch, 77);
        let requests = requests.lock().unwrap().clone();
        assert_eq!(requests[0].method, "POST");
        assert_eq!(
            requests[0].path,
            "/v1/code/sessions/cse_session_1/worker/register"
        );
        assert_eq!(
            requests[0].authorization.as_deref(),
            Some("Bearer worker-token")
        );
        let body: Value = serde_json::from_str(&requests[0].body).unwrap();
        assert_eq!(body, json!({}));
    }

    #[tokio::test]
    async fn worker_client_reports_state_metadata_and_internal_events() {
        let (base_url, requests) = spawn_mock_worker_server(vec![
            (200, json!({"ok": true}).to_string(), "application/json"),
            (200, json!({"ok": true}).to_string(), "application/json"),
            (200, json!({"ok": true}).to_string(), "application/json"),
        ])
        .await;
        let client = CcrV2WorkerClient::new(
            format!("{base_url}/v1/code/sessions/cse_session_1"),
            "cse_session_1",
            "worker-token",
            42,
        )
        .unwrap();

        client
            .report_worker_state(
                "requires_action",
                Some(json!({
                    "tool_name": "Bash",
                    "action_description": "Run command",
                    "request_id": "req-1"
                })),
            )
            .await
            .unwrap();
        client
            .report_metadata(json!({
                "task_summary": "checking tests"
            }))
            .await
            .unwrap();
        client
            .write_internal_events(vec![CcrV2WorkerEvent {
                payload: json!({
                    "type": "transcript",
                    "uuid": "internal-1"
                }),
                is_compaction: Some(true),
                agent_id: Some("agent-1".to_string()),
            }])
            .await
            .unwrap();

        let requests = requests.lock().unwrap().clone();
        assert_eq!(requests[0].method, "PUT");
        let state_body: Value = serde_json::from_str(&requests[0].body).unwrap();
        assert_eq!(state_body["worker_status"], "requires_action");
        assert_eq!(state_body["requires_action_details"]["tool_name"], "Bash");
        assert_eq!(state_body["worker_epoch"], 42);

        assert_eq!(requests[1].method, "PUT");
        let metadata_body: Value = serde_json::from_str(&requests[1].body).unwrap();
        assert_eq!(
            metadata_body["external_metadata"]["task_summary"],
            "checking tests"
        );

        assert_eq!(requests[2].method, "POST");
        assert_eq!(
            requests[2].path,
            "/v1/code/sessions/cse_session_1/worker/internal-events"
        );
        let internal_body: Value = serde_json::from_str(&requests[2].body).unwrap();
        assert_eq!(internal_body["events"][0]["payload"]["type"], "transcript");
        assert_eq!(internal_body["events"][0]["is_compaction"], true);
        assert_eq!(internal_body["events"][0]["agent_id"], "agent-1");
    }

    #[tokio::test]
    async fn worker_client_reads_paginated_internal_events() {
        let (base_url, requests) = spawn_mock_worker_server(vec![
            (
                200,
                json!({
                    "data": [{
                        "event_id": "int-1",
                        "event_type": "transcript",
                        "payload": {
                            "type": "user",
                            "uuid": "user-1",
                            "message": {
                                "role": "user",
                                "content": "hello"
                            }
                        },
                        "event_metadata": null,
                        "is_compaction": false,
                        "created_at": "2026-06-16T00:00:00Z"
                    }],
                    "next_cursor": "cursor-2"
                })
                .to_string(),
                "application/json",
            ),
            (
                200,
                json!({
                    "data": [{
                        "event_id": "int-2",
                        "event_type": "transcript",
                        "payload": {
                            "type": "assistant",
                            "uuid": "assistant-1",
                            "message": {
                                "role": "assistant",
                                "content": [{"type": "text", "text": "hi"}]
                            }
                        },
                        "is_compaction": true,
                        "created_at": "2026-06-16T00:00:01Z"
                    }],
                    "next_cursor": null
                })
                .to_string(),
                "application/json",
            ),
        ])
        .await;
        let client = CcrV2WorkerClient::new(
            format!("{base_url}/v1/code/sessions/cse_session_1"),
            "cse_session_1",
            "worker-token",
            42,
        )
        .unwrap();

        let events = client.read_internal_events().await.unwrap();

        assert_eq!(events.len(), 2);
        assert_eq!(events[0].event_id, "int-1");
        assert_eq!(events[0].payload["type"], "user");
        assert_eq!(events[0].agent_id, None);
        assert!(!events[0].is_compaction);
        assert_eq!(events[1].event_id, "int-2");
        assert_eq!(events[1].payload["type"], "assistant");
        assert!(events[1].is_compaction);

        let requests = requests.lock().unwrap().clone();
        assert_eq!(requests.len(), 2);
        assert_eq!(
            requests[0].path,
            "/v1/code/sessions/cse_session_1/worker/internal-events"
        );
        assert_eq!(
            requests[1].path,
            "/v1/code/sessions/cse_session_1/worker/internal-events?cursor=cursor-2"
        );
        assert_eq!(requests[0].method, "GET");
        assert_eq!(
            requests[0].authorization.as_deref(),
            Some("Bearer worker-token")
        );
        assert_eq!(
            requests[0].anthropic_version.as_deref(),
            Some(ANTHROPIC_VERSION)
        );
    }

    #[tokio::test]
    async fn worker_client_reads_subagent_internal_events() {
        let (base_url, requests) = spawn_mock_worker_server(vec![(
            200,
            json!({
                "data": [{
                    "event_id": "int-agent-1",
                    "event_type": "transcript",
                    "payload": {
                        "type": "assistant",
                        "uuid": "agent-assistant-1"
                    },
                    "is_compaction": false,
                    "created_at": "2026-06-16T00:00:00Z",
                    "agent_id": "agent-1"
                }]
            })
            .to_string(),
            "application/json",
        )])
        .await;
        let client = CcrV2WorkerClient::new(
            format!("{base_url}/v1/code/sessions/cse_session_1"),
            "cse_session_1",
            "worker-token",
            42,
        )
        .unwrap();

        let events = client.read_subagent_internal_events().await.unwrap();

        assert_eq!(events.len(), 1);
        assert_eq!(events[0].event_id, "int-agent-1");
        assert_eq!(events[0].agent_id.as_deref(), Some("agent-1"));
        let requests = requests.lock().unwrap().clone();
        assert_eq!(
            requests[0].path,
            "/v1/code/sessions/cse_session_1/worker/internal-events?subagents=true"
        );
    }

    #[tokio::test]
    async fn worker_client_retries_internal_event_reads() {
        let (base_url, requests) = spawn_mock_worker_server(vec![
            (
                500,
                json!({"error": {"message": "transient"}}).to_string(),
                "application/json",
            ),
            (
                200,
                json!({
                    "data": [{
                        "event_id": "int-retry-1",
                        "event_type": "transcript",
                        "payload": {
                            "type": "user",
                            "uuid": "retry-user-1"
                        },
                        "is_compaction": false,
                        "created_at": "2026-06-16T00:00:00Z"
                    }]
                })
                .to_string(),
                "application/json",
            ),
        ])
        .await;
        let client = CcrV2WorkerClient::new(
            format!("{base_url}/v1/code/sessions/cse_session_1"),
            "cse_session_1",
            "worker-token",
            42,
        )
        .unwrap()
        .with_request_retry_policy(CcrV2RequestRetryPolicy::no_delay(2));

        let events = client.read_internal_events().await.unwrap();

        assert_eq!(events.len(), 1);
        assert_eq!(events[0].event_id, "int-retry-1");
        let requests = requests.lock().unwrap().clone();
        assert_eq!(requests.len(), 2);
        assert_eq!(requests[0].method, "GET");
        assert_eq!(requests[0].path, requests[1].path);
    }

    #[tokio::test]
    async fn worker_client_splits_internal_events_by_reference_batch_count() {
        let (base_url, requests) = spawn_mock_worker_server(vec![
            (200, json!({"ok": true}).to_string(), "application/json"),
            (200, json!({"ok": true}).to_string(), "application/json"),
        ])
        .await;
        let client = CcrV2WorkerClient::new(
            format!("{base_url}/v1/code/sessions/cse_session_1"),
            "cse_session_1",
            "worker-token",
            42,
        )
        .unwrap();
        let events = (0..150)
            .map(|index| internal_test_event(&format!("internal-{index}"), 8))
            .collect::<Vec<_>>();

        client.write_internal_events(events).await.unwrap();

        assert_eq!(client.internal_events_pending().await, 0);
        let requests = requests.lock().unwrap().clone();
        assert_eq!(requests.len(), 2);
        assert_eq!(
            requests[0].path,
            "/v1/code/sessions/cse_session_1/worker/internal-events"
        );
        assert_eq!(requests[0].path, requests[1].path);
        let first_body: Value = serde_json::from_str(&requests[0].body).unwrap();
        let second_body: Value = serde_json::from_str(&requests[1].body).unwrap();
        assert_eq!(first_body["worker_epoch"], 42);
        assert_eq!(first_body["events"].as_array().unwrap().len(), 100);
        assert_eq!(second_body["events"].as_array().unwrap().len(), 50);
        assert_eq!(first_body["events"][0]["payload"]["uuid"], "internal-0");
        assert_eq!(second_body["events"][0]["payload"]["uuid"], "internal-100");
        assert_eq!(second_body["events"][49]["payload"]["uuid"], "internal-149");
    }

    #[tokio::test]
    async fn worker_client_flushes_queued_internal_events() {
        let (base_url, requests) = spawn_mock_worker_server(vec![(
            200,
            json!({"ok": true}).to_string(),
            "application/json",
        )])
        .await;
        let client = CcrV2WorkerClient::new(
            format!("{base_url}/v1/code/sessions/cse_session_1"),
            "cse_session_1",
            "worker-token",
            42,
        )
        .unwrap();
        client
            .internal_events
            .lock()
            .await
            .enqueue(vec![internal_test_event("queued-internal-1", 4)])
            .unwrap();

        assert_eq!(client.internal_events_pending().await, 1);
        client.flush_internal_events().await.unwrap();

        assert_eq!(client.internal_events_pending().await, 0);
        let requests = requests.lock().unwrap().clone();
        assert_eq!(requests.len(), 1);
        let body: Value = serde_json::from_str(&requests[0].body).unwrap();
        assert_eq!(body["events"][0]["payload"]["uuid"], "queued-internal-1");
    }

    #[tokio::test]
    async fn worker_client_keeps_internal_events_pending_after_post_failure() {
        let (base_url, requests) = spawn_mock_worker_server(vec![
            (
                500,
                json!({"error": {"message": "transient"}}).to_string(),
                "application/json",
            ),
            (200, json!({"ok": true}).to_string(), "application/json"),
        ])
        .await;
        let client = CcrV2WorkerClient::new(
            format!("{base_url}/v1/code/sessions/cse_session_1"),
            "cse_session_1",
            "worker-token",
            42,
        )
        .unwrap()
        .with_request_retry_policy(CcrV2RequestRetryPolicy::no_delay(1));

        let error = client
            .write_internal_events(vec![internal_test_event("retry-internal-1", 4)])
            .await
            .unwrap_err();

        assert!(matches!(error, CcrV2WorkerError::HttpStatus { .. }));
        assert_eq!(client.internal_events_pending().await, 1);
        client.flush_internal_events().await.unwrap();
        assert_eq!(client.internal_events_pending().await, 0);

        let requests = requests.lock().unwrap().clone();
        assert_eq!(requests.len(), 2);
        let first_body: Value = serde_json::from_str(&requests[0].body).unwrap();
        let second_body: Value = serde_json::from_str(&requests[1].body).unwrap();
        assert_eq!(
            first_body["events"][0]["payload"]["uuid"],
            "retry-internal-1"
        );
        assert_eq!(first_body, second_body);
    }

    #[test]
    fn internal_event_uploader_splits_batches_by_serialized_bytes() {
        let first = internal_test_event("byte-internal-1", 32);
        let second = internal_test_event("byte-internal-2", 32);
        let byte_limit = serde_json::to_vec(&first).unwrap().len() + 1;
        let mut uploader = InternalEventUploader::with_limits(10, byte_limit, 10);

        uploader
            .enqueue(vec![first.clone(), second.clone()])
            .unwrap();

        assert_eq!(uploader.take_batch().unwrap(), vec![first]);
        assert_eq!(uploader.take_batch().unwrap(), vec![second]);
        assert_eq!(uploader.pending_count(), 0);
    }

    #[test]
    fn internal_event_uploader_allows_oversized_first_item() {
        let event = internal_test_event("oversized-internal-1", 64);
        let mut uploader = InternalEventUploader::with_limits(10, 1, 10);

        uploader.enqueue(vec![event.clone()]).unwrap();

        assert_eq!(uploader.take_batch().unwrap(), vec![event]);
        assert_eq!(uploader.pending_count(), 0);
    }

    #[test]
    fn internal_event_uploader_reports_queue_backpressure() {
        let mut uploader = InternalEventUploader::with_limits(10, 10_000, 2);

        uploader
            .enqueue(vec![
                internal_test_event("queued-internal-1", 4),
                internal_test_event("queued-internal-2", 4),
            ])
            .unwrap();
        let error = uploader
            .enqueue(vec![internal_test_event("queued-internal-3", 4)])
            .unwrap_err();

        assert!(error
            .to_string()
            .contains("CCR v2 internal event queue backpressure exceeded"));
    }

    #[tokio::test]
    async fn worker_client_reports_epoch_mismatch_on_conflict() {
        let (base_url, _requests) = spawn_mock_worker_server(vec![(
            409,
            json!({"ok": false}).to_string(),
            "application/json",
        )])
        .await;
        let client = CcrV2WorkerClient::new(
            format!("{base_url}/v1/code/sessions/cse_session_1"),
            "cse_session_1",
            "worker-token",
            42,
        )
        .unwrap();

        let error = client.send_heartbeat().await.unwrap_err();
        assert!(matches!(error, CcrV2WorkerError::EpochMismatch));
    }

    #[test]
    fn parse_sse_frames_handles_comments_multiline_data_and_remainder() {
        let input =
            ":keepalive\n\nid: 4\nevent: client_event\ndata: {\"a\":1}\ndata: {\"b\":2}\n\nid: ";

        let (frames, remaining) = parse_sse_frames(input);

        assert_eq!(frames.len(), 2);
        assert!(frames[0].is_comment);
        assert_eq!(frames[1].id.as_deref(), Some("4"));
        assert_eq!(frames[1].event.as_deref(), Some("client_event"));
        assert_eq!(frames[1].data.as_deref(), Some("{\"a\":1}\n{\"b\":2}"));
        assert_eq!(remaining, "id: ");
    }

    #[test]
    fn worker_path_url_trims_session_url_slash() {
        assert_eq!(
            worker_path_url(
                "https://api.example/v1/code/sessions/cse_1/",
                "/worker/events"
            ),
            "https://api.example/v1/code/sessions/cse_1/worker/events"
        );
    }

    async fn spawn_mock_worker_server(
        responses: Vec<(u16, String, &'static str)>,
    ) -> (String, Arc<StdMutex<Vec<RecordedRequest>>>) {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let requests = Arc::new(StdMutex::new(Vec::new()));
        let shared_requests = requests.clone();

        tokio::spawn(async move {
            for (status, body, content_type) in responses {
                let Ok((mut stream, _)) = listener.accept().await else {
                    return;
                };
                if let Ok(request) = read_http_request(&mut stream).await {
                    shared_requests.lock().unwrap().push(request);
                }
                let _ = write_http_response(&mut stream, status, &body, content_type).await;
            }
        });

        (format!("http://{}", address), requests)
    }

    async fn spawn_open_sse_server(
        frame: String,
    ) -> (
        String,
        Arc<StdMutex<Vec<RecordedRequest>>>,
        oneshot::Sender<()>,
    ) {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let requests = Arc::new(StdMutex::new(Vec::new()));
        let shared_requests = requests.clone();
        let (close_tx, close_rx) = oneshot::channel::<()>();

        tokio::spawn(async move {
            let Ok((mut stream, _)) = listener.accept().await else {
                return;
            };
            if let Ok(request) = read_http_request(&mut stream).await {
                shared_requests.lock().unwrap().push(request);
            }
            let headers = concat!(
                "HTTP/1.1 200 OK\r\n",
                "content-type: text/event-stream\r\n",
                "connection: keep-alive\r\n",
                "\r\n"
            );
            let _ = stream.write_all(headers.as_bytes()).await;
            let _ = stream.write_all(frame.as_bytes()).await;
            let _ = stream.flush().await;
            let _ = close_rx.await;
        });

        (format!("http://{}", address), requests, close_tx)
    }

    async fn spawn_idle_then_sse_event_server(
        second_body: String,
    ) -> (String, Arc<StdMutex<Vec<RecordedRequest>>>) {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let requests = Arc::new(StdMutex::new(Vec::new()));
        let shared_requests = requests.clone();

        tokio::spawn(async move {
            let Ok((mut first_stream, _)) = listener.accept().await else {
                return;
            };
            if let Ok(request) = read_http_request(&mut first_stream).await {
                shared_requests.lock().unwrap().push(request);
            }
            let headers = concat!(
                "HTTP/1.1 200 OK\r\n",
                "content-type: text/event-stream\r\n",
                "connection: keep-alive\r\n",
                "\r\n"
            );
            let _ = first_stream.write_all(headers.as_bytes()).await;
            let _ = first_stream.flush().await;

            tokio::spawn(async move {
                tokio::time::sleep(Duration::from_secs(2)).await;
                let _ = first_stream.shutdown().await;
            });

            let Ok((mut second_stream, _)) = listener.accept().await else {
                return;
            };
            if let Ok(request) = read_http_request(&mut second_stream).await {
                shared_requests.lock().unwrap().push(request);
            }
            let _ = write_http_response(&mut second_stream, 200, &second_body, "text/event-stream")
                .await;
        });

        (format!("http://{}", address), requests)
    }

    async fn spawn_delayed_sse_server(
        frames: Vec<(Duration, String)>,
    ) -> (String, Arc<StdMutex<Vec<RecordedRequest>>>) {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let requests = Arc::new(StdMutex::new(Vec::new()));
        let shared_requests = requests.clone();

        tokio::spawn(async move {
            let Ok((mut stream, _)) = listener.accept().await else {
                return;
            };
            if let Ok(request) = read_http_request(&mut stream).await {
                shared_requests.lock().unwrap().push(request);
            }
            let headers = concat!(
                "HTTP/1.1 200 OK\r\n",
                "content-type: text/event-stream\r\n",
                "connection: keep-alive\r\n",
                "\r\n"
            );
            let _ = stream.write_all(headers.as_bytes()).await;
            let _ = stream.flush().await;

            for (delay, frame) in frames {
                tokio::time::sleep(delay).await;
                if stream.write_all(frame.as_bytes()).await.is_err() {
                    return;
                }
                let _ = stream.flush().await;
            }
            let _ = stream.shutdown().await;
        });

        (format!("http://{}", address), requests)
    }

    async fn read_http_request(stream: &mut TcpStream) -> std::io::Result<RecordedRequest> {
        let mut buffer = Vec::new();
        let mut chunk = [0_u8; 1024];
        let header_end = loop {
            let read = stream.read(&mut chunk).await?;
            if read == 0 {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::UnexpectedEof,
                    "connection closed before headers",
                ));
            }
            buffer.extend_from_slice(&chunk[..read]);
            if let Some(index) = find_header_end(&buffer) {
                break index;
            }
        };

        let header_text = String::from_utf8_lossy(&buffer[..header_end]);
        let mut lines = header_text.lines();
        let request_line = lines.next().unwrap_or_default();
        let mut request_parts = request_line.split_whitespace();
        let method = request_parts.next().unwrap_or_default().to_string();
        let path = request_parts.next().unwrap_or_default().to_string();
        let mut authorization = None;
        let mut anthropic_version = None;
        let mut accept = None;
        let mut last_event_id = None;
        let mut content_type = None;
        let mut content_length = 0_usize;

        for line in lines {
            let Some((name, value)) = line.split_once(':') else {
                continue;
            };
            let value = value.trim().to_string();
            if name.eq_ignore_ascii_case("authorization") {
                authorization = Some(value);
            } else if name.eq_ignore_ascii_case("anthropic-version") {
                anthropic_version = Some(value);
            } else if name.eq_ignore_ascii_case("accept") {
                accept = Some(value);
            } else if name.eq_ignore_ascii_case("last-event-id") {
                last_event_id = Some(value);
            } else if name.eq_ignore_ascii_case("content-type") {
                content_type = Some(value);
            } else if name.eq_ignore_ascii_case("content-length") {
                content_length = value.parse().unwrap_or_default();
            }
        }

        let body_start = header_end + 4;
        while buffer.len() < body_start + content_length {
            let read = stream.read(&mut chunk).await?;
            if read == 0 {
                break;
            }
            buffer.extend_from_slice(&chunk[..read]);
        }
        let body = String::from_utf8_lossy(
            &buffer[body_start..buffer.len().min(body_start + content_length)],
        )
        .to_string();

        Ok(RecordedRequest {
            method,
            path,
            authorization,
            anthropic_version,
            accept,
            last_event_id,
            content_type,
            body,
        })
    }

    fn find_header_end(buffer: &[u8]) -> Option<usize> {
        buffer.windows(4).position(|window| window == b"\r\n\r\n")
    }

    fn internal_test_event(uuid: &str, payload_size: usize) -> CcrV2WorkerEvent {
        CcrV2WorkerEvent {
            payload: json!({
                "type": "transcript",
                "uuid": uuid,
                "text": "x".repeat(payload_size),
            }),
            is_compaction: None,
            agent_id: None,
        }
    }

    async fn write_http_response(
        stream: &mut TcpStream,
        status: u16,
        body: &str,
        content_type: &str,
    ) -> std::io::Result<()> {
        let reason = match status {
            200 => "OK",
            201 => "Created",
            409 => "Conflict",
            _ => "Status",
        };
        let response = format!(
            "HTTP/1.1 {status} {reason}\r\ncontent-type: {content_type}\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{body}",
            body.len()
        );
        stream.write_all(response.as_bytes()).await?;
        stream.shutdown().await
    }
}

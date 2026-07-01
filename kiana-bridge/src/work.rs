use crate::api::{bridge_error_status, decode_work_secret, BridgeApiClient};
use crate::session::SessionManager;
use crate::transport::{build_sdk_url, BridgeTransport, CcrV2Transport, Transport};
use crate::types::{
    BridgeConfig, ContentBlock, ControlRequestType, ControlResponseType, MessageContent,
    SDKMessage, SessionActivity, SessionHandle, SpawnMode, WorkResponse,
};
use anyhow::{anyhow, Result};
use async_trait::async_trait;
use kiana_remote::{
    build_ccr_v2_sdk_url, register_worker, CcrV2ReconnectPolicy, CcrV2WorkerClient,
};
use reqwest::StatusCode;
use serde_json::{json, Value};
use std::collections::{HashMap, VecDeque};
use std::ffi::OsStr;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{ExitStatus, Stdio};
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};
use tokio::io::{AsyncBufReadExt, AsyncRead, AsyncReadExt, AsyncWriteExt, BufReader};
use tokio::process::{Child, ChildStdin, ChildStdout, Command};
use tokio::sync::{Mutex as AsyncMutex, OwnedSemaphorePermit, Semaphore, TryAcquireError};
use tokio::time::{interval_at, sleep, sleep_until, Duration, Instant, MissedTickBehavior};
use uuid::Uuid;

const BRIDGE_RUNNER_COMMAND_ENV: &str = "KIANA_BRIDGE_RUNNER_COMMAND";
const BRIDGE_RUNNER_ARGS_ENV: &str = "KIANA_BRIDGE_RUNNER_ARGS";
const BRIDGE_RUNNER_MODE_ENV: &str = "KIANA_BRIDGE_RUNNER_MODE";
const BRIDGE_RUNNER_TIMEOUT_MS_ENV: &str = "KIANA_BRIDGE_RUNNER_TIMEOUT_MS";
const BRIDGE_DEBUG_FILE_ENV: &str = "KIANA_BRIDGE_DEBUG_FILE";
const STOP_WORK_MAX_ATTEMPTS: usize = 3;
const STOP_WORK_RETRY_BASE_DELAY_MS: u64 = 1000;

pub struct WorkPollLoop {
    api: Arc<BridgeApiClient>,
    session_manager: Arc<SessionManager>,
    config: BridgeConfig,
    runner: Arc<dyn BridgeSessionRunner>,
    active_work: Arc<AsyncMutex<HashMap<String, ActiveBridgeWork>>>,
}

impl WorkPollLoop {
    pub fn new(
        api: Arc<BridgeApiClient>,
        session_manager: Arc<SessionManager>,
        config: BridgeConfig,
    ) -> Result<Self> {
        let runner = Arc::new(CommandBridgeSessionRunner::for_config(&config)?);
        Ok(Self::with_runner(api, session_manager, config, runner))
    }

    pub fn recording(
        api: Arc<BridgeApiClient>,
        session_manager: Arc<SessionManager>,
        config: BridgeConfig,
    ) -> Self {
        Self::with_runner(
            api,
            session_manager,
            config,
            Arc::new(RecordingBridgeSessionRunner),
        )
    }

    pub fn with_runner(
        api: Arc<BridgeApiClient>,
        session_manager: Arc<SessionManager>,
        config: BridgeConfig,
        runner: Arc<dyn BridgeSessionRunner>,
    ) -> Self {
        Self {
            api,
            session_manager,
            config,
            runner,
            active_work: Arc::new(AsyncMutex::new(HashMap::new())),
        }
    }

    pub async fn run(&self, environment_id: String, environment_secret: String) -> Result<()> {
        let session_permits = Arc::new(Semaphore::new(normalized_max_sessions(
            self.config.max_sessions,
        )));
        loop {
            match self
                .api
                .poll_for_work(&environment_id, &environment_secret)
                .await
            {
                Ok(Some(work)) => {
                    let dispatch = bridge_work_dispatch(
                        &self.session_manager,
                        &self.active_work,
                        self.config.max_sessions,
                        session_permits.clone(),
                        &work,
                    )
                    .await?;
                    let BridgeWorkDispatch::Handle { permit } = dispatch else {
                        sleep(Duration::from_millis(500)).await;
                        continue;
                    };
                    if let Some(permit) = permit {
                        let api = self.api.clone();
                        let session_manager = self.session_manager.clone();
                        let config = self.config.clone();
                        let runner = self.runner.clone();
                        let active_work = self.active_work.clone();
                        let environment_id = environment_id.clone();
                        tokio::spawn(async move {
                            let _permit = permit;
                            if let Err(error) = handle_bridge_work(
                                api,
                                session_manager,
                                config,
                                runner,
                                active_work,
                                work,
                                environment_id,
                            )
                            .await
                            {
                                eprintln!("Work handling error: {}", error);
                            }
                        });
                    } else if let Err(error) = handle_bridge_work(
                        self.api.clone(),
                        self.session_manager.clone(),
                        self.config.clone(),
                        self.runner.clone(),
                        self.active_work.clone(),
                        work,
                        environment_id.clone(),
                    )
                    .await
                    {
                        eprintln!("Work handling error: {}", error);
                    }
                }
                Ok(None) => {
                    sleep(Duration::from_secs(2)).await;
                }
                Err(e) => {
                    eprintln!("Poll error: {}", e);
                    sleep(Duration::from_secs(5)).await;
                }
            }
        }
    }

    pub async fn shutdown_active_work(&self, environment_id: &str) -> Result<()> {
        let active = {
            let mut active_work = self.active_work.lock().await;
            active_work
                .drain()
                .map(|(_, active)| active)
                .collect::<Vec<_>>()
        };
        if active.is_empty() {
            return Ok(());
        }

        let mut errors = Vec::new();
        for active_work in active {
            self.session_manager
                .remove_session(&active_work.session_id)
                .await;
            let lease = active_work.lease_snapshot().await;

            if let Err(error) = self.runner.end_session(&active_work.session_id).await {
                errors.push(format!(
                    "runner cleanup {} failed: {}",
                    active_work.session_id, error
                ));
            }
            if let Err(error) =
                cleanup_bridge_session_work_dir(&self.config, active_work.work_dir.as_deref())
            {
                errors.push(format!(
                    "cleanup {} failed: {}",
                    active_work.session_id, error
                ));
            }
            if let Err(error) =
                stop_bridge_work_with_retry(&self.api, environment_id, &lease.work_id, true).await
            {
                errors.push(format!("stopWork {} failed: {}", lease.work_id, error));
            }
            if let Err(error) = self.api.archive_session(&active_work.session_id).await {
                errors.push(format!(
                    "archiveSession {} failed: {}",
                    active_work.session_id, error
                ));
            }
        }

        if errors.is_empty() {
            Ok(())
        } else {
            Err(anyhow!(
                "bridge shutdown cleanup failed: {}",
                errors.join("; ")
            ))
        }
    }
}

enum BridgeWorkDispatch {
    Handle {
        permit: Option<OwnedSemaphorePermit>,
    },
    DeferAtCapacity,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum BridgeWorkCompletion {
    Complete,
    Reconnect,
}

#[derive(Debug, Clone)]
struct ActiveBridgeWorkLease {
    work_id: String,
    session_ingress_token: String,
}

#[derive(Clone)]
struct ActiveBridgeWork {
    session_id: String,
    work_dir: Option<PathBuf>,
    lease: Arc<AsyncMutex<ActiveBridgeWorkLease>>,
    ccr_v2_client: Option<CcrV2WorkerClient>,
}

impl ActiveBridgeWork {
    fn new(
        work_id: String,
        session_id: String,
        work_dir: Option<PathBuf>,
        session_ingress_token: String,
    ) -> Self {
        Self {
            session_id,
            work_dir,
            lease: Arc::new(AsyncMutex::new(ActiveBridgeWorkLease {
                work_id,
                session_ingress_token,
            })),
            ccr_v2_client: None,
        }
    }

    fn with_ccr_v2_client(mut self, client: CcrV2WorkerClient) -> Self {
        self.ccr_v2_client = Some(client);
        self
    }

    async fn lease_snapshot(&self) -> ActiveBridgeWorkLease {
        self.lease.lock().await.clone()
    }
}

async fn handle_bridge_work(
    api: Arc<BridgeApiClient>,
    session_manager: Arc<SessionManager>,
    config: BridgeConfig,
    runner: Arc<dyn BridgeSessionRunner>,
    active_work: Arc<AsyncMutex<HashMap<String, ActiveBridgeWork>>>,
    work: WorkResponse,
    environment_id: String,
) -> Result<()> {
    if work.data.data_type != "session" {
        let secret = match decode_bridge_work_secret_or_stop(&api, &environment_id, &work).await {
            Ok(secret) => secret,
            Err(error) => return Err(error),
        };
        acknowledge_bridge_work(
            &api,
            &environment_id,
            &work.id,
            &secret.session_ingress_token,
        )
        .await;
        return Ok(());
    }

    let session_id = work.data.id.clone();
    let secret = decode_bridge_work_secret_or_stop(&api, &environment_id, &work).await?;

    if let Some(existing_work) = active_work.lock().await.get(&session_id).cloned() {
        let existing_handle = session_manager.get_session(&session_id).await;
        let refreshed_worker_epoch = if let Some(worker_client) =
            existing_work.ccr_v2_client.as_ref()
        {
            let session_url = existing_handle
                .as_ref()
                .and_then(|handle| handle.sdk_url.clone())
                .unwrap_or_else(|| {
                    build_ccr_v2_sdk_url(&secret.api_base_url, &session_id)
                        .unwrap_or_else(|_| worker_client.session_url().to_string())
                });
            let worker_epoch = register_worker(&session_url, &secret.session_ingress_token).await?;
            worker_client.update_credentials(secret.session_ingress_token.clone(), worker_epoch)?;
            Some(worker_epoch)
        } else {
            None
        };
        {
            let mut lease = existing_work.lease.lock().await;
            lease.work_id = work.id.clone();
            lease.session_ingress_token = secret.session_ingress_token.clone();
        }
        let handle = session_manager
            .update_credentials(
                &session_id,
                secret.session_ingress_token.clone(),
                refreshed_worker_epoch,
            )
            .await
            .unwrap_or_else(|| {
                let use_ccr_v2 = refreshed_worker_epoch.is_some();
                SessionHandle {
                    session_id: session_id.clone(),
                    access_token: secret.session_ingress_token.clone(),
                    sdk_url: existing_handle.and_then(|handle| handle.sdk_url),
                    work_dir: existing_work.work_dir.clone(),
                    use_ccr_v2,
                    worker_epoch: refreshed_worker_epoch,
                }
            });
        session_manager
            .add_activity(
                &session_id,
                activity("token", "Session ingress token refreshed"),
            )
            .await;
        if let Err(error) = runner
            .update_access_token(&handle, secret.session_ingress_token.clone())
            .await
        {
            eprintln!(
                "Bridge session token refresh failed for {}: {}",
                session_id, error
            );
        }
        acknowledge_bridge_work(
            &api,
            &environment_id,
            &work.id,
            &secret.session_ingress_token,
        )
        .await;
        return Ok(());
    }

    acknowledge_bridge_work(
        &api,
        &environment_id,
        &work.id,
        &secret.session_ingress_token,
    )
    .await;

    let session_ingress_token = secret.session_ingress_token.clone();
    let use_ccr_v2 = secret.use_code_sessions.unwrap_or(false);
    let (sdk_url, transport, worker_epoch, ccr_v2_client) = if use_ccr_v2 {
        let session_url = build_ccr_v2_sdk_url(&secret.api_base_url, &session_id)?;
        let worker_epoch = register_worker(&session_url, &session_ingress_token).await?;
        let worker_client = CcrV2WorkerClient::new(
            session_url.clone(),
            session_id.clone(),
            session_ingress_token.clone(),
            worker_epoch,
        )?;
        (
            session_url,
            BridgeTransport::CcrV2(CcrV2Transport::new_with_reconnect_policy(
                worker_client.clone(),
                ccr_v2_sse_reconnect_policy(&config),
            )),
            Some(worker_epoch),
            Some(worker_client),
        )
    } else {
        let ws_url = build_sdk_url(&config.session_ingress_url, &session_id)?;
        (
            ws_url.clone(),
            BridgeTransport::SessionIngress(Transport::new(ws_url, session_ingress_token.clone())),
            None,
            None,
        )
    };
    let work_dir = prepare_bridge_session_work_dir(&config, &session_id)?;

    let handle = SessionHandle {
        session_id: session_id.clone(),
        access_token: session_ingress_token.clone(),
        sdk_url: Some(sdk_url),
        work_dir: Some(work_dir),
        use_ccr_v2,
        worker_epoch,
    };

    session_manager
        .add_session(session_id.clone(), handle.clone())
        .await;
    let mut active_bridge_work = ActiveBridgeWork::new(
        work.id.clone(),
        session_id.clone(),
        handle.work_dir.clone(),
        session_ingress_token.clone(),
    );
    if let Some(client) = ccr_v2_client {
        active_bridge_work = active_bridge_work.with_ccr_v2_client(client);
    }
    active_work
        .lock()
        .await
        .insert(session_id.clone(), active_bridge_work.clone());

    let mut conn = match transport.connect().await {
        Ok(conn) => conn,
        Err(error) => {
            session_manager.remove_session(&session_id).await;
            let active_entry = active_work.lock().await.remove(&session_id);
            if let Err(runner_error) = runner.end_session(&session_id).await {
                eprintln!("Bridge session runner cleanup error: {}", runner_error);
            }
            if let Err(cleanup_error) =
                cleanup_bridge_session_work_dir(&config, handle.work_dir.as_deref())
            {
                eprintln!(
                    "Bridge session work directory cleanup error: {}",
                    cleanup_error
                );
            }
            if let Some(active_entry) = active_entry {
                let lease = active_entry.lease_snapshot().await;
                if let Err(reconnect_error) = finish_bridge_work_server_state(
                    &api,
                    &environment_id,
                    &lease.work_id,
                    &session_id,
                    BridgeWorkCompletion::Reconnect,
                )
                .await
                {
                    eprintln!(
                        "Bridge reconnect after transport setup failure failed: {}",
                        reconnect_error
                    );
                }
            }
            return Err(error);
        }
    };
    let heartbeat_interval = bridge_heartbeat_interval(&config);
    let mut heartbeat = heartbeat_interval.map(|duration| {
        let mut interval = interval_at(Instant::now() + duration, duration);
        interval.set_missed_tick_behavior(MissedTickBehavior::Delay);
        interval
    });
    let mut transport_maintenance = conn.maintenance_interval().map(|duration| {
        let mut interval = interval_at(Instant::now() + duration, duration);
        interval.set_missed_tick_behavior(MissedTickBehavior::Delay);
        interval
    });
    let session_deadline =
        bridge_session_timeout(&config).map(|duration| Instant::now() + duration);
    let mut completion = BridgeWorkCompletion::Complete;

    'session_loop: loop {
        tokio::select! {
            message = conn.recv() => {
                match message {
                    Ok(Some(msg)) => {
                        let responses = match handle_sdk_message(&session_manager, &handle, msg, runner.as_ref()).await {
                            Ok(responses) => responses,
                            Err(error) => vec![bridge_error_message(error.to_string())],
                        };
                        for response in responses {
                            if let Err(error) = conn.send(&response).await {
                                eprintln!("Transport send error: {}", error);
                                if let Err(flush_error) = conn.flush_pending().await {
                                    eprintln!(
                                        "Bridge transport pending flush after send error failed for {}: {}",
                                        session_id, flush_error
                                    );
                                }
                                completion = BridgeWorkCompletion::Reconnect;
                                break 'session_loop;
                            }
                        }
                    }
                    Ok(None) => {
                        if let Err(error) = conn.flush_pending().await {
                            eprintln!("Bridge transport pending flush error for {}: {}", session_id, error);
                            completion = BridgeWorkCompletion::Reconnect;
                            break;
                        }
                        if conn.reconnect_on_eof() {
                            eprintln!("Bridge transport closed before session completion; reconnecting");
                            completion = BridgeWorkCompletion::Reconnect;
                        }
                        break;
                    }
                    Err(error) => {
                        eprintln!("Transport error: {}", error);
                        if let Err(flush_error) = conn.flush_pending().await {
                            eprintln!(
                                "Bridge transport pending flush after error failed for {}: {}",
                                session_id, flush_error
                            );
                        }
                        completion = BridgeWorkCompletion::Reconnect;
                        break;
                    }
                }
            }
            _ = async {
                if let Some(transport_maintenance) = transport_maintenance.as_mut() {
                    transport_maintenance.tick().await;
                }
            }, if transport_maintenance.is_some() => {
                if let Err(error) = conn.maintenance().await {
                    eprintln!("Bridge transport maintenance error for {}: {}", session_id, error);
                    completion = BridgeWorkCompletion::Reconnect;
                    break;
                }
            }
            _ = async {
                if let Some(deadline) = session_deadline {
                    sleep_until(deadline).await;
                }
            }, if session_deadline.is_some() => {
                eprintln!(
                    "Bridge session {} timed out after {} ms",
                    session_id, config.session_timeout_ms
                );
                if let Err(error) = conn.flush_pending().await {
                    eprintln!("Bridge transport pending flush after timeout failed for {}: {}", session_id, error);
                    completion = BridgeWorkCompletion::Reconnect;
                }
                break;
            }
            _ = async {
                if let Some(heartbeat) = heartbeat.as_mut() {
                    heartbeat.tick().await;
                }
            }, if heartbeat.is_some() => {
                let lease = active_bridge_work.lease_snapshot().await;
                match api.heartbeat_work(&environment_id, &lease.work_id, &lease.session_ingress_token).await {
                    Ok(status) if status.lease_extended => {
                        if let Err(error) = conn.heartbeat().await {
                            eprintln!("Bridge transport heartbeat error for {}: {}", session_id, error);
                            completion = BridgeWorkCompletion::Reconnect;
                            break;
                        }
                    }
                    Ok(status) => {
                        eprintln!(
                            "Heartbeat did not extend bridge work lease for {} (state={})",
                            lease.work_id, status.state
                        );
                        completion = BridgeWorkCompletion::Reconnect;
                        break;
                    }
                    Err(error) => {
                        eprintln!("Heartbeat error for bridge work {}: {}", lease.work_id, error);
                        if should_reconnect_after_heartbeat_error(&error) {
                            completion = BridgeWorkCompletion::Reconnect;
                        }
                        break;
                    }
                }
            }
        }
    }

    session_manager.remove_session(&session_id).await;
    let active_entry = active_work.lock().await.remove(&session_id);
    let runner_cleanup_result = runner.end_session(&session_id).await;
    let cleanup_result = cleanup_bridge_session_work_dir(&config, handle.work_dir.as_deref());
    let server_state_result = if let Some(active_entry) = active_entry {
        let lease = active_entry.lease_snapshot().await;
        finish_bridge_work_server_state(
            &api,
            &environment_id,
            &lease.work_id,
            &session_id,
            completion,
        )
        .await
    } else {
        Ok(())
    };
    if let Err(error) = cleanup_result {
        eprintln!("Bridge session work directory cleanup error: {}", error);
        if server_state_result.is_ok() {
            return Err(error);
        }
    }
    if let Err(error) = runner_cleanup_result {
        eprintln!("Bridge session runner cleanup error: {}", error);
        if server_state_result.is_ok() {
            return Err(error);
        }
    }
    server_state_result?;

    Ok(())
}

async fn decode_bridge_work_secret_or_stop(
    api: &BridgeApiClient,
    environment_id: &str,
    work: &WorkResponse,
) -> Result<crate::types::WorkSecret> {
    match decode_work_secret(&work.secret) {
        Ok(secret) => Ok(secret),
        Err(error) => {
            if let Err(stop_error) =
                stop_bridge_work_with_retry(api, environment_id, &work.id, false).await
            {
                eprintln!(
                    "stopWork {} after work secret decode failure failed: {}",
                    work.id, stop_error
                );
            }
            Err(error)
        }
    }
}

async fn acknowledge_bridge_work(
    api: &BridgeApiClient,
    environment_id: &str,
    work_id: &str,
    session_ingress_token: &str,
) {
    if let Err(error) = api
        .acknowledge_work(environment_id, work_id, session_ingress_token)
        .await
    {
        eprintln!("Acknowledge bridge work {} failed: {}", work_id, error);
    }
}

async fn finish_bridge_work_server_state(
    api: &BridgeApiClient,
    environment_id: &str,
    work_id: &str,
    session_id: &str,
    completion: BridgeWorkCompletion,
) -> Result<()> {
    match completion {
        BridgeWorkCompletion::Complete => {
            stop_bridge_work_with_retry(api, environment_id, work_id, false).await?;
            api.archive_session(session_id).await
        }
        BridgeWorkCompletion::Reconnect => api.reconnect_session(environment_id, session_id).await,
    }
}

#[derive(Debug, Clone, Copy)]
struct StopWorkRetryConfig {
    max_attempts: usize,
    base_delay: Duration,
}

impl StopWorkRetryConfig {
    fn production() -> Self {
        Self {
            max_attempts: STOP_WORK_MAX_ATTEMPTS,
            base_delay: Duration::from_millis(STOP_WORK_RETRY_BASE_DELAY_MS),
        }
    }
}

async fn stop_bridge_work_with_retry(
    api: &BridgeApiClient,
    environment_id: &str,
    work_id: &str,
    force: bool,
) -> Result<()> {
    stop_bridge_work_with_retry_config(
        api,
        environment_id,
        work_id,
        force,
        StopWorkRetryConfig::production(),
    )
    .await
}

async fn stop_bridge_work_with_retry_config(
    api: &BridgeApiClient,
    environment_id: &str,
    work_id: &str,
    force: bool,
    retry: StopWorkRetryConfig,
) -> Result<()> {
    let max_attempts = retry.max_attempts.max(1);
    for attempt in 1..=max_attempts {
        match api.stop_work(environment_id, work_id, force).await {
            Ok(()) => return Ok(()),
            Err(error) if should_not_retry_stop_work_error(&error) => return Err(error),
            Err(error) if attempt == max_attempts => {
                return Err(anyhow!(
                    "stopWork {} failed after {} attempt(s): {}",
                    work_id,
                    max_attempts,
                    error
                ));
            }
            Err(error) => {
                let delay = stop_work_retry_delay(retry.base_delay, attempt);
                eprintln!(
                    "stopWork {} failed on attempt {}/{}; retrying in {} ms: {}",
                    work_id,
                    attempt,
                    max_attempts,
                    delay.as_millis(),
                    error
                );
                sleep(delay).await;
            }
        }
    }
    Ok(())
}

fn stop_work_retry_delay(base_delay: Duration, attempt: usize) -> Duration {
    let multiplier = 1_u32 << (attempt.saturating_sub(1).min(31) as u32);
    base_delay * multiplier
}

fn should_not_retry_stop_work_error(error: &anyhow::Error) -> bool {
    matches!(
        bridge_error_status(error),
        Some(
            StatusCode::UNAUTHORIZED
                | StatusCode::FORBIDDEN
                | StatusCode::NOT_FOUND
                | StatusCode::GONE
        )
    )
}

fn should_reconnect_after_heartbeat_error(error: &anyhow::Error) -> bool {
    matches!(
        bridge_error_status(error),
        Some(
            StatusCode::UNAUTHORIZED
                | StatusCode::FORBIDDEN
                | StatusCode::NOT_FOUND
                | StatusCode::GONE
        )
    )
}

#[async_trait]
pub trait BridgeSessionRunner: Send + Sync {
    async fn handle_user_message(
        &self,
        handle: &SessionHandle,
        uuid: String,
        message: MessageContent,
    ) -> Result<SDKMessage>;

    async fn end_session(&self, _session_id: &str) -> Result<()> {
        Ok(())
    }

    async fn handle_control_response(
        &self,
        _handle: &SessionHandle,
        _response: ControlResponseType,
    ) -> Result<Option<SDKMessage>> {
        Ok(None)
    }

    async fn handle_control_cancel_request(
        &self,
        _handle: &SessionHandle,
        _request_id: String,
        _tool_use_id: Option<String>,
    ) -> Result<()> {
        Ok(())
    }

    async fn handle_control_request(
        &self,
        _handle: &SessionHandle,
        _request_id: String,
        _request: ControlRequestType,
    ) -> Result<Option<ControlResponseType>> {
        Ok(None)
    }

    async fn update_access_token(&self, _handle: &SessionHandle, _token: String) -> Result<()> {
        Ok(())
    }

    async fn drain_activities(&self, _handle: &SessionHandle) -> Result<Vec<SessionActivity>> {
        Ok(Vec::new())
    }

    async fn drain_outbound_messages(&self, _handle: &SessionHandle) -> Result<Vec<SDKMessage>> {
        Ok(Vec::new())
    }
}

pub struct RecordingBridgeSessionRunner;

#[async_trait]
impl BridgeSessionRunner for RecordingBridgeSessionRunner {
    async fn handle_user_message(
        &self,
        handle: &SessionHandle,
        _uuid: String,
        _message: MessageContent,
    ) -> Result<SDKMessage> {
        Ok(SDKMessage::Assistant {
            uuid: Uuid::new_v4().to_string(),
            message: MessageContent {
                content: ContentBlock::Text(format!(
                    "Remote message recorded for bridge session {} by the explicit diagnostic recording runner. WorkPollLoop::new and `kiana bridge start` use executing runners.",
                    handle.session_id
                )),
            },
        })
    }
}

#[derive(Clone)]
pub struct CommandBridgeSessionRunner {
    command: String,
    args: Vec<String>,
    use_default_args: bool,
    cwd: Option<PathBuf>,
    timeout: Option<Duration>,
    debug_file: Option<PathBuf>,
    mode: CommandRunnerMode,
    stream_sessions: Arc<AsyncMutex<HashMap<String, Arc<AsyncMutex<StreamJsonRunnerSession>>>>>,
    session_overrides: Arc<AsyncMutex<HashMap<String, CommandRunnerSessionOverrides>>>,
    initial_permission_mode: Option<String>,
}

impl std::fmt::Debug for CommandBridgeSessionRunner {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("CommandBridgeSessionRunner")
            .field("command", &self.command)
            .field("args", &self.args)
            .field("use_default_args", &self.use_default_args)
            .field("cwd", &self.cwd)
            .field("timeout", &self.timeout)
            .field("debug_file", &self.debug_file)
            .field("mode", &self.mode)
            .field("initial_permission_mode", &self.initial_permission_mode)
            .finish_non_exhaustive()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CommandRunnerMode {
    OneShot,
    StreamJson,
}

#[derive(Debug, Clone, Default)]
struct CommandRunnerSessionOverrides {
    model: Option<Option<String>>,
    permission_mode: Option<String>,
    max_thinking_tokens: Option<Option<u64>>,
}

impl CommandBridgeSessionRunner {
    pub fn new<I, S>(command: impl Into<String>, args: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        Self {
            command: command.into(),
            args: args.into_iter().map(Into::into).collect(),
            use_default_args: false,
            cwd: None,
            timeout: None,
            debug_file: None,
            mode: CommandRunnerMode::OneShot,
            stream_sessions: Arc::new(AsyncMutex::new(HashMap::new())),
            session_overrides: Arc::new(AsyncMutex::new(HashMap::new())),
            initial_permission_mode: None,
        }
    }

    pub fn for_config(config: &BridgeConfig) -> Result<Self> {
        let mut runner = Self::from_env_or_default()?
            .with_cwd(config.dir.clone())
            .with_timeout(runner_timeout_for_config(config)?);
        if let Some(debug_file) = &config.debug_file {
            runner = runner.with_debug_file(debug_file);
        }
        if let Some(permission_mode) = &config.permission_mode {
            let Some(permission_mode) = normalize_bridge_permission_mode(permission_mode) else {
                return Err(anyhow!(
                    "bridge permission mode is not supported: {}",
                    permission_mode
                ));
            };
            runner.initial_permission_mode = Some(permission_mode.to_string());
        }
        Ok(runner)
    }

    pub fn from_env_or_default() -> Result<Self> {
        let command =
            std::env::var(BRIDGE_RUNNER_COMMAND_ENV).unwrap_or_else(|_| "kiana".to_string());
        let mode = command_runner_mode_from_env()?;
        let (args, use_default_args) = match std::env::var(BRIDGE_RUNNER_ARGS_ENV) {
            Ok(value) if !value.trim().is_empty() => (split_runner_args(&value)?, false),
            _ => (default_runner_args_for_mode(mode), true),
        };
        let mut runner = Self::new(command, args).with_mode(mode);
        runner.use_default_args = use_default_args;
        if let Ok(value) = std::env::var(BRIDGE_DEBUG_FILE_ENV) {
            let value = value.trim();
            if !value.is_empty() {
                runner = runner.with_debug_file(value);
            }
        }
        Ok(runner)
    }

    pub fn with_cwd(mut self, cwd: impl Into<PathBuf>) -> Self {
        self.cwd = Some(cwd.into());
        self
    }

    pub fn with_timeout(mut self, timeout: Option<Duration>) -> Self {
        self.timeout = timeout;
        self
    }

    pub fn with_timeout_ms(self, timeout_ms: u64) -> Self {
        self.with_timeout(duration_from_timeout_ms(timeout_ms))
    }

    pub fn with_debug_file(mut self, debug_file: impl AsRef<Path>) -> Self {
        self.debug_file = Some(debug_file.as_ref().to_path_buf());
        self
    }

    fn with_mode(mut self, mode: CommandRunnerMode) -> Self {
        self.mode = mode;
        self
    }

    pub fn with_stream_json_mode(self) -> Self {
        self.with_mode(CommandRunnerMode::StreamJson)
    }

    pub fn command(&self) -> &str {
        &self.command
    }

    pub fn args(&self) -> &[String] {
        &self.args
    }

    pub fn cwd(&self) -> Option<&Path> {
        self.cwd.as_deref()
    }

    pub fn timeout(&self) -> Option<Duration> {
        self.timeout
    }

    pub fn debug_file(&self) -> Option<&Path> {
        self.debug_file.as_deref()
    }

    pub fn is_stream_json_mode(&self) -> bool {
        self.mode == CommandRunnerMode::StreamJson
    }

    pub fn initial_permission_mode(&self) -> Option<&str> {
        self.initial_permission_mode.as_deref()
    }

    async fn session_overrides_for(&self, session_id: &str) -> CommandRunnerSessionOverrides {
        self.session_overrides
            .lock()
            .await
            .get(session_id)
            .cloned()
            .unwrap_or_default()
    }

    async fn handle_one_shot_control_request(
        &self,
        handle: &SessionHandle,
        request_id: String,
        request: ControlRequestType,
    ) -> Result<Option<ControlResponseType>> {
        match request {
            ControlRequestType::SetModel { model } => {
                let model = match model {
                    Some(model) => {
                        let model = model.trim().to_string();
                        if model.is_empty() {
                            return Ok(Some(ControlResponseType::Error {
                                request_id,
                                error: "set_model requires a non-empty model or null".to_string(),
                            }));
                        }
                        Some(model)
                    }
                    None => None,
                };
                let mut overrides = self.session_overrides.lock().await;
                let entry = overrides
                    .entry(handle.session_id.clone())
                    .or_insert_with(CommandRunnerSessionOverrides::default);
                entry.model = Some(model.clone());
                Ok(Some(control_success(
                    request_id,
                    Some(HashMap::from([("model".to_string(), json!(model))])),
                )))
            }
            ControlRequestType::SetPermissionMode { mode } => {
                let Some(mode) = mode
                    .as_deref()
                    .and_then(normalize_bridge_permission_mode)
                    .map(str::to_string)
                else {
                    return Ok(Some(ControlResponseType::Error {
                        request_id,
                        error: "set_permission_mode requires a supported mode".to_string(),
                    }));
                };
                let mut overrides = self.session_overrides.lock().await;
                let entry = overrides
                    .entry(handle.session_id.clone())
                    .or_insert_with(CommandRunnerSessionOverrides::default);
                entry.permission_mode = Some(mode.clone());
                Ok(Some(control_success(
                    request_id,
                    Some(HashMap::from([
                        ("mode".to_string(), json!(mode.clone())),
                        ("permission_mode".to_string(), json!(mode.clone())),
                        ("permissionMode".to_string(), json!(mode)),
                    ])),
                )))
            }
            ControlRequestType::SetMaxThinkingTokens {
                max_thinking_tokens,
            } => {
                let mut overrides = self.session_overrides.lock().await;
                let entry = overrides
                    .entry(handle.session_id.clone())
                    .or_insert_with(CommandRunnerSessionOverrides::default);
                entry.max_thinking_tokens = Some(max_thinking_tokens);
                Ok(Some(control_success(
                    request_id,
                    Some(HashMap::from([(
                        "max_thinking_tokens".to_string(),
                        json!(max_thinking_tokens),
                    )])),
                )))
            }
            _ => Ok(None),
        }
    }
}

fn default_runner_args_for_mode(mode: CommandRunnerMode) -> Vec<String> {
    match mode {
        CommandRunnerMode::OneShot => vec![
            "-p".to_string(),
            "--output-format".to_string(),
            "json".to_string(),
            "--execute".to_string(),
            "--".to_string(),
        ],
        CommandRunnerMode::StreamJson => {
            default_stream_json_runner_args("<sdk-url>", "<session-id>")
        }
    }
}

fn default_stream_json_runner_args(sdk_url: &str, session_id: &str) -> Vec<String> {
    vec![
        "--print".to_string(),
        "--sdk-url".to_string(),
        sdk_url.to_string(),
        "--session-id".to_string(),
        session_id.to_string(),
        "--input-format".to_string(),
        "stream-json".to_string(),
        "--output-format".to_string(),
        "stream-json".to_string(),
        "--replay-user-messages".to_string(),
    ]
}

fn split_runner_args(args: &str) -> Result<Vec<String>> {
    shlex::split(args)
        .ok_or_else(|| anyhow!("{} has invalid shell quoting", BRIDGE_RUNNER_ARGS_ENV))
}

fn command_runner_mode_from_env() -> Result<CommandRunnerMode> {
    match std::env::var(BRIDGE_RUNNER_MODE_ENV) {
        Ok(value) if !value.trim().is_empty() => match value.trim() {
            "oneshot" | "one-shot" | "command" => Ok(CommandRunnerMode::OneShot),
            "stream-json" | "stream_json" => Ok(CommandRunnerMode::StreamJson),
            other => Err(anyhow!(
                "{} must be 'oneshot' or 'stream-json', got '{}'",
                BRIDGE_RUNNER_MODE_ENV,
                other
            )),
        },
        _ => Ok(CommandRunnerMode::OneShot),
    }
}

fn runner_timeout_for_config(config: &BridgeConfig) -> Result<Option<Duration>> {
    let timeout_ms = match std::env::var(BRIDGE_RUNNER_TIMEOUT_MS_ENV) {
        Ok(value) if !value.trim().is_empty() => value.trim().parse::<u64>().map_err(|error| {
            anyhow!(
                "{} must be a non-negative integer number of milliseconds: {}",
                BRIDGE_RUNNER_TIMEOUT_MS_ENV,
                error
            )
        })?,
        _ => config.session_timeout_ms,
    };
    Ok(duration_from_timeout_ms(timeout_ms))
}

fn duration_from_timeout_ms(timeout_ms: u64) -> Option<Duration> {
    if timeout_ms == 0 {
        None
    } else {
        Some(Duration::from_millis(timeout_ms))
    }
}

fn normalize_bridge_permission_mode(mode: &str) -> Option<&'static str> {
    match mode.trim() {
        "acceptEdits" | "accept-edits" | "accept_edits" => Some("acceptEdits"),
        "bypassPermissions" | "bypass-permissions" | "bypass_permissions" => {
            Some("bypassPermissions")
        }
        "dontAsk" | "dont-ask" | "dont_ask" => Some("dontAsk"),
        "default" => Some("default"),
        "plan" => Some("plan"),
        "ask" => Some("ask"),
        _ => None,
    }
}

fn control_success(
    request_id: String,
    response: Option<HashMap<String, Value>>,
) -> ControlResponseType {
    ControlResponseType::Success {
        request_id,
        response,
    }
}

fn apply_command_runner_overrides(
    child: &mut Command,
    overrides: &CommandRunnerSessionOverrides,
    initial_permission_mode: Option<&str>,
) {
    match &overrides.model {
        Some(Some(model)) => {
            child.env("ANTHROPIC_MODEL", model);
        }
        Some(None) => {
            child.env_remove("ANTHROPIC_MODEL");
        }
        None => {}
    }
    if let Some(permission_mode) = overrides
        .permission_mode
        .as_deref()
        .or(initial_permission_mode)
    {
        child.env("KIANA_PERMISSION_MODE", permission_mode);
    }
    match overrides.max_thinking_tokens {
        Some(Some(max_thinking_tokens)) => {
            child.env("KIANA_MAX_THINKING_TOKENS", max_thinking_tokens.to_string());
        }
        Some(None) => {
            child.env_remove("KIANA_MAX_THINKING_TOKENS");
        }
        None => {}
    }
}

#[async_trait]
impl BridgeSessionRunner for CommandBridgeSessionRunner {
    async fn handle_user_message(
        &self,
        handle: &SessionHandle,
        _uuid: String,
        message: MessageContent,
    ) -> Result<SDKMessage> {
        match self.mode {
            CommandRunnerMode::OneShot => self.handle_user_message_one_shot(handle, message).await,
            CommandRunnerMode::StreamJson => {
                self.handle_user_message_stream_json(handle, _uuid, message)
                    .await
            }
        }
    }

    async fn end_session(&self, session_id: &str) -> Result<()> {
        self.session_overrides.lock().await.remove(session_id);
        if self.mode != CommandRunnerMode::StreamJson {
            return Ok(());
        }
        let session = self.stream_sessions.lock().await.remove(session_id);
        if let Some(session) = session {
            session.lock().await.terminate().await?;
        }
        Ok(())
    }

    async fn handle_control_response(
        &self,
        handle: &SessionHandle,
        response: ControlResponseType,
    ) -> Result<Option<SDKMessage>> {
        if self.mode != CommandRunnerMode::StreamJson {
            return Ok(None);
        }
        let Some(session) = self
            .stream_sessions
            .lock()
            .await
            .get(&handle.session_id)
            .cloned()
        else {
            return Ok(None);
        };
        let mut session = session.lock().await;
        let response = session.send_control_response(response).await?;
        Ok(Some(response))
    }

    async fn handle_control_cancel_request(
        &self,
        handle: &SessionHandle,
        request_id: String,
        tool_use_id: Option<String>,
    ) -> Result<()> {
        if self.mode != CommandRunnerMode::StreamJson {
            return Ok(());
        }
        let Some(session) = self
            .stream_sessions
            .lock()
            .await
            .get(&handle.session_id)
            .cloned()
        else {
            return Ok(());
        };
        let mut session = session.lock().await;
        session
            .send_control_cancel_request(request_id, tool_use_id)
            .await
    }

    async fn handle_control_request(
        &self,
        handle: &SessionHandle,
        request_id: String,
        request: ControlRequestType,
    ) -> Result<Option<ControlResponseType>> {
        if self.mode != CommandRunnerMode::StreamJson {
            return self
                .handle_one_shot_control_request(handle, request_id, request)
                .await;
        }
        if !should_forward_control_request_to_child(&request) {
            return Ok(None);
        }
        let session = self.stream_json_session_for(handle).await?;
        let mut session = session.lock().await;
        Ok(Some(
            session.send_control_request(request_id, request).await?,
        ))
    }

    async fn update_access_token(&self, handle: &SessionHandle, token: String) -> Result<()> {
        if self.mode != CommandRunnerMode::StreamJson {
            return Ok(());
        }
        let Some(session) = self
            .stream_sessions
            .lock()
            .await
            .get(&handle.session_id)
            .cloned()
        else {
            return Ok(());
        };
        let mut session = session.lock().await;
        session.update_access_token(token).await
    }

    async fn drain_activities(&self, handle: &SessionHandle) -> Result<Vec<SessionActivity>> {
        if self.mode != CommandRunnerMode::StreamJson {
            return Ok(Vec::new());
        }
        let Some(session) = self
            .stream_sessions
            .lock()
            .await
            .get(&handle.session_id)
            .cloned()
        else {
            return Ok(Vec::new());
        };
        let mut session = session.lock().await;
        Ok(session.drain_pending_activities())
    }

    async fn drain_outbound_messages(&self, handle: &SessionHandle) -> Result<Vec<SDKMessage>> {
        if self.mode != CommandRunnerMode::StreamJson {
            return Ok(Vec::new());
        }
        let Some(session) = self
            .stream_sessions
            .lock()
            .await
            .get(&handle.session_id)
            .cloned()
        else {
            return Ok(Vec::new());
        };
        let mut session = session.lock().await;
        Ok(session.drain_pending_outbound_messages())
    }
}

impl CommandBridgeSessionRunner {
    async fn handle_user_message_one_shot(
        &self,
        handle: &SessionHandle,
        message: MessageContent,
    ) -> Result<SDKMessage> {
        let command = self.command.trim();
        if command.is_empty() {
            return Err(anyhow!(
                "bridge command runner requires a non-empty command"
            ));
        }

        let prompt = bridge_prompt_text(&message);
        let overrides = self.session_overrides_for(&handle.session_id).await;
        let mut child = Command::new(command);
        child
            .args(&self.args)
            .arg(&prompt)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true);
        apply_command_runner_overrides(&mut child, &overrides, self.initial_permission_mode());
        prepare_command_runner_child(&mut child);
        let cwd = handle.work_dir.as_deref().or(self.cwd.as_deref());
        if let Some(cwd) = cwd {
            if !cwd.is_dir() {
                return Err(anyhow!(
                    "bridge command runner cwd does not exist or is not a directory: {}",
                    cwd.display()
                ));
            }
            child.current_dir(cwd);
        }
        let output = run_command_runner_child(command, child, self.timeout).await?;

        if !output.status.success() {
            return Err(anyhow!(
                "bridge command runner '{}' exited with status {}: {}",
                command,
                output.status,
                truncate_summary(String::from_utf8_lossy(&output.stderr).to_string())
            ));
        }

        let assistant_text = assistant_text_from_runner_stdout(&output.stdout);
        Ok(SDKMessage::Assistant {
            uuid: Uuid::new_v4().to_string(),
            message: MessageContent {
                content: ContentBlock::Text(assistant_text),
            },
        })
    }

    async fn handle_user_message_stream_json(
        &self,
        handle: &SessionHandle,
        uuid: String,
        message: MessageContent,
    ) -> Result<SDKMessage> {
        let session = self.stream_json_session_for(handle).await?;
        let mut session = session.lock().await;
        let response = session.send_user_message(uuid, message).await;
        response
    }

    async fn stream_json_session_for(
        &self,
        handle: &SessionHandle,
    ) -> Result<Arc<AsyncMutex<StreamJsonRunnerSession>>> {
        if let Some(session) = self
            .stream_sessions
            .lock()
            .await
            .get(&handle.session_id)
            .cloned()
        {
            return Ok(session);
        }

        let session = Arc::new(AsyncMutex::new(
            self.spawn_stream_json_session(handle).await?,
        ));
        self.stream_sessions
            .lock()
            .await
            .insert(handle.session_id.clone(), session.clone());
        Ok(session)
    }

    async fn spawn_stream_json_session(
        &self,
        handle: &SessionHandle,
    ) -> Result<StreamJsonRunnerSession> {
        let command = self.command.trim();
        if command.is_empty() {
            return Err(anyhow!(
                "bridge stream-json runner requires a non-empty command"
            ));
        }

        let debug_paths = self
            .debug_file
            .as_deref()
            .map(|base| bridge_session_debug_paths(base, &handle.session_id));
        let mut args = self.stream_json_args_for_handle(handle)?;
        if self.use_default_args {
            if let Some(paths) = &debug_paths {
                args.push("--debug-file".to_string());
                args.push(paths.debug_file.to_string_lossy().to_string());
            }
        }
        let mut debug_log = match debug_paths.as_ref() {
            Some(paths) => BridgeDebugLog::open(&paths.debug_file).await,
            None => None,
        };
        if let Some(log) = &mut debug_log {
            if let Some(paths) = &debug_paths {
                log.write_line(format!(
                    "[bridge:session] Transcript log: {}",
                    paths.transcript_file.display()
                ))
                .await;
            }
            log.write_line(format!(
                "[bridge:session] Spawning sessionId={} sdkUrl={} accessToken={}",
                handle.session_id,
                handle.sdk_url.as_deref().unwrap_or("<missing>"),
                if handle.access_token.is_empty() {
                    "MISSING"
                } else {
                    "present"
                }
            ))
            .await;
            log.write_line(format!(
                "[bridge:session] Child args: {}",
                debug_command_args(&args)
            ))
            .await;
        }
        let mut command_builder = Command::new(command);
        command_builder
            .args(&args)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true);
        prepare_command_runner_child(&mut command_builder);
        let cwd = handle.work_dir.as_deref().or(self.cwd.as_deref());
        if let Some(cwd) = cwd {
            if !cwd.is_dir() {
                return Err(anyhow!(
                    "bridge stream-json runner cwd does not exist or is not a directory: {}",
                    cwd.display()
                ));
            }
            command_builder.current_dir(cwd);
        }
        command_builder.env("KIANA_BRIDGE_SESSION_ID", &handle.session_id);
        command_builder.env("KIANA_BRIDGE_SESSION_ACCESS_TOKEN", &handle.access_token);
        command_builder.env_remove("CLAUDE_CODE_OAUTH_TOKEN");
        command_builder.env("CLAUDE_CODE_ENVIRONMENT_KIND", "bridge");
        command_builder.env("CLAUDE_CODE_SESSION_ACCESS_TOKEN", &handle.access_token);
        command_builder.env("CLAUDE_CODE_POST_FOR_SESSION_INGRESS_V2", "1");
        command_builder.env_remove("CLAUDE_CODE_USE_CCR_V2");
        command_builder.env_remove("CLAUDE_CODE_WORKER_EPOCH");
        if handle.use_ccr_v2 {
            let worker_epoch = handle.worker_epoch.ok_or_else(|| {
                anyhow!("CCR v2 bridge sessions require a registered worker epoch")
            })?;
            command_builder.env("CLAUDE_CODE_USE_CCR_V2", "1");
            command_builder.env("CLAUDE_CODE_WORKER_EPOCH", worker_epoch.to_string());
        }
        if let Some(permission_mode) = self.initial_permission_mode() {
            command_builder.env("KIANA_PERMISSION_MODE", permission_mode);
        }

        let mut child = command_builder.spawn()?;
        let child_pid = child.id();
        if let Some(log) = &mut debug_log {
            log.write_line(format!(
                "[bridge:session] sessionId={} pid={}",
                handle.session_id,
                child_pid
                    .map(|pid| pid.to_string())
                    .unwrap_or_else(|| "unknown".to_string())
            ))
            .await;
        }
        let stdin = child
            .stdin
            .take()
            .ok_or_else(|| anyhow!("bridge stream-json runner stdin pipe was not available"))?;
        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| anyhow!("bridge stream-json runner stdout pipe was not available"))?;
        let stderr = child
            .stderr
            .take()
            .ok_or_else(|| anyhow!("bridge stream-json runner stderr pipe was not available"))?;

        let transcript_log = match debug_paths.as_ref() {
            Some(paths) => BridgeDebugLog::open(&paths.transcript_file).await,
            None => None,
        };

        Ok(StreamJsonRunnerSession::new(
            handle.session_id.clone(),
            command.to_string(),
            child,
            stdin,
            stdout,
            stderr,
            self.timeout,
            debug_log,
            transcript_log,
        ))
    }

    fn stream_json_args_for_handle(&self, handle: &SessionHandle) -> Result<Vec<String>> {
        if !self.use_default_args {
            return Ok(self.args.clone());
        }
        let sdk_url = handle.sdk_url.as_deref().ok_or_else(|| {
            anyhow!("bridge stream-json runner default arguments require a session SDK URL")
        })?;
        let mut args = default_stream_json_runner_args(sdk_url, &handle.session_id);
        if let Some(permission_mode) = self.initial_permission_mode() {
            args.push("--permission-mode".to_string());
            args.push(permission_mode.to_string());
        }
        Ok(args)
    }
}

struct StreamJsonRunnerSession {
    session_id: String,
    command_name: String,
    child: Child,
    stdin: ChildStdin,
    stdout_lines: tokio::io::Lines<BufReader<ChildStdout>>,
    stderr_lines: Arc<AsyncMutex<VecDeque<String>>>,
    stderr_task: tokio::task::JoinHandle<()>,
    pending_activities: Vec<SessionActivity>,
    pending_outbound_messages: Vec<SDKMessage>,
    timeout: Option<Duration>,
    debug_log: Option<BridgeDebugLog>,
    transcript_log: Option<BridgeDebugLog>,
    terminated: bool,
}

impl StreamJsonRunnerSession {
    fn new(
        session_id: String,
        command_name: String,
        child: Child,
        stdin: ChildStdin,
        stdout: ChildStdout,
        stderr: tokio::process::ChildStderr,
        timeout: Option<Duration>,
        debug_log: Option<BridgeDebugLog>,
        transcript_log: Option<BridgeDebugLog>,
    ) -> Self {
        let stderr_lines = Arc::new(AsyncMutex::new(VecDeque::new()));
        let stderr_task = tokio::spawn(buffer_child_stderr(stderr, stderr_lines.clone()));
        Self {
            session_id,
            command_name,
            child,
            stdin,
            stdout_lines: BufReader::new(stdout).lines(),
            stderr_lines,
            stderr_task,
            pending_activities: Vec::new(),
            pending_outbound_messages: Vec::new(),
            timeout,
            debug_log,
            transcript_log,
            terminated: false,
        }
    }

    async fn send_user_message(
        &mut self,
        uuid: String,
        message: MessageContent,
    ) -> Result<SDKMessage> {
        self.write_sdk_message(&SDKMessage::User { uuid, message })
            .await?;
        self.read_response().await
    }

    async fn send_control_response(&mut self, response: ControlResponseType) -> Result<SDKMessage> {
        self.write_sdk_message(&SDKMessage::ControlResponse { response })
            .await?;
        self.read_response().await
    }

    async fn send_control_cancel_request(
        &mut self,
        request_id: String,
        tool_use_id: Option<String>,
    ) -> Result<()> {
        self.write_sdk_message(&SDKMessage::ControlCancelRequest {
            request_id,
            tool_use_id,
        })
        .await
    }

    async fn send_control_request(
        &mut self,
        request_id: String,
        request: ControlRequestType,
    ) -> Result<ControlResponseType> {
        self.write_sdk_message(&SDKMessage::ControlRequest {
            request_id,
            request,
        })
        .await?;
        self.read_control_response().await
    }

    async fn update_access_token(&mut self, token: String) -> Result<()> {
        self.write_sdk_message(&SDKMessage::UpdateEnvironmentVariables {
            variables: HashMap::from([("CLAUDE_CODE_SESSION_ACCESS_TOKEN".to_string(), token)]),
        })
        .await
    }

    async fn write_sdk_message(&mut self, msg: &SDKMessage) -> Result<()> {
        let line = serde_json::to_string(msg)?;
        let debug_line = debug_sdk_message_line(msg, &line);
        self.write_debug_line(format!(
            "[bridge:ws] sessionId={} >>> {}",
            self.session_id,
            truncate_summary(debug_line)
        ))
        .await;
        self.stdin.write_all(line.as_bytes()).await?;
        self.stdin.write_all(b"\n").await?;
        self.stdin.flush().await?;
        Ok(())
    }

    async fn read_response(&mut self) -> Result<SDKMessage> {
        self.read_response_message(false).await
    }

    async fn read_control_response(&mut self) -> Result<ControlResponseType> {
        match self.read_response_message(true).await? {
            SDKMessage::ControlResponse { response } => Ok(response),
            other => Err(anyhow!(
                "bridge stream-json runner '{}' emitted {:?} while waiting for control_response",
                self.command_name,
                other
            )),
        }
    }

    async fn read_response_message(&mut self, accept_control_response: bool) -> Result<SDKMessage> {
        loop {
            let line = match self.timeout {
                Some(timeout) => {
                    match tokio::time::timeout(timeout, self.stdout_lines.next_line()).await {
                        Ok(line) => line?,
                        Err(_) => {
                            let stderr = self.stderr_summary().await;
                            self.terminate().await?;
                            return Err(anyhow!(
                            "bridge stream-json runner '{}' timed out after {} ms waiting for output; stderr: {}",
                            self.command_name,
                            timeout.as_millis(),
                            stderr
                        ));
                        }
                    }
                }
                None => self.stdout_lines.next_line().await?,
            };

            let Some(line) = line else {
                let stderr = self.stderr_summary().await;
                self.write_debug_line(format!(
                    "[bridge:session] sessionId={} exited before response; stderr: {}",
                    self.session_id, stderr
                ))
                .await;
                return Err(anyhow!(
                    "bridge stream-json runner '{}' exited before sending a response; stderr: {}",
                    self.command_name,
                    stderr
                ));
            };
            self.write_transcript_line(&line).await;
            self.write_debug_line(format!(
                "[bridge:ws] sessionId={} <<< {}",
                self.session_id,
                truncate_summary(line.clone())
            ))
            .await;
            let line = line.trim();
            if line.is_empty() {
                continue;
            }

            let msg: SDKMessage = serde_json::from_str(line).map_err(|error| {
                anyhow!(
                    "bridge stream-json runner '{}' emitted invalid SDK JSON: {}; line: {}",
                    self.command_name,
                    error,
                    truncate_summary(line.to_string())
                )
            })?;
            match msg {
                SDKMessage::Assistant { .. } | SDKMessage::ControlRequest { .. } => return Ok(msg),
                SDKMessage::ControlResponse { .. } if accept_control_response => return Ok(msg),
                msg @ SDKMessage::ControlCancelRequest { .. } => {
                    self.pending_outbound_messages.push(msg);
                    continue;
                }
                SDKMessage::User { .. }
                | SDKMessage::ControlResponse { .. }
                | SDKMessage::UpdateEnvironmentVariables { .. }
                | SDKMessage::Unknown => continue,
                SDKMessage::Result { .. }
                | SDKMessage::System { .. }
                | SDKMessage::StreamEvent { .. }
                | SDKMessage::ToolProgress { .. } => {
                    self.pending_outbound_messages.push(msg);
                    continue;
                }
            }
        }
    }

    fn drain_pending_activities(&mut self) -> Vec<SessionActivity> {
        std::mem::take(&mut self.pending_activities)
    }

    fn drain_pending_outbound_messages(&mut self) -> Vec<SDKMessage> {
        std::mem::take(&mut self.pending_outbound_messages)
    }

    async fn stderr_summary(&self) -> String {
        let stderr = self.stderr_lines.lock().await;
        if stderr.is_empty() {
            "<empty>".to_string()
        } else {
            truncate_summary(stderr.iter().cloned().collect::<Vec<_>>().join("\n"))
        }
    }

    async fn terminate(&mut self) -> Result<()> {
        if self.terminated {
            return Ok(());
        }
        self.terminated = true;
        let _ = self.stdin.shutdown().await;
        terminate_command_runner_child(&mut self.child)?;
        let _ = self.child.wait().await;
        let stderr = self.stderr_summary().await;
        self.write_debug_line(format!(
            "[bridge:session] sessionId={} terminated; stderr: {}",
            self.session_id, stderr
        ))
        .await;
        self.stderr_task.abort();
        Ok(())
    }

    async fn write_debug_line(&mut self, line: String) {
        if let Some(log) = &mut self.debug_log {
            log.write_line(line).await;
        }
    }

    async fn write_transcript_line(&mut self, line: &str) {
        if let Some(log) = &mut self.transcript_log {
            log.write_line(line).await;
        }
    }
}

#[derive(Debug, Clone)]
struct BridgeDebugPaths {
    debug_file: PathBuf,
    transcript_file: PathBuf,
}

fn bridge_session_debug_paths(base_file: &Path, session_id: &str) -> BridgeDebugPaths {
    let safe_id = safe_bridge_session_slug(session_id);
    let parent = base_file
        .parent()
        .filter(|path| !path.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    let file_name = base_file
        .file_name()
        .and_then(OsStr::to_str)
        .unwrap_or("bridge-session.log");
    let debug_name = if let Some((stem, extension)) = file_name.rsplit_once('.') {
        if stem.is_empty() {
            format!("{file_name}-{safe_id}")
        } else {
            format!("{stem}-{safe_id}.{extension}")
        }
    } else {
        format!("{file_name}-{safe_id}")
    };
    BridgeDebugPaths {
        debug_file: parent.join(debug_name),
        transcript_file: parent.join(format!("bridge-transcript-{safe_id}.jsonl")),
    }
}

struct BridgeDebugLog {
    file: tokio::fs::File,
}

impl BridgeDebugLog {
    async fn open(path: &Path) -> Option<Self> {
        if let Some(parent) = path.parent().filter(|path| !path.as_os_str().is_empty()) {
            if tokio::fs::create_dir_all(parent).await.is_err() {
                return None;
            }
        }
        let file = tokio::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(path)
            .await
            .ok()?;
        Some(Self { file })
    }

    async fn write_line(&mut self, line: impl AsRef<str>) {
        let _ = self.file.write_all(line.as_ref().as_bytes()).await;
        let _ = self.file.write_all(b"\n").await;
        let _ = self.file.flush().await;
    }
}

fn debug_command_args(args: &[String]) -> String {
    args.iter()
        .map(|arg| {
            if arg.chars().any(char::is_whitespace) {
                format!("{arg:?}")
            } else {
                arg.clone()
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

fn debug_sdk_message_line(msg: &SDKMessage, raw_line: &str) -> String {
    match msg {
        SDKMessage::UpdateEnvironmentVariables { variables } => {
            let mut redacted = variables.clone();
            for (key, value) in &mut redacted {
                let key = key.to_ascii_uppercase();
                if key.contains("TOKEN") || key.contains("AUTH") || key.contains("SECRET") {
                    *value = "[redacted]".to_string();
                }
            }
            serde_json::to_string(&json!({
                "type": "update_environment_variables",
                "variables": redacted,
            }))
            .unwrap_or_else(|_| "<update_environment_variables>".to_string())
        }
        _ => raw_line.to_string(),
    }
}

async fn buffer_child_stderr(
    stderr: tokio::process::ChildStderr,
    stderr_lines: Arc<AsyncMutex<VecDeque<String>>>,
) {
    let mut lines = BufReader::new(stderr).lines();
    while let Ok(Some(line)) = lines.next_line().await {
        let mut stderr_lines = stderr_lines.lock().await;
        if stderr_lines.len() >= 10 {
            stderr_lines.pop_front();
        }
        stderr_lines.push_back(line);
    }
}

struct RunnerProcessOutput {
    status: ExitStatus,
    stdout: Vec<u8>,
    stderr: Vec<u8>,
}

async fn run_command_runner_child(
    command_name: &str,
    mut command: Command,
    timeout: Option<Duration>,
) -> Result<RunnerProcessOutput> {
    let mut child = command.spawn()?;
    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| anyhow!("bridge command runner stdout pipe was not available"))?;
    let stderr = child
        .stderr
        .take()
        .ok_or_else(|| anyhow!("bridge command runner stderr pipe was not available"))?;
    let stdout_task = tokio::spawn(read_child_pipe(stdout));
    let stderr_task = tokio::spawn(read_child_pipe(stderr));

    let status_result = wait_for_command_runner_child(command_name, &mut child, timeout).await;
    let stdout = join_child_pipe(stdout_task, "stdout").await?;
    let stderr = join_child_pipe(stderr_task, "stderr").await?;
    let status = status_result.map_err(|error| {
        anyhow!(
            "{}; stderr: {}",
            error,
            truncate_summary(String::from_utf8_lossy(&stderr).to_string())
        )
    })?;

    Ok(RunnerProcessOutput {
        status,
        stdout,
        stderr,
    })
}

async fn wait_for_command_runner_child(
    command_name: &str,
    child: &mut tokio::process::Child,
    timeout: Option<Duration>,
) -> Result<ExitStatus> {
    if let Some(timeout) = timeout {
        match tokio::time::timeout(timeout, child.wait()).await {
            Ok(status) => Ok(status?),
            Err(_) => {
                if let Err(error) = terminate_command_runner_child(child) {
                    return Err(anyhow!(
                        "bridge command runner '{}' timed out after {} ms and kill failed: {}",
                        command_name,
                        timeout.as_millis(),
                        error
                    ));
                }
                let _ = child.wait().await;
                Err(anyhow!(
                    "bridge command runner '{}' timed out after {} ms",
                    command_name,
                    timeout.as_millis()
                ))
            }
        }
    } else {
        Ok(child.wait().await?)
    }
}

#[cfg(unix)]
fn prepare_command_runner_child(command: &mut Command) {
    command.process_group(0);
}

#[cfg(not(unix))]
fn prepare_command_runner_child(_command: &mut Command) {}

#[cfg(unix)]
fn terminate_command_runner_child(child: &mut tokio::process::Child) -> std::io::Result<()> {
    let Some(pid) = child.id() else {
        return child.start_kill();
    };
    let pgid = i32::try_from(pid).map_err(|_| {
        std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            format!("child pid {pid} does not fit pid_t"),
        )
    })?;
    let result = unsafe { libc::kill(-pgid, libc::SIGKILL) };
    if result == 0 {
        Ok(())
    } else {
        let group_error = std::io::Error::last_os_error();
        child.start_kill().map_err(|child_error| {
            std::io::Error::new(
                child_error.kind(),
                format!(
                    "process-group kill failed: {group_error}; child kill failed: {child_error}"
                ),
            )
        })
    }
}

#[cfg(windows)]
fn terminate_command_runner_child(child: &mut tokio::process::Child) -> std::io::Result<()> {
    let Some(pid) = child.id() else {
        return child.start_kill();
    };
    let status = std::process::Command::new("taskkill")
        .arg("/PID")
        .arg(pid.to_string())
        .arg("/T")
        .arg("/F")
        .status()?;
    if status.success() {
        Ok(())
    } else {
        child.start_kill()
    }
}

#[cfg(not(any(unix, windows)))]
fn terminate_command_runner_child(child: &mut tokio::process::Child) -> std::io::Result<()> {
    child.start_kill()
}

async fn read_child_pipe<R>(mut pipe: R) -> std::io::Result<Vec<u8>>
where
    R: AsyncRead + Unpin,
{
    let mut output = Vec::new();
    pipe.read_to_end(&mut output).await?;
    Ok(output)
}

async fn join_child_pipe(
    task: tokio::task::JoinHandle<std::io::Result<Vec<u8>>>,
    stream_name: &str,
) -> Result<Vec<u8>> {
    task.await
        .map_err(|error| anyhow!("bridge command runner {stream_name} reader failed: {error}"))?
        .map_err(|error| anyhow!("bridge command runner {stream_name} read failed: {error}"))
}

pub async fn handle_sdk_message(
    session_manager: &SessionManager,
    handle: &SessionHandle,
    msg: SDKMessage,
    runner: &dyn BridgeSessionRunner,
) -> Result<Vec<SDKMessage>> {
    match msg {
        SDKMessage::User { uuid, message } => {
            let summary = content_summary(&message);
            session_manager
                .add_activity(
                    &handle.session_id,
                    activity("user", format!("{}: {}", uuid, summary)),
                )
                .await;

            let response = runner.handle_user_message(handle, uuid, message).await?;
            collect_runner_output_messages(session_manager, handle, runner, Some(response)).await
        }
        SDKMessage::ControlRequest {
            request_id,
            request,
        } => {
            let subtype = control_request_subtype(&request).to_string();
            let response = runner
                .handle_control_request(handle, request_id.clone(), request.clone())
                .await?
                .unwrap_or_else(|| control_response(handle, request_id, request));
            session_manager
                .add_activity(
                    &handle.session_id,
                    activity(
                        "control",
                        format!(
                            "control request {subtype} -> {}",
                            control_response_subtype(&response)
                        ),
                    ),
                )
                .await;
            collect_runner_output_messages(
                session_manager,
                handle,
                runner,
                Some(SDKMessage::ControlResponse { response }),
            )
            .await
        }
        SDKMessage::Assistant { uuid, message } => {
            session_manager
                .add_activity(
                    &handle.session_id,
                    activity(
                        "assistant",
                        format!("{}: {}", uuid, content_summary(&message)),
                    ),
                )
                .await;
            Ok(Vec::new())
        }
        SDKMessage::ControlResponse { response } => {
            session_manager
                .add_activity(
                    &handle.session_id,
                    activity("control_response", format!("{:?}", response)),
                )
                .await;
            let response = runner.handle_control_response(handle, response).await?;
            collect_runner_output_messages(session_manager, handle, runner, response).await
        }
        SDKMessage::ControlCancelRequest {
            request_id,
            tool_use_id,
        } => {
            session_manager
                .add_activity(
                    &handle.session_id,
                    activity(
                        "permission_cancelled",
                        format!("Permission request cancelled: {request_id}"),
                    ),
                )
                .await;
            runner
                .handle_control_cancel_request(handle, request_id, tool_use_id)
                .await?;
            collect_runner_output_messages(session_manager, handle, runner, None).await
        }
        SDKMessage::UpdateEnvironmentVariables { variables } => {
            session_manager
                .add_activity(
                    &handle.session_id,
                    activity(
                        "environment",
                        format!("updated {} variable(s)", variables.len()),
                    ),
                )
                .await;
            Ok(Vec::new())
        }
        SDKMessage::Result { .. }
        | SDKMessage::System { .. }
        | SDKMessage::StreamEvent { .. }
        | SDKMessage::ToolProgress { .. }
        | SDKMessage::Unknown => {
            add_sdk_message_activities(session_manager, &handle.session_id, &msg).await;
            Ok(Vec::new())
        }
    }
}

async fn collect_runner_output_messages(
    session_manager: &SessionManager,
    handle: &SessionHandle,
    runner: &dyn BridgeSessionRunner,
    response: Option<SDKMessage>,
) -> Result<Vec<SDKMessage>> {
    add_runner_activities(session_manager, handle, runner).await?;
    let mut responses = runner.drain_outbound_messages(handle).await?;
    if let Some(response) = response {
        responses = order_runner_response_messages(responses, response);
    }
    for response in &responses {
        add_sdk_message_activities(session_manager, &handle.session_id, response).await;
    }
    Ok(responses)
}

fn order_runner_response_messages(
    pending: Vec<SDKMessage>,
    response: SDKMessage,
) -> Vec<SDKMessage> {
    if !matches!(response, SDKMessage::Assistant { .. }) {
        let mut responses = pending;
        responses.push(response);
        return responses;
    }

    let mut before_response = Vec::with_capacity(pending.len() + 1);
    let mut terminal_results = Vec::new();
    for message in pending {
        if matches!(message, SDKMessage::Result { .. }) {
            terminal_results.push(message);
        } else {
            before_response.push(message);
        }
    }
    before_response.push(response);
    before_response.extend(terminal_results);
    before_response
}

async fn add_runner_activities(
    session_manager: &SessionManager,
    handle: &SessionHandle,
    runner: &dyn BridgeSessionRunner,
) -> Result<()> {
    for activity in runner.drain_activities(handle).await? {
        session_manager
            .add_activity(&handle.session_id, activity)
            .await;
    }
    Ok(())
}

async fn add_sdk_message_activities(
    session_manager: &SessionManager,
    session_id: &str,
    msg: &SDKMessage,
) {
    for activity in sdk_message_activities(msg) {
        session_manager.add_activity(session_id, activity).await;
    }
}

fn sdk_message_activities(msg: &SDKMessage) -> Vec<SessionActivity> {
    match msg {
        SDKMessage::Assistant { message, .. } => assistant_message_activities(message),
        SDKMessage::ControlRequest {
            request:
                ControlRequestType::CanUseTool {
                    tool_name, input, ..
                },
            ..
        } => vec![activity(
            "permission_request",
            format!("Permission requested: {}", tool_summary(tool_name, input)),
        )],
        SDKMessage::ControlCancelRequest { request_id, .. } => vec![activity(
            "permission_cancelled",
            format!("Permission request cancelled: {request_id}"),
        )],
        SDKMessage::Result { data } => result_message_activities(data),
        SDKMessage::System { data } => system_message_activities(data),
        SDKMessage::StreamEvent { data } => stream_event_activities(data),
        SDKMessage::ToolProgress { data } => tool_progress_activities(data),
        _ => Vec::new(),
    }
}

fn result_message_activities(data: &HashMap<String, Value>) -> Vec<SessionActivity> {
    let subtype = data
        .get("subtype")
        .and_then(Value::as_str)
        .unwrap_or("unknown");
    let is_error = data
        .get("is_error")
        .and_then(Value::as_bool)
        .unwrap_or(subtype != "success");
    if is_error {
        let summary = data
            .get("error")
            .or_else(|| data.get("result"))
            .or_else(|| data.get("message"))
            .and_then(Value::as_str)
            .filter(|value| !value.trim().is_empty())
            .unwrap_or("Session failed");
        vec![activity("error", summary)]
    } else {
        vec![activity("result", "Session completed")]
    }
}

fn system_message_activities(data: &HashMap<String, Value>) -> Vec<SessionActivity> {
    match data.get("subtype").and_then(Value::as_str) {
        Some("init") => vec![activity("system", "Session initialized")],
        Some(subtype) => vec![activity("system", format!("System event: {subtype}"))],
        None => Vec::new(),
    }
}

fn stream_event_activities(data: &HashMap<String, Value>) -> Vec<SessionActivity> {
    let Some(event) = data.get("event").and_then(Value::as_object) else {
        return Vec::new();
    };
    match event.get("type").and_then(Value::as_str) {
        Some("content_block_start") => event
            .get("content_block")
            .and_then(Value::as_object)
            .and_then(|block| block.get("type").and_then(Value::as_str))
            .map(|block_type| activity("stream_event", format!("Started {block_type} block")))
            .into_iter()
            .collect(),
        Some("content_block_delta") => event
            .get("delta")
            .and_then(Value::as_object)
            .and_then(|delta| delta.get("text").and_then(Value::as_str))
            .map(str::trim)
            .filter(|text| !text.is_empty())
            .map(|text| activity("text", text))
            .into_iter()
            .collect(),
        _ => Vec::new(),
    }
}

fn tool_progress_activities(data: &HashMap<String, Value>) -> Vec<SessionActivity> {
    let tool_name = data
        .get("tool_name")
        .or_else(|| data.get("toolName"))
        .and_then(Value::as_str)
        .unwrap_or("Tool");
    let summary = data
        .get("summary")
        .or_else(|| data.get("message"))
        .and_then(Value::as_str)
        .filter(|value| !value.trim().is_empty())
        .map(str::to_string)
        .unwrap_or_else(|| format!("{tool_name} progress"));
    vec![activity("tool_progress", summary)]
}

fn assistant_message_activities(message: &MessageContent) -> Vec<SessionActivity> {
    match &message.content {
        ContentBlock::Text(text) => {
            let text = text.trim();
            if text.is_empty() {
                Vec::new()
            } else {
                vec![activity("text", text)]
            }
        }
        ContentBlock::Blocks(blocks) => {
            blocks.iter().filter_map(assistant_block_activity).collect()
        }
    }
}

fn assistant_block_activity(block: &HashMap<String, Value>) -> Option<SessionActivity> {
    match block.get("type").and_then(Value::as_str) {
        Some("tool_use") => {
            let name = block.get("name").and_then(Value::as_str).unwrap_or("Tool");
            let input = block
                .get("input")
                .and_then(Value::as_object)
                .map(|object| {
                    object
                        .iter()
                        .map(|(key, value)| (key.clone(), value.clone()))
                        .collect::<HashMap<_, _>>()
                })
                .unwrap_or_default();
            Some(activity("tool_start", tool_summary(name, &input)))
        }
        Some("text") => block
            .get("text")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|text| !text.is_empty())
            .map(|text| activity("text", text)),
        _ => None,
    }
}

fn tool_summary(name: &str, input: &HashMap<String, Value>) -> String {
    let verb = match name {
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
    let target = tool_summary_target(input);
    if target.is_empty() {
        verb.to_string()
    } else {
        format!("{verb} {target}")
    }
}

fn tool_summary_target(input: &HashMap<String, Value>) -> String {
    for key in [
        "file_path",
        "filePath",
        "pattern",
        "command",
        "url",
        "query",
    ] {
        if let Some(value) = input.get(key).and_then(Value::as_str) {
            let value = if key == "command" {
                value.chars().take(60).collect::<String>()
            } else {
                value.to_string()
            };
            if !value.trim().is_empty() {
                return value;
            }
        }
    }
    String::new()
}

fn should_forward_control_request_to_child(request: &ControlRequestType) -> bool {
    matches!(
        request,
        ControlRequestType::Initialize
            | ControlRequestType::Interrupt
            | ControlRequestType::SetModel { .. }
            | ControlRequestType::SetMaxThinkingTokens { .. }
            | ControlRequestType::SetPermissionMode { .. }
            | ControlRequestType::McpStatus
    )
}

fn control_response(
    handle: &SessionHandle,
    request_id: String,
    request: ControlRequestType,
) -> ControlResponseType {
    let mut response = HashMap::<String, Value>::new();
    response.insert("session_id".to_string(), json!(handle.session_id));

    match request {
        ControlRequestType::Initialize => {
            response.insert("status".to_string(), json!("ready"));
            response.insert("commands".to_string(), json!([]));
            response.insert("output_style".to_string(), json!("normal"));
            response.insert("available_output_styles".to_string(), json!(["normal"]));
            response.insert("models".to_string(), json!([]));
            response.insert("account".to_string(), json!({}));
            response.insert("pid".to_string(), json!(std::process::id()));
        }
        ControlRequestType::Interrupt => {
            response.insert("interrupted".to_string(), json!(true));
        }
        ControlRequestType::SetModel { model } => {
            response.insert("model".to_string(), json!(model));
        }
        ControlRequestType::SetMaxThinkingTokens {
            max_thinking_tokens,
        } => {
            response.insert(
                "max_thinking_tokens".to_string(),
                json!(max_thinking_tokens),
            );
        }
        ControlRequestType::McpStatus => {
            response.insert("mcpServers".to_string(), json!([]));
        }
        ControlRequestType::SetPermissionMode { mode } => {
            return ControlResponseType::Error {
                request_id,
                error: match mode {
                    Some(mode) => format!(
                        "set_permission_mode is not supported by the current Rust bridge runner: {mode}"
                    ),
                    None => {
                        "set_permission_mode requires a mode value".to_string()
                    }
                },
            };
        }
        ControlRequestType::CanUseTool { .. } => {
            return ControlResponseType::Error {
                request_id,
                error: "bridge-local can_use_tool requests must be handled by the session runner"
                    .to_string(),
            };
        }
        ControlRequestType::Unknown => {
            return ControlResponseType::Error {
                request_id,
                error: "REPL bridge does not handle control_request subtype: unknown".to_string(),
            };
        }
    }

    ControlResponseType::Success {
        request_id,
        response: Some(response),
    }
}

fn control_request_subtype(request: &ControlRequestType) -> &'static str {
    match request {
        ControlRequestType::Initialize => "initialize",
        ControlRequestType::Interrupt => "interrupt",
        ControlRequestType::SetModel { .. } => "set_model",
        ControlRequestType::SetMaxThinkingTokens { .. } => "set_max_thinking_tokens",
        ControlRequestType::SetPermissionMode { .. } => "set_permission_mode",
        ControlRequestType::McpStatus => "mcp_status",
        ControlRequestType::CanUseTool { .. } => "can_use_tool",
        ControlRequestType::Unknown => "unknown",
    }
}

fn control_response_subtype(response: &ControlResponseType) -> &'static str {
    match response {
        ControlResponseType::Success { .. } => "success",
        ControlResponseType::Error { .. } => "error",
    }
}

fn bridge_error_message(error: String) -> SDKMessage {
    SDKMessage::Assistant {
        uuid: Uuid::new_v4().to_string(),
        message: MessageContent {
            content: ContentBlock::Text(format!("Bridge message handling failed: {}", error)),
        },
    }
}

fn activity(activity_type: impl Into<String>, summary: impl Into<String>) -> SessionActivity {
    SessionActivity {
        activity_type: activity_type.into(),
        summary: truncate_summary(summary.into()),
        timestamp: now_unix_seconds(),
    }
}

fn content_summary(message: &MessageContent) -> String {
    match &message.content {
        ContentBlock::Text(text) => truncate_summary(text.clone()),
        ContentBlock::Blocks(blocks) => {
            let text = blocks
                .iter()
                .filter_map(|block| {
                    block
                        .get("text")
                        .or_else(|| block.get("content"))
                        .and_then(Value::as_str)
                })
                .collect::<Vec<_>>()
                .join(" ");
            if text.trim().is_empty() {
                format!("{} structured block(s)", blocks.len())
            } else {
                truncate_summary(text)
            }
        }
    }
}

fn bridge_prompt_text(message: &MessageContent) -> String {
    match &message.content {
        ContentBlock::Text(text) => text.clone(),
        ContentBlock::Blocks(blocks) => {
            let text = blocks
                .iter()
                .filter_map(|block| {
                    block
                        .get("text")
                        .or_else(|| block.get("content"))
                        .and_then(Value::as_str)
                })
                .collect::<Vec<_>>()
                .join("\n");
            if text.trim().is_empty() {
                serde_json::to_string(blocks)
                    .unwrap_or_else(|_| "<structured remote prompt>".into())
            } else {
                text
            }
        }
    }
}

fn assistant_text_from_runner_stdout(stdout: &[u8]) -> String {
    let raw = String::from_utf8_lossy(stdout).trim().to_string();
    if raw.is_empty() {
        return "Remote prompt completed without assistant text.".to_string();
    }

    if let Ok(value) = serde_json::from_str::<Value>(&raw) {
        if let Some(text) = value.get("assistant_text").and_then(Value::as_str) {
            return text.to_string();
        }
        if let Some(text) = value.get("result").and_then(Value::as_str) {
            return text.to_string();
        }
        if let Some(text) = value.as_str() {
            return text.to_string();
        }
    }

    raw
}

fn normalized_max_sessions(max_sessions: usize) -> usize {
    max_sessions.max(1)
}

fn bridge_heartbeat_interval(config: &BridgeConfig) -> Option<Duration> {
    let interval_ms = if config.heartbeat_interval_ms == 0 {
        return None;
    } else {
        config.heartbeat_interval_ms
    };
    Some(Duration::from_millis(interval_ms.max(100)))
}

fn bridge_session_timeout(config: &BridgeConfig) -> Option<Duration> {
    if config.session_timeout_ms == 0 {
        None
    } else {
        Some(Duration::from_millis(config.session_timeout_ms))
    }
}

fn ccr_v2_sse_reconnect_policy(config: &BridgeConfig) -> CcrV2ReconnectPolicy {
    CcrV2ReconnectPolicy::new(
        Duration::from_secs(1),
        Duration::from_secs(30),
        Duration::from_millis(
            config
                .ccr_v2_sse_reconnect_give_up_ms
                .unwrap_or(10 * 60 * 1000),
        ),
    )
    .with_liveness_timeout(ccr_v2_sse_liveness_timeout(config))
}

fn ccr_v2_sse_liveness_timeout(config: &BridgeConfig) -> Option<Duration> {
    match config.ccr_v2_sse_liveness_timeout_ms {
        Some(0) => None,
        Some(timeout_ms) => Some(Duration::from_millis(timeout_ms)),
        None => Some(Duration::from_secs(45)),
    }
}

async fn has_bridge_work_capacity(session_manager: &SessionManager, max_sessions: usize) -> bool {
    session_manager.session_count().await < normalized_max_sessions(max_sessions)
}

async fn bridge_work_dispatch(
    session_manager: &SessionManager,
    active_work: &AsyncMutex<HashMap<String, ActiveBridgeWork>>,
    max_sessions: usize,
    session_permits: Arc<Semaphore>,
    work: &WorkResponse,
) -> Result<BridgeWorkDispatch> {
    if work.data.data_type != "session" {
        return Ok(BridgeWorkDispatch::Handle { permit: None });
    }

    if active_work.lock().await.contains_key(&work.data.id) {
        return Ok(BridgeWorkDispatch::Handle { permit: None });
    }

    if !has_bridge_work_capacity(session_manager, max_sessions).await {
        return Ok(BridgeWorkDispatch::DeferAtCapacity);
    }

    match session_permits.try_acquire_owned() {
        Ok(permit) => Ok(BridgeWorkDispatch::Handle {
            permit: Some(permit),
        }),
        Err(TryAcquireError::NoPermits) => Ok(BridgeWorkDispatch::DeferAtCapacity),
        Err(TryAcquireError::Closed) => Err(anyhow!("bridge session scheduler closed")),
    }
}

fn prepare_bridge_session_work_dir(config: &BridgeConfig, session_id: &str) -> Result<PathBuf> {
    let base_dir = PathBuf::from(&config.dir);
    if !base_dir.is_dir() {
        return Err(anyhow!(
            "bridge working directory does not exist or is not a directory: {}",
            base_dir.display()
        ));
    }

    match config.spawn_mode {
        SpawnMode::SingleSession | SpawnMode::SameDir => Ok(base_dir),
        SpawnMode::Worktree => create_bridge_worktree_or_snapshot(&base_dir, session_id),
    }
}

fn cleanup_bridge_session_work_dir(config: &BridgeConfig, work_dir: Option<&Path>) -> Result<()> {
    if config.spawn_mode != SpawnMode::Worktree {
        return Ok(());
    }
    let Some(work_dir) = work_dir else {
        return Ok(());
    };
    let base_dir = PathBuf::from(&config.dir);
    if work_dir == base_dir {
        return Ok(());
    }
    let Some(managed_root) = bridge_managed_worktree_root(&base_dir) else {
        return Ok(());
    };
    if !work_dir.starts_with(&managed_root) {
        return Ok(());
    }

    if let Some(repo_root) = git_repo_root(&base_dir) {
        let output = std::process::Command::new("git")
            .arg("-C")
            .arg(&repo_root)
            .arg("worktree")
            .arg("remove")
            .arg("--force")
            .arg(work_dir)
            .output();
        if let Ok(output) = output {
            if output.status.success() {
                return Ok(());
            }
        }
    }

    if work_dir.exists() {
        fs::remove_dir_all(work_dir)?;
    }
    Ok(())
}

fn bridge_managed_worktree_root(base_dir: &Path) -> Option<PathBuf> {
    git_repo_root(base_dir)
        .or_else(|| Some(base_dir.to_path_buf()))
        .map(|root| root.join(".kiana").join("bridge-worktrees"))
}

fn create_bridge_worktree_or_snapshot(base_dir: &Path, session_id: &str) -> Result<PathBuf> {
    let slug = safe_bridge_session_slug(session_id);
    if let Some(repo_root) = git_repo_root(base_dir) {
        let worktree_path = repo_root
            .join(".kiana")
            .join("bridge-worktrees")
            .join(&slug);
        if worktree_path.is_dir() {
            return Ok(worktree_path);
        }
        if let Some(parent) = worktree_path.parent() {
            fs::create_dir_all(parent)?;
        }
        let output = std::process::Command::new("git")
            .arg("-C")
            .arg(&repo_root)
            .arg("worktree")
            .arg("add")
            .arg("--detach")
            .arg(&worktree_path)
            .arg("HEAD")
            .output();
        if let Ok(output) = output {
            if output.status.success() && worktree_path.is_dir() {
                return Ok(worktree_path);
            }
        }
        let _ = fs::remove_dir_all(&worktree_path);
    }

    let snapshot_path = base_dir.join(".kiana").join("bridge-worktrees").join(&slug);
    if snapshot_path.exists() {
        fs::remove_dir_all(&snapshot_path)?;
    }
    fs::create_dir_all(&snapshot_path)?;
    copy_bridge_snapshot(base_dir, &snapshot_path, &snapshot_path)?;
    Ok(snapshot_path)
}

fn git_repo_root(base_dir: &Path) -> Option<PathBuf> {
    let output = std::process::Command::new("git")
        .arg("-C")
        .arg(base_dir)
        .arg("rev-parse")
        .arg("--show-toplevel")
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let root = String::from_utf8(output.stdout).ok()?.trim().to_string();
    if root.is_empty() {
        None
    } else {
        Some(PathBuf::from(root))
    }
}

fn safe_bridge_session_slug(session_id: &str) -> String {
    let slug: String = session_id
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() || ch == '-' || ch == '_' {
                ch
            } else {
                '_'
            }
        })
        .collect();
    let slug = slug.trim_matches('_');
    if slug.is_empty() {
        format!("session-{}", Uuid::new_v4())
    } else {
        slug.to_string()
    }
}

fn copy_bridge_snapshot(src: &Path, dest: &Path, snapshot_root: &Path) -> Result<()> {
    for entry in fs::read_dir(src)? {
        let entry = entry?;
        let path = entry.path();
        if should_skip_bridge_snapshot_entry(src, &path, snapshot_root, &entry.file_name()) {
            continue;
        }
        let target = dest.join(entry.file_name());
        let file_type = entry.file_type()?;
        if file_type.is_dir() {
            fs::create_dir_all(&target)?;
            copy_bridge_snapshot(&path, &target, snapshot_root)?;
        } else if file_type.is_file() {
            fs::copy(&path, &target)?;
        }
    }
    Ok(())
}

fn should_skip_bridge_snapshot_entry(
    src: &Path,
    path: &Path,
    snapshot_root: &Path,
    name: &OsStr,
) -> bool {
    if path == snapshot_root || path.starts_with(snapshot_root) {
        return true;
    }
    if name == OsStr::new(".git") || name == OsStr::new("target") {
        return true;
    }
    if name == OsStr::new("bridge-worktrees") {
        if let Some(parent) = src.file_name() {
            return parent == OsStr::new(".kiana");
        }
    }
    false
}

fn truncate_summary(summary: String) -> String {
    let mut chars = summary.trim().chars();
    let shortened: String = chars.by_ref().take(140).collect();
    if chars.next().is_some() {
        format!("{}...", shortened)
    } else {
        shortened
    }
}

fn now_unix_seconds() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs())
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;
    use base64::Engine;
    #[cfg(unix)]
    use futures_util::SinkExt;
    use futures_util::StreamExt;
    use std::collections::VecDeque;
    use std::ffi::OsString;
    use std::fs;
    use std::sync::Mutex;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::{TcpListener, TcpStream};

    static ENV_LOCK: Mutex<()> = Mutex::new(());

    #[derive(Debug)]
    struct MockResponse {
        status: u16,
        body: String,
    }

    #[derive(Debug, Clone)]
    struct RecordedRequest {
        method: String,
        path: String,
        authorization: Option<String>,
        body: String,
    }

    fn session_handle() -> SessionHandle {
        SessionHandle {
            session_id: "remote-session".to_string(),
            access_token: "token".to_string(),
            sdk_url: Some("wss://example.test/v1/session_ingress/ws/remote-session".to_string()),
            work_dir: None,
            use_ccr_v2: false,
            worker_epoch: None,
        }
    }

    fn single_message(messages: Vec<SDKMessage>) -> SDKMessage {
        assert_eq!(messages.len(), 1, "expected exactly one SDK message");
        messages.into_iter().next().unwrap()
    }

    fn bridge_config(dir: String) -> BridgeConfig {
        BridgeConfig {
            dir,
            machine_name: "machine".to_string(),
            branch: "main".to_string(),
            git_repo_url: None,
            max_sessions: 1,
            spawn_mode: crate::types::SpawnMode::SingleSession,
            bridge_id: "bridge".to_string(),
            worker_type: "kiana".to_string(),
            environment_id: "env".to_string(),
            api_base_url: "https://example.test".to_string(),
            session_ingress_url: "wss://example.test".to_string(),
            heartbeat_interval_ms: 60_000,
            session_timeout_ms: 24 * 60 * 60 * 1000,
            ccr_v2_sse_reconnect_give_up_ms: Some(0),
            ccr_v2_sse_liveness_timeout_ms: Some(0),
            debug_file: None,
            permission_mode: None,
        }
    }

    #[derive(Default)]
    struct TokenRecordingRunner {
        updated_tokens: AsyncMutex<Vec<String>>,
        handled_messages: AsyncMutex<usize>,
    }

    #[async_trait]
    impl BridgeSessionRunner for TokenRecordingRunner {
        async fn handle_user_message(
            &self,
            _handle: &SessionHandle,
            _uuid: String,
            _message: MessageContent,
        ) -> Result<SDKMessage> {
            *self.handled_messages.lock().await += 1;
            Ok(SDKMessage::Assistant {
                uuid: "assistant".to_string(),
                message: MessageContent {
                    content: ContentBlock::Text("handled".to_string()),
                },
            })
        }

        async fn update_access_token(&self, _handle: &SessionHandle, token: String) -> Result<()> {
            self.updated_tokens.lock().await.push(token);
            Ok(())
        }
    }

    struct FixedAssistantRunner;

    #[async_trait]
    impl BridgeSessionRunner for FixedAssistantRunner {
        async fn handle_user_message(
            &self,
            _handle: &SessionHandle,
            _uuid: String,
            _message: MessageContent,
        ) -> Result<SDKMessage> {
            Ok(SDKMessage::Assistant {
                uuid: "assistant".to_string(),
                message: MessageContent {
                    content: ContentBlock::Text("handled".to_string()),
                },
            })
        }
    }

    struct StreamEventOnlyRunner;

    #[async_trait]
    impl BridgeSessionRunner for StreamEventOnlyRunner {
        async fn handle_user_message(
            &self,
            _handle: &SessionHandle,
            _uuid: String,
            _message: MessageContent,
        ) -> Result<SDKMessage> {
            Ok(SDKMessage::StreamEvent {
                data: HashMap::from([
                    ("uuid".to_string(), json!("stream-1")),
                    ("session_id".to_string(), json!("cse_session_1")),
                    ("parent_tool_use_id".to_string(), Value::Null),
                    (
                        "event".to_string(),
                        json!({
                            "type": "content_block_delta",
                            "index": 0,
                            "delta": {"type": "text_delta", "text": "partial"}
                        }),
                    ),
                ]),
            })
        }
    }

    struct ManyStreamEventsRunner {
        count: usize,
    }

    #[async_trait]
    impl BridgeSessionRunner for ManyStreamEventsRunner {
        async fn handle_user_message(
            &self,
            _handle: &SessionHandle,
            _uuid: String,
            _message: MessageContent,
        ) -> Result<SDKMessage> {
            Ok(SDKMessage::Assistant {
                uuid: "assistant-batched".to_string(),
                message: MessageContent {
                    content: ContentBlock::Text("batched".to_string()),
                },
            })
        }

        async fn drain_outbound_messages(
            &self,
            _handle: &SessionHandle,
        ) -> Result<Vec<SDKMessage>> {
            Ok((0..self.count)
                .map(|index| SDKMessage::StreamEvent {
                    data: HashMap::from([
                        ("uuid".to_string(), json!(format!("stream-{index}"))),
                        ("session_id".to_string(), json!("cse_session_1")),
                        ("parent_tool_use_id".to_string(), Value::Null),
                        (
                            "event".to_string(),
                            json!({
                                "type": "content_block_delta",
                                "index": 0,
                                "delta": {
                                    "type": "input_json_delta",
                                    "partial_json": format!("{{\"index\":{index}")
                                }
                            }),
                        ),
                    ]),
                })
                .collect())
        }
    }

    #[tokio::test]
    async fn shutdown_active_work_force_stops_archives_and_cleans_managed_worktree() {
        let root = std::env::temp_dir().join(format!("kiana-bridge-shutdown-{}", Uuid::new_v4()));
        fs::create_dir_all(&root).unwrap();
        fs::write(root.join("repo.txt"), "snapshot").unwrap();
        let mut config = bridge_config(root.to_string_lossy().to_string());
        config.spawn_mode = crate::types::SpawnMode::Worktree;
        let work_dir = prepare_bridge_session_work_dir(&config, "remote-session").unwrap();
        assert!(work_dir.is_dir());

        let (base_url, requests) = spawn_mock_server(vec![
            MockResponse::json(200, r#"{}"#),
            MockResponse::json(200, r#"{}"#),
        ])
        .await;
        let api = Arc::new(BridgeApiClient::new(base_url, "api-token".to_string()));
        let sessions = Arc::new(SessionManager::new());
        let work_loop = WorkPollLoop::recording(api, sessions.clone(), config.clone());
        sessions
            .add_session(
                "remote-session".to_string(),
                SessionHandle {
                    session_id: "remote-session".to_string(),
                    access_token: "session-token".to_string(),
                    sdk_url: Some(
                        "wss://example.test/v1/session_ingress/ws/remote-session".to_string(),
                    ),
                    work_dir: Some(work_dir.clone()),
                    use_ccr_v2: false,
                    worker_epoch: None,
                },
            )
            .await;
        work_loop.active_work.lock().await.insert(
            "remote-session".to_string(),
            ActiveBridgeWork::new(
                "work-1".to_string(),
                "remote-session".to_string(),
                Some(work_dir.clone()),
                "session-token".to_string(),
            ),
        );

        work_loop.shutdown_active_work("env-1").await.unwrap();

        assert_eq!(sessions.session_count().await, 0);
        assert_eq!(work_loop.active_work.lock().await.len(), 0);
        assert!(!work_dir.exists());
        let requests = requests.lock().unwrap();
        assert_eq!(requests.len(), 2);
        assert_eq!(requests[0].method, "POST");
        assert_eq!(requests[0].path, "/v1/environments/env-1/work/work-1/stop");
        assert_eq!(
            requests[0].authorization.as_deref(),
            Some("Bearer api-token")
        );
        assert_eq!(requests[0].body, r#"{"force":true}"#);
        assert_eq!(requests[1].method, "POST");
        assert_eq!(requests[1].path, "/v1/sessions/remote-session/archive");
        assert_eq!(
            requests[1].authorization.as_deref(),
            Some("Bearer api-token")
        );

        let _ = fs::remove_dir_all(root);
    }

    #[tokio::test]
    async fn non_session_work_is_acknowledged_without_spawning_session() {
        let root = std::env::temp_dir().join(format!("kiana-bridge-health-{}", Uuid::new_v4()));
        fs::create_dir_all(&root).unwrap();
        let (api_base_url, requests) =
            spawn_mock_server(vec![MockResponse::json(200, r#"{}"#)]).await;
        let mut config = bridge_config(root.to_string_lossy().to_string());
        config.api_base_url = api_base_url.clone();

        let api = Arc::new(BridgeApiClient::new(api_base_url, "api-token".to_string()));
        let sessions = Arc::new(SessionManager::new());
        let active_work = Arc::new(AsyncMutex::new(HashMap::new()));
        let work = WorkResponse {
            id: "work-health".to_string(),
            work_type: "bridge".to_string(),
            environment_id: "env-1".to_string(),
            state: "queued".to_string(),
            data: crate::types::WorkData {
                data_type: "healthcheck".to_string(),
                id: "healthcheck".to_string(),
            },
            secret: encoded_work_secret("health-token"),
        };

        handle_bridge_work(
            api,
            sessions.clone(),
            config,
            Arc::new(RecordingBridgeSessionRunner),
            active_work.clone(),
            work,
            "env-1".to_string(),
        )
        .await
        .unwrap();

        assert_eq!(sessions.session_count().await, 0);
        assert!(active_work.lock().await.is_empty());
        let requests = requests.lock().unwrap();
        assert_eq!(requests.len(), 1);
        assert_eq!(
            requests[0].path,
            "/v1/environments/env-1/work/work-health/ack"
        );
        assert_eq!(
            requests[0].authorization.as_deref(),
            Some("Bearer health-token")
        );

        let _ = fs::remove_dir_all(root);
    }

    #[tokio::test]
    async fn code_session_work_uses_ccr_v2_worker_transport() {
        let root =
            std::env::temp_dir().join(format!("kiana-bridge-ccr-v2-work-{}", Uuid::new_v4()));
        fs::create_dir_all(&root).unwrap();
        let user_event = json!({
            "event_id": "evt-user-1",
            "sequence_num": 1,
            "event_type": "user",
            "source": "frontend",
            "payload": {
                "type": "user",
                "uuid": "user-1",
                "message": {
                    "content": "run ccr v2"
                }
            },
            "created_at": "2026-06-16T00:00:00Z"
        });
        let sse_body = format!("id: 1\nevent: client_event\ndata: {}\n\n", user_event);
        let (api_base_url, requests) = spawn_mock_server(vec![
            MockResponse::json(200, r#"{}"#),
            MockResponse::json(200, r#"{"worker_epoch":"42"}"#),
            MockResponse::json(200, r#"{}"#),
            MockResponse {
                status: 200,
                body: sse_body,
            },
            MockResponse::json(200, r#"{}"#),
            MockResponse::json(200, r#"{}"#),
            MockResponse::json(200, r#"{}"#),
            MockResponse::json(200, r#"{}"#),
            MockResponse::json(200, r#"{}"#),
            MockResponse::json(200, r#"{}"#),
            MockResponse::json(200, r#"{}"#),
            MockResponse::json(200, r#"{}"#),
            MockResponse::json(200, r#"{}"#),
            MockResponse::json(200, r#"{}"#),
            MockResponse::json(200, r#"{}"#),
            MockResponse::json(200, r#"{}"#),
            MockResponse::json(200, r#"{}"#),
            MockResponse::json(200, r#"{}"#),
            MockResponse::json(200, r#"{}"#),
            MockResponse::json(200, r#"{}"#),
            MockResponse::json(200, r#"{}"#),
            MockResponse::json(200, r#"{}"#),
            MockResponse::json(200, r#"{}"#),
            MockResponse::json(200, r#"{}"#),
            MockResponse::json(200, r#"{}"#),
        ])
        .await;
        let mut config = bridge_config(root.to_string_lossy().to_string());
        config.api_base_url = api_base_url.clone();
        config.session_ingress_url = "wss://unused-session-ingress.example".to_string();
        config.heartbeat_interval_ms = 0;
        config.session_timeout_ms = 5_000;

        let api = Arc::new(BridgeApiClient::new(
            api_base_url.clone(),
            "api-token".to_string(),
        ));
        let sessions = Arc::new(SessionManager::new());
        let active_work = Arc::new(AsyncMutex::new(HashMap::new()));
        let work = WorkResponse {
            id: "work-v2".to_string(),
            work_type: "bridge".to_string(),
            environment_id: "env-1".to_string(),
            state: "queued".to_string(),
            data: crate::types::WorkData {
                data_type: "session".to_string(),
                id: "cse_session_1".to_string(),
            },
            secret: encoded_code_session_work_secret("worker-token", &api_base_url),
        };

        handle_bridge_work(
            api,
            sessions.clone(),
            config,
            Arc::new(FixedAssistantRunner),
            active_work.clone(),
            work,
            "env-1".to_string(),
        )
        .await
        .unwrap();

        assert_eq!(sessions.session_count().await, 0);
        assert!(active_work.lock().await.is_empty());
        let requests = requests.lock().unwrap().clone();
        assert_eq!(requests.len(), 10);
        assert_eq!(requests[0].method, "POST");
        assert_eq!(requests[0].path, "/v1/environments/env-1/work/work-v2/ack");
        assert_eq!(
            requests[0].authorization.as_deref(),
            Some("Bearer worker-token")
        );

        assert_eq!(requests[1].method, "POST");
        assert_eq!(
            requests[1].path,
            "/v1/code/sessions/cse_session_1/worker/register"
        );
        assert_eq!(
            requests[1].authorization.as_deref(),
            Some("Bearer worker-token")
        );

        assert_eq!(requests[2].method, "PUT");
        assert_eq!(requests[2].path, "/v1/code/sessions/cse_session_1/worker");
        let init_body: Value = serde_json::from_str(&requests[2].body).unwrap();
        assert_eq!(init_body["worker_status"], json!("idle"));
        assert_eq!(init_body["worker_epoch"], json!(42));

        assert_eq!(requests[3].method, "GET");
        assert_eq!(
            requests[3].path,
            "/v1/code/sessions/cse_session_1/worker/events/stream"
        );
        assert_eq!(
            requests[3].authorization.as_deref(),
            Some("Bearer worker-token")
        );

        let running_state = requests
            .iter()
            .find(|request| {
                request.method == "PUT"
                    && request.path == "/v1/code/sessions/cse_session_1/worker"
                    && serde_json::from_str::<Value>(&request.body)
                        .map(|body| body["worker_status"] == json!("running"))
                        .unwrap_or(false)
            })
            .expect("inbound user event should report worker running state");
        let running_body: Value = serde_json::from_str(&running_state.body).unwrap();
        assert_eq!(running_body["worker_epoch"], json!(42));

        let events_index = requests
            .iter()
            .position(|request| request.path == "/v1/code/sessions/cse_session_1/worker/events")
            .expect("assistant response should be written to worker events");
        let delivery_index = requests
            .iter()
            .position(|request| {
                request.path == "/v1/code/sessions/cse_session_1/worker/events/delivery"
            })
            .expect("delivery updates should be flushed before reconnect cleanup");

        assert_eq!(requests[delivery_index].method, "POST");
        let delivery_body: Value = serde_json::from_str(&requests[delivery_index].body).unwrap();
        assert_eq!(delivery_body["worker_epoch"], json!(42));
        assert_eq!(delivery_body["updates"][0]["event_id"], json!("evt-user-1"));
        assert_eq!(delivery_body["updates"][0]["status"], json!("received"));
        assert_eq!(delivery_body["updates"][1]["event_id"], json!("evt-user-1"));
        assert_eq!(delivery_body["updates"][1]["status"], json!("processed"));

        assert_eq!(requests[events_index].method, "POST");
        let events_body: Value = serde_json::from_str(&requests[events_index].body).unwrap();
        assert_eq!(events_body["worker_epoch"], json!(42));
        assert_eq!(
            events_body["events"][0]["payload"]["type"],
            json!("assistant")
        );
        assert_eq!(
            events_body["events"][0]["payload"]["message"]["content"],
            json!("handled")
        );

        let internal_posts = requests
            .iter()
            .filter(|request| {
                request.path == "/v1/code/sessions/cse_session_1/worker/internal-events"
            })
            .collect::<Vec<_>>();
        assert_eq!(internal_posts.len(), 2);
        let user_internal_body: Value = serde_json::from_str(&internal_posts[0].body).unwrap();
        assert_eq!(user_internal_body["worker_epoch"], json!(42));
        assert_eq!(
            user_internal_body["events"][0]["payload"]["type"],
            json!("transcript")
        );
        assert_eq!(
            user_internal_body["events"][0]["payload"]["message"]["role"],
            json!("user")
        );
        assert_eq!(
            user_internal_body["events"][0]["payload"]["message"]["content"],
            json!("run ccr v2")
        );
        let assistant_internal_body: Value = serde_json::from_str(&internal_posts[1].body).unwrap();
        assert_eq!(
            assistant_internal_body["events"][0]["payload"]["message"]["role"],
            json!("assistant")
        );
        assert_eq!(
            assistant_internal_body["events"][0]["payload"]["message"]["content"],
            json!("handled")
        );

        assert_eq!(requests[9].method, "POST");
        assert_eq!(requests[9].path, "/v1/environments/env-1/bridge/reconnect");
        assert_eq!(
            requests[9].authorization.as_deref(),
            Some("Bearer api-token")
        );
        let reconnect_body: Value = serde_json::from_str(&requests[9].body).unwrap();
        assert_eq!(reconnect_body["session_id"], json!("cse_session_1"));

        let _ = fs::remove_dir_all(root);
    }

    #[tokio::test]
    async fn code_session_client_events_are_split_into_reference_sized_batches() {
        let root =
            std::env::temp_dir().join(format!("kiana-bridge-ccr-v2-batch-{}", Uuid::new_v4()));
        fs::create_dir_all(&root).unwrap();
        let user_event = json!({
            "event_id": "evt-user-batch",
            "sequence_num": 1,
            "event_type": "user",
            "source": "frontend",
            "payload": {
                "type": "user",
                "uuid": "user-batch",
                "message": {
                    "content": "batch events"
                }
            },
            "created_at": "2026-06-16T00:00:00Z"
        });
        let sse_body = format!("id: 1\nevent: client_event\ndata: {}\n\n", user_event);
        let (api_base_url, requests) = spawn_mock_server(vec![
            MockResponse::json(200, r#"{}"#),
            MockResponse::json(200, r#"{"worker_epoch":"42"}"#),
            MockResponse::json(200, r#"{}"#),
            MockResponse {
                status: 200,
                body: sse_body,
            },
            MockResponse::json(200, r#"{}"#),
            MockResponse::json(200, r#"{}"#),
            MockResponse::json(200, r#"{}"#),
            MockResponse::json(200, r#"{}"#),
            MockResponse::json(200, r#"{}"#),
            MockResponse::json(200, r#"{}"#),
            MockResponse::json(200, r#"{}"#),
        ])
        .await;
        let mut config = bridge_config(root.to_string_lossy().to_string());
        config.api_base_url = api_base_url.clone();
        config.session_ingress_url = "wss://unused-session-ingress.example".to_string();
        config.heartbeat_interval_ms = 0;
        config.session_timeout_ms = 5_000;

        let api = Arc::new(BridgeApiClient::new(
            api_base_url.clone(),
            "api-token".to_string(),
        ));
        let sessions = Arc::new(SessionManager::new());
        let active_work = Arc::new(AsyncMutex::new(HashMap::new()));
        let work = WorkResponse {
            id: "work-v2-batch".to_string(),
            work_type: "bridge".to_string(),
            environment_id: "env-1".to_string(),
            state: "queued".to_string(),
            data: crate::types::WorkData {
                data_type: "session".to_string(),
                id: "cse_session_1".to_string(),
            },
            secret: encoded_code_session_work_secret("worker-token", &api_base_url),
        };

        handle_bridge_work(
            api,
            sessions.clone(),
            config,
            Arc::new(ManyStreamEventsRunner { count: 150 }),
            active_work.clone(),
            work,
            "env-1".to_string(),
        )
        .await
        .unwrap();

        assert_eq!(sessions.session_count().await, 0);
        assert!(active_work.lock().await.is_empty());
        let requests = requests.lock().unwrap().clone();
        let event_posts = requests
            .iter()
            .filter(|request| request.path == "/v1/code/sessions/cse_session_1/worker/events")
            .collect::<Vec<_>>();
        assert_eq!(event_posts.len(), 2);

        let first_body: Value = serde_json::from_str(&event_posts[0].body).unwrap();
        let second_body: Value = serde_json::from_str(&event_posts[1].body).unwrap();
        assert_eq!(first_body["worker_epoch"], json!(42));
        assert_eq!(second_body["worker_epoch"], json!(42));
        assert_eq!(first_body["events"].as_array().unwrap().len(), 100);
        assert_eq!(second_body["events"].as_array().unwrap().len(), 51);
        assert_eq!(
            first_body["events"][0]["payload"]["uuid"],
            json!("stream-0")
        );
        assert_eq!(
            first_body["events"][99]["payload"]["uuid"],
            json!("stream-99")
        );
        assert_eq!(
            second_body["events"][0]["payload"]["uuid"],
            json!("stream-100")
        );
        assert_eq!(
            second_body["events"][50]["payload"]["uuid"],
            json!("assistant-batched")
        );

        let internal_posts = requests
            .iter()
            .filter(|request| {
                request.path == "/v1/code/sessions/cse_session_1/worker/internal-events"
            })
            .collect::<Vec<_>>();
        assert_eq!(internal_posts.len(), 2);
        let user_internal_body: Value = serde_json::from_str(&internal_posts[0].body).unwrap();
        assert_eq!(
            user_internal_body["events"][0]["payload"]["message"]["role"],
            json!("user")
        );
        assert_eq!(
            user_internal_body["events"][0]["payload"]["message"]["content"],
            json!("batch events")
        );
        let assistant_internal_body: Value = serde_json::from_str(&internal_posts[1].body).unwrap();
        assert_eq!(
            assistant_internal_body["events"][0]["payload"]["message"]["role"],
            json!("assistant")
        );
        assert_eq!(
            assistant_internal_body["events"][0]["payload"]["message"]["content"],
            json!("batched")
        );

        let _ = fs::remove_dir_all(root);
    }

    #[tokio::test]
    async fn code_session_delivery_updates_are_batched_before_reconnect_cleanup() {
        let root =
            std::env::temp_dir().join(format!("kiana-bridge-ccr-v2-delivery-{}", Uuid::new_v4()));
        fs::create_dir_all(&root).unwrap();
        let sse_body = (0..3)
            .map(|index| {
                let event = json!({
                    "event_id": format!("evt-delivery-{index}"),
                    "sequence_num": index + 1,
                    "event_type": "user",
                    "source": "frontend",
                    "payload": {
                        "type": "user",
                        "uuid": format!("user-delivery-{index}"),
                        "message": {
                            "content": format!("delivery {index}")
                        }
                    },
                    "created_at": "2026-06-16T00:00:00Z"
                });
                format!(
                    "id: {}\nevent: client_event\ndata: {}\n\n",
                    index + 1,
                    event
                )
            })
            .collect::<String>();
        let (api_base_url, requests) = spawn_mock_server(vec![
            MockResponse::json(200, r#"{}"#),
            MockResponse::json(200, r#"{"worker_epoch":"42"}"#),
            MockResponse::json(200, r#"{}"#),
            MockResponse {
                status: 200,
                body: sse_body,
            },
            MockResponse::json(200, r#"{}"#),
            MockResponse::json(200, r#"{}"#),
            MockResponse::json(200, r#"{}"#),
            MockResponse::json(200, r#"{}"#),
            MockResponse::json(200, r#"{}"#),
            MockResponse::json(200, r#"{}"#),
            MockResponse::json(200, r#"{}"#),
            MockResponse::json(200, r#"{}"#),
            MockResponse::json(200, r#"{}"#),
            MockResponse::json(200, r#"{}"#),
            MockResponse::json(200, r#"{}"#),
            MockResponse::json(200, r#"{}"#),
        ])
        .await;
        let mut config = bridge_config(root.to_string_lossy().to_string());
        config.api_base_url = api_base_url.clone();
        config.session_ingress_url = "wss://unused-session-ingress.example".to_string();
        config.heartbeat_interval_ms = 0;
        config.session_timeout_ms = 5_000;

        let api = Arc::new(BridgeApiClient::new(
            api_base_url.clone(),
            "api-token".to_string(),
        ));
        let sessions = Arc::new(SessionManager::new());
        let active_work = Arc::new(AsyncMutex::new(HashMap::new()));
        let work = WorkResponse {
            id: "work-v2-delivery".to_string(),
            work_type: "bridge".to_string(),
            environment_id: "env-1".to_string(),
            state: "queued".to_string(),
            data: crate::types::WorkData {
                data_type: "session".to_string(),
                id: "cse_session_1".to_string(),
            },
            secret: encoded_code_session_work_secret("worker-token", &api_base_url),
        };

        handle_bridge_work(
            api,
            sessions.clone(),
            config,
            Arc::new(FixedAssistantRunner),
            active_work.clone(),
            work,
            "env-1".to_string(),
        )
        .await
        .unwrap();

        assert_eq!(sessions.session_count().await, 0);
        assert!(active_work.lock().await.is_empty());
        let requests = requests.lock().unwrap().clone();
        let delivery_posts = requests
            .iter()
            .filter(|request| {
                request.path == "/v1/code/sessions/cse_session_1/worker/events/delivery"
            })
            .collect::<Vec<_>>();
        assert_eq!(delivery_posts.len(), 1);
        let delivery_body: Value = serde_json::from_str(&delivery_posts[0].body).unwrap();
        assert_eq!(delivery_body["worker_epoch"], json!(42));
        assert_eq!(delivery_body["updates"].as_array().unwrap().len(), 6);
        for index in 0..3 {
            let received_index = index * 2;
            let processed_index = received_index + 1;
            assert_eq!(
                delivery_body["updates"][received_index]["event_id"],
                json!(format!("evt-delivery-{index}"))
            );
            assert_eq!(
                delivery_body["updates"][received_index]["status"],
                json!("received")
            );
            assert_eq!(
                delivery_body["updates"][processed_index]["event_id"],
                json!(format!("evt-delivery-{index}"))
            );
            assert_eq!(
                delivery_body["updates"][processed_index]["status"],
                json!("processed")
            );
        }

        let event_posts = requests
            .iter()
            .filter(|request| request.path == "/v1/code/sessions/cse_session_1/worker/events")
            .count();
        assert_eq!(event_posts, 3);

        let internal_posts = requests
            .iter()
            .filter(|request| {
                request.path == "/v1/code/sessions/cse_session_1/worker/internal-events"
            })
            .collect::<Vec<_>>();
        assert_eq!(internal_posts.len(), 6);
        let first_internal_body: Value = serde_json::from_str(&internal_posts[0].body).unwrap();
        assert_eq!(
            first_internal_body["events"][0]["payload"]["message"]["role"],
            json!("user")
        );
        assert_eq!(
            first_internal_body["events"][0]["payload"]["message"]["content"],
            json!("delivery 0")
        );
        let second_internal_body: Value = serde_json::from_str(&internal_posts[1].body).unwrap();
        assert_eq!(
            second_internal_body["events"][0]["payload"]["message"]["role"],
            json!("assistant")
        );

        let _ = fs::remove_dir_all(root);
    }

    #[tokio::test]
    async fn code_session_stream_events_flush_on_maintenance_timer_without_followup_message() {
        let root =
            std::env::temp_dir().join(format!("kiana-bridge-ccr-v2-stream-{}", Uuid::new_v4()));
        fs::create_dir_all(&root).unwrap();
        let user_event = json!({
            "event_id": "evt-user-stream",
            "sequence_num": 1,
            "event_type": "user",
            "source": "frontend",
            "payload": {
                "type": "user",
                "uuid": "user-stream",
                "message": {
                    "content": "stream only"
                }
            },
            "created_at": "2026-06-16T00:00:00Z"
        });
        let (api_base_url, requests) =
            spawn_ccr_v2_timer_mock_server(user_event, Duration::from_millis(600)).await;
        let mut config = bridge_config(root.to_string_lossy().to_string());
        config.api_base_url = api_base_url.clone();
        config.session_ingress_url = "wss://unused-session-ingress.example".to_string();
        config.heartbeat_interval_ms = 0;
        config.session_timeout_ms = 350;

        let api = Arc::new(BridgeApiClient::new(
            api_base_url.clone(),
            "api-token".to_string(),
        ));
        let sessions = Arc::new(SessionManager::new());
        let active_work = Arc::new(AsyncMutex::new(HashMap::new()));
        let work = WorkResponse {
            id: "work-v2-stream".to_string(),
            work_type: "bridge".to_string(),
            environment_id: "env-1".to_string(),
            state: "queued".to_string(),
            data: crate::types::WorkData {
                data_type: "session".to_string(),
                id: "cse_session_1".to_string(),
            },
            secret: encoded_code_session_work_secret("worker-token", &api_base_url),
        };

        handle_bridge_work(
            api,
            sessions.clone(),
            config,
            Arc::new(StreamEventOnlyRunner),
            active_work.clone(),
            work,
            "env-1".to_string(),
        )
        .await
        .unwrap();

        assert_eq!(sessions.session_count().await, 0);
        assert!(active_work.lock().await.is_empty());
        let requests = requests.lock().unwrap().clone();
        let worker_events_index = requests
            .iter()
            .position(|request| request.path == "/v1/code/sessions/cse_session_1/worker/events")
            .expect("maintenance timer should flush stream_event to /worker/events");
        let stop_index = requests
            .iter()
            .position(|request| request.path == "/v1/environments/env-1/work/work-v2-stream/stop")
            .expect("session timeout should stop bridge work");
        assert!(
            worker_events_index < stop_index,
            "stream_event should flush before timeout cleanup; requests={requests:?}"
        );

        let events_body: Value = serde_json::from_str(&requests[worker_events_index].body).unwrap();
        assert_eq!(events_body["worker_epoch"], json!(42));
        assert_eq!(events_body["events"].as_array().unwrap().len(), 1);
        assert_eq!(
            events_body["events"][0]["payload"]["type"],
            json!("stream_event")
        );
        assert_eq!(events_body["events"][0]["ephemeral"], json!(true));
        assert_eq!(
            events_body["events"][0]["payload"]["event"]["delta"]["text"],
            json!("partial")
        );

        let _ = fs::remove_dir_all(root);
    }

    #[tokio::test]
    async fn code_session_worker_write_epoch_mismatch_reconnects_and_cleans_up() {
        let root =
            std::env::temp_dir().join(format!("kiana-bridge-ccr-v2-reconnect-{}", Uuid::new_v4()));
        fs::create_dir_all(&root).unwrap();
        let user_event = json!({
            "event_id": "evt-user-epoch",
            "sequence_num": 1,
            "event_type": "user",
            "source": "frontend",
            "payload": {
                "type": "user",
                "uuid": "user-epoch",
                "message": {
                    "content": "trigger epoch mismatch"
                }
            },
            "created_at": "2026-06-16T00:00:00Z"
        });
        let sse_body = format!("id: 1\nevent: client_event\ndata: {}\n\n", user_event);
        let (api_base_url, requests) = spawn_mock_server(vec![
            MockResponse::json(200, r#"{}"#),
            MockResponse::json(200, r#"{"worker_epoch":"42"}"#),
            MockResponse::json(200, r#"{}"#),
            MockResponse {
                status: 200,
                body: sse_body,
            },
            MockResponse::json(200, r#"{}"#),
            MockResponse::json(200, r#"{}"#),
            MockResponse::json(409, r#"{"error":"epoch superseded"}"#),
            MockResponse::json(200, r#"{}"#),
            MockResponse::json(200, r#"{}"#),
        ])
        .await;
        let mut config = bridge_config(root.to_string_lossy().to_string());
        config.api_base_url = api_base_url.clone();
        config.session_ingress_url = "wss://unused-session-ingress.example".to_string();
        config.heartbeat_interval_ms = 0;
        config.session_timeout_ms = 5_000;

        let api = Arc::new(BridgeApiClient::new(
            api_base_url.clone(),
            "api-token".to_string(),
        ));
        let sessions = Arc::new(SessionManager::new());
        let active_work = Arc::new(AsyncMutex::new(HashMap::new()));
        let work = WorkResponse {
            id: "work-v2".to_string(),
            work_type: "bridge".to_string(),
            environment_id: "env-1".to_string(),
            state: "queued".to_string(),
            data: crate::types::WorkData {
                data_type: "session".to_string(),
                id: "cse_session_1".to_string(),
            },
            secret: encoded_code_session_work_secret("worker-token", &api_base_url),
        };

        handle_bridge_work(
            api,
            sessions.clone(),
            config,
            Arc::new(FixedAssistantRunner),
            active_work.clone(),
            work,
            "env-1".to_string(),
        )
        .await
        .unwrap();

        assert_eq!(sessions.session_count().await, 0);
        assert!(active_work.lock().await.is_empty());
        let requests = requests.lock().unwrap().clone();
        assert_eq!(requests.len(), 9);
        let internal_index = requests
            .iter()
            .position(|request| {
                request.path == "/v1/code/sessions/cse_session_1/worker/internal-events"
            })
            .expect("user transcript should be written to internal events before client event");
        let events_index = requests
            .iter()
            .position(|request| request.path == "/v1/code/sessions/cse_session_1/worker/events")
            .expect("assistant response should be written to worker events");
        assert!(internal_index < events_index);
        assert_eq!(requests[events_index].method, "POST");
        let events_body: Value = serde_json::from_str(&requests[events_index].body).unwrap();
        assert_eq!(events_body["worker_epoch"], json!(42));

        let delivery_index = requests
            .iter()
            .position(|request| {
                request.path == "/v1/code/sessions/cse_session_1/worker/events/delivery"
            })
            .expect("pending delivery updates should flush after send error");
        assert!(
            events_index < delivery_index,
            "client event should hit epoch mismatch before delivery flush; requests={requests:?}"
        );

        assert_eq!(requests[8].method, "POST");
        assert_eq!(requests[8].path, "/v1/environments/env-1/bridge/reconnect");
        assert_eq!(
            requests[8].authorization.as_deref(),
            Some("Bearer api-token")
        );
        let reconnect_body: Value = serde_json::from_str(&requests[8].body).unwrap();
        assert_eq!(reconnect_body["session_id"], json!("cse_session_1"));

        assert!(!requests.iter().any(|request| {
            request.path == "/v1/environments/env-1/work/work-v2/stop"
                || request.path == "/v1/sessions/cse_session_1/archive"
        }));

        let _ = fs::remove_dir_all(root);
    }

    #[tokio::test]
    async fn code_session_sse_eof_reconnects_instead_of_completing_work() {
        let root = std::env::temp_dir().join(format!("kiana-bridge-ccr-v2-eof-{}", Uuid::new_v4()));
        fs::create_dir_all(&root).unwrap();
        let (api_base_url, requests) = spawn_mock_server(vec![
            MockResponse::json(200, r#"{}"#),
            MockResponse::json(200, r#"{"worker_epoch":"42"}"#),
            MockResponse::json(200, r#"{}"#),
            MockResponse {
                status: 200,
                body: String::new(),
            },
            MockResponse::json(200, r#"{}"#),
        ])
        .await;
        let mut config = bridge_config(root.to_string_lossy().to_string());
        config.api_base_url = api_base_url.clone();
        config.session_ingress_url = "wss://unused-session-ingress.example".to_string();
        config.heartbeat_interval_ms = 0;
        config.session_timeout_ms = 5_000;

        let api = Arc::new(BridgeApiClient::new(
            api_base_url.clone(),
            "api-token".to_string(),
        ));
        let sessions = Arc::new(SessionManager::new());
        let active_work = Arc::new(AsyncMutex::new(HashMap::new()));
        let work = WorkResponse {
            id: "work-v2".to_string(),
            work_type: "bridge".to_string(),
            environment_id: "env-1".to_string(),
            state: "queued".to_string(),
            data: crate::types::WorkData {
                data_type: "session".to_string(),
                id: "cse_session_1".to_string(),
            },
            secret: encoded_code_session_work_secret("worker-token", &api_base_url),
        };

        handle_bridge_work(
            api,
            sessions.clone(),
            config,
            Arc::new(RecordingBridgeSessionRunner),
            active_work.clone(),
            work,
            "env-1".to_string(),
        )
        .await
        .unwrap();

        assert_eq!(sessions.session_count().await, 0);
        assert!(active_work.lock().await.is_empty());
        let requests = requests.lock().unwrap().clone();
        assert_eq!(requests.len(), 5);
        assert_eq!(requests[3].method, "GET");
        assert_eq!(
            requests[3].path,
            "/v1/code/sessions/cse_session_1/worker/events/stream"
        );
        assert_eq!(requests[4].method, "POST");
        assert_eq!(requests[4].path, "/v1/environments/env-1/bridge/reconnect");
        let reconnect_body: Value = serde_json::from_str(&requests[4].body).unwrap();
        assert_eq!(reconnect_body["session_id"], json!("cse_session_1"));

        assert!(!requests.iter().any(|request| {
            request.path == "/v1/environments/env-1/work/work-v2/stop"
                || request.path == "/v1/sessions/cse_session_1/archive"
        }));

        let _ = fs::remove_dir_all(root);
    }

    #[tokio::test]
    async fn code_session_sse_reconnect_budget_exhaustion_reconnects_bridge_session() {
        let root = std::env::temp_dir().join(format!(
            "kiana-bridge-ccr-v2-reconnect-exhausted-{}",
            Uuid::new_v4()
        ));
        fs::create_dir_all(&root).unwrap();
        let (api_base_url, requests) = spawn_mock_server(vec![
            MockResponse::json(200, r#"{}"#),
            MockResponse::json(200, r#"{"worker_epoch":"42"}"#),
            MockResponse::json(200, r#"{}"#),
            MockResponse {
                status: 200,
                body: String::new(),
            },
            MockResponse::json(500, r#"{"error":"temporary"}"#),
            MockResponse::json(200, r#"{}"#),
        ])
        .await;
        let mut config = bridge_config(root.to_string_lossy().to_string());
        config.api_base_url = api_base_url.clone();
        config.session_ingress_url = "wss://unused-session-ingress.example".to_string();
        config.heartbeat_interval_ms = 0;
        config.session_timeout_ms = 5_000;
        config.ccr_v2_sse_reconnect_give_up_ms = Some(15);
        config.ccr_v2_sse_liveness_timeout_ms = Some(0);

        let api = Arc::new(BridgeApiClient::new(
            api_base_url.clone(),
            "api-token".to_string(),
        ));
        let sessions = Arc::new(SessionManager::new());
        let active_work = Arc::new(AsyncMutex::new(HashMap::new()));
        let work = WorkResponse {
            id: "work-v2".to_string(),
            work_type: "bridge".to_string(),
            environment_id: "env-1".to_string(),
            state: "queued".to_string(),
            data: crate::types::WorkData {
                data_type: "session".to_string(),
                id: "cse_session_1".to_string(),
            },
            secret: encoded_code_session_work_secret("worker-token", &api_base_url),
        };

        handle_bridge_work(
            api,
            sessions.clone(),
            config,
            Arc::new(RecordingBridgeSessionRunner),
            active_work.clone(),
            work,
            "env-1".to_string(),
        )
        .await
        .unwrap();

        assert_eq!(sessions.session_count().await, 0);
        assert!(active_work.lock().await.is_empty());
        let requests = requests.lock().unwrap().clone();
        assert_eq!(requests.len(), 6);
        let stream_requests = requests
            .iter()
            .filter(|request| {
                request.path == "/v1/code/sessions/cse_session_1/worker/events/stream"
            })
            .collect::<Vec<_>>();
        assert_eq!(stream_requests.len(), 2);
        assert_eq!(requests[5].method, "POST");
        assert_eq!(requests[5].path, "/v1/environments/env-1/bridge/reconnect");
        let reconnect_body: Value = serde_json::from_str(&requests[5].body).unwrap();
        assert_eq!(reconnect_body["session_id"], json!("cse_session_1"));

        assert!(!requests.iter().any(|request| {
            request.path == "/v1/environments/env-1/work/work-v2/stop"
                || request.path == "/v1/sessions/cse_session_1/archive"
        }));

        let _ = fs::remove_dir_all(root);
    }

    #[tokio::test]
    async fn bad_work_secret_stops_poisoned_work_item() {
        let root = std::env::temp_dir().join(format!("kiana-bridge-bad-secret-{}", Uuid::new_v4()));
        fs::create_dir_all(&root).unwrap();
        let (api_base_url, requests) =
            spawn_mock_server(vec![MockResponse::json(200, r#"{}"#)]).await;
        let mut config = bridge_config(root.to_string_lossy().to_string());
        config.api_base_url = api_base_url.clone();

        let api = Arc::new(BridgeApiClient::new(api_base_url, "api-token".to_string()));
        let sessions = Arc::new(SessionManager::new());
        let active_work = Arc::new(AsyncMutex::new(HashMap::new()));
        let work = WorkResponse {
            id: "work-bad".to_string(),
            work_type: "bridge".to_string(),
            environment_id: "env-1".to_string(),
            state: "queued".to_string(),
            data: crate::types::WorkData {
                data_type: "session".to_string(),
                id: "remote-session".to_string(),
            },
            secret: "not-valid-base64".to_string(),
        };

        let error = handle_bridge_work(
            api,
            sessions.clone(),
            config,
            Arc::new(RecordingBridgeSessionRunner),
            active_work.clone(),
            work,
            "env-1".to_string(),
        )
        .await
        .unwrap_err()
        .to_string();

        assert!(!error.trim().is_empty());
        assert_eq!(sessions.session_count().await, 0);
        assert!(active_work.lock().await.is_empty());
        let requests = requests.lock().unwrap();
        assert_eq!(requests.len(), 1);
        assert_eq!(
            requests[0].path,
            "/v1/environments/env-1/work/work-bad/stop"
        );
        assert_eq!(
            requests[0].authorization.as_deref(),
            Some("Bearer api-token")
        );
        assert_eq!(requests[0].body, r#"{"force":false}"#);

        let _ = fs::remove_dir_all(root);
    }

    #[tokio::test]
    async fn duplicate_session_work_refreshes_existing_session_instead_of_spawning() {
        let root = std::env::temp_dir().join(format!("kiana-bridge-existing-{}", Uuid::new_v4()));
        fs::create_dir_all(&root).unwrap();
        let (api_base_url, requests) =
            spawn_mock_server(vec![MockResponse::json(200, r#"{}"#)]).await;
        let mut config = bridge_config(root.to_string_lossy().to_string());
        config.api_base_url = api_base_url.clone();
        config.session_ingress_url = "wss://unreachable.example.test".to_string();

        let api = Arc::new(BridgeApiClient::new(api_base_url, "api-token".to_string()));
        let sessions = Arc::new(SessionManager::new());
        sessions
            .add_session(
                "remote-session".to_string(),
                SessionHandle {
                    session_id: "remote-session".to_string(),
                    access_token: "old-token".to_string(),
                    sdk_url: Some(
                        "wss://example.test/v1/session_ingress/ws/remote-session".to_string(),
                    ),
                    work_dir: None,
                    use_ccr_v2: false,
                    worker_epoch: None,
                },
            )
            .await;
        let active_work = Arc::new(AsyncMutex::new(HashMap::new()));
        active_work.lock().await.insert(
            "remote-session".to_string(),
            ActiveBridgeWork::new(
                "work-old".to_string(),
                "remote-session".to_string(),
                None,
                "old-token".to_string(),
            ),
        );
        let runner = Arc::new(TokenRecordingRunner::default());
        let work = WorkResponse {
            id: "work-fresh".to_string(),
            work_type: "bridge".to_string(),
            environment_id: "env-1".to_string(),
            state: "queued".to_string(),
            data: crate::types::WorkData {
                data_type: "session".to_string(),
                id: "remote-session".to_string(),
            },
            secret: encoded_work_secret("fresh-token"),
        };

        handle_bridge_work(
            api,
            sessions.clone(),
            config,
            runner.clone(),
            active_work.clone(),
            work,
            "env-1".to_string(),
        )
        .await
        .unwrap();

        assert_eq!(sessions.session_count().await, 1);
        assert_eq!(
            sessions
                .get_session("remote-session")
                .await
                .unwrap()
                .access_token,
            "fresh-token"
        );
        assert_eq!(
            runner.updated_tokens.lock().await.as_slice(),
            ["fresh-token"]
        );
        assert_eq!(*runner.handled_messages.lock().await, 0);
        let active = active_work
            .lock()
            .await
            .get("remote-session")
            .cloned()
            .unwrap();
        let lease = active.lease_snapshot().await;
        assert_eq!(lease.work_id, "work-fresh");
        assert_eq!(lease.session_ingress_token, "fresh-token");
        let status = sessions.status("remote-session").await.unwrap();
        assert!(status.activities.iter().any(|activity| {
            activity.activity_type == "token"
                && activity.summary == "Session ingress token refreshed"
        }));
        let requests = requests.lock().unwrap();
        assert_eq!(requests.len(), 1);
        assert_eq!(
            requests[0].path,
            "/v1/environments/env-1/work/work-fresh/ack"
        );
        assert_eq!(
            requests[0].authorization.as_deref(),
            Some("Bearer fresh-token")
        );

        let _ = fs::remove_dir_all(root);
    }

    #[tokio::test]
    async fn duplicate_code_session_work_refreshes_worker_credentials() {
        let root =
            std::env::temp_dir().join(format!("kiana-bridge-existing-ccr-v2-{}", Uuid::new_v4()));
        fs::create_dir_all(&root).unwrap();
        let (api_base_url, requests) = spawn_mock_server(vec![
            MockResponse::json(200, r#"{"worker_epoch":"77"}"#),
            MockResponse::json(200, r#"{}"#),
            MockResponse::json(200, r#"{}"#),
        ])
        .await;
        let session_url = format!("{api_base_url}/v1/code/sessions/cse_session_1");
        let mut config = bridge_config(root.to_string_lossy().to_string());
        config.api_base_url = api_base_url.clone();
        config.session_ingress_url = "wss://unreachable.example.test".to_string();

        let worker_client =
            CcrV2WorkerClient::new(session_url.clone(), "cse_session_1", "old-token", 1).unwrap();
        let api = Arc::new(BridgeApiClient::new(
            api_base_url.clone(),
            "api-token".to_string(),
        ));
        let sessions = Arc::new(SessionManager::new());
        sessions
            .add_session(
                "cse_session_1".to_string(),
                SessionHandle {
                    session_id: "cse_session_1".to_string(),
                    access_token: "old-token".to_string(),
                    sdk_url: Some(session_url),
                    work_dir: None,
                    use_ccr_v2: true,
                    worker_epoch: Some(1),
                },
            )
            .await;
        let active_work = Arc::new(AsyncMutex::new(HashMap::new()));
        active_work.lock().await.insert(
            "cse_session_1".to_string(),
            ActiveBridgeWork::new(
                "work-old".to_string(),
                "cse_session_1".to_string(),
                None,
                "old-token".to_string(),
            )
            .with_ccr_v2_client(worker_client.clone()),
        );
        let runner = Arc::new(TokenRecordingRunner::default());
        let work = WorkResponse {
            id: "work-fresh".to_string(),
            work_type: "bridge".to_string(),
            environment_id: "env-1".to_string(),
            state: "queued".to_string(),
            data: crate::types::WorkData {
                data_type: "session".to_string(),
                id: "cse_session_1".to_string(),
            },
            secret: encoded_code_session_work_secret("fresh-token", &api_base_url),
        };

        handle_bridge_work(
            api,
            sessions.clone(),
            config,
            runner.clone(),
            active_work.clone(),
            work,
            "env-1".to_string(),
        )
        .await
        .unwrap();
        worker_client.send_heartbeat().await.unwrap();

        let handle = sessions.get_session("cse_session_1").await.unwrap();
        assert_eq!(handle.access_token, "fresh-token");
        assert_eq!(handle.worker_epoch, Some(77));
        assert!(handle.use_ccr_v2);
        assert_eq!(
            runner.updated_tokens.lock().await.as_slice(),
            ["fresh-token"]
        );
        let active = active_work
            .lock()
            .await
            .get("cse_session_1")
            .cloned()
            .unwrap();
        let lease = active.lease_snapshot().await;
        assert_eq!(lease.work_id, "work-fresh");
        assert_eq!(lease.session_ingress_token, "fresh-token");

        let requests = requests.lock().unwrap().clone();
        assert_eq!(requests.len(), 3);
        assert_eq!(requests[0].method, "POST");
        assert_eq!(
            requests[0].path,
            "/v1/code/sessions/cse_session_1/worker/register"
        );
        assert_eq!(
            requests[0].authorization.as_deref(),
            Some("Bearer fresh-token")
        );

        assert_eq!(requests[1].method, "POST");
        assert_eq!(
            requests[1].path,
            "/v1/environments/env-1/work/work-fresh/ack"
        );
        assert_eq!(
            requests[1].authorization.as_deref(),
            Some("Bearer fresh-token")
        );

        assert_eq!(requests[2].method, "POST");
        assert_eq!(
            requests[2].path,
            "/v1/code/sessions/cse_session_1/worker/heartbeat"
        );
        assert_eq!(
            requests[2].authorization.as_deref(),
            Some("Bearer fresh-token")
        );
        let heartbeat_body: Value = serde_json::from_str(&requests[2].body).unwrap();
        assert_eq!(heartbeat_body["worker_epoch"], json!(77));

        let _ = fs::remove_dir_all(root);
    }

    #[tokio::test]
    async fn bridge_work_dispatch_defers_new_session_when_at_capacity() {
        let sessions = SessionManager::new();
        sessions
            .add_session("busy-session".to_string(), session_handle())
            .await;
        let active_work = AsyncMutex::new(HashMap::new());
        let permits = Arc::new(Semaphore::new(1));
        let work = work_response("work-new", "session", "new-session", "new-token");

        let dispatch = bridge_work_dispatch(&sessions, &active_work, 1, permits, &work)
            .await
            .unwrap();

        assert!(matches!(dispatch, BridgeWorkDispatch::DeferAtCapacity));
    }

    #[tokio::test]
    async fn run_polls_at_capacity_to_refresh_existing_session_work() {
        let root =
            std::env::temp_dir().join(format!("kiana-bridge-run-capacity-{}", Uuid::new_v4()));
        fs::create_dir_all(&root).unwrap();
        let poll_body =
            work_response_json("work-fresh", "session", "remote-session", "fresh-token");
        let (api_base_url, requests) = spawn_mock_server(vec![
            MockResponse::json(200, &poll_body),
            MockResponse::json(200, r#"{}"#),
        ])
        .await;
        let mut config = bridge_config(root.to_string_lossy().to_string());
        config.api_base_url = api_base_url.clone();
        config.session_ingress_url = "wss://unreachable.example.test".to_string();
        config.max_sessions = 1;

        let api = Arc::new(BridgeApiClient::new(api_base_url, "api-token".to_string()));
        let sessions = Arc::new(SessionManager::new());
        sessions
            .add_session(
                "remote-session".to_string(),
                SessionHandle {
                    session_id: "remote-session".to_string(),
                    access_token: "old-token".to_string(),
                    sdk_url: Some(
                        "wss://example.test/v1/session_ingress/ws/remote-session".to_string(),
                    ),
                    work_dir: None,
                    use_ccr_v2: false,
                    worker_epoch: None,
                },
            )
            .await;
        let runner = Arc::new(TokenRecordingRunner::default());
        let work_loop = WorkPollLoop::with_runner(api, sessions.clone(), config, runner.clone());
        let active_work = work_loop.active_work.clone();
        active_work.lock().await.insert(
            "remote-session".to_string(),
            ActiveBridgeWork::new(
                "work-old".to_string(),
                "remote-session".to_string(),
                None,
                "old-token".to_string(),
            ),
        );

        let run_task = tokio::spawn(async move {
            let _ = work_loop
                .run("env-1".to_string(), "environment-secret".to_string())
                .await;
        });

        wait_for_requests(&requests, 2).await;
        run_task.abort();

        assert_eq!(
            sessions
                .get_session("remote-session")
                .await
                .unwrap()
                .access_token,
            "fresh-token"
        );
        assert_eq!(
            runner.updated_tokens.lock().await.as_slice(),
            ["fresh-token"]
        );
        let active = active_work
            .lock()
            .await
            .get("remote-session")
            .cloned()
            .unwrap();
        let lease = active.lease_snapshot().await;
        assert_eq!(lease.work_id, "work-fresh");
        assert_eq!(lease.session_ingress_token, "fresh-token");

        let requests = requests.lock().unwrap();
        assert_eq!(requests[0].method, "GET");
        assert_eq!(requests[0].path, "/v1/environments/env-1/work/poll");
        assert_eq!(
            requests[0].authorization.as_deref(),
            Some("Bearer environment-secret")
        );
        assert_eq!(requests[1].method, "POST");
        assert_eq!(
            requests[1].path,
            "/v1/environments/env-1/work/work-fresh/ack"
        );
        assert_eq!(
            requests[1].authorization.as_deref(),
            Some("Bearer fresh-token")
        );

        let _ = fs::remove_dir_all(root);
    }

    #[cfg(unix)]
    #[tokio::test(flavor = "current_thread")]
    async fn work_poll_loop_stream_json_smoke_round_trips_websocket_permission_and_child() {
        let _guard = ENV_LOCK.lock().unwrap();
        let command = take_env(BRIDGE_RUNNER_COMMAND_ENV);
        let args = take_env(BRIDGE_RUNNER_ARGS_ENV);
        let mode = take_env(BRIDGE_RUNNER_MODE_ENV);

        let root = std::env::temp_dir().join(format!("kiana-bridge-e2e-{}", Uuid::new_v4()));
        fs::create_dir_all(&root).unwrap();
        let script_path = root.join("bridge-child");
        fs::write(
            &script_path,
            r#"#!/bin/sh
IFS= read -r user_line
case "$user_line" in
  *'"type":"user"'*remote\ run*) ;;
  *) printf '{"type":"assistant","uuid":"assistant","message":{"content":"missing user"}}\n'; exit 0 ;;
esac
printf '{"type":"control_request","request_id":"perm-e2e","request":{"subtype":"can_use_tool","tool_name":"Bash","input":{"command":"echo ok"},"tool_use_id":"toolu_e2e"}}\n'
IFS= read -r response_line
case "$response_line" in
  *control_response*success*perm-e2e*) printf '{"type":"assistant","uuid":"assistant","message":{"content":"bridge child done"}}\n' ;;
  *) printf '{"type":"assistant","uuid":"assistant","message":{"content":"permission missing"}}\n' ;;
esac
"#,
        )
        .unwrap();
        use std::os::unix::fs::PermissionsExt;
        let mut permissions = fs::metadata(&script_path).unwrap().permissions();
        permissions.set_mode(0o755);
        fs::set_permissions(&script_path, permissions).unwrap();

        std::env::set_var(
            BRIDGE_RUNNER_COMMAND_ENV,
            script_path.to_string_lossy().to_string(),
        );
        std::env::set_var(BRIDGE_RUNNER_MODE_ENV, "stream-json");

        let (session_ingress_url, ws_done) = spawn_bridge_session_ws_smoke_server().await;
        let poll_body =
            work_response_json("work-e2e", "session", "remote-session", "session-token");
        let (api_base_url, requests) = spawn_bridge_api_smoke_server(poll_body).await;
        let mut config = bridge_config(root.to_string_lossy().to_string());
        config.api_base_url = api_base_url.clone();
        config.session_ingress_url = session_ingress_url;
        config.heartbeat_interval_ms = 0;
        config.session_timeout_ms = 5_000;

        let api = Arc::new(BridgeApiClient::new(api_base_url, "api-token".to_string()));
        let sessions = Arc::new(SessionManager::new());
        let work_loop = Arc::new(WorkPollLoop::new(api, sessions.clone(), config).unwrap());
        let run_loop = work_loop.clone();
        let run_task = tokio::spawn(async move {
            let _ = run_loop
                .run("env-1".to_string(), "environment-secret".to_string())
                .await;
        });

        let ws_messages = tokio::time::timeout(Duration::from_secs(3), ws_done)
            .await
            .unwrap()
            .unwrap();
        wait_for_request_path(&requests, "/v1/sessions/remote-session/archive").await;
        run_task.abort();

        assert!(ws_messages.iter().any(|message| {
            matches!(
                message,
                SDKMessage::ControlRequest {
                    request_id,
                    request:
                        ControlRequestType::CanUseTool {
                            tool_name,
                            input,
                            tool_use_id,
                        },
                } if request_id == "perm-e2e"
                    && tool_name == "Bash"
                    && input["command"] == "echo ok"
                    && tool_use_id == "toolu_e2e"
            )
        }));
        assert!(ws_messages.iter().any(|message| {
            matches!(
                message,
                SDKMessage::Assistant { message, .. }
                    if content_summary(message) == "bridge child done"
            )
        }));
        assert_eq!(sessions.session_count().await, 0);
        assert!(work_loop.active_work.lock().await.is_empty());

        let requests = requests.lock().unwrap();
        assert!(requests.iter().any(|request| {
            request.method == "GET"
                && request.path == "/v1/environments/env-1/work/poll"
                && request.authorization.as_deref() == Some("Bearer environment-secret")
        }));
        assert!(requests.iter().any(|request| {
            request.method == "POST"
                && request.path == "/v1/environments/env-1/work/work-e2e/ack"
                && request.authorization.as_deref() == Some("Bearer session-token")
        }));
        assert!(requests.iter().any(|request| {
            request.method == "POST"
                && request.path == "/v1/environments/env-1/work/work-e2e/stop"
                && request.authorization.as_deref() == Some("Bearer api-token")
                && request.body == r#"{"force":false}"#
        }));
        assert!(requests.iter().any(|request| {
            request.method == "POST"
                && request.path == "/v1/sessions/remote-session/archive"
                && request.authorization.as_deref() == Some("Bearer api-token")
        }));
        drop(requests);

        let _ = fs::remove_dir_all(root);
        restore_env(BRIDGE_RUNNER_COMMAND_ENV, command);
        restore_env(BRIDGE_RUNNER_ARGS_ENV, args);
        restore_env(BRIDGE_RUNNER_MODE_ENV, mode);
    }

    #[tokio::test]
    async fn bridge_session_timeout_stops_and_archives_work() {
        let root = std::env::temp_dir().join(format!("kiana-bridge-timeout-{}", Uuid::new_v4()));
        fs::create_dir_all(&root).unwrap();
        let (api_base_url, requests) = spawn_mock_server(vec![
            MockResponse::json(200, r#"{}"#),
            MockResponse::json(200, r#"{}"#),
            MockResponse::json(200, r#"{}"#),
        ])
        .await;
        let session_ingress_url = spawn_idle_ws_server().await;
        let mut config = bridge_config(root.to_string_lossy().to_string());
        config.api_base_url = api_base_url.clone();
        config.session_ingress_url = session_ingress_url;
        config.heartbeat_interval_ms = 0;
        config.session_timeout_ms = 50;

        let api = Arc::new(BridgeApiClient::new(api_base_url, "api-token".to_string()));
        let sessions = Arc::new(SessionManager::new());
        let active_work = Arc::new(AsyncMutex::new(HashMap::new()));
        let work = WorkResponse {
            id: "work-1".to_string(),
            work_type: "bridge".to_string(),
            environment_id: "env-1".to_string(),
            state: "queued".to_string(),
            data: crate::types::WorkData {
                data_type: "session".to_string(),
                id: "remote-session".to_string(),
            },
            secret: encoded_work_secret("session-token"),
        };

        handle_bridge_work(
            api,
            sessions.clone(),
            config,
            Arc::new(RecordingBridgeSessionRunner),
            active_work.clone(),
            work,
            "env-1".to_string(),
        )
        .await
        .unwrap();

        assert_eq!(sessions.session_count().await, 0);
        assert!(active_work.lock().await.is_empty());
        let requests = requests.lock().unwrap();
        assert_eq!(requests.len(), 3);
        assert_eq!(requests[0].path, "/v1/environments/env-1/work/work-1/ack");
        assert_eq!(
            requests[0].authorization.as_deref(),
            Some("Bearer session-token")
        );
        assert_eq!(requests[1].path, "/v1/environments/env-1/work/work-1/stop");
        assert_eq!(requests[1].body, r#"{"force":false}"#);
        assert_eq!(requests[2].path, "/v1/sessions/remote-session/archive");

        let _ = fs::remove_dir_all(root);
    }

    #[tokio::test]
    async fn stop_bridge_work_retries_transient_failure() {
        let (base_url, requests) = spawn_mock_server(vec![
            MockResponse::json(500, r#"{"error":"temporary"}"#),
            MockResponse::json(200, r#"{}"#),
        ])
        .await;
        let api = BridgeApiClient::new(base_url, "api-token".to_string());

        stop_bridge_work_with_retry_config(
            &api,
            "env-1",
            "work-1",
            false,
            StopWorkRetryConfig {
                max_attempts: 3,
                base_delay: Duration::from_millis(1),
            },
        )
        .await
        .unwrap();

        let requests = requests.lock().unwrap();
        assert_eq!(requests.len(), 2);
        assert_eq!(requests[0].path, "/v1/environments/env-1/work/work-1/stop");
        assert_eq!(requests[0].body, r#"{"force":false}"#);
        assert_eq!(requests[1].body, r#"{"force":false}"#);
    }

    #[tokio::test]
    async fn stop_bridge_work_does_not_retry_fatal_auth_failure() {
        let (base_url, requests) = spawn_mock_server(vec![
            MockResponse::json(401, r#"{"error":"expired"}"#),
            MockResponse::json(200, r#"{}"#),
        ])
        .await;
        let api = BridgeApiClient::new(base_url, "api-token".to_string());

        let error = stop_bridge_work_with_retry_config(
            &api,
            "env-1",
            "work-1",
            true,
            StopWorkRetryConfig {
                max_attempts: 3,
                base_delay: Duration::from_millis(1),
            },
        )
        .await
        .unwrap_err()
        .to_string();

        assert!(error.contains("StopWork failed: HTTP 401"));
        let requests = requests.lock().unwrap();
        assert_eq!(requests.len(), 1);
        assert_eq!(requests[0].path, "/v1/environments/env-1/work/work-1/stop");
        assert_eq!(requests[0].body, r#"{"force":true}"#);
    }

    #[test]
    fn command_runner_uses_default_kiana_print_arguments() {
        let _guard = ENV_LOCK.lock().unwrap();
        let command = take_env(BRIDGE_RUNNER_COMMAND_ENV);
        let args = take_env(BRIDGE_RUNNER_ARGS_ENV);
        let mode = take_env(BRIDGE_RUNNER_MODE_ENV);

        let runner = CommandBridgeSessionRunner::from_env_or_default().unwrap();

        assert_eq!(runner.command(), "kiana");
        assert_eq!(
            runner.args(),
            ["-p", "--output-format", "json", "--execute", "--"]
        );

        restore_env(BRIDGE_RUNNER_COMMAND_ENV, command);
        restore_env(BRIDGE_RUNNER_ARGS_ENV, args);
        restore_env(BRIDGE_RUNNER_MODE_ENV, mode);
    }

    #[test]
    fn command_runner_accepts_env_overrides() {
        let _guard = ENV_LOCK.lock().unwrap();
        let command = take_env(BRIDGE_RUNNER_COMMAND_ENV);
        let args = take_env(BRIDGE_RUNNER_ARGS_ENV);
        let mode = take_env(BRIDGE_RUNNER_MODE_ENV);

        std::env::set_var(BRIDGE_RUNNER_COMMAND_ENV, "/tmp/kiana");
        std::env::set_var(BRIDGE_RUNNER_ARGS_ENV, "-p --record-only --");
        std::env::set_var(BRIDGE_RUNNER_MODE_ENV, "stream-json");
        let runner = CommandBridgeSessionRunner::from_env_or_default().unwrap();

        assert_eq!(runner.command(), "/tmp/kiana");
        assert_eq!(runner.args(), ["-p", "--record-only", "--"]);
        assert!(runner.is_stream_json_mode());

        restore_env(BRIDGE_RUNNER_COMMAND_ENV, command);
        restore_env(BRIDGE_RUNNER_ARGS_ENV, args);
        restore_env(BRIDGE_RUNNER_MODE_ENV, mode);
    }

    #[cfg(unix)]
    #[tokio::test(flavor = "current_thread")]
    async fn stream_json_runner_defaults_to_sdk_url_child_arguments() {
        let _guard = ENV_LOCK.lock().unwrap();
        let command = take_env(BRIDGE_RUNNER_COMMAND_ENV);
        let args = take_env(BRIDGE_RUNNER_ARGS_ENV);
        let mode = take_env(BRIDGE_RUNNER_MODE_ENV);
        let oauth = take_env("CLAUDE_CODE_OAUTH_TOKEN");

        let root =
            std::env::temp_dir().join(format!("kiana-bridge-default-args-{}", Uuid::new_v4()));
        fs::create_dir_all(&root).unwrap();
        let script_path = root.join("bridge-child");
        fs::write(
            &script_path,
            r#"#!/bin/sh
IFS= read -r user_line
if [ "$1" = "--print" ] &&
   [ "$2" = "--sdk-url" ] &&
   [ "$3" = "wss://example.test/v1/session_ingress/ws/remote-session" ] &&
   [ "$4" = "--session-id" ] &&
   [ "$5" = "remote-session" ] &&
   [ "$6" = "--input-format" ] &&
   [ "$7" = "stream-json" ] &&
   [ "$8" = "--output-format" ] &&
   [ "$9" = "stream-json" ] &&
   [ "${10}" = "--replay-user-messages" ] &&
   [ "$CLAUDE_CODE_SESSION_ACCESS_TOKEN" = "token" ] &&
   [ "$KIANA_BRIDGE_SESSION_ACCESS_TOKEN" = "token" ] &&
   [ "$CLAUDE_CODE_ENVIRONMENT_KIND" = "bridge" ] &&
   [ "$CLAUDE_CODE_POST_FOR_SESSION_INGRESS_V2" = "1" ] &&
   [ -z "${CLAUDE_CODE_OAUTH_TOKEN+x}" ]; then
  printf '{"type":"assistant","uuid":"assistant","message":{"content":"default args ok"}}\n'
else
  printf 'args=%s env=%s/%s/%s/%s oauth=%s\n' "$*" "$CLAUDE_CODE_SESSION_ACCESS_TOKEN" "$KIANA_BRIDGE_SESSION_ACCESS_TOKEN" "$CLAUDE_CODE_ENVIRONMENT_KIND" "$CLAUDE_CODE_POST_FOR_SESSION_INGRESS_V2" "${CLAUDE_CODE_OAUTH_TOKEN-unset}" >&2
  printf '{"type":"assistant","uuid":"assistant","message":{"content":"default args missing"}}\n'
fi
"#,
        )
        .unwrap();
        use std::os::unix::fs::PermissionsExt;
        let mut permissions = fs::metadata(&script_path).unwrap().permissions();
        permissions.set_mode(0o755);
        fs::set_permissions(&script_path, permissions).unwrap();

        std::env::set_var(
            BRIDGE_RUNNER_COMMAND_ENV,
            script_path.to_string_lossy().to_string(),
        );
        std::env::set_var(BRIDGE_RUNNER_MODE_ENV, "stream-json");
        std::env::set_var("CLAUDE_CODE_OAUTH_TOKEN", "parent-oauth-token");

        let runner = CommandBridgeSessionRunner::from_env_or_default()
            .unwrap()
            .with_timeout_ms(1_000);
        let response = runner
            .handle_user_message(
                &session_handle(),
                "u1".to_string(),
                MessageContent {
                    content: ContentBlock::Text("hello".to_string()),
                },
            )
            .await
            .unwrap();

        match response {
            SDKMessage::Assistant { message, .. } => {
                assert_eq!(content_summary(&message), "default args ok");
            }
            other => panic!("expected assistant message, got {:?}", other),
        }

        runner.end_session("remote-session").await.unwrap();
        let _ = fs::remove_dir_all(root);
        restore_env(BRIDGE_RUNNER_COMMAND_ENV, command);
        restore_env(BRIDGE_RUNNER_ARGS_ENV, args);
        restore_env(BRIDGE_RUNNER_MODE_ENV, mode);
        restore_env("CLAUDE_CODE_OAUTH_TOKEN", oauth);
    }

    #[cfg(unix)]
    #[tokio::test(flavor = "current_thread")]
    async fn stream_json_runner_sets_ccr_v2_child_environment() {
        let _guard = ENV_LOCK.lock().unwrap();
        let command = take_env(BRIDGE_RUNNER_COMMAND_ENV);
        let args = take_env(BRIDGE_RUNNER_ARGS_ENV);
        let mode = take_env(BRIDGE_RUNNER_MODE_ENV);
        let oauth = take_env("CLAUDE_CODE_OAUTH_TOKEN");
        let parent_use_ccr_v2 = take_env("CLAUDE_CODE_USE_CCR_V2");
        let parent_worker_epoch = take_env("CLAUDE_CODE_WORKER_EPOCH");

        let root = std::env::temp_dir().join(format!("kiana-bridge-ccr-v2-{}", Uuid::new_v4()));
        fs::create_dir_all(&root).unwrap();
        let script_path = root.join("bridge-child-ccr-v2");
        fs::write(
            &script_path,
            r#"#!/bin/sh
IFS= read -r user_line
if [ "$1" = "--print" ] &&
   [ "$2" = "--sdk-url" ] &&
   [ "$3" = "https://api.example/v1/code/sessions/cse_session_1" ] &&
   [ "$4" = "--session-id" ] &&
   [ "$5" = "cse_session_1" ] &&
   [ "$6" = "--input-format" ] &&
   [ "$7" = "stream-json" ] &&
   [ "$8" = "--output-format" ] &&
   [ "$9" = "stream-json" ] &&
   [ "${10}" = "--replay-user-messages" ] &&
   [ "$CLAUDE_CODE_SESSION_ACCESS_TOKEN" = "worker-token" ] &&
   [ "$KIANA_BRIDGE_SESSION_ACCESS_TOKEN" = "worker-token" ] &&
   [ "$CLAUDE_CODE_ENVIRONMENT_KIND" = "bridge" ] &&
   [ "$CLAUDE_CODE_POST_FOR_SESSION_INGRESS_V2" = "1" ] &&
   [ "$CLAUDE_CODE_USE_CCR_V2" = "1" ] &&
   [ "$CLAUDE_CODE_WORKER_EPOCH" = "42" ] &&
   [ -z "${CLAUDE_CODE_OAUTH_TOKEN+x}" ]; then
  printf '{"type":"assistant","uuid":"assistant","message":{"content":"ccr v2 env ok"}}\n'
else
  printf 'args=%s env=%s/%s/%s/%s/%s/%s oauth=%s\n' "$*" "$CLAUDE_CODE_SESSION_ACCESS_TOKEN" "$KIANA_BRIDGE_SESSION_ACCESS_TOKEN" "$CLAUDE_CODE_ENVIRONMENT_KIND" "$CLAUDE_CODE_POST_FOR_SESSION_INGRESS_V2" "$CLAUDE_CODE_USE_CCR_V2" "$CLAUDE_CODE_WORKER_EPOCH" "${CLAUDE_CODE_OAUTH_TOKEN-unset}" >&2
  printf '{"type":"assistant","uuid":"assistant","message":{"content":"ccr v2 env missing"}}\n'
fi
"#,
        )
        .unwrap();
        use std::os::unix::fs::PermissionsExt;
        let mut permissions = fs::metadata(&script_path).unwrap().permissions();
        permissions.set_mode(0o755);
        fs::set_permissions(&script_path, permissions).unwrap();

        std::env::set_var(
            BRIDGE_RUNNER_COMMAND_ENV,
            script_path.to_string_lossy().to_string(),
        );
        std::env::set_var(BRIDGE_RUNNER_MODE_ENV, "stream-json");
        std::env::set_var("CLAUDE_CODE_OAUTH_TOKEN", "parent-oauth-token");
        std::env::set_var("CLAUDE_CODE_USE_CCR_V2", "parent-value");
        std::env::set_var("CLAUDE_CODE_WORKER_EPOCH", "7");

        let handle = SessionHandle {
            session_id: "cse_session_1".to_string(),
            access_token: "worker-token".to_string(),
            sdk_url: Some("https://api.example/v1/code/sessions/cse_session_1".to_string()),
            work_dir: None,
            use_ccr_v2: true,
            worker_epoch: Some(42),
        };
        let runner = CommandBridgeSessionRunner::from_env_or_default()
            .unwrap()
            .with_timeout_ms(1_000);
        let response = runner
            .handle_user_message(
                &handle,
                "u1".to_string(),
                MessageContent {
                    content: ContentBlock::Text("hello".to_string()),
                },
            )
            .await
            .unwrap();

        match response {
            SDKMessage::Assistant { message, .. } => {
                assert_eq!(content_summary(&message), "ccr v2 env ok");
            }
            other => panic!("expected assistant message, got {:?}", other),
        }

        runner.end_session("cse_session_1").await.unwrap();
        let _ = fs::remove_dir_all(root);
        restore_env(BRIDGE_RUNNER_COMMAND_ENV, command);
        restore_env(BRIDGE_RUNNER_ARGS_ENV, args);
        restore_env(BRIDGE_RUNNER_MODE_ENV, mode);
        restore_env("CLAUDE_CODE_OAUTH_TOKEN", oauth);
        restore_env("CLAUDE_CODE_USE_CCR_V2", parent_use_ccr_v2);
        restore_env("CLAUDE_CODE_WORKER_EPOCH", parent_worker_epoch);
    }

    #[cfg(unix)]
    #[tokio::test(flavor = "current_thread")]
    async fn stream_json_runner_applies_initial_permission_mode_to_child_arguments() {
        let _guard = ENV_LOCK.lock().unwrap();
        let command = take_env(BRIDGE_RUNNER_COMMAND_ENV);
        let args = take_env(BRIDGE_RUNNER_ARGS_ENV);
        let mode = take_env(BRIDGE_RUNNER_MODE_ENV);
        let oauth = take_env("CLAUDE_CODE_OAUTH_TOKEN");

        let root =
            std::env::temp_dir().join(format!("kiana-bridge-permission-args-{}", Uuid::new_v4()));
        fs::create_dir_all(&root).unwrap();
        let script_path = root.join("bridge-child-permission");
        fs::write(
            &script_path,
            r#"#!/bin/sh
IFS= read -r user_line
if [ "$1" = "--print" ] &&
   [ "$2" = "--sdk-url" ] &&
   [ "$3" = "wss://example.test/v1/session_ingress/ws/remote-session" ] &&
   [ "$4" = "--session-id" ] &&
   [ "$5" = "remote-session" ] &&
   [ "$6" = "--input-format" ] &&
   [ "$7" = "stream-json" ] &&
   [ "$8" = "--output-format" ] &&
   [ "$9" = "stream-json" ] &&
   [ "${10}" = "--replay-user-messages" ] &&
   [ "${11}" = "--permission-mode" ] &&
   [ "${12}" = "ask" ] &&
   [ "$KIANA_PERMISSION_MODE" = "ask" ]; then
  printf '{"type":"assistant","uuid":"assistant","message":{"content":"permission args ok"}}\n'
else
  printf 'args=%s env=%s\n' "$*" "$KIANA_PERMISSION_MODE" >&2
  printf '{"type":"assistant","uuid":"assistant","message":{"content":"permission args missing"}}\n'
fi
"#,
        )
        .unwrap();
        use std::os::unix::fs::PermissionsExt;
        let mut permissions = fs::metadata(&script_path).unwrap().permissions();
        permissions.set_mode(0o755);
        fs::set_permissions(&script_path, permissions).unwrap();

        std::env::set_var(
            BRIDGE_RUNNER_COMMAND_ENV,
            script_path.to_string_lossy().to_string(),
        );
        std::env::set_var(BRIDGE_RUNNER_MODE_ENV, "stream-json");

        let config = crate::types::BridgeConfig {
            dir: std::env::current_dir()
                .unwrap()
                .to_string_lossy()
                .to_string(),
            machine_name: "machine".to_string(),
            branch: "main".to_string(),
            git_repo_url: None,
            max_sessions: 1,
            spawn_mode: crate::types::SpawnMode::SingleSession,
            bridge_id: "bridge".to_string(),
            worker_type: "kiana".to_string(),
            environment_id: "env".to_string(),
            api_base_url: "https://example.test".to_string(),
            session_ingress_url: "wss://example.test".to_string(),
            heartbeat_interval_ms: 60_000,
            session_timeout_ms: 24 * 60 * 60 * 1000,
            ccr_v2_sse_reconnect_give_up_ms: Some(0),
            ccr_v2_sse_liveness_timeout_ms: Some(0),
            debug_file: None,
            permission_mode: Some("ask".to_string()),
        };
        let runner = CommandBridgeSessionRunner::for_config(&config)
            .unwrap()
            .with_timeout_ms(1_000);

        let response = runner
            .handle_user_message(
                &session_handle(),
                "u1".to_string(),
                MessageContent {
                    content: ContentBlock::Text("hello".to_string()),
                },
            )
            .await
            .unwrap();

        match response {
            SDKMessage::Assistant { message, .. } => {
                assert_eq!(content_summary(&message), "permission args ok");
            }
            other => panic!("expected assistant message, got {:?}", other),
        }

        runner.end_session("remote-session").await.unwrap();
        let _ = fs::remove_dir_all(root);
        restore_env(BRIDGE_RUNNER_COMMAND_ENV, command);
        restore_env(BRIDGE_RUNNER_ARGS_ENV, args);
        restore_env(BRIDGE_RUNNER_MODE_ENV, mode);
        restore_env("CLAUDE_CODE_OAUTH_TOKEN", oauth);
    }

    #[test]
    fn command_runner_args_use_shell_like_quoting() {
        let _guard = ENV_LOCK.lock().unwrap();
        let command = take_env(BRIDGE_RUNNER_COMMAND_ENV);
        let args = take_env(BRIDGE_RUNNER_ARGS_ENV);
        let mode = take_env(BRIDGE_RUNNER_MODE_ENV);

        std::env::set_var(
            BRIDGE_RUNNER_ARGS_ENV,
            "--system-prompt 'hello world' --json-schema '{\"type\":\"object\"}' --",
        );
        let runner = CommandBridgeSessionRunner::from_env_or_default().unwrap();

        assert_eq!(
            runner.args(),
            [
                "--system-prompt",
                "hello world",
                "--json-schema",
                "{\"type\":\"object\"}",
                "--"
            ]
        );

        restore_env(BRIDGE_RUNNER_COMMAND_ENV, command);
        restore_env(BRIDGE_RUNNER_ARGS_ENV, args);
        restore_env(BRIDGE_RUNNER_MODE_ENV, mode);
    }

    #[test]
    fn command_runner_rejects_invalid_runner_args_quoting() {
        let _guard = ENV_LOCK.lock().unwrap();
        let command = take_env(BRIDGE_RUNNER_COMMAND_ENV);
        let args = take_env(BRIDGE_RUNNER_ARGS_ENV);
        let mode = take_env(BRIDGE_RUNNER_MODE_ENV);

        std::env::set_var(BRIDGE_RUNNER_ARGS_ENV, "--system-prompt 'unterminated");
        let error = CommandBridgeSessionRunner::from_env_or_default()
            .unwrap_err()
            .to_string();

        assert!(error.contains(BRIDGE_RUNNER_ARGS_ENV));
        assert!(error.contains("invalid shell quoting"));

        restore_env(BRIDGE_RUNNER_COMMAND_ENV, command);
        restore_env(BRIDGE_RUNNER_ARGS_ENV, args);
        restore_env(BRIDGE_RUNNER_MODE_ENV, mode);
    }

    #[test]
    fn command_runner_for_config_uses_configured_directory() {
        let _guard = ENV_LOCK.lock().unwrap();
        let command = take_env(BRIDGE_RUNNER_COMMAND_ENV);
        let args = take_env(BRIDGE_RUNNER_ARGS_ENV);
        let mode = take_env(BRIDGE_RUNNER_MODE_ENV);
        let timeout = take_env(BRIDGE_RUNNER_TIMEOUT_MS_ENV);

        let config = bridge_config("/tmp/kiana-project".to_string());
        let runner = CommandBridgeSessionRunner::for_config(&config).unwrap();

        assert_eq!(runner.cwd(), Some(Path::new("/tmp/kiana-project")));
        assert_eq!(
            runner.timeout(),
            Some(Duration::from_millis(config.session_timeout_ms))
        );

        restore_env(BRIDGE_RUNNER_COMMAND_ENV, command);
        restore_env(BRIDGE_RUNNER_ARGS_ENV, args);
        restore_env(BRIDGE_RUNNER_MODE_ENV, mode);
        restore_env(BRIDGE_RUNNER_TIMEOUT_MS_ENV, timeout);
    }

    #[test]
    fn command_runner_timeout_accepts_env_override_and_disable() {
        let _guard = ENV_LOCK.lock().unwrap();
        let mode = take_env(BRIDGE_RUNNER_MODE_ENV);
        let timeout = take_env(BRIDGE_RUNNER_TIMEOUT_MS_ENV);
        let mut config = bridge_config("/tmp/kiana-project".to_string());
        config.session_timeout_ms = 250;

        std::env::set_var(BRIDGE_RUNNER_TIMEOUT_MS_ENV, "75");
        let runner = CommandBridgeSessionRunner::for_config(&config).unwrap();
        assert_eq!(runner.timeout(), Some(Duration::from_millis(75)));

        std::env::set_var(BRIDGE_RUNNER_TIMEOUT_MS_ENV, "0");
        let runner = CommandBridgeSessionRunner::for_config(&config).unwrap();
        assert_eq!(runner.timeout(), None);

        std::env::set_var(BRIDGE_RUNNER_TIMEOUT_MS_ENV, "not-a-number");
        let error = CommandBridgeSessionRunner::for_config(&config)
            .unwrap_err()
            .to_string();
        assert!(error.contains(BRIDGE_RUNNER_TIMEOUT_MS_ENV));

        restore_env(BRIDGE_RUNNER_MODE_ENV, mode);
        restore_env(BRIDGE_RUNNER_TIMEOUT_MS_ENV, timeout);
    }

    #[tokio::test]
    async fn user_message_is_recorded_and_acknowledged() {
        let sessions = SessionManager::new();
        let handle = session_handle();
        let runner = RecordingBridgeSessionRunner;
        sessions
            .add_session(handle.session_id.clone(), handle.clone())
            .await;

        let response = handle_sdk_message(
            &sessions,
            &handle,
            SDKMessage::User {
                uuid: "u1".to_string(),
                message: MessageContent {
                    content: ContentBlock::Text("Inspect the bridge".to_string()),
                },
            },
            &runner,
        )
        .await
        .map(single_message)
        .unwrap();

        match response {
            SDKMessage::Assistant { message, .. } => {
                assert!(content_summary(&message).contains("Remote message recorded"));
            }
            other => panic!("expected assistant ack, got {:?}", other),
        }

        let status = sessions.status(&handle.session_id).await.unwrap();
        assert_eq!(status.activities.len(), 2);
        assert_eq!(status.activities[0].activity_type, "user");
        assert!(status.activities[0].summary.contains("Inspect the bridge"));
        assert_eq!(status.activities[1].activity_type, "text");
        assert!(status.activities[1]
            .summary
            .contains("Remote message recorded"));
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn command_runner_executes_prompt_and_returns_assistant_message() {
        let runner = CommandBridgeSessionRunner::new(
            "/bin/sh",
            [
                "-c",
                "printf '{\"assistant_text\":\"processed:%s\"}' \"$1\"",
                "bridge-test",
            ],
        );
        let response = runner
            .handle_user_message(
                &session_handle(),
                "u1".to_string(),
                MessageContent {
                    content: ContentBlock::Text("hello".to_string()),
                },
            )
            .await
            .unwrap();

        match response {
            SDKMessage::Assistant { message, .. } => {
                assert_eq!(content_summary(&message), "processed:hello");
            }
            other => panic!("expected assistant message, got {:?}", other),
        }
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn one_shot_command_runner_applies_server_control_overrides_to_child_env() {
        let runner = CommandBridgeSessionRunner::new(
            "/bin/sh",
            [
                "-c",
                "printf '{\"assistant_text\":\"model:%s permission:%s thinking:%s prompt:%s\"}' \"$ANTHROPIC_MODEL\" \"$KIANA_PERMISSION_MODE\" \"$KIANA_MAX_THINKING_TOKENS\" \"$1\"",
                "bridge-test",
            ],
        )
        .with_timeout_ms(1_000);
        let sessions = SessionManager::new();
        let handle = session_handle();
        sessions
            .add_session(handle.session_id.clone(), handle.clone())
            .await;

        let model_response = handle_sdk_message(
            &sessions,
            &handle,
            SDKMessage::ControlRequest {
                request_id: "req-model".to_string(),
                request: ControlRequestType::SetModel {
                    model: Some("controlled-model".to_string()),
                },
            },
            &runner,
        )
        .await
        .map(single_message)
        .unwrap();
        match model_response {
            SDKMessage::ControlResponse {
                response:
                    ControlResponseType::Success {
                        request_id,
                        response,
                    },
            } => {
                assert_eq!(request_id, "req-model");
                assert_eq!(response.unwrap()["model"], json!("controlled-model"));
            }
            other => panic!("expected model control success, got {:?}", other),
        }

        let mode_response = handle_sdk_message(
            &sessions,
            &handle,
            SDKMessage::ControlRequest {
                request_id: "req-mode".to_string(),
                request: ControlRequestType::SetPermissionMode {
                    mode: Some("accept-edits".to_string()),
                },
            },
            &runner,
        )
        .await
        .map(single_message)
        .unwrap();
        match mode_response {
            SDKMessage::ControlResponse {
                response:
                    ControlResponseType::Success {
                        request_id,
                        response,
                    },
            } => {
                assert_eq!(request_id, "req-mode");
                assert_eq!(response.unwrap()["permissionMode"], json!("acceptEdits"));
            }
            other => panic!("expected permission control success, got {:?}", other),
        }

        let thinking_response = handle_sdk_message(
            &sessions,
            &handle,
            SDKMessage::ControlRequest {
                request_id: "req-thinking".to_string(),
                request: ControlRequestType::SetMaxThinkingTokens {
                    max_thinking_tokens: Some(2048),
                },
            },
            &runner,
        )
        .await
        .map(single_message)
        .unwrap();
        match thinking_response {
            SDKMessage::ControlResponse {
                response:
                    ControlResponseType::Success {
                        request_id,
                        response,
                    },
            } => {
                assert_eq!(request_id, "req-thinking");
                assert_eq!(response.unwrap()["max_thinking_tokens"], json!(2048));
            }
            other => panic!("expected thinking token control success, got {:?}", other),
        }

        let response = handle_sdk_message(
            &sessions,
            &handle,
            SDKMessage::User {
                uuid: "u1".to_string(),
                message: MessageContent {
                    content: ContentBlock::Text("hello".to_string()),
                },
            },
            &runner,
        )
        .await
        .map(single_message)
        .unwrap();

        match response {
            SDKMessage::Assistant { message, .. } => {
                assert_eq!(
                    content_summary(&message),
                    "model:controlled-model permission:acceptEdits thinking:2048 prompt:hello"
                );
            }
            other => panic!("expected assistant message, got {:?}", other),
        }

        let status = sessions.status(&handle.session_id).await.unwrap();
        assert!(status.activities.iter().any(|activity| {
            activity.activity_type == "control"
                && activity.summary == "control request set_permission_mode -> success"
        }));

        runner.end_session(&handle.session_id).await.unwrap();
        assert!(runner.session_overrides.lock().await.is_empty());
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn stream_json_runner_reuses_child_for_session_messages() {
        let root = std::env::temp_dir().join(format!("kiana-bridge-stream-{}", Uuid::new_v4()));
        fs::create_dir_all(&root).unwrap();
        let script_path = root.join("stream-runner.sh");
        fs::write(
            &script_path,
            r#"echo "started $$" >&2
while IFS= read -r line; do
  printf '{"type":"assistant","uuid":"assistant","message":{"content":"pid:%s"}}\n' "$$"
done
"#,
        )
        .unwrap();
        let runner =
            CommandBridgeSessionRunner::new("/bin/sh", [script_path.to_string_lossy().to_string()])
                .with_stream_json_mode()
                .with_timeout_ms(1_000);
        let handle = session_handle();

        let first = runner
            .handle_user_message(
                &handle,
                "u1".to_string(),
                MessageContent {
                    content: ContentBlock::Text("first".to_string()),
                },
            )
            .await
            .unwrap();
        let second = runner
            .handle_user_message(
                &handle,
                "u2".to_string(),
                MessageContent {
                    content: ContentBlock::Text("second".to_string()),
                },
            )
            .await
            .unwrap();

        let first_summary = match first {
            SDKMessage::Assistant { message, .. } => content_summary(&message),
            other => panic!("expected assistant message, got {:?}", other),
        };
        let second_summary = match second {
            SDKMessage::Assistant { message, .. } => content_summary(&message),
            other => panic!("expected assistant message, got {:?}", other),
        };
        assert!(first_summary.starts_with("pid:"));
        assert_eq!(first_summary, second_summary);
        assert_eq!(runner.stream_sessions.lock().await.len(), 1);

        runner.end_session(&handle.session_id).await.unwrap();
        assert!(runner.stream_sessions.lock().await.is_empty());
        let _ = fs::remove_dir_all(root);
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn stream_json_runner_forwards_control_response_to_child() {
        let root = std::env::temp_dir().join(format!("kiana-bridge-permission-{}", Uuid::new_v4()));
        fs::create_dir_all(&root).unwrap();
        let script_path = root.join("permission-runner.sh");
        fs::write(
            &script_path,
            r#"IFS= read -r user_line
printf '{"type":"control_request","request_id":"perm-1","request":{"subtype":"can_use_tool","tool_name":"Bash","input":{"command":"echo ok"},"tool_use_id":"toolu_1"}}\n'
IFS= read -r response_line
case "$response_line" in
  *'"subtype":"success"'*) printf '{"type":"assistant","uuid":"assistant","message":{"content":"permission accepted"}}\n' ;;
  *) printf '{"type":"assistant","uuid":"assistant","message":{"content":"unexpected response"}}\n' ;;
esac
"#,
        )
        .unwrap();
        let runner =
            CommandBridgeSessionRunner::new("/bin/sh", [script_path.to_string_lossy().to_string()])
                .with_stream_json_mode()
                .with_timeout_ms(1_000);
        let handle = session_handle();

        let request = runner
            .handle_user_message(
                &handle,
                "u1".to_string(),
                MessageContent {
                    content: ContentBlock::Text("run command".to_string()),
                },
            )
            .await
            .unwrap();
        match request {
            SDKMessage::ControlRequest {
                request_id,
                request:
                    ControlRequestType::CanUseTool {
                        tool_name,
                        input,
                        tool_use_id,
                    },
            } => {
                assert_eq!(request_id, "perm-1");
                assert_eq!(tool_name, "Bash");
                assert_eq!(tool_use_id, "toolu_1");
                assert_eq!(input["command"], "echo ok");
            }
            other => panic!("expected control request, got {:?}", other),
        }

        let response = runner
            .handle_control_response(
                &handle,
                ControlResponseType::Success {
                    request_id: "perm-1".to_string(),
                    response: Some(HashMap::from([("allowed".to_string(), json!(true))])),
                },
            )
            .await
            .unwrap()
            .unwrap();
        match response {
            SDKMessage::Assistant { message, .. } => {
                assert_eq!(content_summary(&message), "permission accepted");
            }
            other => panic!("expected assistant message, got {:?}", other),
        }

        runner.end_session(&handle.session_id).await.unwrap();
        let _ = fs::remove_dir_all(root);
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn stream_json_runner_forwards_token_refresh_to_child() {
        let root =
            std::env::temp_dir().join(format!("kiana-bridge-token-refresh-{}", Uuid::new_v4()));
        fs::create_dir_all(&root).unwrap();
        let script_path = root.join("token-runner.sh");
        fs::write(
            &script_path,
            r#"IFS= read -r first_user_line
printf '{"type":"assistant","uuid":"assistant","message":{"content":"ready"}}\n'
IFS= read -r refresh_line
IFS= read -r second_user_line
case "$refresh_line" in
  *update_environment_variables*CLAUDE_CODE_SESSION_ACCESS_TOKEN*fresh-token*)
    case "$second_user_line" in
      *'"type":"user"'*) printf '{"type":"assistant","uuid":"assistant","message":{"content":"fresh-token received"}}\n' ;;
      *) printf '{"type":"assistant","uuid":"assistant","message":{"content":"missing next user"}}\n' ;;
    esac
    ;;
  *) printf '{"type":"assistant","uuid":"assistant","message":{"content":"missing token refresh"}}\n' ;;
esac
"#,
        )
        .unwrap();
        let runner =
            CommandBridgeSessionRunner::new("/bin/sh", [script_path.to_string_lossy().to_string()])
                .with_stream_json_mode()
                .with_timeout_ms(1_000);
        let handle = session_handle();

        let ready = runner
            .handle_user_message(
                &handle,
                "u1".to_string(),
                MessageContent {
                    content: ContentBlock::Text("start".to_string()),
                },
            )
            .await
            .unwrap();
        match ready {
            SDKMessage::Assistant { message, .. } => {
                assert_eq!(content_summary(&message), "ready");
            }
            other => panic!("expected assistant message, got {:?}", other),
        }

        runner
            .update_access_token(&handle, "fresh-token".to_string())
            .await
            .unwrap();

        let response = runner
            .handle_user_message(
                &handle,
                "u2".to_string(),
                MessageContent {
                    content: ContentBlock::Text("continue".to_string()),
                },
            )
            .await
            .unwrap();
        match response {
            SDKMessage::Assistant { message, .. } => {
                assert_eq!(content_summary(&message), "fresh-token received");
            }
            other => panic!("expected assistant message, got {:?}", other),
        }

        runner.end_session(&handle.session_id).await.unwrap();
        let _ = fs::remove_dir_all(root);
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn handle_sdk_message_forwards_control_response_to_stream_runner() {
        let root = std::env::temp_dir().join(format!("kiana-bridge-control-{}", Uuid::new_v4()));
        fs::create_dir_all(&root).unwrap();
        let script_path = root.join("control-runner.sh");
        fs::write(
            &script_path,
            r#"IFS= read -r user_line
printf '{"type":"control_request","request_id":"perm-2","request":{"subtype":"can_use_tool","tool_name":"Bash","input":{"command":"pwd"},"tool_use_id":"toolu_2"}}\n'
IFS= read -r response_line
printf '{"type":"assistant","uuid":"assistant","message":{"content":"after control response"}}\n'
"#,
        )
        .unwrap();
        let runner =
            CommandBridgeSessionRunner::new("/bin/sh", [script_path.to_string_lossy().to_string()])
                .with_stream_json_mode()
                .with_timeout_ms(1_000);
        let sessions = SessionManager::new();
        let handle = session_handle();
        sessions
            .add_session(handle.session_id.clone(), handle.clone())
            .await;

        let request = handle_sdk_message(
            &sessions,
            &handle,
            SDKMessage::User {
                uuid: "u1".to_string(),
                message: MessageContent {
                    content: ContentBlock::Text("needs permission".to_string()),
                },
            },
            &runner,
        )
        .await
        .map(single_message)
        .unwrap();
        assert!(matches!(request, SDKMessage::ControlRequest { .. }));
        let status = sessions.status(&handle.session_id).await.unwrap();
        assert!(status.activities.iter().any(|activity| {
            activity.activity_type == "permission_request"
                && activity.summary == "Permission requested: Running pwd"
        }));

        let response = handle_sdk_message(
            &sessions,
            &handle,
            SDKMessage::ControlResponse {
                response: ControlResponseType::Success {
                    request_id: "perm-2".to_string(),
                    response: Some(HashMap::from([("allowed".to_string(), json!(true))])),
                },
            },
            &runner,
        )
        .await
        .map(single_message)
        .unwrap();
        match response {
            SDKMessage::Assistant { message, .. } => {
                assert_eq!(content_summary(&message), "after control response");
            }
            other => panic!("expected assistant message, got {:?}", other),
        }

        let status = sessions.status(&handle.session_id).await.unwrap();
        assert!(status
            .activities
            .iter()
            .any(|activity| activity.activity_type == "control_response"));
        assert!(status
            .activities
            .iter()
            .any(|activity| activity.activity_type == "text"
                && activity.summary == "after control response"));

        runner.end_session(&handle.session_id).await.unwrap();
        let _ = fs::remove_dir_all(root);
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn handle_sdk_message_forwards_control_cancel_request_to_stream_runner() {
        let root =
            std::env::temp_dir().join(format!("kiana-bridge-control-cancel-{}", Uuid::new_v4()));
        fs::create_dir_all(&root).unwrap();
        let script_path = root.join("control-cancel-runner.sh");
        fs::write(
            &script_path,
            r#"IFS= read -r user_line
printf '{"type":"assistant","uuid":"assistant","message":{"content":"ready"}}\n'
IFS= read -r cancel_line
IFS= read -r next_user_line
case "$cancel_line" in
  *control_cancel_request*perm-3*toolu_3*) printf '{"type":"assistant","uuid":"assistant","message":{"content":"cancel forwarded"}}\n' ;;
  *) printf '{"type":"assistant","uuid":"assistant","message":{"content":"missing cancel"}}\n' ;;
esac
"#,
        )
        .unwrap();
        let runner =
            CommandBridgeSessionRunner::new("/bin/sh", [script_path.to_string_lossy().to_string()])
                .with_stream_json_mode()
                .with_timeout_ms(1_000);
        let sessions = SessionManager::new();
        let handle = session_handle();
        sessions
            .add_session(handle.session_id.clone(), handle.clone())
            .await;

        let ready = handle_sdk_message(
            &sessions,
            &handle,
            SDKMessage::User {
                uuid: "u1".to_string(),
                message: MessageContent {
                    content: ContentBlock::Text("start".to_string()),
                },
            },
            &runner,
        )
        .await
        .map(single_message)
        .unwrap();
        assert!(matches!(ready, SDKMessage::Assistant { .. }));

        let responses = handle_sdk_message(
            &sessions,
            &handle,
            SDKMessage::ControlCancelRequest {
                request_id: "perm-3".to_string(),
                tool_use_id: Some("toolu_3".to_string()),
            },
            &runner,
        )
        .await
        .unwrap();
        assert!(responses.is_empty());
        let status = sessions.status(&handle.session_id).await.unwrap();
        assert!(status.activities.iter().any(|activity| {
            activity.activity_type == "permission_cancelled"
                && activity.summary == "Permission request cancelled: perm-3"
        }));

        let response = handle_sdk_message(
            &sessions,
            &handle,
            SDKMessage::User {
                uuid: "u2".to_string(),
                message: MessageContent {
                    content: ContentBlock::Text("after cancel".to_string()),
                },
            },
            &runner,
        )
        .await
        .map(single_message)
        .unwrap();
        match response {
            SDKMessage::Assistant { message, .. } => {
                assert_eq!(content_summary(&message), "cancel forwarded");
            }
            other => panic!("expected assistant message, got {:?}", other),
        }

        runner.end_session(&handle.session_id).await.unwrap();
        let _ = fs::remove_dir_all(root);
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn handle_sdk_message_returns_child_control_cancel_before_assistant() {
        let root =
            std::env::temp_dir().join(format!("kiana-bridge-child-cancel-{}", Uuid::new_v4()));
        fs::create_dir_all(&root).unwrap();
        let script_path = root.join("child-cancel-runner.sh");
        fs::write(
            &script_path,
            r#"IFS= read -r user_line
printf '{"type":"control_request","request_id":"perm-4","request":{"subtype":"can_use_tool","tool_name":"Bash","input":{"command":"whoami"},"tool_use_id":"toolu_4"}}\n'
IFS= read -r response_line
printf '{"type":"control_cancel_request","request_id":"perm-4"}\n'
printf '{"type":"assistant","uuid":"assistant","message":{"content":"after remote permission"}}\n'
"#,
        )
        .unwrap();
        let runner =
            CommandBridgeSessionRunner::new("/bin/sh", [script_path.to_string_lossy().to_string()])
                .with_stream_json_mode()
                .with_timeout_ms(1_000);
        let sessions = SessionManager::new();
        let handle = session_handle();
        sessions
            .add_session(handle.session_id.clone(), handle.clone())
            .await;

        let request = handle_sdk_message(
            &sessions,
            &handle,
            SDKMessage::User {
                uuid: "u1".to_string(),
                message: MessageContent {
                    content: ContentBlock::Text("needs permission".to_string()),
                },
            },
            &runner,
        )
        .await
        .map(single_message)
        .unwrap();
        assert!(matches!(request, SDKMessage::ControlRequest { .. }));

        let responses = handle_sdk_message(
            &sessions,
            &handle,
            SDKMessage::ControlResponse {
                response: ControlResponseType::Success {
                    request_id: "perm-4".to_string(),
                    response: Some(HashMap::from([("allowed".to_string(), json!(true))])),
                },
            },
            &runner,
        )
        .await
        .unwrap();
        assert_eq!(responses.len(), 2);
        match &responses[0] {
            SDKMessage::ControlCancelRequest {
                request_id,
                tool_use_id,
            } => {
                assert_eq!(request_id, "perm-4");
                assert!(tool_use_id.is_none());
            }
            other => panic!("expected control cancel request, got {:?}", other),
        }
        match &responses[1] {
            SDKMessage::Assistant { message, .. } => {
                assert_eq!(content_summary(message), "after remote permission");
            }
            other => panic!("expected assistant message, got {:?}", other),
        }
        let status = sessions.status(&handle.session_id).await.unwrap();
        assert!(status.activities.iter().any(|activity| {
            activity.activity_type == "permission_cancelled"
                && activity.summary == "Permission request cancelled: perm-4"
        }));

        runner.end_session(&handle.session_id).await.unwrap();
        let _ = fs::remove_dir_all(root);
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn handle_sdk_message_forwards_server_control_request_to_stream_runner() {
        let root =
            std::env::temp_dir().join(format!("kiana-bridge-server-control-{}", Uuid::new_v4()));
        fs::create_dir_all(&root).unwrap();
        let script_path = root.join("server-control-runner.sh");
        fs::write(
            &script_path,
            r#"IFS= read -r control_line
case "$control_line" in
  *set_permission_mode*acceptEdits*) printf '{"type":"control_response","response":{"subtype":"success","request_id":"req-mode","response":{"mode":"acceptEdits"}}}\n' ;;
  *) printf '{"type":"control_response","response":{"subtype":"error","request_id":"req-mode","error":"missing mode"}}\n' ;;
esac
IFS= read -r user_line
case "$user_line" in
  *'"type":"user"'*) printf '{"type":"assistant","uuid":"assistant","message":{"content":"mode applied"}}\n' ;;
  *) printf '{"type":"assistant","uuid":"assistant","message":{"content":"missing user"}}\n' ;;
esac
"#,
        )
        .unwrap();
        let runner =
            CommandBridgeSessionRunner::new("/bin/sh", [script_path.to_string_lossy().to_string()])
                .with_stream_json_mode()
                .with_timeout_ms(1_000);
        let sessions = SessionManager::new();
        let handle = session_handle();
        sessions
            .add_session(handle.session_id.clone(), handle.clone())
            .await;

        let control_response = handle_sdk_message(
            &sessions,
            &handle,
            SDKMessage::ControlRequest {
                request_id: "req-mode".to_string(),
                request: ControlRequestType::SetPermissionMode {
                    mode: Some("acceptEdits".to_string()),
                },
            },
            &runner,
        )
        .await
        .map(single_message)
        .unwrap();
        match control_response {
            SDKMessage::ControlResponse {
                response:
                    ControlResponseType::Success {
                        request_id,
                        response,
                    },
            } => {
                assert_eq!(request_id, "req-mode");
                assert_eq!(response.unwrap()["mode"], "acceptEdits");
            }
            other => panic!("expected forwarded control success, got {:?}", other),
        }

        let response = handle_sdk_message(
            &sessions,
            &handle,
            SDKMessage::User {
                uuid: "u1".to_string(),
                message: MessageContent {
                    content: ContentBlock::Text("continue".to_string()),
                },
            },
            &runner,
        )
        .await
        .map(single_message)
        .unwrap();
        match response {
            SDKMessage::Assistant { message, .. } => {
                assert_eq!(content_summary(&message), "mode applied");
            }
            other => panic!("expected assistant message, got {:?}", other),
        }

        let status = sessions.status(&handle.session_id).await.unwrap();
        assert!(status.activities.iter().any(|activity| {
            activity.activity_type == "control"
                && activity.summary == "control request set_permission_mode -> success"
        }));

        runner.end_session(&handle.session_id).await.unwrap();
        let _ = fs::remove_dir_all(root);
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn stream_json_runner_records_non_response_output_activities() {
        let root = std::env::temp_dir().join(format!("kiana-bridge-activity-{}", Uuid::new_v4()));
        fs::create_dir_all(&root).unwrap();
        let script_path = root.join("activity-runner.sh");
        fs::write(
            &script_path,
            r#"IFS= read -r user_line
printf '{"type":"system","subtype":"init","session_id":"remote-session"}\n'
printf '{"type":"stream_event","event":{"type":"content_block_delta","delta":{"text":"partial update"}}}\n'
printf '{"type":"result","subtype":"success","is_error":false,"result":"done"}\n'
printf '{"type":"assistant","uuid":"assistant","message":{"content":[{"type":"text","text":"final text"}]}}\n'
"#,
        )
        .unwrap();
        let runner =
            CommandBridgeSessionRunner::new("/bin/sh", [script_path.to_string_lossy().to_string()])
                .with_stream_json_mode()
                .with_timeout_ms(1_000);
        let sessions = SessionManager::new();
        let handle = session_handle();
        sessions
            .add_session(handle.session_id.clone(), handle.clone())
            .await;

        let responses = handle_sdk_message(
            &sessions,
            &handle,
            SDKMessage::User {
                uuid: "u1".to_string(),
                message: MessageContent {
                    content: ContentBlock::Text("start".to_string()),
                },
            },
            &runner,
        )
        .await
        .unwrap();
        assert_eq!(responses.len(), 4);
        assert!(matches!(responses[0], SDKMessage::System { .. }));
        assert!(matches!(responses[1], SDKMessage::StreamEvent { .. }));
        assert!(matches!(responses[2], SDKMessage::Assistant { .. }));
        assert!(matches!(responses[3], SDKMessage::Result { .. }));

        let status = sessions.status(&handle.session_id).await.unwrap();
        assert!(status
            .activities
            .iter()
            .any(|activity| activity.activity_type == "system"
                && activity.summary == "Session initialized"));
        assert!(status.activities.iter().any(
            |activity| activity.activity_type == "text" && activity.summary == "partial update"
        ));
        assert!(status
            .activities
            .iter()
            .any(|activity| activity.activity_type == "result"
                && activity.summary == "Session completed"));
        assert!(status
            .activities
            .iter()
            .any(|activity| activity.activity_type == "text" && activity.summary == "final text"));

        runner.end_session(&handle.session_id).await.unwrap();
        let _ = fs::remove_dir_all(root);
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn stream_json_runner_writes_debug_and_transcript_files() {
        let root = std::env::temp_dir().join(format!("kiana-bridge-debug-{}", Uuid::new_v4()));
        fs::create_dir_all(&root).unwrap();
        let script_path = root.join("debug-runner.sh");
        fs::write(
            &script_path,
            r#"IFS= read -r user_line
printf '{"type":"assistant","uuid":"assistant","message":{"content":"debug ready"}}\n'
"#,
        )
        .unwrap();
        let debug_base = root.join("bridge.log");
        let runner =
            CommandBridgeSessionRunner::new("/bin/sh", [script_path.to_string_lossy().to_string()])
                .with_stream_json_mode()
                .with_debug_file(&debug_base)
                .with_timeout_ms(1_000);
        let handle = session_handle();

        let response = runner
            .handle_user_message(
                &handle,
                "u1".to_string(),
                MessageContent {
                    content: ContentBlock::Text("start".to_string()),
                },
            )
            .await
            .unwrap();
        match response {
            SDKMessage::Assistant { message, .. } => {
                assert_eq!(content_summary(&message), "debug ready");
            }
            other => panic!("expected assistant message, got {:?}", other),
        }

        let debug_file = root.join("bridge-remote-session.log");
        let transcript_file = root.join("bridge-transcript-remote-session.jsonl");
        let debug_log = fs::read_to_string(&debug_file).unwrap();
        let transcript = fs::read_to_string(&transcript_file).unwrap();
        assert!(debug_log.contains("sessionId=remote-session"));
        assert!(debug_log.contains("[bridge:ws] sessionId=remote-session >>>"));
        assert!(debug_log.contains("[bridge:ws] sessionId=remote-session <<<"));
        assert!(debug_log.contains("Transcript log:"));
        assert!(transcript.contains(
            r#"{"type":"assistant","uuid":"assistant","message":{"content":"debug ready"}}"#
        ));

        runner.end_session(&handle.session_id).await.unwrap();
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn sdk_message_activities_record_result_errors() {
        let message = SDKMessage::Result {
            data: HashMap::from([
                ("subtype".to_string(), json!("error_max_turns")),
                ("is_error".to_string(), json!(true)),
                ("error".to_string(), json!("turn limit reached")),
            ]),
        };

        let activities = sdk_message_activities(&message);

        assert_eq!(activities.len(), 1);
        assert_eq!(activities[0].activity_type, "error");
        assert_eq!(activities[0].summary, "turn limit reached");
    }

    struct ToolActivityRunner;

    #[async_trait]
    impl BridgeSessionRunner for ToolActivityRunner {
        async fn handle_user_message(
            &self,
            _handle: &SessionHandle,
            _uuid: String,
            _message: MessageContent,
        ) -> Result<SDKMessage> {
            Ok(SDKMessage::Assistant {
                uuid: "assistant".to_string(),
                message: MessageContent {
                    content: ContentBlock::Blocks(vec![
                        serde_json::from_value(json!({
                            "type": "tool_use",
                            "name": "Bash",
                            "input": { "command": "cargo test -p kiana-bridge --all-targets" }
                        }))
                        .unwrap(),
                        serde_json::from_value(json!({
                            "type": "text",
                            "text": "checking bridge tests"
                        }))
                        .unwrap(),
                    ]),
                },
            })
        }
    }

    #[tokio::test]
    async fn handle_sdk_message_records_runner_tool_and_text_activities() {
        let sessions = SessionManager::new();
        let handle = session_handle();
        sessions
            .add_session(handle.session_id.clone(), handle.clone())
            .await;

        let response = handle_sdk_message(
            &sessions,
            &handle,
            SDKMessage::User {
                uuid: "u1".to_string(),
                message: MessageContent {
                    content: ContentBlock::Text("run tests".to_string()),
                },
            },
            &ToolActivityRunner,
        )
        .await
        .map(single_message)
        .unwrap();
        assert!(matches!(response, SDKMessage::Assistant { .. }));

        let status = sessions.status(&handle.session_id).await.unwrap();
        assert!(status
            .activities
            .iter()
            .any(|activity| activity.activity_type == "tool_start"
                && activity.summary == "Running cargo test -p kiana-bridge --all-targets"));
        assert!(status
            .activities
            .iter()
            .any(|activity| activity.activity_type == "text"
                && activity.summary == "checking bridge tests"));
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn command_runner_times_out_and_kills_hung_child() {
        let runner = CommandBridgeSessionRunner::new(
            "/bin/sh",
            ["-c", "echo before-timeout >&2; sleep 5", "bridge-test"],
        )
        .with_timeout_ms(500);

        let error = runner
            .handle_user_message(
                &session_handle(),
                "u1".to_string(),
                MessageContent {
                    content: ContentBlock::Text("hello".to_string()),
                },
            )
            .await
            .unwrap_err()
            .to_string();

        assert!(error.contains("timed out after"));
        assert!(error.contains("before-timeout"));
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn command_runner_executes_inside_configured_directory() {
        let root = std::env::temp_dir().join(format!("kiana-bridge-{}", Uuid::new_v4()));
        fs::create_dir_all(&root).unwrap();
        let runner = CommandBridgeSessionRunner::new("/bin/pwd", Vec::<String>::new())
            .with_cwd(root.clone());

        let response = runner
            .handle_user_message(
                &session_handle(),
                "u1".to_string(),
                MessageContent {
                    content: ContentBlock::Text("ignored".to_string()),
                },
            )
            .await
            .unwrap();

        match response {
            SDKMessage::Assistant { message, .. } => {
                assert_eq!(content_summary(&message), root.to_string_lossy());
            }
            other => panic!("expected assistant message, got {:?}", other),
        }
        let _ = fs::remove_dir_all(root);
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn command_runner_prefers_session_work_dir_over_configured_directory() {
        let root = std::env::temp_dir().join(format!("kiana-bridge-root-{}", Uuid::new_v4()));
        let session_root =
            std::env::temp_dir().join(format!("kiana-bridge-session-{}", Uuid::new_v4()));
        fs::create_dir_all(&root).unwrap();
        fs::create_dir_all(&session_root).unwrap();
        let runner = CommandBridgeSessionRunner::new("/bin/pwd", Vec::<String>::new())
            .with_cwd(root.clone());
        let handle = SessionHandle {
            session_id: "remote-session".to_string(),
            access_token: "token".to_string(),
            sdk_url: Some("wss://example.test/v1/session_ingress/ws/remote-session".to_string()),
            work_dir: Some(session_root.clone()),
            use_ccr_v2: false,
            worker_epoch: None,
        };

        let response = runner
            .handle_user_message(
                &handle,
                "u1".to_string(),
                MessageContent {
                    content: ContentBlock::Text("ignored".to_string()),
                },
            )
            .await
            .unwrap();

        match response {
            SDKMessage::Assistant { message, .. } => {
                assert_eq!(content_summary(&message), session_root.to_string_lossy());
            }
            other => panic!("expected assistant message, got {:?}", other),
        }
        let _ = fs::remove_dir_all(root);
        let _ = fs::remove_dir_all(session_root);
    }

    #[test]
    fn worktree_spawn_mode_creates_snapshot_when_git_worktree_is_unavailable() {
        let root = std::env::temp_dir().join(format!("kiana-bridge-worktree-{}", Uuid::new_v4()));
        fs::create_dir_all(&root).unwrap();
        fs::write(root.join("repo.txt"), "snapshot").unwrap();
        let mut config = bridge_config(root.to_string_lossy().to_string());
        config.spawn_mode = crate::types::SpawnMode::Worktree;

        let work_dir = prepare_bridge_session_work_dir(&config, "remote/session..1").unwrap();

        assert_ne!(work_dir, root);
        assert!(work_dir.ends_with(".kiana/bridge-worktrees/remote_session__1"));
        assert_eq!(
            fs::read_to_string(work_dir.join("repo.txt")).unwrap(),
            "snapshot"
        );
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn cleanup_removes_managed_worktree_snapshot_only_in_worktree_mode() {
        let root = std::env::temp_dir().join(format!("kiana-bridge-cleanup-{}", Uuid::new_v4()));
        fs::create_dir_all(&root).unwrap();
        fs::write(root.join("repo.txt"), "snapshot").unwrap();
        let mut config = bridge_config(root.to_string_lossy().to_string());
        config.spawn_mode = crate::types::SpawnMode::Worktree;
        let work_dir = prepare_bridge_session_work_dir(&config, "remote-session").unwrap();
        assert!(work_dir.is_dir());

        cleanup_bridge_session_work_dir(&config, Some(&work_dir)).unwrap();

        assert!(!work_dir.exists());
        assert!(root.is_dir());
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn cleanup_does_not_remove_same_dir_sessions() {
        let root = std::env::temp_dir().join(format!("kiana-bridge-samedir-{}", Uuid::new_v4()));
        fs::create_dir_all(&root).unwrap();
        let config = bridge_config(root.to_string_lossy().to_string());

        cleanup_bridge_session_work_dir(&config, Some(&root)).unwrap();

        assert!(root.is_dir());
        let _ = fs::remove_dir_all(root);
    }

    #[tokio::test]
    async fn bridge_work_capacity_honors_max_sessions_and_clamps_zero() {
        let sessions = SessionManager::new();
        assert!(has_bridge_work_capacity(&sessions, 0).await);

        let handle = SessionHandle {
            session_id: "s1".to_string(),
            access_token: "token".to_string(),
            sdk_url: Some("wss://example.test/v1/session_ingress/ws/s1".to_string()),
            work_dir: None,
            use_ccr_v2: false,
            worker_epoch: None,
        };
        sessions.add_session("s1".to_string(), handle).await;

        assert!(!has_bridge_work_capacity(&sessions, 0).await);
        assert!(!has_bridge_work_capacity(&sessions, 1).await);
        assert!(has_bridge_work_capacity(&sessions, 2).await);
    }

    #[test]
    fn bridge_heartbeat_interval_uses_config_and_allows_disable() {
        let mut config = bridge_config("/tmp/kiana-project".to_string());
        config.heartbeat_interval_ms = 25;
        assert_eq!(
            bridge_heartbeat_interval(&config),
            Some(Duration::from_millis(100))
        );
        config.heartbeat_interval_ms = 250;
        assert_eq!(
            bridge_heartbeat_interval(&config),
            Some(Duration::from_millis(250))
        );
        config.heartbeat_interval_ms = 0;
        assert_eq!(bridge_heartbeat_interval(&config), None);
    }

    #[test]
    fn bridge_session_timeout_uses_config_and_allows_disable() {
        let mut config = bridge_config("/tmp/kiana-project".to_string());
        config.session_timeout_ms = 250;
        assert_eq!(
            bridge_session_timeout(&config),
            Some(Duration::from_millis(250))
        );
        config.session_timeout_ms = 0;
        assert_eq!(bridge_session_timeout(&config), None);
    }

    #[test]
    fn ccr_v2_sse_liveness_timeout_uses_config_and_allows_disable() {
        let mut config = bridge_config("/tmp/kiana-project".to_string());
        config.ccr_v2_sse_liveness_timeout_ms = None;
        assert_eq!(
            ccr_v2_sse_liveness_timeout(&config),
            Some(Duration::from_secs(45))
        );
        config.ccr_v2_sse_liveness_timeout_ms = Some(250);
        assert_eq!(
            ccr_v2_sse_liveness_timeout(&config),
            Some(Duration::from_millis(250))
        );
        config.ccr_v2_sse_liveness_timeout_ms = Some(0);
        assert_eq!(ccr_v2_sse_liveness_timeout(&config), None);
    }

    #[tokio::test]
    async fn initialize_control_request_gets_success_response() {
        let sessions = SessionManager::new();
        let handle = session_handle();
        let runner = RecordingBridgeSessionRunner;
        sessions
            .add_session(handle.session_id.clone(), handle.clone())
            .await;

        let response = handle_sdk_message(
            &sessions,
            &handle,
            SDKMessage::ControlRequest {
                request_id: "req-1".to_string(),
                request: ControlRequestType::Initialize,
            },
            &runner,
        )
        .await
        .map(single_message)
        .unwrap();

        match response {
            SDKMessage::ControlResponse {
                response:
                    ControlResponseType::Success {
                        request_id,
                        response,
                    },
            } => {
                assert_eq!(request_id, "req-1");
                let response = response.unwrap();
                assert_eq!(response["status"], json!("ready"));
                assert_eq!(response["session_id"], json!("remote-session"));
            }
            other => panic!("expected control success, got {:?}", other),
        }
    }

    #[test]
    fn control_request_type_parses_reference_server_subtypes_and_unknown() {
        let max_tokens: SDKMessage = serde_json::from_str(
            r#"{"type":"control_request","request_id":"req-2","request":{"subtype":"set_max_thinking_tokens","max_thinking_tokens":4096}}"#,
        )
        .unwrap();
        match max_tokens {
            SDKMessage::ControlRequest {
                request:
                    ControlRequestType::SetMaxThinkingTokens {
                        max_thinking_tokens,
                    },
                ..
            } => assert_eq!(max_thinking_tokens, Some(4096)),
            other => panic!("expected set_max_thinking_tokens, got {:?}", other),
        }

        let permission_mode: SDKMessage = serde_json::from_str(
            r#"{"type":"control_request","request_id":"req-3","request":{"subtype":"set_permission_mode","mode":"acceptEdits"}}"#,
        )
        .unwrap();
        match permission_mode {
            SDKMessage::ControlRequest {
                request: ControlRequestType::SetPermissionMode { mode },
                ..
            } => assert_eq!(mode.as_deref(), Some("acceptEdits")),
            other => panic!("expected set_permission_mode, got {:?}", other),
        }

        let unknown: SDKMessage = serde_json::from_str(
            r#"{"type":"control_request","request_id":"req-4","request":{"subtype":"future_control","value":true}}"#,
        )
        .unwrap();
        match unknown {
            SDKMessage::ControlRequest {
                request: ControlRequestType::Unknown,
                ..
            } => {}
            other => panic!("expected unknown control request, got {:?}", other),
        }

        let mcp_status: SDKMessage = serde_json::from_str(
            r#"{"type":"control_request","request_id":"req-5","request":{"subtype":"mcp_status"}}"#,
        )
        .unwrap();
        match mcp_status {
            SDKMessage::ControlRequest {
                request: ControlRequestType::McpStatus,
                ..
            } => {}
            other => panic!("expected mcp_status control request, got {:?}", other),
        }

        let cancel: SDKMessage =
            serde_json::from_str(r#"{"type":"control_cancel_request","request_id":"perm-1"}"#)
                .unwrap();
        match cancel {
            SDKMessage::ControlCancelRequest {
                request_id,
                tool_use_id,
            } => {
                assert_eq!(request_id, "perm-1");
                assert!(tool_use_id.is_none());
            }
            other => panic!("expected control cancel request, got {:?}", other),
        }
    }

    #[tokio::test]
    async fn mutable_server_control_requests_get_prompt_responses() {
        let sessions = SessionManager::new();
        let handle = session_handle();
        let runner = RecordingBridgeSessionRunner;
        sessions
            .add_session(handle.session_id.clone(), handle.clone())
            .await;

        let max_tokens_response = handle_sdk_message(
            &sessions,
            &handle,
            SDKMessage::ControlRequest {
                request_id: "req-max".to_string(),
                request: ControlRequestType::SetMaxThinkingTokens {
                    max_thinking_tokens: Some(2048),
                },
            },
            &runner,
        )
        .await
        .map(single_message)
        .unwrap();
        match max_tokens_response {
            SDKMessage::ControlResponse {
                response:
                    ControlResponseType::Success {
                        request_id,
                        response,
                    },
            } => {
                assert_eq!(request_id, "req-max");
                assert_eq!(response.unwrap()["max_thinking_tokens"], json!(2048));
            }
            other => panic!("expected max token success, got {:?}", other),
        }

        let mcp_status_response = handle_sdk_message(
            &sessions,
            &handle,
            SDKMessage::ControlRequest {
                request_id: "req-mcp-status".to_string(),
                request: ControlRequestType::McpStatus,
            },
            &runner,
        )
        .await
        .map(single_message)
        .unwrap();
        match mcp_status_response {
            SDKMessage::ControlResponse {
                response:
                    ControlResponseType::Success {
                        request_id,
                        response,
                    },
            } => {
                assert_eq!(request_id, "req-mcp-status");
                assert_eq!(response.unwrap()["mcpServers"], json!([]));
            }
            other => panic!("expected mcp status success, got {:?}", other),
        }

        let permission_response = handle_sdk_message(
            &sessions,
            &handle,
            SDKMessage::ControlRequest {
                request_id: "req-permission".to_string(),
                request: ControlRequestType::SetPermissionMode {
                    mode: Some("acceptEdits".to_string()),
                },
            },
            &runner,
        )
        .await
        .map(single_message)
        .unwrap();
        match permission_response {
            SDKMessage::ControlResponse {
                response: ControlResponseType::Error { request_id, error },
            } => {
                assert_eq!(request_id, "req-permission");
                assert!(error.contains("set_permission_mode is not supported"));
            }
            other => panic!("expected permission mode error, got {:?}", other),
        }

        let unknown_response = handle_sdk_message(
            &sessions,
            &handle,
            SDKMessage::ControlRequest {
                request_id: "req-unknown".to_string(),
                request: ControlRequestType::Unknown,
            },
            &runner,
        )
        .await
        .map(single_message)
        .unwrap();
        match unknown_response {
            SDKMessage::ControlResponse {
                response: ControlResponseType::Error { request_id, error },
            } => {
                assert_eq!(request_id, "req-unknown");
                assert!(error.contains("does not handle control_request subtype"));
            }
            other => panic!("expected unknown request error, got {:?}", other),
        }

        let status = sessions.status(&handle.session_id).await.unwrap();
        assert!(status.activities.iter().any(|activity| {
            activity.activity_type == "control"
                && activity.summary == "control request set_permission_mode -> error"
        }));
    }

    async fn spawn_mock_server(
        responses: Vec<MockResponse>,
    ) -> (String, Arc<Mutex<Vec<RecordedRequest>>>) {
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
                match read_http_request(&mut stream).await {
                    Ok(request) => shared_requests.lock().unwrap().push(request),
                    Err(_) => break,
                }
                let _ = write_http_response(&mut stream, response).await;
            }
        });

        (format!("http://{}", address), requests)
    }

    async fn spawn_ccr_v2_timer_mock_server(
        user_event: Value,
        stream_hold: Duration,
    ) -> (String, Arc<Mutex<Vec<RecordedRequest>>>) {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let requests = Arc::new(Mutex::new(Vec::new()));
        let shared_requests = requests.clone();
        let sse_body = Arc::new(format!(
            "id: 1\nevent: client_event\ndata: {}\n\n",
            user_event
        ));

        tokio::spawn(async move {
            loop {
                let Ok((mut stream, _)) = listener.accept().await else {
                    break;
                };
                let shared_requests = shared_requests.clone();
                let sse_body = sse_body.clone();
                tokio::spawn(async move {
                    let Ok(request) = read_http_request(&mut stream).await else {
                        return;
                    };
                    let path = request.path.clone();
                    shared_requests.lock().unwrap().push(request);

                    if path == "/v1/code/sessions/cse_session_1/worker/register" {
                        let _ = write_http_response(
                            &mut stream,
                            MockResponse::json(200, r#"{"worker_epoch":"42"}"#),
                        )
                        .await;
                    } else if path == "/v1/code/sessions/cse_session_1/worker/events/stream" {
                        if write_sse_response(&mut stream, &sse_body).await.is_ok() {
                            sleep(stream_hold).await;
                        }
                    } else {
                        let _ = write_http_response(&mut stream, MockResponse::json(200, r#"{}"#))
                            .await;
                    }
                });
            }
        });

        (format!("http://{}", address), requests)
    }

    #[cfg(unix)]
    async fn spawn_bridge_api_smoke_server(
        work_body: String,
    ) -> (String, Arc<Mutex<Vec<RecordedRequest>>>) {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let requests = Arc::new(Mutex::new(Vec::new()));
        let shared_requests = requests.clone();
        let poll_count = Arc::new(Mutex::new(0_usize));
        let shared_poll_count = poll_count.clone();

        tokio::spawn(async move {
            loop {
                let Ok((mut stream, _)) = listener.accept().await else {
                    break;
                };
                let Ok(request) = read_http_request(&mut stream).await else {
                    break;
                };
                let response = if request.method == "GET"
                    && request.path == "/v1/environments/env-1/work/poll"
                {
                    let mut poll_count = shared_poll_count.lock().unwrap();
                    *poll_count += 1;
                    if *poll_count == 1 {
                        MockResponse::json(200, &work_body)
                    } else {
                        MockResponse::json(204, "")
                    }
                } else {
                    MockResponse::json(200, r#"{}"#)
                };
                shared_requests.lock().unwrap().push(request);
                let _ = write_http_response(&mut stream, response).await;
            }
        });

        (format!("http://{}", address), requests)
    }

    #[cfg(unix)]
    async fn spawn_bridge_session_ws_smoke_server(
    ) -> (String, tokio::sync::oneshot::Receiver<Vec<SDKMessage>>) {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let (done_tx, done_rx) = tokio::sync::oneshot::channel();

        tokio::spawn(async move {
            let mut received = Vec::new();
            let Ok((stream, _)) = listener.accept().await else {
                let _ = done_tx.send(received);
                return;
            };
            let Ok(mut websocket) = tokio_tungstenite::accept_async(stream).await else {
                let _ = done_tx.send(received);
                return;
            };
            let user = SDKMessage::User {
                uuid: "u-remote".to_string(),
                message: MessageContent {
                    content: ContentBlock::Text("remote run".to_string()),
                },
            };
            let _ = websocket
                .send(tokio_tungstenite::tungstenite::Message::Text(
                    serde_json::to_string(&user).unwrap(),
                ))
                .await;

            while let Some(message) = websocket.next().await {
                let Ok(tokio_tungstenite::tungstenite::Message::Text(text)) = message else {
                    continue;
                };
                let Ok(message) = serde_json::from_str::<SDKMessage>(&text) else {
                    continue;
                };
                let done = matches!(
                    &message,
                    SDKMessage::Assistant { message, .. }
                        if content_summary(message) == "bridge child done"
                );
                if let SDKMessage::ControlRequest {
                    request_id,
                    request,
                } = &message
                {
                    if matches!(
                        request,
                        ControlRequestType::CanUseTool {
                            tool_name,
                            tool_use_id,
                            ..
                        } if tool_name == "Bash" && tool_use_id == "toolu_e2e"
                    ) {
                        let response = SDKMessage::ControlResponse {
                            response: ControlResponseType::Success {
                                request_id: request_id.clone(),
                                response: Some(HashMap::from([(
                                    "allowed".to_string(),
                                    json!(true),
                                )])),
                            },
                        };
                        let _ = websocket
                            .send(tokio_tungstenite::tungstenite::Message::Text(
                                serde_json::to_string(&response).unwrap(),
                            ))
                            .await;
                    }
                }
                received.push(message);
                if done {
                    break;
                }
            }

            let _ = websocket.close(None).await;
            let _ = done_tx.send(received);
        });

        (format!("ws://{}", address), done_rx)
    }

    async fn spawn_idle_ws_server() -> String {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        tokio::spawn(async move {
            let Ok((stream, _)) = listener.accept().await else {
                return;
            };
            let Ok(mut websocket) = tokio_tungstenite::accept_async(stream).await else {
                return;
            };
            while websocket.next().await.is_some() {}
        });
        format!("ws://{}", address)
    }

    fn encoded_work_secret(session_token: &str) -> String {
        let secret = json!({
            "version": 1,
            "session_ingress_token": session_token,
            "api_base_url": "https://example.test",
            "use_code_sessions": false
        })
        .to_string();
        base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(secret)
    }

    fn encoded_code_session_work_secret(session_token: &str, api_base_url: &str) -> String {
        let secret = json!({
            "version": 1,
            "session_ingress_token": session_token,
            "api_base_url": api_base_url,
            "use_code_sessions": true
        })
        .to_string();
        base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(secret)
    }

    fn work_response(
        work_id: &str,
        data_type: &str,
        session_id: &str,
        session_token: &str,
    ) -> WorkResponse {
        WorkResponse {
            id: work_id.to_string(),
            work_type: "bridge".to_string(),
            environment_id: "env-1".to_string(),
            state: "queued".to_string(),
            data: crate::types::WorkData {
                data_type: data_type.to_string(),
                id: session_id.to_string(),
            },
            secret: encoded_work_secret(session_token),
        }
    }

    fn work_response_json(
        work_id: &str,
        data_type: &str,
        session_id: &str,
        session_token: &str,
    ) -> String {
        serde_json::to_string(&work_response(
            work_id,
            data_type,
            session_id,
            session_token,
        ))
        .unwrap()
    }

    async fn wait_for_requests(requests: &Arc<Mutex<Vec<RecordedRequest>>>, expected: usize) {
        let deadline = Instant::now() + Duration::from_secs(2);
        loop {
            if requests.lock().unwrap().len() >= expected {
                return;
            }
            if Instant::now() >= deadline {
                panic!(
                    "timed out waiting for {expected} request(s), got {}",
                    requests.lock().unwrap().len()
                );
            }
            sleep(Duration::from_millis(10)).await;
        }
    }

    #[cfg(unix)]
    async fn wait_for_request_path(requests: &Arc<Mutex<Vec<RecordedRequest>>>, path: &str) {
        let deadline = Instant::now() + Duration::from_secs(3);
        loop {
            if requests
                .lock()
                .unwrap()
                .iter()
                .any(|request| request.path == path)
            {
                return;
            }
            if Instant::now() >= deadline {
                let paths = requests
                    .lock()
                    .unwrap()
                    .iter()
                    .map(|request| request.path.clone())
                    .collect::<Vec<_>>();
                panic!("timed out waiting for request path {path}, got {paths:?}");
            }
            sleep(Duration::from_millis(10)).await;
        }
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
        let mut content_length = 0_usize;

        for line in lines {
            let Some((name, value)) = line.split_once(':') else {
                continue;
            };
            if name.eq_ignore_ascii_case("authorization") {
                authorization = Some(value.trim().to_string());
            } else if name.eq_ignore_ascii_case("content-length") {
                content_length = value.trim().parse().unwrap_or_default();
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
            body,
        })
    }

    fn find_header_end(buffer: &[u8]) -> Option<usize> {
        buffer.windows(4).position(|window| window == b"\r\n\r\n")
    }

    async fn write_sse_response(stream: &mut TcpStream, body: &str) -> std::io::Result<()> {
        stream
            .write_all(
                b"HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nCache-Control: no-cache\r\nConnection: keep-alive\r\n\r\n",
            )
            .await?;
        stream.write_all(body.as_bytes()).await?;
        stream.flush().await
    }

    async fn write_http_response(
        stream: &mut TcpStream,
        response: MockResponse,
    ) -> std::io::Result<()> {
        let status_text = match response.status {
            200 => "OK",
            204 => "No Content",
            401 => "Unauthorized",
            404 => "Not Found",
            409 => "Conflict",
            500 => "Internal Server Error",
            _ => "Status",
        };
        let payload = response.body.as_bytes();
        stream
            .write_all(
                format!(
                    "HTTP/1.1 {} {}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                    response.status,
                    status_text,
                    payload.len()
                )
                .as_bytes(),
            )
            .await?;
        stream.write_all(payload).await?;
        stream.flush().await
    }

    impl MockResponse {
        fn json(status: u16, body: &str) -> Self {
            Self {
                status,
                body: body.to_string(),
            }
        }
    }

    fn take_env(name: &str) -> Option<OsString> {
        let value = std::env::var_os(name);
        std::env::remove_var(name);
        value
    }

    fn restore_env(name: &str, value: Option<OsString>) {
        if let Some(value) = value {
            std::env::set_var(name, value);
        } else {
            std::env::remove_var(name);
        }
    }
}

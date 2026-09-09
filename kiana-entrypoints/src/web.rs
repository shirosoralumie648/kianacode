//! 基于 loopback HTTP 的 Web Workbench 入口。
//!
//! Web 服务器只提供同一个 [`DaemonHost`] 的本地展示与命令路由：每个 session 的 run、
//! continue、cancel 和 receipt 均复用 `harness_run`。它不自行执行工具、不创建额外模型
//! 循环，也不把浏览器内存状态升级为执行事实；正式授权、信任、审批和收据仍在产品脊柱中。
//!
//! 服务强制绑定 loopback，并对每次 API 状态/变更请求检查进程启动时随机生成的 token、
//! Host 以及可选 Origin；`/api/events` 额外提供只读 SSE 增量投影。delta 只用于展示，
//! 终态和事实仍以响应/Receipt 为准。这些防护用于本地 UI 暴露面，不能替代系统级网络、
//! 浏览器或项目资源信任边界。`/api/state` 还会合并由同一 `DaemonHost` 的
//! `EventStorePort` 事件账本投影出的只读历史会话；历史会话不能进入
//! run/continue/cancel 等变更路径。

use anyhow::{anyhow, Context, Result};
use axum::extract::{DefaultBodyLimit, Query, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::sse::{Event, KeepAlive, Sse};
use axum::response::{Html, IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use futures_util::stream;
use futures_util::Stream;
use kiana_daemon::{DaemonHost, StreamingRedactor};
use kiana_protocol::{
    ResponseEnvelope, RoleSpec, RunId, RunStreamEnvelope, RunStreamEvent, ROLE_BUILDER,
};
use kiana_types::{write_project_trust, ProjectTrust};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::HashMap;
use std::convert::Infallible;
use std::io::{self, Write};
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr};
use std::path::{Path, PathBuf};
use std::pin::Pin;
use std::process::Command;
use std::sync::{Arc, Mutex};
use tokio::net::TcpListener;

use crate::harness_run;
use crate::web_thread::{self, ThreadView, TurnView};
use crate::web_ui::PAGE;
use crate::workbench_chat;

/// Web 子命令的人工可读用法文本。
///
/// 实际 bind、角色和沙箱合法性由解析函数及控制平面共同验证，帮助文本本身不产生配置。
pub const WEB_USAGE: &str = "\
Usage: kiana web [--workdir DIR] [--bind 127.0.0.1:3080] [--sandbox read-only|workspace-write] [--role builder] [--no-open]

Local-only Web workbench on DaemonHost (dsh-shaped: sidebar / chat / details).
Loopback bind only. SSE streams display deltas; receipts remain authoritative. kiana tui stays parked.
";

const DEFAULT_BIND: &str = "127.0.0.1:3080";
const MAX_WEB_BODY_BYTES: usize = 128 * 1024;
const MAX_WEB_PROMPT_BYTES: usize = 64 * 1024;
const MAX_WEB_SESSIONS: usize = 128;
const MAX_WEB_TURNS_PER_SESSION: usize = 256;
const MAX_WEB_TRANSCRIPT_BYTES_PER_SESSION: usize = 1024 * 1024;
const MAX_WEB_ITEM_BODY_BYTES: usize = 16 * 1024;
const MAX_WEB_ITEMS_PER_TURN: usize = 128;
const MAX_WEB_SUMMARY_TEXT_BYTES: usize = 16 * 1024;
const MAX_WEB_FILES_PER_TURN: usize = 256;
const MAX_WEB_FILE_PATH_BYTES: usize = 1024;
const STREAM_GAP_RUN_IN_PROGRESS: &str = "subscription_attached_after_run_started";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WebLaunch {
    /// 可选工作目录；缺失时使用调用进程当前目录。
    pub workdir: Option<PathBuf>,
    /// 将作为统一 harness 选项传递的已请求沙箱档位。
    pub sandbox: String,
    /// 可选角色标识；启动前会检查该角色是否存在。
    pub role: Option<String>,
    /// 用户提供的监听地址文字，后续必须解析为 loopback socket。
    pub bind: String,
    /// 是否禁止启动后尝试打开浏览器。
    pub no_open: bool,
    /// 是否只输出帮助并返回。
    pub help: bool,
}

impl Default for WebLaunch {
    fn default() -> Self {
        Self {
            workdir: None,
            sandbox: "workspace-write".to_owned(),
            role: None,
            bind: DEFAULT_BIND.to_owned(),
            no_open: false,
            help: false,
        }
    }
}

#[derive(Clone)]
struct WebApp {
    host: Arc<DaemonHost>,
    workdir: PathBuf,
    sandbox: Arc<Mutex<String>>,
    role: Arc<Mutex<String>>,
    sessions: Arc<Mutex<HashMap<String, WebSession>>>,
    active: Arc<Mutex<String>>,
    web_token: String,
    bound_addr: SocketAddr,
}

#[derive(Clone, Debug)]
struct WebSession {
    run_id: Option<RunId>,
    running: bool,
    name: String,
    last: Option<TurnSummary>,
    turns: Vec<TurnView>,
}

#[derive(Clone, Debug, Serialize)]
struct TurnSummary {
    status: String,
    error: Option<String>,
    text: String,
    files_changed: Vec<String>,
    run_id: Option<String>,
}

#[derive(Clone, Debug)]
struct LedgerSession {
    id: String,
    run_id: String,
    role_id: String,
    department_id: String,
    project_root: String,
    name: String,
    last_event_sequence: u64,
    last_event_kind: String,
    terminal_status: String,
    order: usize,
}

#[derive(Clone, Debug)]
struct LedgerThread {
    session: LedgerSession,
    turns: Vec<TurnView>,
    last: Option<TurnSummary>,
}

#[derive(Clone, Debug, Deserialize)]
struct LedgerEvent {
    #[serde(default)]
    event_id: Option<String>,
    #[serde(default)]
    request_id: Option<String>,
    sequence: u64,
    kind: String,
    #[serde(default)]
    data: Value,
}

#[derive(Deserialize)]
struct RunBody {
    prompt: String,
    #[serde(default)]
    session_id: Option<String>,
}

#[derive(Deserialize)]
struct SessionBody {
    #[serde(default)]
    session_id: Option<String>,
}

#[derive(Deserialize)]
struct EventsQuery {
    #[serde(default)]
    session_id: Option<String>,
    #[serde(default)]
    token: Option<String>,
}

#[derive(Deserialize)]
struct SandboxBody {
    sandbox: String,
    #[serde(default)]
    session_id: Option<String>,
}

#[derive(Debug)]
struct ApiError {
    status: StatusCode,
    error: String,
}

impl ApiError {
    fn bad(error: impl Into<String>) -> Self {
        Self {
            status: StatusCode::BAD_REQUEST,
            error: error.into(),
        }
    }

    fn unauthorized() -> Self {
        Self {
            status: StatusCode::UNAUTHORIZED,
            error: "web_auth_required".to_owned(),
        }
    }

    fn conflict(error: impl Into<String>) -> Self {
        Self {
            status: StatusCode::CONFLICT,
            error: error.into(),
        }
    }

    fn fail(error: impl Into<String>) -> Self {
        Self {
            status: StatusCode::INTERNAL_SERVER_ERROR,
            error: error.into(),
        }
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        (self.status, Json(json!({ "error": self.error }))).into_response()
    }
}

/// 解析 Web 参数并启动本地 Web Workbench。
pub async fn main_from_args(args: &[String]) -> Result<()> {
    let launch = parse_web_args(args)?;
    run_web(launch).await
}

/// 从 CLI 词元解析并进行本地语法级验证。
///
/// 解析阶段会规范化沙箱名并验证可选角色是否存在，未知选项立即失败。工作目录可用性、
/// 项目 trust、监听端口绑定和实际能力授权不在此函数中完成。
pub fn parse_web_args(args: &[String]) -> Result<WebLaunch> {
    let mut launch = WebLaunch::default();
    let mut index = 0;
    if args.first().map(String::as_str) == Some("web") {
        index = 1;
    }
    while index < args.len() {
        let argument = &args[index];
        match argument.as_str() {
            "help" | "--help" | "-h" => launch.help = true,
            "--no-open" => launch.no_open = true,
            "--bind" => {
                index += 1;
                launch.bind = args
                    .get(index)
                    .ok_or_else(|| anyhow!("bind_required"))?
                    .clone();
            }
            value if value.starts_with("--bind=") => {
                launch.bind = value.trim_start_matches("--bind=").to_owned();
            }
            "--workdir" => {
                index += 1;
                launch.workdir = Some(PathBuf::from(
                    args.get(index).ok_or_else(|| anyhow!("workdir_required"))?,
                ));
            }
            value if value.starts_with("--workdir=") => {
                launch.workdir = Some(PathBuf::from(value.trim_start_matches("--workdir=")));
            }
            "--sandbox" => {
                index += 1;
                launch.sandbox = args
                    .get(index)
                    .ok_or_else(|| anyhow!("run_sandbox_required"))?
                    .clone();
            }
            value if value.starts_with("--sandbox=") => {
                launch.sandbox = value.trim_start_matches("--sandbox=").to_owned();
            }
            "--role" => {
                index += 1;
                launch.role = Some(
                    args.get(index)
                        .ok_or_else(|| anyhow!("run_role_required"))?
                        .clone(),
                );
            }
            value if value.starts_with("--role=") => {
                launch.role = Some(value.trim_start_matches("--role=").to_owned());
            }
            value => return Err(anyhow!("unknown web option: {value}")),
        }
        index += 1;
    }
    launch.sandbox = workbench_chat::normalize_sandbox(&launch.sandbox)?;
    if let Some(role_id) = launch.role.as_deref() {
        RoleSpec::lookup(role_id).ok_or_else(|| anyhow!("role_unknown"))?;
    }
    Ok(launch)
}

/// 解析并强制校验只允许 loopback 的监听地址。
///
/// 接受完整 `SocketAddr` 或常用 host:port 形式，其中 `localhost`、`127.0.0.1` 与 `::1`
/// 会归一为对应 IP。任何非本地地址均返回 `bind_loopback_only`，防止默认 Web Workbench
/// 因参数误用暴露到局域网或公网。
pub fn parse_bind(value: &str) -> Result<SocketAddr> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return Err(anyhow!("bind_required"));
    }
    let addr = if let Ok(addr) = trimmed.parse::<SocketAddr>() {
        addr
    } else {
        let (host, port) = trimmed
            .rsplit_once(':')
            .ok_or_else(|| anyhow!("bind_invalid"))?;
        let port: u16 = port.parse().map_err(|_| anyhow!("bind_invalid"))?;
        let ip = match host {
            "localhost" | "127.0.0.1" => IpAddr::V4(Ipv4Addr::LOCALHOST),
            "::1" => IpAddr::V6(Ipv6Addr::LOCALHOST),
            other => other
                .parse::<IpAddr>()
                .map_err(|_| anyhow!("bind_invalid"))?,
        };
        SocketAddr::new(ip, port)
    };
    ensure_loopback(addr)?;
    Ok(addr)
}

fn ensure_loopback(addr: SocketAddr) -> Result<()> {
    if matches!(
        addr.ip(),
        IpAddr::V4(ip) if ip == Ipv4Addr::LOCALHOST
    ) || matches!(addr.ip(), IpAddr::V6(ip) if ip == Ipv6Addr::LOCALHOST)
    {
        Ok(())
    } else {
        Err(anyhow!("bind_loopback_only"))
    }
}

/// 绑定 loopback 监听器、创建 Web 状态并运行 Axum 服务。
///
/// 每次启动会生成内存中的随机 token，并将其嵌入首页供同一页面后续请求使用；token 不会
/// 被持久化，也不应被视为跨进程认证机制。实际会话工作仍委派给本地 `DaemonHost`，所以
/// 即使 Web 侧状态丢失，授权与事件事实也不应依赖它恢复。
pub async fn run_web(launch: WebLaunch) -> Result<()> {
    if launch.help {
        print!("{WEB_USAGE}");
        return Ok(());
    }
    let workdir = resolve_workdir(launch.workdir.as_deref())?;
    let role = launch.role.unwrap_or_else(|| ROLE_BUILDER.to_owned());
    let bind = parse_bind(&launch.bind)?;
    let listener = TcpListener::bind(bind)
        .await
        .with_context(|| format!("failed to bind {bind}"))?;
    let addr = listener
        .local_addr()
        .context("failed to read bind address")?;
    let host = Arc::new(DaemonHost::local().map_err(anyhow::Error::msg)?);
    let app = WebApp::new(host, workdir.clone(), launch.sandbox.clone(), role, addr);
    let url = format!("http://{addr}");
    let trusted = harness_run::project_trusted(&workdir.to_string_lossy())?;
    println!("Kiana web");
    println!("folder: {}", workdir.display());
    println!("trusted: {}", if trusted { "yes" } else { "no" });
    println!("sandbox: {}", launch.sandbox);
    println!("loopback only. SSE display stream; receipts remain authoritative.");
    println!("KIANA_WEB_URL={url}");
    println!("{url}");
    let _ = io::stdout().flush();
    if !launch.no_open {
        maybe_open(&url);
    }
    axum::serve(listener, router(app))
        .await
        .context("web server stopped")?;
    Ok(())
}

fn router(app: WebApp) -> Router {
    Router::new()
        .route("/", get(index))
        .route("/api/health", get(health))
        .route("/api/state", get(state))
        .route("/api/sessions", get(list_sessions))
        .route("/api/events", get(events))
        .route("/api/run", post(run_turn))
        .route("/api/cancel", post(cancel_turn))
        .route("/api/trust", post(trust_folder))
        .route("/api/sandbox", post(set_sandbox))
        .route("/api/session", post(new_session))
        .route("/api/receipt", post(read_receipt))
        .layer(DefaultBodyLimit::max(MAX_WEB_BODY_BYTES))
        .with_state(Arc::new(app))
}

impl WebApp {
    // 初始化一个活动 session 和进程内 token。该状态只服务于当前 Web 进程，不能作为
    // EventLog、审批记录或跨重启会话恢复的替代品。
    fn new(
        host: Arc<DaemonHost>,
        workdir: PathBuf,
        sandbox: String,
        role: String,
        bound_addr: SocketAddr,
    ) -> Self {
        let session_id = new_session_id();
        let mut sessions = HashMap::new();
        sessions.insert(session_id.clone(), WebSession::default());
        Self {
            host,
            workdir,
            sandbox: Arc::new(Mutex::new(sandbox)),
            role: Arc::new(Mutex::new(role)),
            sessions: Arc::new(Mutex::new(sessions)),
            active: Arc::new(Mutex::new(session_id)),
            web_token: uuid::Uuid::new_v4().to_string(),
            bound_addr,
        }
    }

    // 汇集 UI 所需的即时状态。Mutex 中的数据可能与 daemon 已持久化的状态不同步，
    // 因此只作为展示快照返回，不用于授权结论。历史会话只从事件账本投影，且永远
    // 标记为 read_only，不能通过该快照进入 run/continue/cancel 路径。
    async fn snapshot(&self, session_id: &str) -> Result<Value, ApiError> {
        let trusted = harness_run::project_trusted(&self.workdir.to_string_lossy())
            .map_err(|error| ApiError::fail(error.to_string()))?;
        let sandbox = lock_string(&self.sandbox)?;
        let role_id = lock_string(&self.role)?;
        let role = RoleSpec::lookup(&role_id).ok_or_else(|| ApiError::bad("role_unknown"))?;
        let (events, history) = self.read_session_ledger().await?;
        let (active_id, current, threads, memory_sessions) = {
            let sessions = self
                .sessions
                .lock()
                .map_err(|_| ApiError::fail("web_state_poisoned"))?;
            let active_id = lock_string(&self.active)?;
            let current = sessions.get(session_id).cloned();
            let mut threads = Vec::with_capacity(sessions.len());
            if let Some(session) = sessions.get(&active_id) {
                threads.push(ThreadView {
                    id: active_id.clone(),
                    name: session.name.clone(),
                    running: session.running,
                    turns: session.turns.clone(),
                });
            }
            for (id, session) in sessions.iter() {
                if id != &active_id {
                    threads.push(ThreadView {
                        id: id.clone(),
                        name: session.name.clone(),
                        running: session.running,
                        turns: session.turns.clone(),
                    });
                }
            }
            (
                active_id,
                current,
                threads,
                sessions
                    .iter()
                    .map(|(id, session)| (id.clone(), session.clone()))
                    .collect::<HashMap<_, _>>(),
            )
        };

        let historical = if current.is_none() {
            ledger_thread_for_session(&events, &history, session_id)
        } else {
            None
        };
        if current.is_none() && historical.is_none() {
            return Err(ApiError::bad("session_unknown"));
        }
        let read_only = historical.is_some();
        let running = current.as_ref().is_some_and(|session| session.running);
        let last = current
            .as_ref()
            .and_then(|session| session.last.clone())
            .or_else(|| historical.as_ref().and_then(|thread| thread.last.clone()));

        let mut session_views = Vec::with_capacity(memory_sessions.len() + history.len());
        if let Some(session) = memory_sessions.get(&active_id) {
            session_views.push(memory_session_view(&active_id, session));
        }
        for (id, session) in &memory_sessions {
            if id != &active_id {
                session_views.push(memory_session_view(id, session));
            }
        }
        for session in &history {
            if !memory_sessions.contains_key(&session.id) {
                session_views.push(historical_session_view(session));
            }
        }

        let thread = if let Some(thread) = &historical {
            historical_thread_view(thread)
        } else {
            threads
                .iter()
                .find(|thread| thread.id == session_id)
                .and_then(|thread| serde_json::to_value(thread).ok())
                .unwrap_or(Value::Null)
        };
        Ok(json!({
            "harness": harness_run::HARNESS_ID,
            "folder": self.workdir.display().to_string(),
            "trusted": trusted,
            "sandbox": sandbox,
            "role": role.role_id,
            "department": role.department_id,
            "session_id": session_id,
            "running": running,
            "read_only": read_only,
            "sessions": session_views,
            "threads": threads,
            "thread": thread,
            "last": last,
            "streaming": true,
            "streaming_transport": "sse",
            "shape": "codex-app",
        }))
    }

    async fn read_session_ledger(
        &self,
    ) -> Result<(Vec<LedgerEvent>, Vec<LedgerSession>), ApiError> {
        let events: Vec<LedgerEvent> = match self.host.persisted_events().await {
            Ok(Some(events)) => events
                .into_iter()
                .map(|event| LedgerEvent {
                    event_id: Some(event.event_id.to_string()),
                    request_id: Some(event.request_id.to_string()),
                    sequence: event.sequence,
                    kind: event.kind,
                    data: event.data,
                })
                .collect(),
            Ok(None) => {
                return Err(ApiError::fail("web_session_history_unsupported"));
            }
            Err(error) => {
                return Err(ApiError::fail(format!(
                    "web_session_history_unreadable:{error}"
                )));
            }
        };
        let sessions = ledger_sessions_for_root(&events, &self.workdir);
        Ok((events, sessions))
    }

    async fn history_sessions(&self) -> Result<Vec<LedgerSession>, ApiError> {
        self.read_session_ledger()
            .await
            .map(|(_, sessions)| sessions)
    }

    // 为每次 daemon 调用重建显式选项，避免把可变 Web 状态隐式散落到 handler 中。
    fn options(&self) -> Result<HashMap<String, Value>, ApiError> {
        let mut options = HashMap::new();
        options.insert(
            "cwd".to_string(),
            Value::String(self.workdir.to_string_lossy().into_owned()),
        );
        options.insert(
            "sandbox".to_string(),
            Value::String(lock_string(&self.sandbox)?),
        );
        options.insert("role".to_string(), Value::String(lock_string(&self.role)?));
        Ok(options)
    }
}

impl Default for WebSession {
    fn default() -> Self {
        Self {
            run_id: None,
            running: false,
            name: "New thread".to_owned(),
            last: None,
            turns: Vec::new(),
        }
    }
}

async fn index(
    State(app): State<Arc<WebApp>>,
    headers: HeaderMap,
) -> Result<Html<String>, ApiError> {
    authorize_host(&app, &headers)?;
    Ok(Html(
        PAGE.replace("__KIANA_WEB_TOKEN_VALUE__", &app.web_token),
    ))
}

async fn health(State(app): State<Arc<WebApp>>) -> Json<Value> {
    Json(json!({
        "ok": true,
        "harness": harness_run::HARNESS_ID,
        "loopback": true,
        "folder": app.workdir.display().to_string(),
        "streaming": true,
        "streaming_transport": "sse",
    }))
}

async fn state(
    State(app): State<Arc<WebApp>>,
    headers: HeaderMap,
    Query(query): Query<SessionBody>,
) -> Result<Json<Value>, ApiError> {
    authorize_mutation(&app, &headers)?;
    let session_id = match query
        .session_id
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        Some(session_id) => session_id.to_owned(),
        None => lock_string(&app.active)?,
    };
    Ok(Json(app.snapshot(&session_id).await?))
}

async fn list_sessions(
    State(app): State<Arc<WebApp>>,
    headers: HeaderMap,
) -> Result<Json<Value>, ApiError> {
    authorize_mutation(&app, &headers)?;
    let sessions = app
        .history_sessions()
        .await?
        .iter()
        .map(historical_session_view)
        .collect::<Vec<_>>();
    Ok(Json(json!({
        "source": "event_log",
        "read_only": true,
        "sessions": sessions,
    })))
}

struct EventStreamState {
    subscription: kiana_daemon::RunStreamSubscription,
    gap: Option<Event>,
    done: bool,
}

struct StreamAttachState {
    run_id: RunId,
    gap_reason: Option<&'static str>,
}

async fn events(
    State(app): State<Arc<WebApp>>,
    headers: HeaderMap,
    Query(query): Query<EventsQuery>,
) -> Result<Sse<impl Stream<Item = Result<Event, Infallible>> + Send + 'static>, ApiError> {
    authorize_sse(&app, &headers, query.token.as_deref())?;
    let session_id = resolve_mutable_session(&app, query.session_id.as_deref()).await?;
    let attach = stream_attach_state(&app, &session_id)?;
    // Subscribe before returning the SSE response headers. The browser waits for
    // EventSource.onopen before issuing /api/run, so the first delta is not lost.
    let subscription = app.host.subscribe_run(attach.run_id);
    let gap = attach
        .gap_reason
        .map(|reason| stream_gap_sse_event(attach.run_id, reason));
    let stream = stream::unfold(
        EventStreamState {
            subscription,
            gap,
            done: false,
        },
        |mut state| async move {
            if state.done {
                return None;
            }
            if let Some(gap) = state.gap.take() {
                return Some((Ok(gap), state));
            }
            loop {
                match state.subscription.recv().await {
                    Ok(envelope) => {
                        let (name, terminal) = match &envelope.event {
                            RunStreamEvent::Delta { .. } => ("delta", false),
                            RunStreamEvent::Terminal { .. } => ("terminal", true),
                            RunStreamEvent::Unknown => continue,
                        };
                        state.done = terminal;
                        return Some((Ok(run_stream_sse_event(name, &envelope)), state));
                    }
                    Err(tokio::sync::broadcast::error::RecvError::Lagged(skipped)) => {
                        state.done = true;
                        return Some((
                            Ok(stream_error_sse_event(&format!(
                                "stream_subscription_lagged:{skipped}"
                            ))),
                            state,
                        ));
                    }
                    Err(tokio::sync::broadcast::error::RecvError::Closed) => {
                        state.done = true;
                        return Some((
                            Ok(stream_error_sse_event("stream_closed_before_terminal")),
                            state,
                        ));
                    }
                }
            }
        },
    );
    Ok(Sse::new(Box::pin(stream)).keep_alive(KeepAlive::default()))
}

fn stream_attach_state(app: &WebApp, session_id: &str) -> Result<StreamAttachState, ApiError> {
    let sessions = app
        .sessions
        .lock()
        .map_err(|_| ApiError::fail("web_state_poisoned"))?;
    let session = sessions
        .get(session_id)
        .ok_or_else(|| ApiError::bad("session_unknown"))?;
    let run_id = session
        .run_id
        .or_else(|| RunId::parse_str(session_id))
        .ok_or_else(|| ApiError::bad("web_stream_run_id_unavailable"))?;
    Ok(StreamAttachState {
        run_id,
        gap_reason: session.running.then_some(STREAM_GAP_RUN_IN_PROGRESS),
    })
}

fn run_stream_sse_event(name: &str, envelope: &RunStreamEnvelope) -> Event {
    let data = serde_json::to_string(envelope).unwrap_or_else(|error| {
        json!({ "error": format!("stream_serialize_failed:{error}") }).to_string()
    });
    Event::default().event(name).data(data)
}

fn stream_gap_sse_event(run_id: RunId, reason: &str) -> Event {
    Event::default().event("stream_gap").data(
        json!({
            "run_id": run_id.to_string(),
            "reason": reason,
        })
        .to_string(),
    )
}

fn stream_error_sse_event(error: &str) -> Event {
    Event::default()
        .event("stream_error")
        .data(json!({ "error": error }).to_string())
}

async fn run_turn(
    State(app): State<Arc<WebApp>>,
    headers: HeaderMap,
    Json(body): Json<RunBody>,
) -> Result<Json<Value>, ApiError> {
    authorize_mutation(&app, &headers)?;
    let prompt = validate_web_prompt(body.prompt)?;
    let session_id = resolve_mutable_session(&app, body.session_id.as_deref()).await?;
    let run_id = session_run_id(&app, &session_id)?;
    let options = app.options()?;
    mark_running(&app, &session_id, true)?;
    let _running_guard = RunningGuard {
        app: Arc::clone(&app),
        session_id: session_id.clone(),
    };
    let result = if run_id.is_some() {
        harness_run::continue_envelope_on_host(
            Arc::clone(&app.host),
            session_id.clone(),
            prompt.clone(),
            run_id,
            &options,
        )
        .await
    } else {
        harness_run::run_envelope_on_host(
            Arc::clone(&app.host),
            session_id.clone(),
            prompt.clone(),
            &options,
        )
        .await
    };
    match result {
        Ok(response) => {
            store_turn(&app, &session_id, &prompt, &response)?;
            let mut payload = app.snapshot(&session_id).await?;
            payload["response"] = serde_json::to_value(&response).unwrap_or(Value::Null);
            Ok(Json(payload))
        }
        Err(error) => {
            mark_running(&app, &session_id, false)?;
            Err(ApiError::fail(error.to_string()))
        }
    }
}

async fn cancel_turn(
    State(app): State<Arc<WebApp>>,
    headers: HeaderMap,
    Json(body): Json<SessionBody>,
) -> Result<Json<Value>, ApiError> {
    authorize_mutation(&app, &headers)?;
    let session_id = resolve_mutable_session(&app, body.session_id.as_deref()).await?;
    let run_id = session_run_id(&app, &session_id)?;
    let options = app.options()?;
    let response = harness_run::cancel_envelope_on_host(
        Arc::clone(&app.host),
        session_id.clone(),
        run_id,
        "web_user",
        &options,
    )
    .await
    .map_err(|error| ApiError::fail(error.to_string()))?;
    store_turn(&app, &session_id, "(cancel)", &response)?;
    Ok(Json(app.snapshot(&session_id).await?))
}

async fn trust_folder(
    State(app): State<Arc<WebApp>>,
    headers: HeaderMap,
    body: Option<Json<SessionBody>>,
) -> Result<Json<Value>, ApiError> {
    authorize_mutation(&app, &headers)?;
    let requested = body
        .as_ref()
        .and_then(|Json(body)| body.session_id.as_deref());
    let session_id = resolve_mutable_session(&app, requested).await?;
    write_project_trust(&app.workdir, ProjectTrust::Trusted).map_err(ApiError::fail)?;
    Ok(Json(app.snapshot(&session_id).await?))
}

async fn set_sandbox(
    State(app): State<Arc<WebApp>>,
    headers: HeaderMap,
    Json(body): Json<SandboxBody>,
) -> Result<Json<Value>, ApiError> {
    authorize_mutation(&app, &headers)?;
    let session_id = resolve_mutable_session(&app, body.session_id.as_deref()).await?;
    let sandbox = workbench_chat::normalize_sandbox(&body.sandbox)
        .map_err(|error| ApiError::bad(error.to_string()))?;
    *app.sandbox
        .lock()
        .map_err(|_| ApiError::fail("web_state_poisoned"))? = sandbox;
    Ok(Json(app.snapshot(&session_id).await?))
}

async fn new_session(
    State(app): State<Arc<WebApp>>,
    headers: HeaderMap,
) -> Result<Json<Value>, ApiError> {
    authorize_mutation(&app, &headers)?;
    let session_id = new_session_id();
    {
        let mut sessions = app
            .sessions
            .lock()
            .map_err(|_| ApiError::fail("web_state_poisoned"))?;
        ensure_session_capacity(&sessions)?;
        sessions.insert(session_id.clone(), WebSession::default());
    }
    *app.active
        .lock()
        .map_err(|_| ApiError::fail("web_state_poisoned"))? = session_id.clone();
    Ok(Json(app.snapshot(&session_id).await?))
}

async fn read_receipt(
    State(app): State<Arc<WebApp>>,
    headers: HeaderMap,
    Json(body): Json<SessionBody>,
) -> Result<Json<Value>, ApiError> {
    authorize_mutation(&app, &headers)?;
    let session_id = resolve_mutable_session(&app, body.session_id.as_deref()).await?;
    let run_id = session_run_id(&app, &session_id)?;
    let options = app.options()?;
    let response = harness_run::receipt_envelope_on_host(
        Arc::clone(&app.host),
        session_id.clone(),
        run_id,
        &options,
    )
    .await
    .map_err(|error| ApiError::fail(error.to_string()))?;
    Ok(Json(json!({
        "state": app.snapshot(&session_id).await?,
        "receipt": response,
    })))
}

fn memory_session_view(id: &str, session: &WebSession) -> Value {
    json!({
        "id": id,
        "running": session.running,
        "name": session.name,
        "historical": false,
        "read_only": false,
    })
}

fn historical_session_view(session: &LedgerSession) -> Value {
    json!({
        "id": session.id,
        "running": false,
        "name": session.name,
        "historical": true,
        "read_only": true,
        "run_id": session.run_id,
        "role_id": session.role_id,
        "department_id": session.department_id,
        "project_root": session.project_root,
        "last_event_sequence": session.last_event_sequence,
        "last_event_kind": session.last_event_kind,
        "terminal_status": session.terminal_status,
    })
}

fn historical_thread_view(thread: &LedgerThread) -> Value {
    json!({
        "id": thread.session.id,
        "name": thread.session.name,
        "running": false,
        "turns": thread.turns,
        "historical": true,
        "read_only": true,
        "run_id": thread.session.run_id,
        "role_id": thread.session.role_id,
        "department_id": thread.session.department_id,
        "last_event_sequence": thread.session.last_event_sequence,
        "last_event_kind": thread.session.last_event_kind,
        "terminal_status": thread.session.terminal_status,
    })
}

fn ledger_sessions_for_root(events: &[LedgerEvent], root: &Path) -> Vec<LedgerSession> {
    let mut sessions = HashMap::<String, LedgerSession>::new();
    let mut run_to_session = HashMap::<String, String>::new();
    for (index, event) in events.iter().enumerate() {
        if event.kind == "run.authorized" {
            let Some(session_id) = event.data.get("session_id").and_then(Value::as_str) else {
                continue;
            };
            let Some(run_id) = event.data.get("run_id").and_then(Value::as_str) else {
                continue;
            };
            let Some(project_root) = event.data.get("project_root").and_then(Value::as_str) else {
                continue;
            };
            if !history_project_root_matches(project_root, root) {
                continue;
            }
            let Some(role_id) = event.data.get("role_id").and_then(Value::as_str) else {
                continue;
            };
            let Some(department_id) = event.data.get("department_id").and_then(Value::as_str)
            else {
                continue;
            };
            let session = sessions
                .entry(session_id.to_owned())
                .or_insert_with(|| LedgerSession {
                    id: session_id.to_owned(),
                    run_id: run_id.to_owned(),
                    role_id: role_id.to_owned(),
                    department_id: department_id.to_owned(),
                    project_root: project_root.to_owned(),
                    name: "New thread".to_owned(),
                    last_event_sequence: event.sequence,
                    last_event_kind: event.kind.clone(),
                    terminal_status: "unknown".to_owned(),
                    order: index,
                });
            session.run_id = run_id.to_owned();
            session.role_id = role_id.to_owned();
            session.department_id = department_id.to_owned();
            session.project_root = project_root.to_owned();
            session.last_event_sequence = event.sequence;
            session.last_event_kind = event.kind.clone();
            session.order = index;
            run_to_session.insert(run_id.to_owned(), session_id.to_owned());
            continue;
        }

        let Some(run_id) = event.data.get("run_id").and_then(Value::as_str) else {
            continue;
        };
        let Some(session_id) = run_to_session.get(run_id) else {
            continue;
        };
        let Some(session) = sessions.get_mut(session_id) else {
            continue;
        };
        session.last_event_sequence = event.sequence;
        session.last_event_kind = event.kind.clone();
        session.order = index;
        if let Some(status) = terminal_status_for_kind(&event.kind) {
            session.terminal_status = status.to_owned();
        } else if event.kind == "run.receipt" && session.terminal_status == "unknown" {
            session.terminal_status = "completed".to_owned();
        }
        if event.kind == "run.prompt" && session.name == "New thread" {
            if let Some(text) = event.data.get("text").and_then(Value::as_str) {
                session.name = web_thread::thread_name(&redact_history_text(text));
            }
        }
    }
    let mut sessions = sessions.into_values().collect::<Vec<_>>();
    sessions.sort_by(|left, right| right.order.cmp(&left.order));
    sessions.truncate(MAX_WEB_SESSIONS);
    sessions
}

fn ledger_thread_for_session(
    events: &[LedgerEvent],
    sessions: &[LedgerSession],
    session_id: &str,
) -> Option<LedgerThread> {
    let session = sessions.iter().find(|session| session.id == session_id)?;
    let mut turns = Vec::new();
    let mut current: Option<HistoryTurnBuilder> = None;
    for event in events {
        if event.data.get("run_id").and_then(Value::as_str) != Some(session.run_id.as_str()) {
            continue;
        }
        match event.kind.as_str() {
            "run.prompt" => {
                if let Some(turn) = current.take() {
                    push_history_turn(&mut turns, turn);
                }
                let prompt = event
                    .data
                    .get("text")
                    .and_then(Value::as_str)
                    .unwrap_or_default();
                current = Some(HistoryTurnBuilder::new(event, prompt));
            }
            "run.delta" => {
                if let Some(turn) = current.as_mut() {
                    if let Some(text) = event.data.get("text").and_then(Value::as_str) {
                        append_history_text(&mut turn.delta_text, text, MAX_WEB_ITEM_BODY_BYTES);
                    }
                }
            }
            "run.completed" => {
                if let Some(turn) = current.as_mut() {
                    turn.status = "completed".to_owned();
                    turn.final_text = event
                        .data
                        .get("text")
                        .and_then(Value::as_str)
                        .or_else(|| {
                            event
                                .data
                                .get("output")
                                .and_then(|output| output.get("text"))
                                .and_then(Value::as_str)
                        })
                        .unwrap_or_default()
                        .to_owned();
                    turn.files = history_string_list(
                        &event.data,
                        "files_changed",
                        MAX_WEB_FILES_PER_TURN,
                        MAX_WEB_FILE_PATH_BYTES,
                    );
                    turn.capabilities = history_capabilities(&event.data);
                }
            }
            "run.receipt" => {
                if let Some(turn) = current.as_mut() {
                    if turn.status == "unknown" {
                        turn.status = "completed".to_owned();
                    }
                    if turn.final_text.is_empty() {
                        turn.final_text = event
                            .data
                            .get("output")
                            .and_then(|output| output.get("text"))
                            .and_then(Value::as_str)
                            .unwrap_or_default()
                            .to_owned();
                    }
                    if turn.files.is_empty() {
                        turn.files = history_string_list(
                            &event.data,
                            "files_changed",
                            MAX_WEB_FILES_PER_TURN,
                            MAX_WEB_FILE_PATH_BYTES,
                        );
                    }
                    if turn.capabilities.is_empty() {
                        turn.capabilities = history_capabilities(&event.data);
                    }
                }
            }
            kind => {
                if let Some(status) = terminal_status_for_kind(kind) {
                    if let Some(turn) = current.as_mut() {
                        turn.status = status.to_owned();
                        turn.error = event
                            .data
                            .get("error")
                            .and_then(Value::as_str)
                            .map(redact_history_text);
                    }
                }
            }
        }
    }
    if let Some(turn) = current {
        push_history_turn(&mut turns, turn);
    }
    let last = turns.last().map(|turn| TurnSummary {
        status: turn.status.clone(),
        error: turn
            .items
            .iter()
            .rev()
            .find(|item| item.kind == "error")
            .map(|item| item.body.clone()),
        text: turn
            .items
            .iter()
            .rev()
            .find(|item| item.kind == "agentMessage")
            .map(|item| item.body.clone())
            .unwrap_or_default(),
        files_changed: turn
            .items
            .iter()
            .rev()
            .find(|item| item.kind == "fileChange")
            .map(|item| item.body.lines().map(str::to_owned).collect())
            .unwrap_or_default(),
        run_id: Some(session.run_id.clone()),
    });
    Some(LedgerThread {
        session: session.clone(),
        turns,
        last,
    })
}

struct HistoryTurnBuilder {
    id: String,
    status: String,
    prompt: String,
    delta_text: String,
    final_text: String,
    files: Vec<String>,
    capabilities: Vec<String>,
    error: Option<String>,
}

impl HistoryTurnBuilder {
    fn new(event: &LedgerEvent, prompt: &str) -> Self {
        Self {
            id: event.event_id.clone().unwrap_or_else(|| {
                format!(
                    "{}:{}:{}",
                    event.request_id.as_deref().unwrap_or_default(),
                    event.sequence,
                    event.kind
                )
            }),
            status: "unknown".to_owned(),
            prompt: prompt.to_owned(),
            delta_text: String::new(),
            final_text: String::new(),
            files: Vec::new(),
            capabilities: Vec::new(),
            error: None,
        }
    }

    fn into_turn(self) -> TurnView {
        let text = if self.final_text.is_empty() {
            redact_history_text(&self.delta_text)
        } else {
            redact_history_text(&self.final_text)
        };
        let mut items = vec![web_thread::ItemView {
            kind: "userMessage".to_owned(),
            status: "completed".to_owned(),
            title: "You".to_owned(),
            body: redact_history_text(&self.prompt),
        }];
        for capability in self.capabilities {
            items.push(web_thread::ItemView {
                kind: "commandExecution".to_owned(),
                status: self.status.clone(),
                title: format!("tool · {capability}"),
                body: capability,
            });
        }
        if !self.files.is_empty() {
            items.push(web_thread::ItemView {
                kind: "fileChange".to_owned(),
                status: self.status.clone(),
                title: format!(
                    "{} file{}",
                    self.files.len(),
                    if self.files.len() == 1 { "" } else { "s" }
                ),
                body: self.files.join("\n"),
            });
        }
        if !text.trim().is_empty() {
            items.push(web_thread::ItemView {
                kind: "agentMessage".to_owned(),
                status: self.status.clone(),
                title: "Builder".to_owned(),
                body: text,
            });
        }
        if let Some(error) = self.error.filter(|error| !error.trim().is_empty()) {
            items.push(web_thread::ItemView {
                kind: "error".to_owned(),
                status: self.status.clone(),
                title: "blocked".to_owned(),
                body: redact_history_text(&error),
            });
        }
        TurnView {
            id: self.id,
            status: self.status,
            items,
        }
    }
}

fn push_history_turn(turns: &mut Vec<TurnView>, turn: HistoryTurnBuilder) {
    push_turn_bounded(turns, turn.into_turn());
}

fn append_history_text(buffer: &mut String, text: &str, max_bytes: usize) {
    if buffer.len() >= max_bytes {
        return;
    }
    let remaining = max_bytes - buffer.len();
    let mut end = remaining.min(text.len());
    while end > 0 && !text.is_char_boundary(end) {
        end -= 1;
    }
    buffer.push_str(&text[..end]);
}

fn history_string_list(data: &Value, key: &str, max_items: usize, max_bytes: usize) -> Vec<String> {
    data.get(key)
        .and_then(Value::as_array)
        .map(|items| {
            items
                .iter()
                .filter_map(Value::as_str)
                .take(max_items)
                .map(|item| {
                    let mut item = redact_history_text(item);
                    truncate_utf8(&mut item, max_bytes);
                    item
                })
                .collect()
        })
        .unwrap_or_default()
}

fn history_capabilities(data: &Value) -> Vec<String> {
    data.get("capabilities")
        .and_then(Value::as_array)
        .map(|items| {
            items
                .iter()
                .filter_map(|item| {
                    item.get("operation")
                        .and_then(Value::as_str)
                        .or_else(|| item.as_str())
                })
                .take(MAX_WEB_ITEMS_PER_TURN)
                .map(redact_history_text)
                .collect()
        })
        .unwrap_or_default()
}

fn terminal_status_for_kind(kind: &str) -> Option<&'static str> {
    match kind {
        "run.completed" => Some("completed"),
        "run.cancelled" => Some("cancelled"),
        "run.failed" => Some("failed"),
        "run.result_unknown" => Some("result_unknown"),
        _ => None,
    }
}

fn redact_history_text(text: &str) -> String {
    let mut redactor = StreamingRedactor::new();
    let mut redacted = redactor.push(text);
    redacted.push_str(&redactor.finish());
    redacted
}

fn history_project_root_matches(recorded: &str, root: &Path) -> bool {
    canonical_history_path(Path::new(recorded)) == canonical_history_path(root)
}

fn canonical_history_path(path: &Path) -> PathBuf {
    path.canonicalize().unwrap_or_else(|_| path.to_path_buf())
}

fn ensure_session_capacity(sessions: &HashMap<String, WebSession>) -> Result<(), ApiError> {
    if sessions.len() >= MAX_WEB_SESSIONS {
        return Err(ApiError::conflict("web_session_limit_exceeded"));
    }
    Ok(())
}

fn validate_web_prompt(prompt: String) -> Result<String, ApiError> {
    let prompt = prompt.trim().to_owned();
    if prompt.is_empty() {
        return Err(ApiError::bad("prompt_required"));
    }
    if prompt.len() > MAX_WEB_PROMPT_BYTES {
        return Err(ApiError::bad("prompt_too_large"));
    }
    Ok(prompt)
}

fn authorize_mutation(app: &WebApp, headers: &HeaderMap) -> Result<(), ApiError> {
    let supplied = web_token_from_header(headers);
    authorize_web_request(app, headers, supplied)
}

fn authorize_sse(
    app: &WebApp,
    headers: &HeaderMap,
    query_token: Option<&str>,
) -> Result<(), ApiError> {
    let supplied = web_token_from_header(headers)
        .or_else(|| query_token.map(str::trim).filter(|value| !value.is_empty()));
    authorize_web_request(app, headers, supplied)
}

fn web_token_from_header(headers: &HeaderMap) -> Option<&str> {
    headers
        .get("x-kiana-web-token")
        .and_then(|value| value.to_str().ok())
        .map(str::trim)
        .filter(|value| !value.is_empty())
}

fn authorize_web_request(
    app: &WebApp,
    headers: &HeaderMap,
    supplied: Option<&str>,
) -> Result<(), ApiError> {
    if supplied != Some(app.web_token.as_str()) {
        return Err(ApiError::unauthorized());
    }
    authorize_host(app, headers)?;
    if let Some(origin) = headers.get("origin").and_then(|value| value.to_str().ok()) {
        if !origin_matches_bound_addr(origin, app.bound_addr) {
            return Err(ApiError::unauthorized());
        }
    }
    Ok(())
}

fn authorize_host(app: &WebApp, headers: &HeaderMap) -> Result<(), ApiError> {
    let host = headers
        .get("host")
        .and_then(|value| value.to_str().ok())
        .filter(|value| authority_matches_bound_addr(value, app.bound_addr));
    if host.is_none() {
        return Err(ApiError::unauthorized());
    }
    Ok(())
}

fn authority_matches_bound_addr(authority: &str, bound_addr: SocketAddr) -> bool {
    url::Url::parse(&format!("http://{authority}"))
        .ok()
        .is_some_and(|url| url_matches_bound_addr(&url, bound_addr))
}

fn origin_matches_bound_addr(origin: &str, bound_addr: SocketAddr) -> bool {
    url::Url::parse(origin)
        .ok()
        .is_some_and(|url| url_matches_bound_addr(&url, bound_addr))
}

fn url_matches_bound_addr(url: &url::Url, bound_addr: SocketAddr) -> bool {
    url.scheme() == "http"
        && url.username().is_empty()
        && url.password().is_none()
        && url.path() == "/"
        && url.query().is_none()
        && url.fragment().is_none()
        && url.host_str().and_then(|host| host.parse::<IpAddr>().ok()) == Some(bound_addr.ip())
        && url.port_or_known_default() == Some(bound_addr.port())
}

async fn resolve_mutable_session(
    app: &WebApp,
    requested: Option<&str>,
) -> Result<String, ApiError> {
    let id = match requested.map(str::trim).filter(|value| !value.is_empty()) {
        Some(id) => id.to_owned(),
        None => lock_string(&app.active)?,
    };
    let known_session = {
        let sessions = app
            .sessions
            .lock()
            .map_err(|_| ApiError::fail("web_state_poisoned"))?;
        sessions.contains_key(&id)
    };
    if !known_session {
        if app
            .history_sessions()
            .await?
            .iter()
            .any(|session| session.id == id)
        {
            return Err(ApiError::conflict("session_read_only"));
        }
        return Err(ApiError::bad("session_unknown"));
    }
    Ok(id)
}

struct RunningGuard {
    app: Arc<WebApp>,
    session_id: String,
}

impl Drop for RunningGuard {
    fn drop(&mut self) {
        let _ = mark_running(&self.app, &self.session_id, false);
    }
}

fn mark_running(app: &WebApp, session_id: &str, running: bool) -> Result<(), ApiError> {
    let mut sessions = app
        .sessions
        .lock()
        .map_err(|_| ApiError::fail("web_state_poisoned"))?;
    let session = sessions
        .get_mut(session_id)
        .ok_or_else(|| ApiError::bad("session_unknown"))?;
    if running && session.running {
        return Err(ApiError::conflict("session_busy"));
    }
    session.running = running;
    Ok(())
}

fn session_run_id(app: &WebApp, session_id: &str) -> Result<Option<RunId>, ApiError> {
    let sessions = app
        .sessions
        .lock()
        .map_err(|_| ApiError::fail("web_state_poisoned"))?;
    Ok(sessions.get(session_id).and_then(|session| session.run_id))
}

fn store_turn(
    app: &WebApp,
    session_id: &str,
    prompt: &str,
    response: &ResponseEnvelope,
) -> Result<(), ApiError> {
    let mut sessions = app
        .sessions
        .lock()
        .map_err(|_| ApiError::fail("web_state_poisoned"))?;
    let session = sessions
        .get_mut(session_id)
        .ok_or_else(|| ApiError::bad("session_unknown"))?;
    session.running = false;
    if session.name == "New thread" && !prompt.is_empty() && prompt != "(cancel)" {
        session.name = web_thread::thread_name(prompt);
    }
    if let Some(run_id) = run_id_from(response) {
        session.run_id = Some(run_id);
    }
    let status = web_thread::status_of(response);
    let mut summary_text = web_thread::assistant_text(response);
    truncate_utf8(&mut summary_text, MAX_WEB_SUMMARY_TEXT_BYTES);
    session.last = Some(TurnSummary {
        status: status.clone(),
        error: response.error.clone(),
        text: summary_text,
        files_changed: bounded_files(response),
        run_id: run_id_from(response).map(|id| id.to_string()),
    });
    push_turn_bounded(
        &mut session.turns,
        TurnView {
            id: run_id_from(response)
                .map(|id| id.to_string())
                .unwrap_or_else(|| response.request_id.to_string()),
            status,
            items: web_thread::items_from_turn(prompt, response),
        },
    );
    Ok(())
}

fn bounded_files(response: &ResponseEnvelope) -> Vec<String> {
    web_thread::files_changed(response)
        .into_iter()
        .take(MAX_WEB_FILES_PER_TURN)
        .map(|mut path| {
            truncate_utf8(&mut path, MAX_WEB_FILE_PATH_BYTES);
            path
        })
        .collect()
}

fn push_turn_bounded(turns: &mut Vec<TurnView>, mut turn: TurnView) {
    turn.items.truncate(MAX_WEB_ITEMS_PER_TURN);
    for item in &mut turn.items {
        truncate_utf8(&mut item.body, MAX_WEB_ITEM_BODY_BYTES);
    }
    let turn_bytes = turn_storage_bytes(&turn);
    while !turns.is_empty()
        && (turns.len() >= MAX_WEB_TURNS_PER_SESSION
            || transcript_storage_bytes(turns).saturating_add(turn_bytes)
                > MAX_WEB_TRANSCRIPT_BYTES_PER_SESSION)
    {
        turns.remove(0);
    }
    turns.push(turn);
}

fn transcript_storage_bytes(turns: &[TurnView]) -> usize {
    turns.iter().map(turn_storage_bytes).sum()
}

fn turn_storage_bytes(turn: &TurnView) -> usize {
    turn.id.len()
        + turn.status.len()
        + turn
            .items
            .iter()
            .map(|item| item.kind.len() + item.status.len() + item.title.len() + item.body.len())
            .sum::<usize>()
}

fn truncate_utf8(value: &mut String, max_bytes: usize) {
    if value.len() <= max_bytes {
        return;
    }
    let boundary = value
        .char_indices()
        .take_while(|(index, _)| *index <= max_bytes)
        .map(|(index, _)| index)
        .last()
        .unwrap_or(0);
    value.truncate(boundary);
    value.push_str("\n…truncated…");
}

fn run_id_from(response: &ResponseEnvelope) -> Option<RunId> {
    response
        .output
        .get("run_id")
        .and_then(Value::as_str)
        .and_then(RunId::parse_str)
}

fn lock_string(lock: &Mutex<String>) -> Result<String, ApiError> {
    lock.lock()
        .map(|value| value.clone())
        .map_err(|_| ApiError::fail("web_state_poisoned"))
}

fn new_session_id() -> String {
    uuid::Uuid::new_v4().to_string()
}

fn resolve_workdir(explicit: Option<&Path>) -> Result<PathBuf> {
    let path = match explicit {
        Some(path) => path.to_path_buf(),
        None => std::env::current_dir().context("failed to resolve current directory")?,
    };
    let canonical = if path.exists() {
        path.canonicalize()
            .with_context(|| format!("workdir_not_found: {}", path.display()))?
    } else {
        return Err(anyhow!("workdir_not_found: {}", path.display()));
    };
    if !canonical.is_dir() {
        return Err(anyhow!("workdir_not_found: {}", canonical.display()));
    }
    Ok(canonical)
}

fn maybe_open(url: &str) {
    let _ = if cfg!(target_os = "macos") {
        Command::new("open").arg(url).spawn()
    } else if cfg!(target_os = "windows") {
        Command::new("cmd").args(["/C", "start", url]).spawn()
    } else {
        Command::new("xdg-open").arg(url).spawn()
    };
}

#[cfg(test)]
mod tests {
    use super::*;
    use kiana_types::write_project_trust;
    use std::fs;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn unique_dir(label: &str) -> PathBuf {
        let stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("time")
            .as_nanos();
        let dir =
            std::env::temp_dir().join(format!("kiana-web-{label}-{}-{stamp}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        fs::canonicalize(&dir).unwrap()
    }

    fn ledger_event(sequence: u64, kind: &str, data: Value) -> Value {
        json!({
            "event_id": uuid::Uuid::new_v4().to_string(),
            "request_id": uuid::Uuid::new_v4().to_string(),
            "sequence": sequence,
            "kind": kind,
            "data": data,
        })
    }

    fn write_ledger(home: &Path, events: &[Value]) -> PathBuf {
        let path = home.join("sessions").join("events.jsonl");
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        let body = events
            .iter()
            .map(Value::to_string)
            .collect::<Vec<_>>()
            .join("\n")
            + "\n";
        fs::write(&path, body).unwrap();
        path
    }

    fn parse_ledger(events: &[Value]) -> Vec<LedgerEvent> {
        events
            .iter()
            .cloned()
            .map(|event| serde_json::from_value(event).unwrap())
            .collect()
    }

    fn historical_events(root: &Path, session_id: &str, run_id: &str) -> Vec<Value> {
        vec![
            ledger_event(
                1,
                "run.authorized",
                json!({
                    "run_id": run_id,
                    "session_id": session_id,
                    "actor_id": "local-user",
                    "project_root": root.to_string_lossy(),
                    "role_id": ROLE_BUILDER,
                    "department_id": kiana_protocol::DEPARTMENT_EXECUTING,
                    "sandbox": "read-only",
                }),
            ),
            ledger_event(
                2,
                "run.prompt",
                json!({
                    "run_id": run_id,
                    "session_id": session_id,
                    "text": "historical task",
                }),
            ),
            ledger_event(
                3,
                "run.delta",
                json!({ "run_id": run_id, "text": "historical " }),
            ),
            ledger_event(
                4,
                "run.completed",
                json!({ "run_id": run_id, "text": "historical response" }),
            ),
            ledger_event(
                5,
                "run.receipt",
                json!({
                    "run_id": run_id,
                    "session_id": session_id,
                    "output": { "text": "historical response" },
                    "files_changed": ["src/history.rs"],
                    "capabilities": [{"operation": "shell"}],
                }),
            ),
        ]
    }

    #[test]
    fn web_projection_output_limits_are_bounded() {
        let response = ResponseEnvelope {
            schema: "kiana.protocol.v1".to_owned(),
            request_id: kiana_protocol::RequestId::new(),
            status: kiana_protocol::ExecutionStatus::Completed,
            output: serde_json::json!({
                "output": { "text": "回答".repeat(MAX_WEB_SUMMARY_TEXT_BYTES) },
                "files_changed": (0..(MAX_WEB_FILES_PER_TURN + 1))
                    .map(|index| format!("{index}-{}", "x".repeat(MAX_WEB_FILE_PATH_BYTES)))
                    .collect::<Vec<_>>(),
            }),
            error: None,
        };
        let mut turns = Vec::new();
        push_turn_bounded(
            &mut turns,
            TurnView {
                id: "run".to_owned(),
                status: "completed".to_owned(),
                items: (0..(MAX_WEB_ITEMS_PER_TURN + 1))
                    .map(|index| web_thread::ItemView {
                        kind: "agentMessage".to_owned(),
                        status: "completed".to_owned(),
                        title: index.to_string(),
                        body: "body".to_owned(),
                    })
                    .collect(),
            },
        );
        assert_eq!(turns[0].items.len(), MAX_WEB_ITEMS_PER_TURN);
        assert_eq!(bounded_files(&response).len(), MAX_WEB_FILES_PER_TURN);
        assert!(bounded_files(&response)
            .iter()
            .all(|path| path.len() <= MAX_WEB_FILE_PATH_BYTES + "\n…truncated…".len()));
        let mut summary = web_thread::assistant_text(&response);
        truncate_utf8(&mut summary, MAX_WEB_SUMMARY_TEXT_BYTES);
        assert!(summary.len() <= MAX_WEB_SUMMARY_TEXT_BYTES + "\n…truncated…".len());
    }

    #[test]
    fn web_turn_history_byte_quota_is_bounded() {
        let mut turns = Vec::new();
        for index in 0..MAX_WEB_TURNS_PER_SESSION {
            push_turn_bounded(
                &mut turns,
                TurnView {
                    id: index.to_string(),
                    status: "completed".to_owned(),
                    items: vec![web_thread::ItemView {
                        kind: "agentMessage".to_owned(),
                        status: "completed".to_owned(),
                        title: "Builder".to_owned(),
                        body: "数据".repeat(MAX_WEB_ITEM_BODY_BYTES),
                    }],
                },
            );
        }
        assert!(transcript_storage_bytes(&turns) <= MAX_WEB_TRANSCRIPT_BYTES_PER_SESSION);
        assert!(turns.len() < MAX_WEB_TURNS_PER_SESSION);
        assert_eq!(turns.last().map(|turn| turn.id.as_str()), Some("255"));
        assert!(turns
            .iter()
            .flat_map(|turn| turn.items.iter())
            .all(|item| item.body.contains("truncated")));
    }

    #[test]
    fn web_turn_history_is_bounded() {
        let mut turns = Vec::new();
        for index in 0..=MAX_WEB_TURNS_PER_SESSION {
            push_turn_bounded(
                &mut turns,
                TurnView {
                    id: index.to_string(),
                    status: "completed".to_owned(),
                    items: Vec::new(),
                },
            );
        }
        assert_eq!(turns.len(), MAX_WEB_TURNS_PER_SESSION);
        assert_eq!(turns.first().map(|turn| turn.id.as_str()), Some("1"));
        assert_eq!(turns.last().map(|turn| turn.id.as_str()), Some("256"));
    }

    #[test]
    fn web_session_capacity_fails_closed() {
        let sessions = HashMap::new();
        assert!(ensure_session_capacity(&sessions).is_ok());
        let sessions = (0..MAX_WEB_SESSIONS)
            .map(|index| (index.to_string(), WebSession::default()))
            .collect::<HashMap<_, _>>();
        let error = ensure_session_capacity(&sessions).unwrap_err();
        assert_eq!(error.status, StatusCode::CONFLICT);
        assert_eq!(error.error, "web_session_limit_exceeded");
    }

    #[test]
    fn web_prompt_limit_fails_closed() {
        assert_eq!(
            validate_web_prompt(" ".to_owned()).unwrap_err().error,
            "prompt_required"
        );
        assert_eq!(
            validate_web_prompt("x".repeat(MAX_WEB_PROMPT_BYTES + 1))
                .unwrap_err()
                .error,
            "prompt_too_large"
        );
        assert_eq!(
            validate_web_prompt("  hello  ".to_owned()).unwrap(),
            "hello"
        );
    }

    #[tokio::test]
    async fn explicit_session_resolution_does_not_change_global_active_thread() {
        let env = crate::test_support::scoped_env(&["KIANA_HOME"]);
        let home = unique_dir("session-isolation-home");
        env.set_var("KIANA_HOME", &home);
        let root = unique_dir("session-isolation");
        fs::create_dir_all(root.join(".git")).unwrap();
        let app = WebApp::new(
            std::sync::Arc::new(DaemonHost::local().unwrap()),
            root.clone(),
            "read-only".to_owned(),
            ROLE_BUILDER.to_owned(),
            "127.0.0.1:0".parse().unwrap(),
        );
        let active = lock_string(&app.active).unwrap();
        let other = new_session_id();
        app.sessions
            .lock()
            .unwrap()
            .insert(other.clone(), WebSession::default());

        assert_eq!(
            resolve_mutable_session(&app, Some(&other)).await.unwrap(),
            other
        );
        assert_eq!(lock_string(&app.active).unwrap(), active);
        let snapshot = app.snapshot(&other).await.unwrap();
        assert_eq!(snapshot["session_id"].as_str(), Some(other.as_str()));
        assert_eq!(snapshot["thread"]["id"].as_str(), Some(other.as_str()));
        assert_eq!(lock_string(&app.active).unwrap(), active);
        let _ = fs::remove_dir_all(root);
        let _ = fs::remove_dir_all(home);
    }

    #[tokio::test]
    async fn fresh_web_app_lists_previous_sessions_from_the_event_ledger() {
        let env = crate::test_support::scoped_env(&[
            "KIANA_HOME",
            "HOME",
            "KIANA_HARNESS_SCRIPT",
            "KIANA_PROVIDER",
            "KIANA_STREAMING",
            "ANTHROPIC_API_KEY",
            "KIANA_OPENAI_API_KEY",
            "OPENAI_API_KEY",
        ]);
        let home = unique_dir("history-restart-home");
        let root = unique_dir("history-restart-root");
        fs::create_dir_all(root.join(".git")).unwrap();
        let script = unique_dir("history-restart-script").join("script.json");
        fs::write(&script, r#"[{"text":"first response"}]"#).unwrap();
        env.set_var("KIANA_HOME", &home);
        env.set_var("HOME", &home);
        env.set_var("KIANA_HARNESS_SCRIPT", &script);
        env.set_var("KIANA_PROVIDER", "");
        env.set_var("KIANA_STREAMING", "off");
        std::env::remove_var("ANTHROPIC_API_KEY");
        std::env::remove_var("KIANA_OPENAI_API_KEY");
        std::env::remove_var("OPENAI_API_KEY");
        write_project_trust(&root, ProjectTrust::Trusted).unwrap();

        let address = "127.0.0.1:0".parse().unwrap();
        let host = Arc::new(DaemonHost::local().unwrap());
        let app = WebApp::new(
            Arc::clone(&host),
            root.clone(),
            "read-only".to_owned(),
            ROLE_BUILDER.to_owned(),
            address,
        );
        let historical_session = "history-session-from-ledger".to_owned();
        let response = harness_run::run_envelope_on_host(
            Arc::clone(&host),
            historical_session.clone(),
            "first task",
            &app.options().unwrap(),
        )
        .await
        .unwrap();
        assert_eq!(
            response.status,
            kiana_protocol::ExecutionStatus::Completed,
            "{response:?}"
        );
        let historical_run = response.output["run_id"]
            .as_str()
            .expect("run id")
            .to_owned();
        let runtime_events = host.persisted_events().await.unwrap().unwrap();
        let ledger_events = app.read_session_ledger().await.unwrap().0;
        assert_eq!(ledger_events.len(), runtime_events.len());
        for (runtime, ledger) in runtime_events.iter().zip(&ledger_events) {
            assert_eq!(ledger.event_id, Some(runtime.event_id.to_string()));
            assert_eq!(ledger.request_id, Some(runtime.request_id.to_string()));
            assert_eq!(ledger.sequence, runtime.sequence);
            assert_eq!(ledger.kind, runtime.kind);
            assert_eq!(ledger.data, runtime.data);
        }
        drop(app);
        drop(host);

        let restarted = WebApp::new(
            Arc::new(DaemonHost::local().unwrap()),
            root.clone(),
            "read-only".to_owned(),
            ROLE_BUILDER.to_owned(),
            address,
        );
        let active = lock_string(&restarted.active).unwrap();
        let snapshot = restarted.snapshot(&active).await.unwrap();
        let sessions = snapshot["sessions"].as_array().unwrap();
        assert_eq!(sessions[0]["id"], active);
        let previous = sessions
            .iter()
            .find(|session| session["id"] == historical_session)
            .expect("historical session");
        assert_eq!(previous["running"], false);
        assert_eq!(previous["read_only"], true);
        assert_eq!(previous["run_id"], historical_run);
        assert_eq!(previous["role_id"], ROLE_BUILDER);
        assert_eq!(
            previous["department_id"],
            kiana_protocol::DEPARTMENT_EXECUTING
        );
        assert_eq!(previous["terminal_status"], "completed");
        assert!(previous["last_event_sequence"].as_u64().unwrap() > 0);

        let historical = restarted.snapshot(&historical_session).await.unwrap();
        assert_eq!(historical["read_only"], true);
        assert_eq!(historical["thread"]["id"], historical_session);
        assert_eq!(historical["thread"]["read_only"], true);
        let turns = historical["thread"]["turns"].as_array().unwrap();
        assert_eq!(turns.len(), 1);
        let items = turns[0]["items"].as_array().unwrap();
        assert!(items
            .iter()
            .any(|item| { item["kind"] == "userMessage" && item["body"] == "first task" }));
        assert!(items
            .iter()
            .any(|item| { item["kind"] == "agentMessage" && item["body"] == "first response" }));

        let _ = fs::remove_dir_all(root);
        let _ = fs::remove_dir_all(home);
    }

    #[tokio::test]
    async fn opening_historical_session_does_not_append_events_or_allow_execution() {
        let env = crate::test_support::scoped_env(&[
            "KIANA_HOME",
            "HOME",
            "KIANA_HARNESS_SCRIPT",
            "KIANA_PROVIDER",
        ]);
        let home = unique_dir("history-readonly-home");
        let root = unique_dir("history-readonly-root");
        fs::create_dir_all(root.join(".git")).unwrap();
        let session_id = "historical-readonly-session";
        let run_id = RunId::new().to_string();
        let ledger = write_ledger(&home, &historical_events(&root, session_id, &run_id));
        env.set_var("KIANA_HOME", &home);
        env.set_var("HOME", &home);
        env.set_var(
            "KIANA_HARNESS_SCRIPT",
            home.join("missing-history-script.json"),
        );
        env.set_var("KIANA_PROVIDER", "");

        let before = fs::read(&ledger).unwrap();
        let app = WebApp::new(
            Arc::new(DaemonHost::local().unwrap()),
            root.clone(),
            "read-only".to_owned(),
            ROLE_BUILDER.to_owned(),
            "127.0.0.1:0".parse().unwrap(),
        );
        let snapshot = app.snapshot(session_id).await.unwrap();
        assert_eq!(snapshot["read_only"], true);
        assert_eq!(snapshot["thread"]["turns"].as_array().unwrap().len(), 1);
        let rejected = resolve_mutable_session(&app, Some(session_id))
            .await
            .unwrap_err();
        assert_eq!(rejected.status, StatusCode::CONFLICT);
        assert_eq!(rejected.error, "session_read_only");
        assert_eq!(fs::read(&ledger).unwrap(), before);

        let _ = fs::remove_dir_all(root);
        let _ = fs::remove_dir_all(home);
    }

    #[tokio::test]
    async fn empty_event_ledger_lists_no_historical_sessions() {
        let env = crate::test_support::scoped_env(&["KIANA_HOME", "HOME", "KIANA_HARNESS_SCRIPT"]);
        let home = unique_dir("history-empty-home");
        let root = unique_dir("history-empty-root");
        fs::create_dir_all(root.join(".git")).unwrap();
        env.set_var("KIANA_HOME", &home);
        env.set_var("HOME", &home);
        env.set_var("KIANA_HARNESS_SCRIPT", "");

        let app = WebApp::new(
            Arc::new(DaemonHost::with_env_harness().unwrap()),
            root.clone(),
            "read-only".to_owned(),
            ROLE_BUILDER.to_owned(),
            "127.0.0.1:0".parse().unwrap(),
        );
        let active = lock_string(&app.active).unwrap();
        let snapshot = app.snapshot(&active).await.unwrap();
        let sessions = snapshot["sessions"].as_array().unwrap();
        assert_eq!(sessions.len(), 1);
        assert_eq!(sessions[0]["id"], active);
        assert!(app.history_sessions().await.unwrap().is_empty());

        let _ = fs::remove_dir_all(root);
        let _ = fs::remove_dir_all(home);
    }

    #[tokio::test]
    async fn unreadable_event_ledger_fails_closed() {
        let env = crate::test_support::scoped_env(&["KIANA_HOME", "HOME", "KIANA_HARNESS_SCRIPT"]);
        let home = unique_dir("history-unreadable-home");
        let root = unique_dir("history-unreadable-root");
        fs::create_dir_all(root.join(".git")).unwrap();
        env.set_var("KIANA_HOME", &home);
        env.set_var("HOME", &home);
        env.set_var("KIANA_HARNESS_SCRIPT", "");

        let host = Arc::new(DaemonHost::local().unwrap());
        let ledger = home.join("sessions").join("events.jsonl");
        fs::create_dir_all(ledger.parent().unwrap()).unwrap();
        fs::write(&ledger, b"{not-json}\n").unwrap();

        let app = WebApp::new(
            host,
            root.clone(),
            "read-only".to_owned(),
            ROLE_BUILDER.to_owned(),
            "127.0.0.1:0".parse().unwrap(),
        );
        let active = lock_string(&app.active).unwrap();
        let error = app.snapshot(&active).await.unwrap_err();
        assert_eq!(error.status, StatusCode::INTERNAL_SERVER_ERROR);
        assert!(
            error.error.starts_with("web_session_history_unreadable"),
            "{error:?}"
        );

        let _ = fs::remove_dir_all(root);
        let _ = fs::remove_dir_all(home);
    }

    #[test]
    fn web_page_renders_historical_sessions_as_read_only() {
        assert!(PAGE.contains("(s.sessions || s.threads || [])"));
        assert!(PAGE.contains("readOnly = !!s.read_only;"));
        assert!(PAGE.contains("promptEl.disabled = readOnly;"));
        assert!(PAGE.contains("document.getElementById('trustBtn').disabled = readOnly;"));
        assert!(PAGE.contains("历史会话只读；新建 Thread 后才能继续。"));
        assert!(PAGE.contains("if (readOnly)"));
    }

    #[test]
    fn historical_ledger_projection_redacts_recorded_secrets() {
        let root = unique_dir("history-redaction-root");
        let session_id = "history-redaction-session";
        let run_id = RunId::new().to_string();
        let events = vec![
            ledger_event(
                1,
                "run.authorized",
                json!({
                    "run_id": run_id.as_str(),
                    "session_id": session_id,
                    "project_root": root.to_string_lossy(),
                    "role_id": ROLE_BUILDER,
                    "department_id": kiana_protocol::DEPARTMENT_EXECUTING,
                }),
            ),
            ledger_event(
                2,
                "run.prompt",
                json!({
                    "run_id": run_id.as_str(),
                    "session_id": session_id,
                    "text": "token=history-secret",
                }),
            ),
            ledger_event(
                3,
                "run.completed",
                json!({ "run_id": run_id.as_str(), "text": "api_key=history-secret" }),
            ),
        ];
        let events = parse_ledger(&events);
        let sessions = ledger_sessions_for_root(&events, &root);
        assert!(!sessions[0].name.contains("history-secret"));
        let thread = ledger_thread_for_session(&events, &sessions, session_id).unwrap();
        let rendered = serde_json::to_string(&thread.turns).unwrap();
        assert!(!rendered.contains("history-secret"), "{rendered}");
        assert!(rendered.contains("[REDACTED]"), "{rendered}");
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn ledger_history_maps_completed_cancelled_failed_result_unknown_and_missing_states() {
        let root = Path::new("/tmp/kiana-history-status-root");
        for (kind, expected) in [
            ("run.completed", "completed"),
            ("run.cancelled", "cancelled"),
            ("run.failed", "failed"),
            ("run.result_unknown", "result_unknown"),
            ("run.delta", "unknown"),
        ] {
            let run_id = RunId::new().to_string();
            let events = parse_ledger(&[
                ledger_event(
                    1,
                    "run.authorized",
                    json!({
                        "run_id": run_id.as_str(),
                        "session_id": "history-status-session",
                        "project_root": root.to_string_lossy(),
                        "role_id": ROLE_BUILDER,
                        "department_id": kiana_protocol::DEPARTMENT_EXECUTING,
                    }),
                ),
                ledger_event(
                    2,
                    kind,
                    json!({ "run_id": run_id.as_str(), "error": "terminal" }),
                ),
            ]);
            let sessions = ledger_sessions_for_root(&events, root);
            assert_eq!(sessions[0].terminal_status, expected, "{kind}");
        }
    }

    #[test]
    fn parse_rejects_non_loopback_bind() {
        let error = parse_bind("0.0.0.0:3080").unwrap_err().to_string();
        assert_eq!(error, "bind_loopback_only");
        assert!(parse_bind("127.0.0.1:0").is_ok());
        assert!(parse_bind("localhost:3080").is_ok());
        assert_eq!(
            parse_bind("127.0.0.2:3080").unwrap_err().to_string(),
            "bind_loopback_only"
        );
    }

    #[test]
    fn parse_args_defaults_workspace_write() {
        let launch = parse_web_args(&["web".to_owned(), "--no-open".to_owned()]).unwrap();
        assert_eq!(launch.sandbox, "workspace-write");
        assert!(launch.no_open);
        assert_eq!(launch.bind, DEFAULT_BIND);
    }

    #[test]
    fn web_page_does_not_mark_turn_complete_before_terminal_event() {
        let delta_handler = PAGE
            .find("function handleStreamDelta")
            .expect("delta handler");
        let terminal_handler = PAGE
            .find("function handleStreamTerminal")
            .expect("terminal handler");
        let gap_handler = PAGE.find("function handleStreamGap").expect("gap handler");
        assert!(delta_handler < terminal_handler);
        assert!(!PAGE[delta_handler..terminal_handler].contains("streamComplete = true"));
        assert!(PAGE[terminal_handler..gap_handler]
            .contains("if (!streamIncomplete && status === 'completed') streamComplete = true;"));
        assert!(PAGE.contains("let streamComplete = false;"));
        assert!(PAGE.contains("streamComplete ? 'complete' : 'not complete'"));
    }

    #[test]
    fn web_page_marks_stream_gap_and_stream_error_as_incomplete_receipt_authoritative() {
        let gap_handler = PAGE.find("function handleStreamGap").expect("gap handler");
        let error_handler = PAGE
            .find("function handleStreamError")
            .expect("error handler");
        let event_stream = PAGE
            .find("function ensureEventStream")
            .expect("event stream");
        assert!(gap_handler < error_handler);
        assert!(error_handler < event_stream);
        assert!(PAGE[gap_handler..error_handler]
            .contains("markStreamIncomplete(gap.reason || 'stream_gap')"));
        assert!(PAGE[error_handler..event_stream]
            .contains("markStreamIncomplete(payload.error || 'stream_error')"));
        assert!(PAGE.contains("source.addEventListener('stream_gap', handleStreamGap);"));
        assert!(PAGE.contains("source.addEventListener('stream_error', handleStreamError);"));
        assert!(PAGE.contains("'Builder · 不完整 / 已中断'"));
        assert!(PAGE.contains("请以 receipt 为准"));
        assert!(PAGE.contains("markStreamIncomplete('stream_connect_failed')"));
    }

    async fn sse_event_body(event: Event) -> String {
        let response =
            Sse::new(futures_util::stream::iter(vec![Ok::<_, Infallible>(event)])).into_response();
        let body = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        String::from_utf8(body.to_vec()).unwrap()
    }

    #[tokio::test]
    async fn web_sse_gap_event_is_machine_readable_with_run_id_and_reason() {
        let run_id = RunId::new();
        let body = sse_event_body(stream_gap_sse_event(run_id, STREAM_GAP_RUN_IN_PROGRESS)).await;
        assert!(body.contains("event: stream_gap"), "{body}");
        assert!(body.contains(&format!("\"run_id\":\"{run_id}\"")), "{body}");
        assert!(
            body.contains("\"reason\":\"subscription_attached_after_run_started\""),
            "{body}"
        );
    }

    #[tokio::test]
    async fn web_sse_connection_failure_emits_explicit_stream_error_event() {
        let body = sse_event_body(stream_error_sse_event("stream_closed_before_terminal")).await;
        assert!(body.contains("event: stream_error"), "{body}");
        assert!(
            body.contains("\"error\":\"stream_closed_before_terminal\""),
            "{body}"
        );
    }

    #[tokio::test]
    async fn loopback_health_and_cassette_write_go_through_daemon_host() {
        let env = crate::test_support::scoped_env(&[
            "KIANA_HOME",
            "HOME",
            "KIANA_HARNESS_SCRIPT",
            "KIANA_PROVIDER",
            "KIANA_FAKE_PROVIDER_SCRIPT",
            "ANTHROPIC_API_KEY",
            "KIANA_OPENAI_API_KEY",
            "OPENAI_API_KEY",
        ]);
        let home = unique_dir("home");
        let root = unique_dir("fixture");
        fs::create_dir_all(root.join(".git")).unwrap();
        let script = unique_dir("script").join("script.json");
        fs::write(
            &script,
            r#"[{"text":"writing","tool_calls":[{"id":"c1","name":"apply_patch","arguments":{"patch":"*** Begin Patch\n*** Add File: GOLDEN_PATH.txt\n+hello\n*** End Patch\n"}}]},{"text":"created GOLDEN_PATH.txt"}]"#,
        )
        .unwrap();
        env.set_var("KIANA_HOME", &home);
        env.set_var("HOME", &home);
        env.set_var("KIANA_HARNESS_SCRIPT", &script);
        env.set_var("KIANA_PROVIDER", "");
        std::env::remove_var("KIANA_PROVIDER");
        std::env::remove_var("KIANA_FAKE_PROVIDER_SCRIPT");
        std::env::remove_var("ANTHROPIC_API_KEY");
        std::env::remove_var("KIANA_OPENAI_API_KEY");
        std::env::remove_var("OPENAI_API_KEY");
        write_project_trust(&root, ProjectTrust::Trusted).unwrap();

        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let host = Arc::new(DaemonHost::with_env_harness().unwrap());
        let app = WebApp::new(
            host,
            root.clone(),
            "workspace-write".to_owned(),
            ROLE_BUILDER.to_owned(),
            addr,
        );
        tokio::spawn(async move {
            axum::serve(listener, router(app)).await.unwrap();
        });

        let client = reqwest::Client::new();
        let health: Value = client
            .get(format!("http://{addr}/api/health"))
            .send()
            .await
            .unwrap()
            .json()
            .await
            .unwrap();
        assert_eq!(health["ok"], true);
        assert_eq!(health["harness"], "kiana-harness");
        assert_eq!(health["streaming"], true);
        assert_eq!(health["streaming_transport"], "sse");

        let page = client
            .get(format!("http://{addr}/"))
            .send()
            .await
            .unwrap()
            .text()
            .await
            .unwrap();
        assert!(page.contains("Kiana"));
        assert!(page.contains("DaemonHost"));
        assert!(page.contains("信任此文件夹"));
        assert!(page.contains("第一次用") || page.contains("Ask Kiana"));
        assert!(page.contains("thread") || page.contains("Threads"));
        let token = page
            .split("window.__KIANA_WEB_TOKEN__ = '")
            .nth(1)
            .and_then(|value| value.split('\'').next())
            .expect("web token")
            .to_owned();
        let auth = |request: reqwest::RequestBuilder| request.header("x-kiana-web-token", &token);

        let run: Value = auth(
            client
                .post(format!("http://{addr}/api/run"))
                .json(&json!({"prompt":"create GOLDEN_PATH.txt containing hello"})),
        )
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
        assert_eq!(run["harness"], "kiana-harness");
        assert_eq!(run["running"], false);
        assert_eq!(run["response"]["status"], "completed");
        let golden = fs::read_to_string(root.join("GOLDEN_PATH.txt")).unwrap();
        assert_eq!(golden.trim(), "hello");

        let denied = parse_bind("192.168.1.8:3080").unwrap_err().to_string();
        assert_eq!(denied, "bind_loopback_only");
    }
}

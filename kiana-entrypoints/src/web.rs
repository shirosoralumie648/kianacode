//! 基于 loopback HTTP 的 Web Workbench 入口。
//!
//! Web 服务器只提供同一个 [`DaemonHost`] 的本地展示与命令路由：每个 session 的 run、
//! continue、cancel 和 receipt 均复用 `harness_run`。它不自行执行工具、不创建额外模型
//! 循环，也不把浏览器内存状态升级为执行事实；正式授权、信任、审批和收据仍在产品脊柱中。
//!
//! 服务强制绑定 loopback，并对每次 API 状态/变更请求检查进程启动时随机生成的 token、
//! Host 以及可选 Origin；`/api/events` 额外提供只读 SSE 增量投影。delta 只用于展示，
//! 终态和事实仍以响应/Receipt 为准。这些防护用于本地 UI 暴露面，不能替代系统级网络、
//! 浏览器或项目资源信任边界。

use anyhow::{anyhow, Context, Result};
use axum::extract::{DefaultBodyLimit, Query, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::sse::{Event, KeepAlive, Sse};
use axum::response::{Html, IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use futures_util::stream;
use futures_util::Stream;
use kiana_daemon::DaemonHost;
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
    // 因此只作为展示快照返回，不用于授权结论。
    fn snapshot(&self, session_id: &str) -> Result<Value, ApiError> {
        let trusted = harness_run::project_trusted(&self.workdir.to_string_lossy())
            .map_err(|error| ApiError::fail(error.to_string()))?;
        let sandbox = lock_string(&self.sandbox)?;
        let role_id = lock_string(&self.role)?;
        let role = RoleSpec::lookup(&role_id).ok_or_else(|| ApiError::bad("role_unknown"))?;
        let sessions = self
            .sessions
            .lock()
            .map_err(|_| ApiError::fail("web_state_poisoned"))?;
        let current = sessions
            .get(session_id)
            .cloned()
            .ok_or_else(|| ApiError::bad("session_unknown"))?;
        let threads: Vec<ThreadView> = sessions
            .iter()
            .map(|(id, session)| ThreadView {
                id: id.clone(),
                name: session.name.clone(),
                running: session.running,
                turns: session.turns.clone(),
            })
            .collect();
        Ok(json!({
            "harness": harness_run::HARNESS_ID,
            "folder": self.workdir.display().to_string(),
            "trusted": trusted,
            "sandbox": sandbox,
            "role": role.role_id,
            "department": role.department_id,
            "session_id": session_id,
            "running": current.running,
            "sessions": threads.iter().map(|thread| json!({
                "id": thread.id,
                "running": thread.running,
                "name": thread.name,
            })).collect::<Vec<_>>(),
            "threads": threads,
            "thread": threads.iter().find(|thread| thread.id == session_id),
            "last": current.last,
            "streaming": true,
            "streaming_transport": "sse",
            "shape": "codex-app",
        }))
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
    let session_id = resolve_session(&app, query.session_id.as_deref())?;
    Ok(Json(app.snapshot(&session_id)?))
}

struct EventStreamState {
    subscription: kiana_daemon::RunStreamSubscription,
    done: bool,
}

async fn events(
    State(app): State<Arc<WebApp>>,
    headers: HeaderMap,
    Query(query): Query<EventsQuery>,
) -> Result<Sse<impl Stream<Item = Result<Event, Infallible>> + Send + 'static>, ApiError> {
    authorize_sse(&app, &headers, query.token.as_deref())?;
    let session_id = resolve_session(&app, query.session_id.as_deref())?;
    let run_id = stream_run_id(&app, &session_id)?;
    // Subscribe before returning the SSE response headers. The browser waits for
    // EventSource.onopen before issuing /api/run, so the first delta is not lost.
    let subscription = app.host.subscribe_run(run_id);
    let stream = stream::unfold(
        EventStreamState {
            subscription,
            done: false,
        },
        |mut state| async move {
            if state.done {
                return None;
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

fn run_stream_sse_event(name: &str, envelope: &RunStreamEnvelope) -> Event {
    let data = serde_json::to_string(envelope).unwrap_or_else(|error| {
        json!({ "error": format!("stream_serialize_failed:{error}") }).to_string()
    });
    Event::default().event(name).data(data)
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
    let session_id = resolve_session(&app, body.session_id.as_deref())?;
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
            let mut payload = app.snapshot(&session_id)?;
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
    let session_id = resolve_session(&app, body.session_id.as_deref())?;
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
    Ok(Json(app.snapshot(&session_id)?))
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
    let session_id = resolve_session(&app, requested)?;
    write_project_trust(&app.workdir, ProjectTrust::Trusted).map_err(ApiError::fail)?;
    Ok(Json(app.snapshot(&session_id)?))
}

async fn set_sandbox(
    State(app): State<Arc<WebApp>>,
    headers: HeaderMap,
    Json(body): Json<SandboxBody>,
) -> Result<Json<Value>, ApiError> {
    authorize_mutation(&app, &headers)?;
    let session_id = resolve_session(&app, body.session_id.as_deref())?;
    let sandbox = workbench_chat::normalize_sandbox(&body.sandbox)
        .map_err(|error| ApiError::bad(error.to_string()))?;
    *app.sandbox
        .lock()
        .map_err(|_| ApiError::fail("web_state_poisoned"))? = sandbox;
    Ok(Json(app.snapshot(&session_id)?))
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
    Ok(Json(app.snapshot(&session_id)?))
}

async fn read_receipt(
    State(app): State<Arc<WebApp>>,
    headers: HeaderMap,
    Json(body): Json<SessionBody>,
) -> Result<Json<Value>, ApiError> {
    authorize_mutation(&app, &headers)?;
    let session_id = resolve_session(&app, body.session_id.as_deref())?;
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
        "state": app.snapshot(&session_id)?,
        "receipt": response,
    })))
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

fn stream_run_id(app: &WebApp, session_id: &str) -> Result<RunId, ApiError> {
    let sessions = app
        .sessions
        .lock()
        .map_err(|_| ApiError::fail("web_state_poisoned"))?;
    let session = sessions
        .get(session_id)
        .ok_or_else(|| ApiError::bad("session_unknown"))?;
    session
        .run_id
        .or_else(|| RunId::parse_str(session_id))
        .ok_or_else(|| ApiError::bad("web_stream_run_id_unavailable"))
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

fn resolve_session(app: &WebApp, requested: Option<&str>) -> Result<String, ApiError> {
    let id = match requested.map(str::trim).filter(|value| !value.is_empty()) {
        Some(id) => id.to_owned(),
        None => lock_string(&app.active)?,
    };
    let sessions = app
        .sessions
        .lock()
        .map_err(|_| ApiError::fail("web_state_poisoned"))?;
    if !sessions.contains_key(&id) {
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

    #[test]
    fn explicit_session_resolution_does_not_change_global_active_thread() {
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

        assert_eq!(resolve_session(&app, Some(&other)).unwrap(), other);
        assert_eq!(lock_string(&app.active).unwrap(), active);
        let snapshot = app.snapshot(&other).unwrap();
        assert_eq!(snapshot["session_id"].as_str(), Some(other.as_str()));
        assert_eq!(snapshot["thread"]["id"].as_str(), Some(other.as_str()));
        assert_eq!(lock_string(&app.active).unwrap(), active);
        let _ = fs::remove_dir_all(root);
        let _ = fs::remove_dir_all(home);
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

//!
//! # 与 `mcp.rs` 的正面对照（本文件最值得先读的一段）
//!
//! 两个文件都把一个 HTTP 服务暴露在本机，防护程度却完全不同：
//!
//! | | 本文件（Web） | `mcp.rs`（MCP HTTP） |
//! |---|---|---|
//! | 绑定地址 | `ensure_loopback` **硬性拒绝**非 loopback | 取配置里的 host，没有拒绝逻辑 |
//! | 每次请求的凭据 | 进程内随机 token，逐请求核对 | 无 |
//! | Host 头 | 必须匹配绑定地址 | 无 |
//! | Origin 头 | 若存在则必须匹配 | 无 |
//! | 工具执行 | 经 `harness_run` 到 `ControlPlane` | 直接 `execute_tool_call` |
//!
//! 差别不在「两个都是本地服务」，而在于**每一个外部可控的输入都有一个对应的检查**。
//! `mcp.rs` 缺的不只是某一个检查，而是这一整列。
//! 那条路径已作为安全发现记录在
//! [安全发现文档](../../docs/security/mcp-http-unauthenticated-tool-execution.md)。
//!
//! # 但要说清楚：这些检查防不住什么
//!
//! 文件头那句「不能替代系统级网络、浏览器或项目资源信任边界」是准确的，展开说是：
//!
//! - **防不住本机进程**。token 在内存里、在首页里；同机的任何进程都能读到。
//!   这三道检查防的是**浏览器里的恶意页面**，不是本机的另一个用户。
//! - **防不住 loopback 本身**。127.0.0.1 不是授权边界，它只是「本机」这一事实的表示。
//! - **防不住已经被授权的操作**。token 证明「这个请求来自本 Web UI」，
//!   不证明「这个操作应该被允许」——后者是控制面的事。
//!
//! 换句话说：这一层是**暴露面收敛**，不是授权。授权始终在 `ControlPlane`。
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
use axum::body::Bytes;
use axum::extract::{DefaultBodyLimit, Query, Request, State};
use axum::http::{HeaderMap, StatusCode};
use axum::middleware::{self, Next};
use axum::response::sse::{Event, KeepAlive, Sse};
use axum::response::{Html, IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use futures_util::stream;
use futures_util::Stream;
use kiana_daemon::{DaemonHost, StreamingRedactor};
use kiana_protocol::{
    EntryPointKind, ResponseEnvelope, RoleSpec, RunId, RunStreamEnvelope, RunStreamEvent,
    SignalStatus, UiAction, UiCursor, UiFeedCursorV1, UiFeedFrameV1, UiInstanceRecord,
    PROTOCOL_SCHEMA, ROLE_BUILDER,
};
use kiana_types::{write_project_trust, ProjectTrust};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::HashMap;
use std::convert::Infallible;
use std::io::{self, Write};
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
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
const MAX_WEB_URI_BYTES: usize = 8 * 1024;
const MAX_WEB_REQUESTS_PER_WINDOW: u32 = 120;
const WEB_RATE_WINDOW: Duration = Duration::from_secs(1);
const STREAM_GAP_RUN_IN_PROGRESS: &str = "subscription_attached_after_run_started";
const WEB_HYDRATE_SCHEMA: &str = "kiana.web-hydrate.v1";
const WEB_HISTORY_SCHEMA: &str = "kiana.web-history-page.v1";
const WEB_ARTIFACT_SCHEMA: &str = "kiana.web-artifact-page.v1";
const WEB_ARTIFACT_DETAIL_SCHEMA: &str = "kiana.web-artifact-detail.v1";
const WEB_DIFF_DETAIL_SCHEMA: &str = "kiana.web-diff-detail.v1";
const WEB_RECEIPT_DETAIL_SCHEMA: &str = "kiana.web-receipt-detail.v1";
const WEB_DETAIL_SCOPE_SCHEMA: &str = "kiana.ui-detail-scope.v1";
const WEB_DETAIL_LINK_SCHEMA: &str = "kiana.ui-detail-link.v1";
const WEB_PAGE_CURSOR_SCHEMA: &str = "kiana.web-page-cursor.v1";
/// Additive Web lease/coordination contracts.  The authenticated daemon principal remains the
/// authority; these values only fence tab-local mutable state and replay.
pub const WEB_TAB_SESSION_SCHEMA: &str = "kiana.ui-tab-session.v1";
pub const WEB_ACTION_SUBMISSION_SCHEMA: &str = "kiana.ui-action-submission.v1";
const MAX_WEB_HISTORY_PAGE: usize = 64;
const MAX_WEB_PAGE_CURSORS: usize = 256;
const MAX_WEB_PAGE_CURSOR_BYTES: usize = 256;
const MAX_WEB_TAB_BYTES: usize = 128;
/// Browser hydrate trace contract.
///
/// The trace is a bounded, credential-free record of the canonical bootstrap order: the first
/// read is a server-owned snapshot, the history page follows that snapshot, and the display feed
/// is installed only afterwards. It is descriptive: it creates no lease, holds no payload bytes
/// and grants no authority.
pub const WEB_HYDRATE_TRACE_SCHEMA: &str = "kiana.web-hydrate-trace.v1";
const MAX_WEB_HYDRATE_TRACE_STEPS: usize = 4;
/// Schema for transport-only SSE control events.  Run deltas retain the versioned
/// `kiana.protocol.v1` envelope; heartbeat/gap/error frames use this additive wrapper.
pub const WEB_SSE_SCHEMA: &str = "kiana.web-sse.v1";
/// SSE is a disposable display projection.  Keep each encoded frame bounded even when a
/// provider or a malformed cassette attempts to send an unexpectedly large delta.
pub const MAX_WEB_SSE_EVENT_BYTES: usize = 256 * 1024;
const MAX_WEB_SSE_REASON_BYTES: usize = 512;
/// The browser supplies a Last-Event-ID header on a native reconnect.  The query fallback is
/// needed for the loopback EventSource wrapper, but both forms must agree when both are present.
const MAX_WEB_SSE_CURSOR_BYTES: usize = 256;
const WEB_SSE_HEARTBEAT_INTERVAL: Duration = Duration::from_secs(15);
const WEB_NOTIFICATION_SSE_HEARTBEAT_INTERVAL: Duration = Duration::from_secs(15);
const MAX_WEB_ACTION_SUBMISSIONS: usize = 512;
const WEB_ACTION_SUBMISSION_TTL: Duration = Duration::from_secs(15 * 60);

/// The stable route inventory used by the Web entrypoint and its source/CI guards.
///
/// Every API route is either host-only (`health`/`csp-report`) or token protected.  The root page is
/// deliberately host-only so a fresh browser can obtain the in-memory token; it never exposes
/// runtime state.  The inventory is descriptive and does not create a second dispatch path.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum WebRouteClass {
    Health,
    Bootstrap,
    State,
    Sessions,
    History,
    Artifact,
    ArtifactDetail,
    Diff,
    Events,
    Notifications,
    NotificationEvents,
    Run,
    Cancel,
    Trust,
    Sandbox,
    Session,
    Receipt,
    ReceiptDetail,
    Approval,
    Resume,
    Steer,
    Inject,
    Command,
    Token,
    Diagnostics,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct WebRouteContract {
    pub path: &'static str,
    pub method: &'static str,
    pub class: WebRouteClass,
    pub token_required: bool,
}

/// Canonical route/auth matrix.  `OPTIONS` is intentionally not listed: Axum's default method
/// rejection remains deny-first and never reaches a mutating handler.
pub const WEB_ROUTE_MATRIX: &[WebRouteContract] = &[
    WebRouteContract {
        path: "/api/health",
        method: "GET",
        class: WebRouteClass::Health,
        token_required: false,
    },
    WebRouteContract {
        path: "/api/csp-report",
        method: "POST",
        class: WebRouteClass::Diagnostics,
        token_required: false,
    },
    WebRouteContract {
        path: "/api/bootstrap",
        method: "GET",
        class: WebRouteClass::Bootstrap,
        token_required: true,
    },
    WebRouteContract {
        path: "/api/state",
        method: "GET",
        class: WebRouteClass::State,
        token_required: true,
    },
    WebRouteContract {
        path: "/api/sessions",
        method: "GET",
        class: WebRouteClass::Sessions,
        token_required: true,
    },
    WebRouteContract {
        path: "/api/history",
        method: "GET",
        class: WebRouteClass::History,
        token_required: true,
    },
    WebRouteContract {
        path: "/api/artifact",
        method: "GET",
        class: WebRouteClass::Artifact,
        token_required: true,
    },
    WebRouteContract {
        path: "/api/artifact/detail",
        method: "GET",
        class: WebRouteClass::ArtifactDetail,
        token_required: true,
    },
    WebRouteContract {
        path: "/api/diff",
        method: "GET",
        class: WebRouteClass::Diff,
        token_required: true,
    },
    WebRouteContract {
        path: "/api/events",
        method: "GET",
        class: WebRouteClass::Events,
        token_required: true,
    },
    WebRouteContract {
        path: "/api/notifications",
        method: "GET",
        class: WebRouteClass::Notifications,
        token_required: true,
    },
    WebRouteContract {
        path: "/api/notifications/events",
        method: "GET",
        class: WebRouteClass::NotificationEvents,
        token_required: true,
    },
    WebRouteContract {
        path: "/api/run",
        method: "POST",
        class: WebRouteClass::Run,
        token_required: true,
    },
    WebRouteContract {
        path: "/api/cancel",
        method: "POST",
        class: WebRouteClass::Cancel,
        token_required: true,
    },
    WebRouteContract {
        path: "/api/trust",
        method: "POST",
        class: WebRouteClass::Trust,
        token_required: true,
    },
    WebRouteContract {
        path: "/api/sandbox",
        method: "POST",
        class: WebRouteClass::Sandbox,
        token_required: true,
    },
    WebRouteContract {
        path: "/api/session",
        method: "POST",
        class: WebRouteClass::Session,
        token_required: true,
    },
    WebRouteContract {
        path: "/api/receipt",
        method: "POST",
        class: WebRouteClass::Receipt,
        token_required: true,
    },
    WebRouteContract {
        path: "/api/receipt/detail",
        method: "GET",
        class: WebRouteClass::ReceiptDetail,
        token_required: true,
    },
    WebRouteContract {
        path: "/api/approvals",
        method: "GET/POST",
        class: WebRouteClass::Approval,
        token_required: true,
    },
    WebRouteContract {
        path: "/api/resume",
        method: "POST",
        class: WebRouteClass::Resume,
        token_required: true,
    },
    WebRouteContract {
        path: "/api/steer",
        method: "POST",
        class: WebRouteClass::Steer,
        token_required: true,
    },
    WebRouteContract {
        path: "/api/inject",
        method: "POST",
        class: WebRouteClass::Inject,
        token_required: true,
    },
    WebRouteContract {
        path: "/api/command",
        method: "GET/POST",
        class: WebRouteClass::Command,
        token_required: true,
    },
    WebRouteContract {
        path: "/api/parity",
        method: "GET",
        class: WebRouteClass::Diagnostics,
        token_required: true,
    },
    WebRouteContract {
        path: "/api/company-governance",
        method: "GET",
        class: WebRouteClass::Diagnostics,
        token_required: true,
    },
    WebRouteContract {
        path: "/api/extensions",
        method: "GET",
        class: WebRouteClass::Diagnostics,
        token_required: true,
    },
    WebRouteContract {
        path: "/api/token/rotate",
        method: "POST",
        class: WebRouteClass::Token,
        token_required: true,
    },
];

/// The single credential header for the loopback Web surface.  The SSE query fallback exists only
/// because `EventSource` cannot set request headers; both forms are compared when both are sent.
pub const WEB_TOKEN_HEADER: &str = "x-kiana-web-token";

/// Stable transport/auth/limit error catalogue for the Web surface.
///
/// Every code here is produced before (or without) a `DaemonHost`/`ControlPlane` call, so the
/// catalogue describes the UI-16 exposure boundary rather than product authorization.  Handler
/// failures that forward an internal error string are deliberately not listed: they are
/// server-owned text, not a stable contract.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct WebErrorContract {
    pub code: &'static str,
    /// HTTP status this code is returned with.
    pub status: u16,
    /// Coarse class used to keep the status mapping mechanical.
    pub class: &'static str,
}

/// Security response headers attached to every Web response, including middleware rejections.
/// This is the reviewed draft of the UI-16 "security header/CSP" artifact.
pub const WEB_SECURITY_HEADERS: &[(&str, &str)] = &[
    ("x-content-type-options", "nosniff"),
    ("cache-control", "no-store"),
    ("referrer-policy", "no-referrer"),
    ("x-frame-options", "DENY"),
];

/// Content-Security-Policy draft template sharing one per-process nonce with the embedded page.
/// The page carries the matching nonce on its static style/script blocks; there is no
/// `unsafe-inline`, `eval` or inline event handler.
pub const WEB_CSP_TEMPLATE: &str = "default-src 'self'; script-src 'nonce-{nonce}'; style-src 'nonce-{nonce}'; connect-src 'self'; object-src 'none'; base-uri 'none'; form-action 'self'; frame-ancestors 'none'; report-uri /api/csp-report";

/// Payload schema of the token-rotation route.  Rotation never echoes the credential: the page
/// must re-hydrate from the host-only root document to obtain the new in-memory token.
pub const WEB_TOKEN_ROTATION_SCHEMA: &str = "kiana.web-token-rotation.v1";

/// `class` -> HTTP status mapping for [`WEB_ERROR_MATRIX`].
pub fn web_error_status_for_class(class: &str) -> Option<u16> {
    match class {
        "request" => Some(400),
        "deny" => Some(401),
        "conflict" => Some(409),
        "limit" => Some(429),
        "internal" => Some(500),
        "unavailable" => Some(503),
        _ => None,
    }
}

/// Deny-first/protocol error catalogue bound to the source literals by the UI-16 guard.
pub const WEB_ERROR_MATRIX: &[WebErrorContract] = &[
    WebErrorContract {
        code: "artifact_not_found",
        status: 400,
        class: "request",
    },
    WebErrorContract {
        code: "command_arguments_invalid",
        status: 400,
        class: "request",
    },
    WebErrorContract {
        code: "command_arguments_limit",
        status: 400,
        class: "request",
    },
    WebErrorContract {
        code: "command_read_only_required",
        status: 400,
        class: "request",
    },
    WebErrorContract {
        code: "command_request_invalid",
        status: 400,
        class: "request",
    },
    WebErrorContract {
        code: "company_governance_project_invalid",
        status: 400,
        class: "request",
    },
    WebErrorContract {
        code: "extension_visibility_action_unsupported",
        status: 400,
        class: "request",
    },
    WebErrorContract {
        code: "extension_visibility_max_results_invalid",
        status: 400,
        class: "request",
    },
    WebErrorContract {
        code: "extension_visibility_surface_invalid",
        status: 400,
        class: "request",
    },
    WebErrorContract {
        code: "parity_entrypoint_invalid",
        status: 400,
        class: "request",
    },
    WebErrorContract {
        code: "parity_run_id_invalid",
        status: 400,
        class: "request",
    },
    WebErrorContract {
        code: "prompt_required",
        status: 400,
        class: "request",
    },
    WebErrorContract {
        code: "prompt_too_large",
        status: 400,
        class: "request",
    },
    WebErrorContract {
        code: "receipt_not_found",
        status: 400,
        class: "request",
    },
    WebErrorContract {
        code: "role_unknown",
        status: 400,
        class: "request",
    },
    WebErrorContract {
        code: "session_invalid",
        status: 400,
        class: "request",
    },
    WebErrorContract {
        code: "session_unknown",
        status: 400,
        class: "request",
    },
    WebErrorContract {
        code: "stream_cursor_conflict",
        status: 400,
        class: "request",
    },
    WebErrorContract {
        code: "stream_cursor_invalid",
        status: 400,
        class: "request",
    },
    WebErrorContract {
        code: "ui_action_invalid",
        status: 400,
        class: "request",
    },
    WebErrorContract {
        code: "ui_action_required",
        status: 400,
        class: "request",
    },
    WebErrorContract {
        code: "ui_cursor_invalid",
        status: 400,
        class: "request",
    },
    WebErrorContract {
        code: "web_input_text_required",
        status: 400,
        class: "request",
    },
    WebErrorContract {
        code: "web_notification_cursor_conflict",
        status: 400,
        class: "request",
    },
    WebErrorContract {
        code: "web_notification_cursor_invalid",
        status: 400,
        class: "request",
    },
    WebErrorContract {
        code: "web_notification_page_limit_invalid",
        status: 400,
        class: "request",
    },
    WebErrorContract {
        code: "web_page_cursor_invalid",
        status: 400,
        class: "request",
    },
    WebErrorContract {
        code: "web_page_cursor_limit_mismatch",
        status: 400,
        class: "request",
    },
    WebErrorContract {
        code: "web_page_cursor_offset_invalid",
        status: 400,
        class: "request",
    },
    WebErrorContract {
        code: "web_page_limit_invalid",
        status: 400,
        class: "request",
    },
    WebErrorContract {
        code: "web_path_traversal_denied",
        status: 400,
        class: "request",
    },
    WebErrorContract {
        code: "web_receipt_cursor_not_supported",
        status: 400,
        class: "request",
    },
    WebErrorContract {
        code: "web_request_target_too_large",
        status: 400,
        class: "request",
    },
    WebErrorContract {
        code: "web_run_id_invalid",
        status: 400,
        class: "request",
    },
    WebErrorContract {
        code: "web_stream_run_id_unavailable",
        status: 400,
        class: "request",
    },
    WebErrorContract {
        code: "web_tab_invalid",
        status: 400,
        class: "request",
    },
    WebErrorContract {
        code: "web_tab_required",
        status: 400,
        class: "request",
    },
    WebErrorContract {
        code: "web_turn_id_invalid",
        status: 400,
        class: "request",
    },
    WebErrorContract {
        code: "web_auth_required",
        status: 401,
        class: "deny",
    },
    WebErrorContract {
        code: "session_busy",
        status: 409,
        class: "conflict",
    },
    WebErrorContract {
        code: "session_owner_required",
        status: 409,
        class: "conflict",
    },
    WebErrorContract {
        code: "session_principal_mismatch",
        status: 409,
        class: "conflict",
    },
    WebErrorContract {
        code: "session_read_only",
        status: 409,
        class: "conflict",
    },
    WebErrorContract {
        code: "ui_action_in_flight",
        status: 409,
        class: "conflict",
    },
    WebErrorContract {
        code: "ui_action_owner_mismatch",
        status: 409,
        class: "conflict",
    },
    WebErrorContract {
        code: "ui_action_submission_expired",
        status: 409,
        class: "conflict",
    },
    WebErrorContract {
        code: "ui_action_target_mismatch",
        status: 409,
        class: "conflict",
    },
    WebErrorContract {
        code: "web_page_cursor_scope_mismatch",
        status: 409,
        class: "conflict",
    },
    WebErrorContract {
        code: "web_page_source_changed",
        status: 409,
        class: "conflict",
    },
    WebErrorContract {
        code: "web_session_limit_exceeded",
        status: 409,
        class: "conflict",
    },
    WebErrorContract {
        code: "web_action_state_unavailable",
        status: 500,
        class: "internal",
    },
    WebErrorContract {
        code: "web_human_action_response_encode",
        status: 500,
        class: "internal",
    },
    WebErrorContract {
        code: "web_human_response_encode",
        status: 500,
        class: "internal",
    },
    WebErrorContract {
        code: "web_hydrate_missing",
        status: 500,
        class: "internal",
    },
    WebErrorContract {
        code: "web_notification_page_encode_failed",
        status: 500,
        class: "internal",
    },
    WebErrorContract {
        code: "web_page_cursor_state_unavailable",
        status: 500,
        class: "internal",
    },
    WebErrorContract {
        code: "web_page_epoch_unavailable",
        status: 500,
        class: "internal",
    },
    WebErrorContract {
        code: "web_rate_state_unavailable",
        status: 500,
        class: "internal",
    },
    WebErrorContract {
        code: "web_session_history_unsupported",
        status: 500,
        class: "internal",
    },
    WebErrorContract {
        code: "web_state_poisoned",
        status: 500,
        class: "internal",
    },
    WebErrorContract {
        code: "web_token_state_unavailable",
        status: 500,
        class: "internal",
    },
    WebErrorContract {
        code: "web_rate_limit_exceeded",
        status: 429,
        class: "limit",
    },
    WebErrorContract {
        code: "web_shutting_down",
        status: 503,
        class: "unavailable",
    },
];

#[derive(Debug)]
struct WebRateWindow {
    started: Instant,
    requests: u32,
}

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
    web_token: Arc<Mutex<String>>,
    /// Per-process nonce shared by the embedded page and the response CSP.  The nonce is not an
    /// authorization value and is never exposed through API projections.
    csp_nonce: Arc<String>,
    token_generation: Arc<std::sync::atomic::AtomicU64>,
    bound_addr: SocketAddr,
    desktop_attach: Option<DesktopAttachIdentity>,
    rate_window: Arc<Mutex<WebRateWindow>>,
    page_cursors: Arc<Mutex<HashMap<String, WebPageCursor>>>,
    consumed_page_cursors: Arc<Mutex<HashMap<String, Instant>>>,
    action_submissions: Arc<Mutex<HashMap<String, WebActionSubmission>>>,
    shutting_down: Arc<AtomicBool>,
    shutdown: tokio::sync::watch::Sender<bool>,
}

#[derive(Clone)]
struct DesktopAttachIdentity {
    instance_id: String,
    authority_epoch: u64,
    workspace_digest: String,
    record_digest: String,
}

impl From<&UiInstanceRecord> for DesktopAttachIdentity {
    fn from(record: &UiInstanceRecord) -> Self {
        Self {
            instance_id: record.instance_id.clone(),
            authority_epoch: record.authority_epoch,
            workspace_digest: record.workspace_digest.clone(),
            record_digest: record.record_digest.clone(),
        }
    }
}

#[derive(Clone, Debug)]
struct WebSession {
    run_id: Option<RunId>,
    running: bool,
    name: String,
    last: Option<TurnSummary>,
    turns: Vec<TurnView>,
    owner_principal_id: String,
    owner_tab_id: Option<String>,
    lease_epoch: u64,
    owner_active: bool,
}

#[derive(Clone, Debug)]
struct WebActionSubmission {
    schema: &'static str,
    session_id: String,
    tab_id: String,
    principal_id: String,
    target_id: String,
    created_at: Instant,
    response: Option<Value>,
}

#[derive(Clone, Debug)]
enum WebActionClaim {
    New { id: String },
    Replay(Value),
}

#[derive(Clone, Debug)]
struct WebPageCursor {
    schema: &'static str,
    kind: &'static str,
    token: String,
    session_id: String,
    tab_id: String,
    instance_id: String,
    epoch: String,
    source_cursor: u64,
    offset: usize,
    limit: usize,
    issued_at: Instant,
}

#[derive(Clone, Debug, Deserialize)]
struct HistoryQuery {
    session_id: String,
    #[serde(default)]
    after: Option<String>,
    #[serde(default = "default_web_history_page")]
    limit: usize,
}

#[derive(Clone, Debug, Deserialize)]
struct ArtifactQuery {
    session_id: String,
    #[serde(default)]
    artifact_id: Option<String>,
    #[serde(default)]
    after: Option<String>,
    #[serde(default = "default_web_history_page")]
    limit: usize,
}

#[derive(Clone, Debug, Deserialize)]
struct ArtifactDetailQuery {
    session_id: String,
    artifact_id: String,
    #[serde(default)]
    after: Option<String>,
    #[serde(default = "default_web_history_page")]
    limit: usize,
}

#[derive(Clone, Debug, Deserialize)]
struct DiffQuery {
    session_id: String,
    artifact_id: String,
    #[serde(default)]
    after: Option<String>,
    #[serde(default = "default_web_history_page")]
    limit: usize,
}

#[derive(Clone, Debug, Deserialize)]
struct ReceiptDetailQuery {
    session_id: String,
    #[serde(default)]
    after: Option<String>,
    #[serde(default = "default_web_history_page")]
    limit: usize,
}

fn default_web_history_page() -> usize {
    32
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
    sandbox: Option<String>,
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
    #[serde(default)]
    last_event_id: Option<String>,
    #[serde(default)]
    tab_id: Option<String>,
}

#[derive(Clone, Debug, Deserialize)]
struct NotificationQuery {
    session_id: String,
    #[serde(default)]
    after_item_id: Option<String>,
    #[serde(default = "default_web_notification_page")]
    limit: u16,
    #[serde(default)]
    expected_source_cursor: Option<u64>,
}

#[derive(Deserialize)]
struct NotificationEventsQuery {
    #[serde(default)]
    session_id: Option<String>,
    #[serde(default)]
    token: Option<String>,
    #[serde(default)]
    last_event_id: Option<String>,
    #[serde(default)]
    tab_id: Option<String>,
}

fn default_web_notification_page() -> u16 {
    30
}

#[derive(Clone, Debug, Deserialize)]
struct ParityQuery {
    #[serde(default)]
    session_id: Option<String>,
    #[serde(default)]
    run_id: Option<String>,
    #[serde(default)]
    entrypoint: Option<String>,
}

#[derive(Clone, Debug, Deserialize)]
struct CompanyGovernanceQuery {
    project_id: String,
    #[serde(default)]
    session_id: Option<String>,
}

#[derive(Deserialize)]
struct SandboxBody {
    sandbox: String,
    #[serde(default)]
    session_id: Option<String>,
}

#[derive(Deserialize)]
struct ApprovalBody {
    session_id: String,
    challenge: kiana_protocol::ApprovalChallenge,
    decision: kiana_protocol::ApprovalDecision,
}

#[derive(Deserialize)]
struct CommandBody {
    session_id: String,
    name: String,
    #[serde(default)]
    arguments: Value,
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

fn notification_api_error(error: String) -> ApiError {
    if error.contains("unavailable") || error.contains("unsupported") {
        return ApiError {
            status: StatusCode::SERVICE_UNAVAILABLE,
            error,
        };
    }
    if error.contains("cursor_stale") || error.contains("scope_mismatch") {
        return ApiError::conflict(error);
    }
    ApiError::fail(error)
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

/// **硬性拒绝**任何非 loopback 的绑定地址。
///
/// 【为什么必须是「拒绝」而不是「警告」】
/// 因为一旦绑到 `0.0.0.0`，这个服务就出现在网络上，而它承载的是一条能发命令的
/// 通路。到那时候再补救已经不是「改个配置」，而是「已经发生过的事」。
/// 拒绝把这件事挡在进程启动之前。
///
/// 【对照 `mcp.rs`】
/// 那边用 `KIANA_MCP_HTTP_HOST` 决定绑哪里，**没有对应的拒绝逻辑**——
/// 设成 `0.0.0.0` 就会真的绑上去。这是两条路径防护差异最直接的一处。
///
/// 【⚠ 它只管绑定，不管连上来的】
/// 绑在 loopback 之后，任何本机进程依然可以连。所以这一条不能被当成
/// 「本服务是安全的」的证明，它只把「不小心暴露到网络」这一类事故消掉了。
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
    // Every product surface resolves its storage root through the same daemon-owned resolver, so
    // Web observes the same store as CLI and Workbench. Resolution is metadata-only and fail
    // closed; it never authorizes a command or opens a second execution path.
    let (storage_root, storage_binding) = host
        .resolve_surface_storage_root(kiana_daemon::StorageSurface::Web, &workdir)
        .map_err(anyhow::Error::msg)?;
    let url = format!("http://{addr}");
    // Desktop launch opts into the workspace lease.  The lease is discovery metadata only;
    // requests still enter this DaemonHost and ControlPlane path.
    let desktop_nonce = std::env::var("KIANA_DESKTOP_READY_NONCE").ok();
    let instance_lease = if desktop_nonce.is_some() {
        Some(
            host.acquire_instance(&workdir, kiana_protocol::UiTransportKind::InProcess, &url)
                .map_err(anyhow::Error::msg)?,
        )
    } else {
        None
    };
    let app = WebApp::new(
        Arc::clone(&host),
        workdir.clone(),
        launch.sandbox.clone(),
        role,
        addr,
    )
    .with_optional_desktop_attach(instance_lease.as_ref().map(|lease| lease.record()));
    let trusted = harness_run::project_trusted(&workdir.to_string_lossy())?;
    println!("Kiana web");
    println!("folder: {}", workdir.display());
    println!("trusted: {}", if trusted { "yes" } else { "no" });
    println!("sandbox: {}", launch.sandbox);
    println!(
        "storage root: {} ({})",
        storage_root.canonical_path, storage_binding.root_id
    );
    println!("loopback only. SSE display stream; receipts remain authoritative.");
    if let Some(ready_nonce) = desktop_nonce {
        let record = instance_lease
            .as_ref()
            .expect("desktop lease was acquired when readiness nonce is present")
            .record();
        let ready = json!({
            "schema": "kiana.desktop-ready.v1",
            "protocol_schema": PROTOCOL_SCHEMA,
            "sidecar_schema": record.schema,
            "instance_id": record.instance_id,
            "authority_epoch": record.authority_epoch,
            "feed_instance_id": host.feed_instance_id(),
            "epoch": host.ui_cursor().epoch,
            "pid": std::process::id(),
            "workspace": workdir.to_string_lossy(),
            "workspace_digest": record.workspace_digest,
            "endpoint_digest": record.endpoint_digest,
            "record_digest": record.record_digest,
            "storage_surface": storage_binding.surface.as_str(),
            "storage_root_id": storage_binding.root_id.to_string(),
            "storage_binding_digest": storage_binding.binding_digest.clone(),
            "url": url,
            "nonce": ready_nonce,
        });
        println!("KIANA_DESKTOP_READY={}", ready);
    }
    println!("KIANA_WEB_URL={url}");
    println!("{url}");
    let _ = io::stdout().flush();
    if !launch.no_open {
        maybe_open(&url);
    }
    let shutdown_app = app.clone();
    axum::serve(listener, router(app))
        .with_graceful_shutdown(async move {
            wait_for_shutdown_signal().await;
            shutdown_app.shutting_down.store(true, Ordering::Release);
            let active = shutdown_app
                .sessions
                .lock()
                .map(|sessions| {
                    sessions
                        .iter()
                        .filter(|(_, session)| {
                            session.running
                                || session
                                    .last
                                    .as_ref()
                                    .is_some_and(|last| last.status == "awaiting_approval")
                        })
                        .map(|(id, session)| (id.clone(), session.run_id))
                        .collect::<Vec<_>>()
                })
                .unwrap_or_default();
            let stopping = async {
                for (session_id, run_id) in active {
                    if let Ok(options) = shutdown_app.options() {
                        let response = harness_run::cancel_envelope_on_host(
                            shutdown_app.host.clone(),
                            session_id,
                            run_id,
                            "web_shutdown",
                            &options,
                        )
                        .await;
                        if !response.is_ok_and(|response| {
                            response.status == kiana_protocol::ExecutionStatus::Cancelled
                        }) {
                            eprintln!("web_shutdown:worker_stop_unconfirmed; consult receipt");
                        }
                    }
                }
            };
            if tokio::time::timeout(std::time::Duration::from_secs(8), stopping)
                .await
                .is_err()
            {
                eprintln!("web_shutdown:worker_stop_timeout; consult receipt");
            }
            shutdown_app.shutdown.send_replace(true);
        })
        .await
        .context("web server stopped")?;
    Ok(())
}

async fn wait_for_shutdown_signal() {
    #[cfg(unix)]
    if let Ok(mut terminate) =
        tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
    {
        tokio::select! { _ = terminate.recv() => {}, _ = tokio::signal::ctrl_c() => {} }
        return;
    }
    let _ = tokio::signal::ctrl_c().await;
}

fn router(app: WebApp) -> Router {
    let shared_state = Arc::new(app);
    Router::new()
        .route("/", get(index))
        .route("/api/health", get(health))
        .route("/api/csp-report", post(csp_report))
        .route("/api/bootstrap", get(bootstrap))
        .route("/api/parity", get(parity))
        .route("/api/company-governance", get(company_governance))
        .route("/api/state", get(state))
        .route("/api/sessions", get(list_sessions))
        .route("/api/history", get(history))
        .route("/api/artifact", get(artifact))
        .route("/api/artifact/detail", get(artifact_detail))
        .route("/api/diff", get(diff_detail))
        .route("/api/events", get(events))
        .route("/api/notifications", get(notifications))
        .route("/api/notifications/events", get(notification_events))
        .route("/api/run", post(run_turn))
        .route("/api/cancel", post(cancel_turn))
        .route("/api/trust", post(trust_folder))
        .route("/api/sandbox", post(set_sandbox))
        .route("/api/session", post(new_session))
        .route("/api/receipt", post(read_receipt))
        .route("/api/receipt/detail", get(receipt_detail))
        .route("/api/approvals", get(list_approvals).post(decide_approval))
        .route("/api/resume", post(resume_turn))
        .route("/api/steer", post(steer_turn))
        .route("/api/inject", post(inject_turn))
        .route("/api/extensions", get(extension_visibility))
        .route("/api/command", get(command_query).post(command_action))
        .route("/api/token/rotate", post(rotate_web_token_route))
        .layer(DefaultBodyLimit::max(MAX_WEB_BODY_BYTES))
        .layer(middleware::from_fn_with_state(
            Arc::clone(&shared_state),
            enforce_request_bounds,
        ))
        // Keep response hardening outermost so URI/rate/body rejection responses carry the
        // same security headers as handler responses.
        .layer(middleware::from_fn_with_state(
            Arc::clone(&shared_state),
            security_headers,
        ))
        .with_state(shared_state)
}

/// Reject unbounded request targets before a query extractor or a mutating handler runs.
///
/// Body bytes are bounded by [`DefaultBodyLimit`].  This guard covers the URI/query side and
/// rejects encoded path traversal rather than trying to normalize it inside an endpoint.
async fn enforce_request_bounds(
    State(app): State<Arc<WebApp>>,
    request: Request,
    next: Next,
) -> Response {
    let target = request.uri().to_string();
    if target.len() > MAX_WEB_URI_BYTES {
        return ApiError::bad("web_request_target_too_large").into_response();
    }
    if path_contains_traversal(request.uri().path()) {
        return ApiError::bad("web_path_traversal_denied").into_response();
    }
    // Rate limiting is deliberately scoped to one WebApp instance and runs before route
    // extraction/authentication so malformed or unauthorized floods cannot reach handlers.
    if let Err(error) = app.enforce_rate_limit() {
        return error.into_response();
    }
    next.run(request).await
}

async fn security_headers(
    State(app): State<Arc<WebApp>>,
    request: Request,
    next: Next,
) -> Response {
    let mut response = next.run(request).await;
    let headers = response.headers_mut();
    for (name, value) in WEB_SECURITY_HEADERS {
        headers.insert(
            name.parse::<axum::http::HeaderName>().unwrap(),
            value.parse::<axum::http::HeaderValue>().unwrap(),
        );
    }
    let csp = WEB_CSP_TEMPLATE.replace("{nonce}", &app.csp_nonce);
    headers.insert("content-security-policy", csp.parse().unwrap());
    response
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
        let principal_id = host.authenticated_principal().principal_id;
        sessions.insert(session_id.clone(), WebSession::new(principal_id));
        Self {
            host,
            workdir,
            sandbox: Arc::new(Mutex::new(sandbox)),
            role: Arc::new(Mutex::new(role)),
            sessions: Arc::new(Mutex::new(sessions)),
            active: Arc::new(Mutex::new(session_id)),
            csp_nonce: Arc::new(uuid::Uuid::new_v4().simple().to_string()),
            bound_addr,
            desktop_attach: None,
            rate_window: Arc::new(Mutex::new(WebRateWindow {
                started: Instant::now(),
                requests: 0,
            })),
            page_cursors: Arc::new(Mutex::new(HashMap::new())),
            consumed_page_cursors: Arc::new(Mutex::new(HashMap::new())),
            action_submissions: Arc::new(Mutex::new(HashMap::new())),
            web_token: Arc::new(Mutex::new(uuid::Uuid::new_v4().to_string())),
            token_generation: Arc::new(std::sync::atomic::AtomicU64::new(1)),
            shutting_down: Arc::new(AtomicBool::new(false)),
            shutdown: tokio::sync::watch::channel(false).0,
        }
    }

    fn with_optional_desktop_attach(mut self, record: Option<&UiInstanceRecord>) -> Self {
        self.desktop_attach = record.map(DesktopAttachIdentity::from);
        self
    }

    // 汇集 UI 所需的即时状态。Mutex 中的数据可能与 daemon 已持久化的状态不同步，
    // 因此只作为展示快照返回，不用于授权结论。历史会话只从事件账本投影，且永远
    // 标记为 read_only；人工命令可核验归属后访问，继续执行必须先显式 Resume。
    async fn snapshot(&self, session_id: &str) -> Result<Value, ApiError> {
        validate_web_session_id(session_id)?;
        // Check ownership before asking DaemonHost for a projection.  This keeps an arbitrary
        // UUID/path-like value from becoming a cross-workspace probe through the host API.
        let (known_live, known_history) = {
            let known_live = self
                .sessions
                .lock()
                .map_err(|_| ApiError::fail("web_state_poisoned"))?
                .contains_key(session_id);
            // The ledger read below is async, so it must run with no session guard held:
            // a `MutexGuard` is not `Send`, and holding one across an await would make this
            // future `!Send` and reject every route that projects a snapshot.
            let known_history = self
                .history_sessions()
                .await?
                .iter()
                .any(|session| session.id == session_id);
            (known_live, known_history)
        };
        if !known_live && !known_history {
            return Err(ApiError::bad("session_unknown"));
        }
        let projection = self
            .host
            .ui_snapshot(session_id)
            .await
            .map_err(|error| ApiError::fail(error.to_string()))?;
        let trusted = harness_run::project_trusted(&self.workdir.to_string_lossy())
            .map_err(|error| ApiError::fail(error.to_string()))?;
        let sandbox = lock_string(&self.sandbox)?;
        let (events, history) = self.read_session_ledger().await?;
        let assignment = history.iter().find(|session| session.id == session_id);
        let role_id = match assignment {
            Some(session) => session.role_id.clone(),
            None => lock_string(&self.role)?,
        };
        let role = RoleSpec::lookup(&role_id).ok_or_else(|| ApiError::bad("role_unknown"))?;
        let run_sandbox = assignment.and_then(|session| session.sandbox.clone());
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
        let history_turn_count = thread
            .get("turns")
            .and_then(Value::as_array)
            .map_or(0, Vec::len);
        let history_limited = historical
            .as_ref()
            .is_some_and(|thread| history_thread_is_limited(&events, &thread.session.run_id))
            || history_turn_count >= MAX_WEB_TURNS_PER_SESSION;
        let history_status = if history_turn_count == 0 {
            "empty"
        } else if history_limited {
            "limited"
        } else {
            "ready"
        };
        let principal_id = self.host.authenticated_principal().principal_id;
        let (owner_tab_id, lease_epoch, owner_active) = current
            .as_ref()
            .map(|session| {
                (
                    session.owner_tab_id.clone(),
                    session.lease_epoch,
                    session.owner_active,
                )
            })
            .unwrap_or_else(|| (None, 1, false));
        let token_generation = self
            .token_generation
            .load(std::sync::atomic::Ordering::Acquire);
        let lease_owner_tab = owner_tab_id.clone().unwrap_or_else(|| "history".to_owned());
        let lease_disposition = if owner_active { "observer" } else { "closed" };
        let hydrate = json!({
            "schema": WEB_HYDRATE_SCHEMA,
            "instance_id": self.host.feed_instance_id(),
            "epoch": projection.cursor.epoch.clone(),
            "snapshot_cursor": projection.cursor.sequence,
            "owner_id": principal_id.clone(),
            "session_id": session_id.clone(),
            "generated_at_unix_ms": web_now_unix_ms(),
            "source": "daemon_snapshot",
            "feed_ready": false,
            "cache": "memory_only",
            "status": "ready",
            "limitations": ["snapshot_is_disposable_projection"],
            // The canonical bootstrap order.  A browser cache or a persisted local value can
            // never stand in for the first server snapshot, and the feed listener is installed
            // only after that snapshot is accepted.
            "trace": {
                "schema": WEB_HYDRATE_TRACE_SCHEMA,
                "max_steps": MAX_WEB_HYDRATE_TRACE_STEPS,
                "steps": [
                    {"step": "snapshot", "schema": WEB_HYDRATE_SCHEMA, "requires": "none"},
                    {"step": "history", "schema": WEB_HISTORY_SCHEMA, "requires": "snapshot"},
                    {"step": "feed", "schema": WEB_SSE_SCHEMA, "requires": "snapshot"}
                ]
            }
        });
        let parity = harness_run::parity_envelope_on_host(
            Arc::clone(&self.host),
            session_id.to_owned(),
            projection.run_id,
            EntryPointKind::Web,
            &self.options()?,
        )
        .await
        .map_err(|error| ApiError::fail(error.to_string()))?;
        Ok(json!({
            "cursor": projection.cursor,
            "projection": projection,
            "harness": harness_run::HARNESS_ID,
            "folder": self.workdir.display().to_string(),
            "trusted": trusted,
            "sandbox": sandbox,
            "run_sandbox": run_sandbox,
            "role": role.role_id,
            "department": role.department_id,
            "session_id": session_id,
            "running": running,
            "read_only": read_only,
            "resume_required": read_only,
            "human_actions_allowed": owner_active && !read_only,
            "lease": {
                "schema": WEB_TAB_SESSION_SCHEMA,
                "principal_id": principal_id,
                "session_id": session_id.clone(),
                "owner_tab_id": lease_owner_tab,
                "lease_epoch": lease_epoch,
                "token_generation": token_generation,
                "disposition": lease_disposition,
                "feed_only": true
            },
            "sessions": session_views,
            "threads": threads,
            "thread": thread,
            "last": last,
            "streaming": true,
            "streaming_transport": "sse",
            "shape": "codex-app",
            "parity": parity.output,
            "parity_response_status": parity.status,
            "hydrate": hydrate,
            "history": {
                "schema": WEB_HISTORY_SCHEMA,
                "status": history_status,
                "session_id": session_id,
                "next_page": Value::Null,
                "limitations": if history_limited {
                    json!(["history_retention_or_memory_bound"])
                } else {
                    json!([])
                }
            }
        }))
    }

    async fn read_session_ledger(
        &self,
    ) -> Result<(Vec<LedgerEvent>, Vec<LedgerSession>), ApiError> {
        let events: Vec<LedgerEvent> = match self.host.ui_events().await {
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

    fn page_context(&self) -> Result<(String, String, u64), ApiError> {
        let cursor = self.host.ui_cursor();
        if cursor.epoch.trim().is_empty() {
            return Err(ApiError::fail("web_page_epoch_unavailable"));
        }
        Ok((self.host.feed_instance_id(), cursor.epoch, cursor.sequence))
    }

    async fn history_page(
        &self,
        session_id: &str,
        tab_id: &str,
        after: Option<&str>,
        limit: usize,
    ) -> Result<Value, ApiError> {
        validate_web_page_limit(limit)?;
        validate_web_tab_id(tab_id)?;
        let (instance_id, epoch, source_cursor) = self.page_context()?;
        let (events, sessions) = self.read_session_ledger().await?;
        let (turns, limited) =
            if let Some(thread) = ledger_thread_for_session(&events, &sessions, session_id) {
                let limited = history_thread_is_limited(&events, &thread.session.run_id);
                (thread.turns, limited)
            } else {
                let live = self
                    .sessions
                    .lock()
                    .map_err(|_| ApiError::fail("web_state_poisoned"))?
                    .contains_key(session_id);
                if !live {
                    return Err(ApiError::bad("session_unknown"));
                }
                (Vec::new(), false)
            };
        let (offset, _cursor_limit) = self.consume_page_cursor(
            after,
            "history",
            session_id,
            tab_id,
            &instance_id,
            &epoch,
            source_cursor,
            limit,
        )?;
        if offset > turns.len() {
            return Err(ApiError::bad("web_page_cursor_offset_invalid"));
        }
        let end = (offset + limit).min(turns.len());
        let entries = turns[offset..end]
            .iter()
            .map(|turn| serde_json::to_value(turn).unwrap_or(Value::Null))
            .collect::<Vec<_>>();
        let next_page = if end < turns.len() {
            Some(self.issue_page_cursor(WebPageCursor {
                schema: WEB_PAGE_CURSOR_SCHEMA,
                kind: "history",
                token: String::new(),
                session_id: session_id.to_owned(),
                tab_id: tab_id.to_owned(),
                instance_id: instance_id.clone(),
                epoch: epoch.clone(),
                source_cursor,
                offset: end,
                limit,
                issued_at: Instant::now(),
            })?)
        } else {
            None
        };
        let status = if turns.is_empty() {
            "empty"
        } else if limited {
            "limited"
        } else if next_page.is_some() {
            "partial"
        } else {
            "ready"
        };
        self.ensure_page_source_unchanged(&epoch, source_cursor)?;
        Ok(json!({
            "schema": WEB_HISTORY_SCHEMA,
            "instance_id": instance_id,
            "epoch": epoch,
            "source_cursor": source_cursor,
            "session_id": session_id,
            "entries": entries,
            "next_page": next_page,
            "status": status,
            "cache": "memory_only",
            "limitations": if limited {
                json!(["history_retention_or_memory_bound"])
            } else {
                json!([])
            }
        }))
    }

    async fn artifact_page(
        &self,
        session_id: &str,
        tab_id: &str,
        artifact_id: Option<&str>,
        after: Option<&str>,
        limit: usize,
    ) -> Result<Value, ApiError> {
        validate_web_page_limit(limit)?;
        validate_web_tab_id(tab_id)?;
        let (instance_id, epoch, source_cursor) = self.page_context()?;
        let (events, sessions) = self.read_session_ledger().await?;
        let entries =
            if let Some(session) = sessions.iter().find(|session| session.id == session_id) {
                events
                    .iter()
                    .filter(|event| {
                        event.data.get("run_id").and_then(Value::as_str)
                            == Some(session.run_id.as_str())
                    })
                    .filter_map(|event| artifact_entry(event, artifact_id))
                    .collect::<Vec<_>>()
            } else if self
                .sessions
                .lock()
                .map_err(|_| ApiError::fail("web_state_poisoned"))?
                .contains_key(session_id)
            {
                Vec::new()
            } else {
                return Err(ApiError::bad("session_unknown"));
            };
        let (offset, _cursor_limit) = self.consume_page_cursor(
            after,
            "artifact",
            session_id,
            tab_id,
            &instance_id,
            &epoch,
            source_cursor,
            limit,
        )?;
        if offset > entries.len() {
            return Err(ApiError::bad("web_page_cursor_offset_invalid"));
        }
        let end = (offset + limit).min(entries.len());
        let next_page = if end < entries.len() {
            Some(self.issue_page_cursor(WebPageCursor {
                schema: WEB_PAGE_CURSOR_SCHEMA,
                kind: "artifact",
                token: String::new(),
                session_id: session_id.to_owned(),
                tab_id: tab_id.to_owned(),
                instance_id: instance_id.clone(),
                epoch: epoch.clone(),
                source_cursor,
                offset: end,
                limit,
                issued_at: Instant::now(),
            })?)
        } else {
            None
        };
        let status = if entries.is_empty() {
            "empty"
        } else if next_page.is_some() {
            "partial"
        } else {
            "ready"
        };
        self.ensure_page_source_unchanged(&epoch, source_cursor)?;
        Ok(json!({
            "schema": WEB_ARTIFACT_SCHEMA,
            "instance_id": instance_id,
            "epoch": epoch,
            "source_cursor": source_cursor,
            "session_id": session_id,
            "artifact_id": artifact_id,
            "entries": entries[offset..end].to_vec(),
            "next_page": next_page,
            "status": status,
            "cache": "memory_only",
            "limitations": ["artifact_content_requires_server_ref"]
        }))
    }

    /// Build the aggregate server-owned artifact projection used by deep links.  The browser gets
    /// references and an optional text page only; it never receives a path/URL that it can fetch.
    async fn artifact_detail_page(
        &self,
        session_id: &str,
        tab_id: &str,
        artifact_id: &str,
        after: Option<&str>,
        limit: usize,
    ) -> Result<Value, ApiError> {
        validate_web_page_limit(limit)?;
        validate_web_tab_id(tab_id)?;
        validate_web_opaque_id(artifact_id, "artifact_id")?;
        let (instance_id, epoch, source_cursor) = self.page_context()?;
        let (events, sessions) = self.read_session_ledger().await?;
        let Some(session) = sessions.iter().find(|session| session.id == session_id) else {
            if !self
                .sessions
                .lock()
                .map_err(|_| ApiError::fail("web_state_poisoned"))?
                .contains_key(session_id)
            {
                return Err(ApiError::bad("session_unknown"));
            }
            return Err(ApiError::bad("artifact_not_found"));
        };
        let entries = events
            .iter()
            .filter(|event| {
                event.data.get("run_id").and_then(Value::as_str) == Some(session.run_id.as_str())
            })
            .filter_map(|event| artifact_entry(event, Some(artifact_id)))
            .collect::<Vec<_>>();
        if entries.is_empty() {
            // Do not turn a private/nonexistent reference into an enumerable empty page.
            return Err(ApiError::bad("artifact_not_found"));
        }
        let (offset, _cursor_limit) = self.consume_page_cursor(
            after,
            "artifact_detail",
            session_id,
            tab_id,
            &instance_id,
            &epoch,
            source_cursor,
            limit,
        )?;
        if offset > entries.len() {
            return Err(ApiError::bad("web_page_cursor_offset_invalid"));
        }
        let end = (offset + limit).min(entries.len());
        let page_entries = &entries[offset..end];
        let revision = page_entries
            .last()
            .and_then(|entry| entry.get("source_sequence"))
            .and_then(Value::as_u64)
            .unwrap_or(source_cursor)
            .max(1);
        let next_cursor = if end < entries.len() {
            Some(self.issue_page_cursor(WebPageCursor {
                schema: WEB_PAGE_CURSOR_SCHEMA,
                kind: "artifact_detail",
                token: String::new(),
                session_id: session_id.to_owned(),
                tab_id: tab_id.to_owned(),
                instance_id: instance_id.clone(),
                epoch: epoch.clone(),
                source_cursor,
                offset: end,
                limit,
                issued_at: Instant::now(),
            })?)
        } else {
            None
        };
        let first = page_entries
            .first()
            .ok_or_else(|| ApiError::bad("artifact_not_found"))?;
        let digest = first
            .get("digest")
            .and_then(Value::as_str)
            .filter(|value| value.starts_with("sha256:"))
            .unwrap_or("sha256:0000000000000000000000000000000000000000000000000000000000000000");
        let owner_tab_id = self
            .sessions
            .lock()
            .map_err(|_| ApiError::fail("web_state_poisoned"))?
            .get(session_id)
            .and_then(|session| session.owner_tab_id.clone())
            .unwrap_or_else(|| "historical".to_owned());
        let scope = json!({
            "schema": WEB_DETAIL_SCOPE_SCHEMA,
            "session_id": session_id,
            "tab_id": tab_id,
            "owner_tab_id": owner_tab_id,
            "instance_id": instance_id,
            "epoch": epoch,
            "source_cursor": source_cursor,
            "revision": revision
        });
        let artifact = json!({
            "schema": "kiana.ui-artifact-ref.v1",
            "artifact_id": artifact_id,
            "version": 1,
            "artifact_schema": first.get("schema").cloned().unwrap_or_else(|| json!("unknown")),
            "mime": "text/plain",
            "size_bytes": 0,
            "content_hash": digest,
            "scope_digest": digest,
            "revision": revision,
            "session_id": session_id,
            "run_id": session.run_id,
            "provenance": "event_log_projection",
            "links": [{"schema": WEB_DETAIL_LINK_SCHEMA, "kind": "timeline", "id": first.get("source_event_id").cloned().unwrap_or(Value::Null), "session_id": session_id}]
        });
        let page = json!({
            "schema": "kiana.ui-artifact-page.v2",
            "scope": scope,
            "artifact": artifact,
            "revision": revision,
            "page_index": 0,
            "page_count": 1,
            "page_digest": digest,
            "content_encoding": "none",
            "content": Value::Null,
            "truncated": false,
            "next_cursor": next_cursor,
            "limitations": ["artifact_content_requires_authorized_server_page"]
        });
        self.ensure_page_source_unchanged(&epoch, source_cursor)?;
        Ok(json!({
            "schema": WEB_ARTIFACT_DETAIL_SCHEMA,
            "scope": page["scope"],
            "artifact_page": page,
            "diff": Value::Null,
            "receipt": Value::Null,
            "status": "partial",
            "limitations": ["artifact_content_requires_authorized_server_page", "diff_requires_server_projection"]
        }))
    }

    async fn diff_detail_page(
        &self,
        session_id: &str,
        tab_id: &str,
        artifact_id: &str,
        after: Option<&str>,
        limit: usize,
    ) -> Result<Value, ApiError> {
        let mut detail = self
            .artifact_detail_page(session_id, tab_id, artifact_id, after, limit)
            .await?;
        let scope = detail["scope"].clone();
        detail["schema"] = json!(WEB_DIFF_DETAIL_SCHEMA);
        detail["diff"] = json!({
            "schema": WEB_DIFF_DETAIL_SCHEMA,
            "scope": scope,
            "artifact": detail["artifact_page"]["artifact"],
            "base_revision": detail["artifact_page"]["revision"],
            "target_revision": detail["artifact_page"]["revision"],
            "status": "unknown",
            "total_additions": 0,
            "total_deletions": 0,
            "files": [],
            "links": [],
            "limitations": ["server_diff_not_available"]
        });
        detail["status"] = json!("unknown");
        detail["limitations"] = json!(["server_diff_not_available"]);
        Ok(detail)
    }

    async fn receipt_detail_page(
        &self,
        session_id: &str,
        tab_id: &str,
        after: Option<&str>,
        limit: usize,
    ) -> Result<Value, ApiError> {
        validate_web_page_limit(limit)?;
        validate_web_tab_id(tab_id)?;
        if after.is_some() {
            return Err(ApiError::bad("web_receipt_cursor_not_supported"));
        }
        let session_id = resolve_human_session(self, Some(session_id)).await?;
        let snapshot = self.snapshot(&session_id).await?;
        let run_id = snapshot["projection"]["run_id"]
            .as_str()
            .and_then(RunId::parse_str)
            .ok_or_else(|| ApiError::bad("receipt_not_found"))?;
        let owner_tab_id = self
            .sessions
            .lock()
            .map_err(|_| ApiError::fail("web_state_poisoned"))?
            .get(&session_id)
            .and_then(|session| session.owner_tab_id.clone())
            .unwrap_or_else(|| "historical".to_owned());
        let (instance_id, epoch, source_cursor) = self.page_context()?;
        let receipt = harness_run::receipt_envelope_on_host(
            Arc::clone(&self.host),
            session_id.clone(),
            Some(run_id),
            &self.options()?,
        )
        .await
        .map_err(|error| ApiError::fail(error.to_string()))?;
        let receipt_value = serde_json::to_value(&receipt)
            .map_err(|error| ApiError::fail(format!("receipt_encode:{error}")))?;
        let status = receipt_value
            .get("status")
            .and_then(Value::as_str)
            .unwrap_or("unknown");
        let unknown =
            matches!(status, "unknown" | "result_unknown") || receipt_value.get("error").is_some();
        let digest = receipt_value
            .get("output")
            .and_then(|output| output.get("receipt_digest"))
            .and_then(Value::as_str)
            .filter(|digest| digest.starts_with("sha256:"))
            .unwrap_or("sha256:0000000000000000000000000000000000000000000000000000000000000000");
        Ok(json!({
            "schema": WEB_RECEIPT_DETAIL_SCHEMA,
            "scope": {
                "schema": WEB_DETAIL_SCOPE_SCHEMA,
                "session_id": session_id,
                "tab_id": tab_id,
                "owner_tab_id": owner_tab_id,
                "instance_id": instance_id,
                "epoch": epoch,
                "source_cursor": source_cursor,
                "revision": source_cursor
            },
            "receipt_id": receipt_value.get("output").and_then(|output| output.get("receipt_id")).cloned().unwrap_or(Value::Null),
            "run_id": run_id,
            "session_id": session_id,
            "receipt_digest": digest,
            "status": status,
            "unknown": unknown,
            "source_cursor": source_cursor,
            "entries": [],
            "artifact_ids": [],
            "timeline_ids": [],
            "inbox_ids": [],
            "limitations": if unknown { json!(["receipt_result_unknown_or_error; query original receipt"]) } else { json!(["detail_entries_are_server_projection_only"]) },
            "receipt": receipt_value
        }))
    }

    fn issue_page_cursor(&self, mut cursor: WebPageCursor) -> Result<String, ApiError> {
        let token = uuid::Uuid::new_v4().to_string();
        cursor.token = token.clone();
        let mut cursors = self
            .page_cursors
            .lock()
            .map_err(|_| ApiError::fail("web_page_cursor_state_unavailable"))?;
        if cursors.len() >= MAX_WEB_PAGE_CURSORS {
            if let Some(oldest) = cursors
                .iter()
                .min_by_key(|(_, cursor)| cursor.issued_at)
                .map(|(token, _)| token.clone())
            {
                cursors.remove(&oldest);
            }
        }
        cursors.insert(token.clone(), cursor);
        Ok(token)
    }

    /// Re-read the server cursor after a page was built.
    ///
    /// A commit between the read and the response must not be reported as a complete page: the
    /// caller has to hydrate a fresh snapshot instead of mixing a stale page with newer state.
    fn ensure_page_source_unchanged(
        &self,
        epoch: &str,
        source_cursor: u64,
    ) -> Result<(), ApiError> {
        let (_, current_epoch, current_cursor) = self.page_context()?;
        if current_epoch != epoch || current_cursor != source_cursor {
            return Err(ApiError::conflict("web_page_source_changed"));
        }
        Ok(())
    }

    fn consume_page_cursor(
        &self,
        token: Option<&str>,
        kind: &str,
        session_id: &str,
        tab_id: &str,
        instance_id: &str,
        epoch: &str,
        source_cursor: u64,
        limit: usize,
    ) -> Result<(usize, usize), ApiError> {
        let Some(token) = token else {
            return Ok((0, 0));
        };
        if token.trim().is_empty()
            || token.len() > MAX_WEB_PAGE_CURSOR_BYTES
            || token.chars().any(char::is_control)
        {
            return Err(ApiError::bad("web_page_cursor_invalid"));
        }
        let mut cursors = self
            .page_cursors
            .lock()
            .map_err(|_| ApiError::fail("web_page_cursor_state_unavailable"))?;
        let cursor = cursors.get(token).cloned();
        let Some(cursor) = cursor else {
            drop(cursors);
            let replayed = self
                .consumed_page_cursors
                .lock()
                .map_err(|_| ApiError::fail("web_page_cursor_state_unavailable"))?
                .contains_key(token);
            return Err(ApiError::bad(if replayed {
                "web_page_cursor_replayed"
            } else {
                "web_page_cursor_unknown"
            }));
        };
        if cursor.schema != WEB_PAGE_CURSOR_SCHEMA
            || cursor.kind != kind
            || cursor.session_id != session_id
            || cursor.tab_id != tab_id
            || cursor.instance_id != instance_id
            || cursor.epoch != epoch
            || cursor.source_cursor != source_cursor
        {
            return Err(ApiError::conflict("web_page_cursor_scope_mismatch"));
        }
        if cursor.limit != limit {
            // A malformed follow-up limit must not burn the owner's cursor: reject it before the
            // token is consumed so the same page can be retried with its issued bound.
            return Err(ApiError::bad("web_page_cursor_limit_mismatch"));
        }
        // Validate the full scope before consuming the token. A foreign tab or stale source
        // cursor must fail closed without burning a still-valid cursor for its owner.
        cursors.remove(token);
        drop(cursors);
        let mut consumed = self
            .consumed_page_cursors
            .lock()
            .map_err(|_| ApiError::fail("web_page_cursor_state_unavailable"))?;
        if consumed.len() >= MAX_WEB_PAGE_CURSORS {
            if let Some(oldest) = consumed
                .iter()
                .min_by_key(|(_, issued_at)| **issued_at)
                .map(|(token, _)| token.clone())
            {
                consumed.remove(&oldest);
            }
        }
        consumed.insert(token.to_owned(), Instant::now());
        Ok((cursor.offset, cursor.limit))
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

impl WebSession {
    fn new(owner_principal_id: impl Into<String>) -> Self {
        Self {
            run_id: None,
            running: false,
            name: "New thread".to_owned(),
            last: None,
            turns: Vec::new(),
            owner_principal_id: owner_principal_id.into(),
            owner_tab_id: None,
            lease_epoch: 1,
            owner_active: true,
        }
    }
}

impl Default for WebSession {
    fn default() -> Self {
        Self::new("unknown")
    }
}

async fn index(
    State(app): State<Arc<WebApp>>,
    headers: HeaderMap,
) -> Result<Html<String>, ApiError> {
    authorize_host(&app, &headers)?;
    let token = app
        .web_token
        .lock()
        .map_err(|_| ApiError::fail("web_token_state_unavailable"))?;
    let page = PAGE
        .replace("__KIANA_WEB_TOKEN_VALUE__", &token)
        .replace("__KIANA_CSP_NONCE__", &app.csp_nonce);
    Ok(Html(page))
}

async fn health(
    State(app): State<Arc<WebApp>>,
    headers: HeaderMap,
) -> Result<Json<Value>, ApiError> {
    authorize_host(&app, &headers)?;
    let mut payload = json!({
        "schema": "kiana.health-snapshot.v1",
        "ok": false,
        "harness": harness_run::HARNESS_ID,
        "loopback": true,
        "streaming": true,
        "streaming_transport": "sse",
        "instance_id": app.host.feed_instance_id(),
        "epoch": app.host.ui_cursor().epoch,
        "protocol_schema": PROTOCOL_SCHEMA,
    });
    if let Some(identity) = &app.desktop_attach {
        payload["pid"] = json!(std::process::id());
        payload["desktop_instance_id"] = json!(identity.instance_id);
        payload["desktop_authority_epoch"] = json!(identity.authority_epoch);
        payload["desktop_workspace_digest"] = json!(identity.workspace_digest);
        payload["desktop_record_digest"] = json!(identity.record_digest);
    }
    match app.host.liveness().await {
        Ok(snapshot) => {
            payload["ok"] = Value::Bool(snapshot.status == SignalStatus::Ok);
            payload["status"] = serde_json::to_value(snapshot.status).unwrap_or(Value::Null);
        }
        Err(_) => {
            payload["status"] = Value::String("unavailable".to_owned());
            // Health is a low-detail liveness signal.  Do not return source paths, storage
            // locations, or nested error text from the ControlPlane/ledger boundary.
            payload["limitations"] = json!(["health_projection_unavailable"]);
        }
    }
    Ok(Json(payload))
}

/// Accept a bounded browser CSP report without turning the report body into runtime authority.
/// The page also exposes a visible `securitypolicyviolation` state; this endpoint is only a
/// diagnostic sink and deliberately does not echo blocked URLs, source paths or report details.
async fn csp_report(
    State(app): State<Arc<WebApp>>,
    headers: HeaderMap,
    _report: Bytes,
) -> Result<StatusCode, ApiError> {
    authorize_host(&app, &headers)?;
    Ok(StatusCode::NO_CONTENT)
}

/// Bootstrap is a read-only snapshot boundary.  The browser must accept this server projection
/// before it installs a feed listener; a browser-persisted value can never stand in for it.
async fn bootstrap(
    State(app): State<Arc<WebApp>>,
    headers: HeaderMap,
    Query(query): Query<SessionBody>,
) -> Result<Json<Value>, ApiError> {
    authorize_mutation(&app, &headers)?;
    let tab_id = require_web_tab(&headers)?;
    let session_id = resolve_human_session(&app, query.session_id.as_deref()).await?;
    // Only a live in-memory session has a mutable lease to claim. A session that exists only as
    // committed history is a read-only EventLog projection: hydrate it without creating or
    // stealing a lease so a fresh page can open any known session, while an unknown id still
    // fails closed in `resolve_human_session`.
    let live_session = app
        .sessions
        .lock()
        .map_err(|_| ApiError::fail("web_state_poisoned"))?
        .contains_key(&session_id);
    if live_session {
        claim_session_tab(&app, &session_id, &tab_id)?;
    }
    let mut snapshot = app.snapshot(&session_id).await?;
    attach_hydrate_tab(&mut snapshot, &tab_id)?;
    if !live_session {
        mark_historical_hydrate(&mut snapshot);
    }
    snapshot["bootstrap"] = json!({
        "schema": WEB_HYDRATE_SCHEMA,
        "snapshot_first": true,
        "feed_after_hydrate": true,
        "tab_id": tab_id,
        "status": "ready"
    });
    Ok(Json(snapshot))
}

async fn parity(
    State(app): State<Arc<WebApp>>,
    headers: HeaderMap,
    Query(query): Query<ParityQuery>,
) -> Result<Json<Value>, ApiError> {
    authorize_mutation(&app, &headers)?;
    let session_id = resolve_human_session(&app, query.session_id.as_deref()).await?;
    let run_id = query
        .run_id
        .as_deref()
        .map(|raw| RunId::parse_str(raw).ok_or_else(|| ApiError::bad("parity_run_id_invalid")))
        .transpose()?;
    let entrypoint = match query.entrypoint.as_deref().unwrap_or("web").trim() {
        "web" => EntryPointKind::Web,
        "desktop" => EntryPointKind::Desktop,
        "workbench" => EntryPointKind::Workbench,
        "cli" => EntryPointKind::Cli,
        _ => return Err(ApiError::bad("parity_entrypoint_invalid")),
    };
    let response = harness_run::parity_envelope_on_host(
        Arc::clone(&app.host),
        session_id,
        run_id,
        entrypoint,
        &app.options()?,
    )
    .await
    .map_err(|error| ApiError::fail(error.to_string()))?;
    Ok(Json(json!({"response": response})))
}

async fn company_governance(
    State(app): State<Arc<WebApp>>,
    headers: HeaderMap,
    Query(query): Query<CompanyGovernanceQuery>,
) -> Result<Json<Value>, ApiError> {
    authorize_mutation(&app, &headers)?;
    if query.project_id.trim().is_empty() || query.project_id.len() > 256 {
        return Err(ApiError::bad("company_governance_project_invalid"));
    }
    let session_id = resolve_human_session(&app, query.session_id.as_deref()).await?;
    let response = harness_run::company_governance_envelope_on_host(
        Arc::clone(&app.host),
        session_id,
        query.project_id,
        &app.options()?,
    )
    .await
    .map_err(|error| ApiError::fail(error.to_string()))?;
    Ok(Json(json!({"response": response})))
}

async fn state(
    State(app): State<Arc<WebApp>>,
    headers: HeaderMap,
    Query(query): Query<SessionBody>,
) -> Result<Json<Value>, ApiError> {
    authorize_mutation(&app, &headers)?;
    let tab_id = optional_web_tab(&headers)?;
    let session_id = match query
        .session_id
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        Some(session_id) => session_id.to_owned(),
        None => lock_string(&app.active)?,
    };
    let mut snapshot = app.snapshot(&session_id).await?;
    if let Some(tab_id) = tab_id {
        attach_hydrate_tab(&mut snapshot, &tab_id)?;
    }
    Ok(Json(snapshot))
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

async fn history(
    State(app): State<Arc<WebApp>>,
    headers: HeaderMap,
    Query(query): Query<HistoryQuery>,
) -> Result<Json<Value>, ApiError> {
    authorize_mutation(&app, &headers)?;
    let tab_id = require_web_tab(&headers)?;
    let session_id = resolve_human_session(&app, Some(&query.session_id)).await?;
    validate_web_page_limit(query.limit)?;
    let page = app
        .history_page(&session_id, &tab_id, query.after.as_deref(), query.limit)
        .await?;
    Ok(Json(page))
}

/// Return a server-scoped notification page.  The page is a disposable projection: item detail
/// and action arguments are removed before the response leaves this entrypoint, and any action
/// still has to be re-admitted by the ControlPlane.
async fn notifications(
    State(app): State<Arc<WebApp>>,
    headers: HeaderMap,
    Query(query): Query<NotificationQuery>,
) -> Result<Json<Value>, ApiError> {
    authorize_mutation(&app, &headers)?;
    let tab_id = require_web_tab(&headers)?;
    let session_id = resolve_human_session(&app, Some(&query.session_id)).await?;
    authorize_feed_tab(&app, &session_id, &tab_id)?;
    if query.limit == 0
        || usize::from(query.limit) > crate::web_notifications::WEB_NOTIFICATION_MAX_ITEMS
    {
        return Err(ApiError::bad("web_notification_page_limit_invalid"));
    }
    if query.expected_source_cursor == Some(0) {
        return Err(ApiError::bad("web_notification_cursor_invalid"));
    }
    if let Some(after) = query.after_item_id.as_deref() {
        validate_web_opaque_id(after, "after_item_id")?;
    }
    let page = app
        .host
        .notification_page(
            query.limit,
            query.after_item_id,
            query.expected_source_cursor,
        )
        .await
        .map_err(|error| notification_api_error(error.to_string()))?;
    let page = serde_json::to_value(page)
        .map_err(|_| ApiError::fail("web_notification_page_encode_failed"))?;
    let cursor = app.host.ui_cursor();
    let payload = crate::web_notifications::present_notification_page(
        &page,
        &session_id,
        &tab_id,
        &app.host.feed_instance_id(),
        &cursor.epoch,
    )
    .map_err(|error| ApiError::fail(error))?;
    Ok(Json(payload))
}

async fn artifact(
    State(app): State<Arc<WebApp>>,
    headers: HeaderMap,
    Query(query): Query<ArtifactQuery>,
) -> Result<Json<Value>, ApiError> {
    authorize_mutation(&app, &headers)?;
    let tab_id = require_web_tab(&headers)?;
    let session_id = resolve_human_session(&app, Some(&query.session_id)).await?;
    validate_web_page_limit(query.limit)?;
    if let Some(artifact_id) = query.artifact_id.as_deref() {
        validate_web_opaque_id(artifact_id, "artifact_id")?;
    }
    let page = app
        .artifact_page(
            &session_id,
            &tab_id,
            query.artifact_id.as_deref(),
            query.after.as_deref(),
            query.limit,
        )
        .await?;
    Ok(Json(page))
}

async fn artifact_detail(
    State(app): State<Arc<WebApp>>,
    headers: HeaderMap,
    Query(query): Query<ArtifactDetailQuery>,
) -> Result<Json<Value>, ApiError> {
    authorize_mutation(&app, &headers)?;
    let tab_id = require_web_tab(&headers)?;
    let session_id = resolve_human_session(&app, Some(&query.session_id)).await?;
    authorize_feed_tab(&app, &session_id, &tab_id)?;
    let page = app
        .artifact_detail_page(
            &session_id,
            &tab_id,
            &query.artifact_id,
            query.after.as_deref(),
            query.limit,
        )
        .await?;
    Ok(Json(page))
}

async fn diff_detail(
    State(app): State<Arc<WebApp>>,
    headers: HeaderMap,
    Query(query): Query<DiffQuery>,
) -> Result<Json<Value>, ApiError> {
    authorize_mutation(&app, &headers)?;
    let tab_id = require_web_tab(&headers)?;
    let session_id = resolve_human_session(&app, Some(&query.session_id)).await?;
    authorize_feed_tab(&app, &session_id, &tab_id)?;
    let page = app
        .diff_detail_page(
            &session_id,
            &tab_id,
            &query.artifact_id,
            query.after.as_deref(),
            query.limit,
        )
        .await?;
    Ok(Json(page))
}

async fn receipt_detail(
    State(app): State<Arc<WebApp>>,
    headers: HeaderMap,
    Query(query): Query<ReceiptDetailQuery>,
) -> Result<Json<Value>, ApiError> {
    authorize_mutation(&app, &headers)?;
    let tab_id = require_web_tab(&headers)?;
    let session_id = resolve_human_session(&app, Some(&query.session_id)).await?;
    authorize_feed_tab(&app, &session_id, &tab_id)?;
    Ok(Json(
        app.receipt_detail_page(&session_id, &tab_id, query.after.as_deref(), query.limit)
            .await?,
    ))
}

struct EventStreamState {
    subscription: kiana_daemon::RunStreamSubscription,
    gap: Option<Event>,
    done: bool,
    shutdown: tokio::sync::watch::Receiver<bool>,
    last_cursor: UiCursor,
}

struct NotificationEventStreamState {
    subscription: kiana_daemon::RunStreamFeedSubscription,
    done: bool,
    shutdown: tokio::sync::watch::Receiver<bool>,
    session_id: String,
    tab_id: String,
    last_cursor: UiFeedCursorV1,
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
    // The token is consumed only by authorize_sse.  It is never copied into an SSE id/data
    // frame, a server log, or a reconnect cursor; EventSource's query fallback is auth-only.
    authorize_sse(&app, &headers, query.token.as_deref())?;
    let tab_id = query
        .tab_id
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| ApiError::bad("web_tab_required"))?;
    validate_web_tab_id(tab_id)?;
    let session_id = resolve_feed_session(&app, query.session_id.as_deref()).await?;
    authorize_feed_tab(&app, &session_id, tab_id)?;
    let attach = stream_attach_state(&app, &session_id)?;
    // Subscribe before returning the SSE response headers. The browser waits for
    // EventSource.onopen before issuing /api/run, so the first delta is not lost.
    let after = stream_cursor_from_request(&headers, query.last_event_id.as_deref())?;
    let subscription = app.host.subscribe_run_after(attach.run_id, after.as_ref());
    let initial_cursor = after
        .clone()
        .unwrap_or_else(|| subscription.cursor().clone());
    let gap_cursor = app.host.run_stream_cursor(attach.run_id);
    let gap = if subscription.has_gap() || (after.is_none() && attach.gap_reason.is_some()) {
        let reason = if after.is_none() {
            attach
                .gap_reason
                .unwrap_or("snapshot_required_after_stream_gap")
        } else {
            "snapshot_required_after_stream_gap"
        };
        Some(stream_gap_sse_event_with_cursor(
            attach.run_id,
            reason,
            &gap_cursor,
        ))
    } else {
        None
    };
    let initial_cursor = if gap.is_some() {
        gap_cursor.clone()
    } else {
        initial_cursor
    };
    let stream = stream::unfold(
        EventStreamState {
            subscription,
            gap,
            done: false,
            shutdown: app.shutdown.subscribe(),
            last_cursor: initial_cursor,
        },
        |mut state| async move {
            next_run_stream_frame(&mut state)
                .await
                .map(|event| (Ok(event), state))
        },
    );
    Ok(Sse::new(Box::pin(stream)).keep_alive(KeepAlive::default()))
}

/// Pull exactly one display frame for a run-stream subscription.
///
/// This is the single implementation of the reconnect/heartbeat/gap/terminal state machine behind
/// `/api/events`.  A dropped, lagged or overloaded consumer is turned into one explicit bounded
/// frame (`stream_error` with the server cursor it last observed) instead of an unbounded buffer,
/// a silent stall or a replay of deltas the browser already rendered.  `terminal` is the last
/// frame of a display stream, and a heartbeat only re-states the current server cursor.
async fn next_run_stream_frame(state: &mut EventStreamState) -> Option<Event> {
    if state.done {
        return None;
    }
    if let Some(gap) = state.gap.take() {
        return Some(gap);
    }
    loop {
        let received = tokio::select! {
            biased;
            _ = state.shutdown.changed() => {
                state.done = true;
                return Some(stream_error_sse_event_with_cursor("web_shutdown", &state.last_cursor));
            }
            _ = tokio::time::sleep(WEB_SSE_HEARTBEAT_INTERVAL) => {
                return Some(stream_heartbeat_sse_event(&state.last_cursor));
            }
            received = state.subscription.recv() => received,
        };
        match received {
            Ok(envelope) => {
                let (name, terminal) = match &envelope.event {
                    RunStreamEvent::Delta { .. } => ("delta", false),
                    RunStreamEvent::Terminal { .. } => ("terminal", true),
                    RunStreamEvent::Usage { .. } => ("usage", false),
                    RunStreamEvent::ToolCall { .. } => ("tool_call", false),
                    RunStreamEvent::ApprovalRequested { .. } => ("approval_requested", false),
                    RunStreamEvent::Error { .. } => ("runtime_error", false),
                    RunStreamEvent::Unknown => continue,
                };
                state.last_cursor = UiCursor {
                    epoch: envelope.epoch.clone(),
                    sequence: envelope.sequence,
                };
                state.done = terminal;
                return Some(run_stream_sse_event(name, &envelope));
            }
            Err(tokio::sync::broadcast::error::RecvError::Lagged(skipped)) => {
                state.done = true;
                return Some(stream_error_sse_event_with_cursor(
                    &format!("stream_subscription_lagged:{skipped}"),
                    &state.last_cursor,
                ));
            }
            Err(tokio::sync::broadcast::error::RecvError::Closed) => {
                state.done = true;
                return Some(stream_error_sse_event_with_cursor(
                    "stream_closed_before_terminal",
                    &state.last_cursor,
                ));
            }
        }
    }
}

/// Notification SSE is a redacted view over the existing versioned feed.  It sends a snapshot
/// boundary first, carries an encoded server cursor for reconnect, and closes on gap/unknown so
/// the browser must hydrate `/api/notifications` before observing more deltas.
async fn notification_events(
    State(app): State<Arc<WebApp>>,
    headers: HeaderMap,
    Query(query): Query<NotificationEventsQuery>,
) -> Result<Sse<impl Stream<Item = Result<Event, Infallible>> + Send + 'static>, ApiError> {
    authorize_sse(&app, &headers, query.token.as_deref())?;
    let tab_id = query
        .tab_id
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| ApiError::bad("web_tab_required"))?;
    validate_web_tab_id(tab_id)?;
    let session_id = resolve_feed_session(&app, query.session_id.as_deref()).await?;
    authorize_feed_tab(&app, &session_id, tab_id)?;
    let attach = stream_attach_state(&app, &session_id)?;
    let after = notification_stream_cursor_from_request(&headers, query.last_event_id.as_deref())?;
    let subscription = app
        .host
        .subscribe_run_feed_after(attach.run_id, after.as_ref());
    let initial_cursor = subscription.cursor().clone();
    let stream = stream::unfold(
        NotificationEventStreamState {
            subscription,
            done: false,
            shutdown: app.shutdown.subscribe(),
            session_id,
            tab_id: tab_id.to_owned(),
            last_cursor: initial_cursor,
        },
        |mut state| async move {
            if state.done {
                return None;
            }
            let event = tokio::select! {
                biased;
                _ = state.shutdown.changed() => {
                    state.done = true;
                    notification_sse_error_event(
                        &state.last_cursor,
                        &state.session_id,
                        &state.tab_id,
                        "web_shutdown",
                    )
                }
                _ = tokio::time::sleep(WEB_NOTIFICATION_SSE_HEARTBEAT_INTERVAL) => {
                    match state.subscription.heartbeat() {
                        Ok(frame) => {
                            state.last_cursor = frame.cursor.clone();
                            notification_sse_frame_event(&frame, &state.session_id, &state.tab_id)
                        }
                        Err(error) => {
                            state.done = true;
                            notification_sse_error_event(
                                &state.last_cursor,
                                &state.session_id,
                                &state.tab_id,
                                &format!("heartbeat:{error}"),
                            )
                        }
                    }
                }
                received = state.subscription.recv_frame() => {
                    match received {
                        Ok(frame) => {
                            state.last_cursor = frame.cursor.clone();
                            if frame.terminal
                                || matches!(
                                    &frame.kind,
                                    &kiana_protocol::UiFeedFrameKind::Unknown
                                )
                            {
                                state.done = true;
                            }
                            notification_sse_frame_event(&frame, &state.session_id, &state.tab_id)
                        }
                        Err(kiana_daemon::RunStreamFeedError::Gap(gap)) => {
                            state.done = true;
                            match crate::web_notifications::notification_gap_frame(gap) {
                                Ok(frame) => {
                                    state.last_cursor = frame.cursor.clone();
                                    notification_sse_frame_event(&frame, &state.session_id, &state.tab_id)
                                }
                                Err(error) => notification_sse_error_event(
                                    &state.last_cursor,
                                    &state.session_id,
                                    &state.tab_id,
                                    &format!("gap:{error}"),
                                ),
                            }
                        }
                        Err(kiana_daemon::RunStreamFeedError::Closed) => {
                            state.done = true;
                            notification_sse_error_event(
                                &state.last_cursor,
                                &state.session_id,
                                &state.tab_id,
                                "stream_closed_before_terminal",
                            )
                        }
                        Err(kiana_daemon::RunStreamFeedError::Invalid(error)) => {
                            state.done = true;
                            notification_sse_error_event(
                                &state.last_cursor,
                                &state.session_id,
                                &state.tab_id,
                                &error,
                            )
                        }
                    }
                }
            };
            Some((Ok(event), state))
        },
    );
    Ok(Sse::new(Box::pin(stream)).keep_alive(KeepAlive::default()))
}

fn notification_sse_frame_event(frame: &UiFeedFrameV1, session_id: &str, tab_id: &str) -> Event {
    let event_name = crate::web_notifications::notification_frame_event_name(frame);
    let data = crate::web_notifications::present_notification_frame(frame, session_id, tab_id)
        .and_then(|payload| {
            serde_json::to_string(&payload)
                .map_err(|_| "web_notification_sse_encode_failed".to_owned())
        });
    let id = crate::web_notifications::encode_notification_cursor(&frame.cursor)
        .unwrap_or_else(|_| "notification-cursor-invalid".to_owned());
    match data {
        Ok(data) => Event::default().id(id).event(event_name).data(data),
        Err(error) => notification_sse_error_event(&frame.cursor, session_id, tab_id, &error),
    }
}

fn notification_sse_error_event(
    cursor: &UiFeedCursorV1,
    session_id: &str,
    tab_id: &str,
    error: &str,
) -> Event {
    let id = crate::web_notifications::encode_notification_cursor(cursor)
        .unwrap_or_else(|_| "notification-cursor-invalid".to_owned());
    let payload = crate::web_notifications::present_notification_stream_error(
        cursor, session_id, tab_id, error,
    )
    .unwrap_or_else(|_| {
        json!({
            "schema": crate::web_notifications::WEB_NOTIFICATION_SSE_SCHEMA,
            "kind": "stream_error",
            "snapshot_required": true,
            "retry": "query_original"
        })
    });
    Event::default()
        .id(id)
        .event("stream_error")
        .data(payload.to_string())
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

fn claim_ui_headers(app: &WebApp, headers: &HeaderMap) -> Result<(), ApiError> {
    let epoch = headers.get("x-kiana-ui-epoch");
    let cursor = headers.get("x-kiana-ui-cursor");
    let key = headers.get("x-kiana-action-id");
    if epoch.is_none() && cursor.is_none() && key.is_none() {
        return Ok(());
    }
    let text = |value: Option<&axum::http::HeaderValue>| -> Result<String, ApiError> {
        value
            .and_then(|value| value.to_str().ok())
            .filter(|value| !value.is_empty())
            .map(str::to_owned)
            .ok_or_else(|| ApiError::bad("ui_action_invalid"))
    };
    let action = UiAction {
        target_id: text(headers.get("x-kiana-action-target"))?,
        expected_epoch: text(epoch)?,
        expected_cursor: text(cursor)?
            .parse()
            .map_err(|_| ApiError::bad("ui_cursor_invalid"))?,
        idempotency_key: text(key)?,
    };
    app.host
        .claim_ui_action(&action)
        .map_err(|error| ApiError::conflict(error.to_string()))?;
    Ok(())
}

/// Claim one tab/session mutation after the server has resolved the session owner.  The browser
/// generated action id is only an idempotency handle; principal, tab lease and UI cursor remain
/// server-owned.  A replay returns the exact cached command response and never reaches a runner.
fn claim_action_submission(
    app: &WebApp,
    headers: &HeaderMap,
    session_id: &str,
    tab_id: &str,
) -> Result<WebActionClaim, ApiError> {
    validate_web_session_id(session_id)?;
    validate_web_tab_id(tab_id)?;
    let raw = headers
        .get("x-kiana-action-id")
        .ok_or_else(|| ApiError::bad("ui_action_required"))?;
    let id = raw
        .to_str()
        .map(str::trim)
        .ok()
        .filter(|value| !value.is_empty())
        .ok_or_else(|| ApiError::bad("ui_action_invalid"))?;
    validate_web_opaque_id(id, "action_id")?;
    let principal_id = app.host.authenticated_principal().principal_id;
    let target_id = headers
        .get("x-kiana-action-target")
        .and_then(|value| value.to_str().ok())
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or(session_id);
    if target_id != session_id && target_id != "workspace" {
        return Err(ApiError::conflict("ui_action_target_mismatch"));
    }
    let now = Instant::now();
    let mut submissions = app
        .action_submissions
        .lock()
        .map_err(|_| ApiError::fail("web_action_state_unavailable"))?;
    submissions.retain(|_, submission| {
        now.duration_since(submission.created_at) < WEB_ACTION_SUBMISSION_TTL
    });
    if let Some(original) = submissions.get(id) {
        if original.schema != WEB_ACTION_SUBMISSION_SCHEMA
            || original.session_id != session_id
            || original.tab_id != tab_id
            || original.principal_id != principal_id
            || original.target_id != target_id
        {
            return Err(ApiError::conflict("ui_action_owner_mismatch"));
        }
        if let Some(response) = &original.response {
            return Ok(WebActionClaim::Replay(response.clone()));
        }
        return Err(ApiError::conflict("ui_action_in_flight"));
    }
    // Keep the existing daemon/UI cursor precondition. This is a display fence only; the
    // eventual command still re-enters DaemonHost → ControlPlane → Broker.
    claim_ui_headers(app, headers)?;
    submissions.insert(
        id.to_owned(),
        WebActionSubmission {
            schema: WEB_ACTION_SUBMISSION_SCHEMA,
            session_id: session_id.to_owned(),
            tab_id: tab_id.to_owned(),
            principal_id,
            target_id: target_id.to_owned(),
            created_at: now,
            response: None,
        },
    );
    while submissions.len() > MAX_WEB_ACTION_SUBMISSIONS {
        let oldest = submissions
            .iter()
            .min_by_key(|(_, submission)| submission.created_at)
            .map(|(key, _)| key.clone());
        if let Some(oldest) = oldest {
            submissions.remove(&oldest);
        } else {
            break;
        }
    }
    Ok(WebActionClaim::New { id: id.to_owned() })
}

fn complete_action_submission(
    app: &WebApp,
    claim: &WebActionClaim,
    response: Value,
) -> Result<(), ApiError> {
    let WebActionClaim::New { id } = claim else {
        return Ok(());
    };
    let mut submissions = app
        .action_submissions
        .lock()
        .map_err(|_| ApiError::fail("web_action_state_unavailable"))?;
    let Some(original) = submissions.get_mut(id) else {
        return Err(ApiError::conflict("ui_action_submission_expired"));
    };
    original.response = Some(response);
    Ok(())
}

/// Rotate the in-memory Web token without changing the principal or ControlPlane authority. A
/// stale token can no longer use an existing lease; the next page must hydrate with the new token.
/// 轮换内存中的 Web token。
///
/// 【⚠ 它不是持久化凭据】
/// token 只活在内存里，进程结束就没了。文件头那句「不应被视为跨进程认证机制」
/// 说的就是这个意思：它防的是**另一个进程顺手复用旧 token**，
/// 不是「另一个用户」——后者要靠操作系统层的账户隔离，不靠这个。
///
/// 【轮换的代价】
/// 轮换之后，已经打开的页面手里的旧 token 立刻失效，界面上正在进行的请求会开始
/// 401。这是有意的：新旧 token 并存的时间越长，泄露窗口越大。
fn rotate_web_token(app: &WebApp) -> Result<(), ApiError> {
    *app.web_token
        .lock()
        .map_err(|_| ApiError::fail("web_token_state_unavailable"))? =
        uuid::Uuid::new_v4().to_string();
    app.token_generation
        .fetch_add(1, std::sync::atomic::Ordering::AcqRel);
    Ok(())
}

fn notification_stream_cursor_from_request(
    headers: &HeaderMap,
    query: Option<&str>,
) -> Result<Option<UiFeedCursorV1>, ApiError> {
    let header = headers
        .get("last-event-id")
        .and_then(|value| value.to_str().ok())
        .map(str::trim)
        .filter(|value| !value.is_empty());
    let query = query.map(str::trim).filter(|value| !value.is_empty());
    if header.is_some() && query.is_some() && header != query {
        return Err(ApiError::bad("web_notification_cursor_conflict"));
    }
    let Some(raw) = header.or(query) else {
        return Ok(None);
    };
    crate::web_notifications::decode_notification_cursor(raw)
        .map(Some)
        .map_err(|error| ApiError::bad(error))
}

fn stream_cursor_from_request(
    headers: &HeaderMap,
    query: Option<&str>,
) -> Result<Option<UiCursor>, ApiError> {
    let header = headers
        .get("last-event-id")
        .and_then(|value| value.to_str().ok())
        .map(str::trim)
        .filter(|value| !value.is_empty());
    let query = query.map(str::trim).filter(|value| !value.is_empty());
    if header.is_some() && query.is_some() && header != query {
        return Err(ApiError::bad("stream_cursor_conflict"));
    }
    let raw = header.or(query);
    let Some(raw) = raw else {
        return Ok(None);
    };
    if raw.len() > MAX_WEB_SSE_CURSOR_BYTES
        || raw.chars().filter(|character| *character == ':').count() != 1
    {
        return Err(ApiError::bad("stream_cursor_invalid"));
    }
    let (epoch, sequence) = raw
        .rsplit_once(':')
        .ok_or_else(|| ApiError::bad("stream_cursor_invalid"))?;
    if epoch.is_empty()
        || epoch.len() > 128
        || epoch
            .chars()
            .any(|character| character.is_control() || character.is_whitespace())
    {
        return Err(ApiError::bad("stream_cursor_invalid"));
    }
    Ok(Some(UiCursor {
        epoch: epoch.to_owned(),
        sequence: sequence
            .parse()
            .map_err(|_| ApiError::bad("stream_cursor_invalid"))?,
    }))
}

fn run_stream_sse_event(name: &str, envelope: &RunStreamEnvelope) -> Event {
    let cursor = UiCursor {
        epoch: envelope.epoch.clone(),
        sequence: envelope.sequence,
    };
    let data = bounded_sse_data(
        serde_json::to_string(envelope),
        "stream_serialize_failed",
        &cursor,
    );
    Event::default()
        .id(format!("{}:{}", envelope.epoch, envelope.sequence))
        .event(name)
        .data(data)
}

fn stream_gap_sse_event(run_id: RunId, reason: &str) -> Event {
    stream_gap_sse_event_with_cursor(run_id, reason, &UiCursor::default())
}

fn stream_error_sse_event(error: &str) -> Event {
    stream_error_sse_event_with_cursor(error, &UiCursor::default())
}

fn stream_gap_sse_event_with_cursor(run_id: RunId, reason: &str, cursor: &UiCursor) -> Event {
    let id = stream_event_id(cursor);
    let reason = bounded_sse_reason(reason);
    Event::default().id(id).event("stream_gap").data(
        json!({
            "schema": WEB_SSE_SCHEMA,
            "epoch": cursor.epoch.clone(),
            "sequence": cursor.sequence,
            "run_id": run_id.to_string(),
            "reason": reason,
            "snapshot_required": true,
            "hydrate": true,
        })
        .to_string(),
    )
}

fn stream_error_sse_event_with_cursor(error: &str, cursor: &UiCursor) -> Event {
    let error = bounded_sse_reason(error);
    Event::default()
        .id(stream_event_id(cursor))
        .event("stream_error")
        .data(
            json!({
                "schema": WEB_SSE_SCHEMA,
                "epoch": cursor.epoch.clone(),
                "sequence": cursor.sequence,
                "error": error,
                "hydrate": true,
            })
            .to_string(),
        )
}

fn bounded_sse_reason(value: &str) -> String {
    value.chars().take(MAX_WEB_SSE_REASON_BYTES).collect()
}

fn stream_heartbeat_sse_event(cursor: &UiCursor) -> Event {
    Event::default()
        .id(stream_event_id(cursor))
        .event("heartbeat")
        .data(
            json!({
                "schema": WEB_SSE_SCHEMA,
                "epoch": cursor.epoch.clone(),
                "sequence": cursor.sequence,
                "kind": "heartbeat",
            })
            .to_string(),
        )
}

fn stream_event_id(cursor: &UiCursor) -> String {
    if cursor.epoch.is_empty() {
        "0:0".to_owned()
    } else {
        format!("{}:{}", cursor.epoch, cursor.sequence)
    }
}

fn bounded_sse_data(
    serialized: Result<String, serde_json::Error>,
    error_prefix: &str,
    cursor: &UiCursor,
) -> String {
    match serialized {
        Ok(data) if data.len() <= MAX_WEB_SSE_EVENT_BYTES => data,
        Ok(_) => json!({
            "schema": WEB_SSE_SCHEMA,
            "epoch": cursor.epoch.clone(),
            "sequence": cursor.sequence,
            "error": "stream_payload_too_large",
            "hydrate": true,
        })
        .to_string(),
        Err(error) => json!({
            "schema": WEB_SSE_SCHEMA,
            "epoch": cursor.epoch.clone(),
            "sequence": cursor.sequence,
            "error": format!("{error_prefix}:{error}"),
            "hydrate": true,
        })
        .to_string(),
    }
}

async fn run_turn(
    State(app): State<Arc<WebApp>>,
    headers: HeaderMap,
    Json(body): Json<RunBody>,
) -> Result<Json<Value>, ApiError> {
    authorize_mutation(&app, &headers)?;
    let tab_id = require_web_tab(&headers)?;
    let prompt = validate_web_prompt(body.prompt)?;
    let session_id = resolve_mutable_session(&app, body.session_id.as_deref()).await?;
    require_session_owner(&app, &session_id, &tab_id)?;
    let action_id = claim_action_submission(&app, &headers, &session_id, &tab_id)?;
    if let WebActionClaim::Replay(response) = &action_id {
        return Ok(Json(response.clone()));
    }
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
            attach_optional_hydrate_tab(&mut payload, &headers)?;
            payload["response"] = serde_json::to_value(&response).unwrap_or(Value::Null);
            complete_action_submission(&app, &action_id, payload.clone())?;
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
    let tab_id = require_web_tab(&headers)?;
    let session_id = resolve_mutable_session(&app, body.session_id.as_deref()).await?;
    require_session_owner(&app, &session_id, &tab_id)?;
    let action_id = claim_action_submission(&app, &headers, &session_id, &tab_id)?;
    if let WebActionClaim::Replay(response) = &action_id {
        return Ok(Json(response.clone()));
    }
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
    let mut snapshot = app.snapshot(&session_id).await?;
    attach_optional_hydrate_tab(&mut snapshot, &headers)?;
    snapshot["response"] = serde_json::to_value(&response).unwrap_or(Value::Null);
    complete_action_submission(&app, &action_id, snapshot.clone())?;
    Ok(Json(snapshot))
}

async fn trust_folder(
    State(app): State<Arc<WebApp>>,
    headers: HeaderMap,
    body: Option<Json<SessionBody>>,
) -> Result<Json<Value>, ApiError> {
    authorize_mutation(&app, &headers)?;
    let tab_id = require_web_tab(&headers)?;
    let requested = body
        .as_ref()
        .and_then(|Json(body)| body.session_id.as_deref());
    let session_id = resolve_mutable_session(&app, requested).await?;
    // 【为什么必须先认领再校验归属】
    // `require_session_owner` 读的是 `owner_tab_id == Some(tab)` 且 `owner_active`；而全新会话
    // 的 `owner_tab_id` 是 `None`。于是这里形成了一个**先有鸡还是先有蛋**的死锁：
    // 从未 bootstrap 过的会话，`/api/trust` 永远拿不到归属，于是永远无法被信任。
    // `bootstrap`（`GET /api/bootstrap`）是先 `claim_session_tab` 再往下走的，trust 这条
    // 路径却没有，于是两条入口对「谁先建立归属」的处理并不一致。
    // 补上认领后，`require_session_owner` 校验的仍是**认领之后**的归属，
    // 并不会绕过它：若该 tab 不是 owner，`claim_session_tab` 不会改写既有归属，
    // 随后的校验仍会以 `session_owner_required` 拒绝。
    claim_session_tab(&app, &session_id, &tab_id)?;
    require_session_owner(&app, &session_id, &tab_id)?;
    let action_id = claim_action_submission(&app, &headers, &session_id, &tab_id)?;
    if let WebActionClaim::Replay(response) = &action_id {
        return Ok(Json(response.clone()));
    }
    write_project_trust(&app.workdir, ProjectTrust::Trusted).map_err(ApiError::fail)?;
    let mut snapshot = app.snapshot(&session_id).await?;
    attach_optional_hydrate_tab(&mut snapshot, &headers)?;
    complete_action_submission(&app, &action_id, snapshot.clone())?;
    Ok(Json(snapshot))
}

async fn set_sandbox(
    State(app): State<Arc<WebApp>>,
    headers: HeaderMap,
    Json(body): Json<SandboxBody>,
) -> Result<Json<Value>, ApiError> {
    authorize_mutation(&app, &headers)?;
    let tab_id = require_web_tab(&headers)?;
    let session_id = resolve_human_session(&app, body.session_id.as_deref()).await?;
    require_session_owner(&app, &session_id, &tab_id)?;
    let action_id = claim_action_submission(&app, &headers, &session_id, &tab_id)?;
    if let WebActionClaim::Replay(response) = &action_id {
        return Ok(Json(response.clone()));
    }
    let sandbox = workbench_chat::normalize_sandbox(&body.sandbox)
        .map_err(|error| ApiError::bad(error.to_string()))?;
    *app.sandbox
        .lock()
        .map_err(|_| ApiError::fail("web_state_poisoned"))? = sandbox;
    let mut snapshot = app.snapshot(&session_id).await?;
    attach_optional_hydrate_tab(&mut snapshot, &headers)?;
    complete_action_submission(&app, &action_id, snapshot.clone())?;
    Ok(Json(snapshot))
}

async fn new_session(
    State(app): State<Arc<WebApp>>,
    headers: HeaderMap,
) -> Result<Json<Value>, ApiError> {
    authorize_mutation(&app, &headers)?;
    let tab_id = require_web_tab(&headers)?;
    let action_scope = lock_string(&app.active)?;
    let action_id = claim_action_submission(&app, &headers, &action_scope, &tab_id)?;
    if let WebActionClaim::Replay(response) = &action_id {
        return Ok(Json(response.clone()));
    }
    let session_id = new_session_id();
    {
        let mut sessions = app
            .sessions
            .lock()
            .map_err(|_| ApiError::fail("web_state_poisoned"))?;
        ensure_session_capacity(&sessions)?;
        sessions.insert(
            session_id.clone(),
            WebSession::new(app.host.authenticated_principal().principal_id),
        );
    }
    claim_session_tab(&app, &session_id, &tab_id)?;
    *app.active
        .lock()
        .map_err(|_| ApiError::fail("web_state_poisoned"))? = session_id.clone();
    let mut snapshot = app.snapshot(&session_id).await?;
    attach_optional_hydrate_tab(&mut snapshot, &headers)?;
    complete_action_submission(&app, &action_id, snapshot.clone())?;
    Ok(Json(snapshot))
}

async fn read_receipt(
    State(app): State<Arc<WebApp>>,
    headers: HeaderMap,
    Json(body): Json<SessionBody>,
) -> Result<Json<Value>, ApiError> {
    authorize_mutation(&app, &headers)?;
    let _ = optional_web_tab(&headers)?;
    let session_id = body.session_id.unwrap_or(lock_string(&app.active)?);
    let snapshot = app.snapshot(&session_id).await?;
    let run_id = snapshot["projection"]["run_id"]
        .as_str()
        .and_then(RunId::parse_str);
    let options = app.options()?;
    let response = harness_run::receipt_envelope_on_host(
        Arc::clone(&app.host),
        session_id.clone(),
        run_id,
        &options,
    )
    .await
    .map_err(|error| ApiError::fail(error.to_string()))?;
    let mut state = app.snapshot(&session_id).await?;
    attach_optional_hydrate_tab(&mut state, &headers)?;
    Ok(Json(json!({
        "state": state,
        "receipt": response,
    })))
}

async fn list_approvals(
    State(app): State<Arc<WebApp>>,
    headers: HeaderMap,
    Query(body): Query<SessionBody>,
) -> Result<Json<Value>, ApiError> {
    authorize_mutation(&app, &headers)?;
    let _ = optional_web_tab(&headers)?;
    let session_id = resolve_human_session(&app, body.session_id.as_deref()).await?;
    let response = harness_run::pending_approvals_envelope_on_host(
        app.host.clone(),
        session_id,
        None,
        &app.options()?,
    )
    .await
    .map_err(|error| ApiError::fail(error.to_string()))?;
    Ok(Json(json!({"response": response})))
}

async fn command_action(
    State(app): State<Arc<WebApp>>,
    headers: HeaderMap,
    Json(body): Json<CommandBody>,
) -> Result<Json<Value>, ApiError> {
    authorize_mutation(&app, &headers)?;
    let tab_id = require_web_tab(&headers)?;
    if body.name.is_empty() || body.name.len() > 128 || !body.arguments.is_object() {
        return Err(ApiError::bad("command_request_invalid"));
    }
    // Human Inbox actions cross the Web boundary as a typed intent.  Validate before claiming
    // the idempotency slot so malformed/forged cards cannot consume a replay key.
    crate::web_inbox::validate_human_action_command(&body.name, &body.arguments)
        .map_err(ApiError::bad)?;
    // INT-14: typed connector commands are re-encoded at the Web boundary through the same
    // surface encoder used by CLI/Workbench/MCP, before any replay key is claimed; legacy
    // untyped arguments keep the existing compatibility path.
    let surface_command = crate::connector_command::surface_command_route(
        kiana_domain::ConnectorSurface::Web,
        &body.name,
        body.arguments,
    )
    .map_err(ApiError::bad)?;
    let session_id = resolve_human_session(&app, Some(&body.session_id)).await?;
    require_session_owner(&app, &session_id, &tab_id)?;
    let action_id = claim_action_submission(&app, &headers, &session_id, &tab_id)?;
    if let WebActionClaim::Replay(response) = &action_id {
        return Ok(Json(response.clone()));
    }
    let human_resolve = body.name == crate::web_inbox::WEB_HUMAN_RESOLVE_COMMAND;
    let response = harness_run::command_envelope_on_host(
        app.host.clone(),
        session_id.clone(),
        surface_command.name,
        surface_command.arguments,
        &app.options()?,
    )
    .await
    .map_err(|error| ApiError::fail(error.to_string()))?;
    let mut response_value = serde_json::to_value(&response)
        .map_err(|_| ApiError::fail("web_human_action_response_encode"))?;
    if human_resolve {
        let result_class = crate::web_inbox::human_result_class(&response_value);
        response_value["web_result_class"] = json!(result_class);
    }
    let mut state = app.snapshot(&session_id).await?;
    attach_optional_hydrate_tab(&mut state, &headers)?;
    let payload = json!({ "state": state, "response": response_value });
    complete_action_submission(&app, &action_id, payload.clone())?;
    Ok(Json(payload))
}

#[derive(Deserialize)]
struct CommandQuery {
    name: String,
    session_id: Option<String>,
    arguments: Option<String>,
}

#[derive(Clone, Debug, Deserialize)]
struct ExtensionVisibilityQuery {
    #[serde(default)]
    session_id: Option<String>,
    #[serde(default)]
    action: Option<String>,
    #[serde(default)]
    query: Option<String>,
    #[serde(default)]
    extension_id: Option<String>,
    #[serde(default)]
    max_results: Option<usize>,
    /// Desktop uses the same Web router; this label only records the presenting surface.
    #[serde(default)]
    surface: Option<String>,
}

async fn extension_visibility(
    State(app): State<Arc<WebApp>>,
    headers: HeaderMap,
    Query(query): Query<ExtensionVisibilityQuery>,
) -> Result<Json<Value>, ApiError> {
    authorize_mutation(&app, &headers)?;
    let action = query.action.unwrap_or_else(|| "list".to_owned());
    if !matches!(action.as_str(), "list" | "search" | "inspect") {
        return Err(ApiError::bad("extension_visibility_action_unsupported"));
    }
    let max_results = query.max_results.unwrap_or(128);
    if max_results == 0 || max_results > kiana_protocol::MAX_EXTENSION_VISIBILITY_ENTRIES {
        return Err(ApiError::bad("extension_visibility_max_results_invalid"));
    }
    let surface = match query.surface.as_deref().unwrap_or("web") {
        "web" => kiana_protocol::EntryPointKind::Web,
        "desktop" => kiana_protocol::EntryPointKind::Desktop,
        _ => return Err(ApiError::bad("extension_visibility_surface_invalid")),
    };
    let session_id = resolve_human_session(&app, query.session_id.as_deref()).await?;
    let options = app.options()?;
    let snapshot = crate::extension_projection::extension_visibility_on_host(
        Arc::clone(&app.host),
        session_id,
        surface,
        &action,
        query.query,
        query.extension_id,
        max_results,
        &options,
    )
    .await
    .map_err(|error| ApiError::fail(error.to_string()))?;
    let projection = crate::extension_projection::visibility_json(&snapshot)
        .map_err(|error| ApiError::fail(error.to_string()))?;
    Ok(Json(json!({
        "schema": "kiana.extension-visibility-route.v1",
        "surface": surface,
        "snapshot": projection,
    })))
}

async fn command_query(
    State(app): State<Arc<WebApp>>,
    headers: HeaderMap,
    Query(query): Query<CommandQuery>,
) -> Result<Json<Value>, ApiError> {
    authorize_mutation(&app, &headers)?;
    if !matches!(
        query.name.as_str(),
        "human.inbox"
            | "failure.incidents"
            | "feedback.list"
            | "workspace.checkpoint.list"
            | "workspace.checkpoint.preview"
    ) {
        return Err(ApiError::bad("command_read_only_required"));
    }
    let session_id = resolve_human_session(&app, query.session_id.as_deref()).await?;
    let arguments = query.arguments.unwrap_or_else(|| "{}".to_owned());
    if arguments.len() > 16 * 1024 {
        return Err(ApiError::bad("command_arguments_limit"));
    }
    let arguments: Value =
        serde_json::from_str(&arguments).map_err(|_| ApiError::bad("command_arguments_invalid"))?;
    let human_inbox_query = query.name == "human.inbox";
    let response = harness_run::command_envelope_on_host(
        app.host.clone(),
        session_id,
        query.name,
        arguments,
        &app.options()?,
    )
    .await
    .map_err(|error| ApiError::fail(error.to_string()))?;
    let mut response =
        serde_json::to_value(response).map_err(|_| ApiError::fail("web_human_response_encode"))?;
    if human_inbox_query {
        crate::web_inbox::annotate_human_inbox_response(&mut response);
    }
    Ok(Json(json!({"response":response})))
}

async fn decide_approval(
    State(app): State<Arc<WebApp>>,
    headers: HeaderMap,
    Json(body): Json<ApprovalBody>,
) -> Result<Json<Value>, ApiError> {
    authorize_mutation(&app, &headers)?;
    let tab_id = require_web_tab(&headers)?;
    let session_id = resolve_human_session(&app, Some(&body.session_id)).await?;
    require_session_owner(&app, &session_id, &tab_id)?;
    let action_id = claim_action_submission(&app, &headers, &session_id, &tab_id)?;
    if let WebActionClaim::Replay(response) = &action_id {
        return Ok(Json(response.clone()));
    }
    let response = harness_run::decide_approval_envelope_on_host(
        app.host.clone(),
        session_id.clone(),
        body.challenge,
        body.decision,
        &app.options()?,
    )
    .await
    .map_err(|error| ApiError::fail(error.to_string()))?;
    // A human decision does not grant an old session a Runner continuation.
    let live_session = app
        .sessions
        .lock()
        .map_err(|_| ApiError::fail("web_state_poisoned"))?
        .contains_key(&session_id);
    if live_session {
        store_turn(&app, &session_id, "(approval)", &response)?;
    }
    let mut snapshot = app.snapshot(&session_id).await?;
    attach_optional_hydrate_tab(&mut snapshot, &headers)?;
    snapshot["response"] = serde_json::to_value(response).unwrap_or(Value::Null);
    complete_action_submission(&app, &action_id, snapshot.clone())?;
    Ok(Json(snapshot))
}

/// 浏览器提交的运行中 steer 请求体。
///
/// `turn_id` 是调用方**在流上观察到的**当前 turn：服务端用它做乐观并发检查，过期时返回
/// `stale_turn_steer`，不会把文本插进更晚的 turn。`deny_unknown_fields` 让
/// `sandbox` / 模型档位之类的字段在协议边界就被拒绝，而不是被静默丢弃。
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct SteerBody {
    session_id: String,
    run_id: String,
    turn_id: String,
    text: String,
}

/// 浏览器提交的 inject 请求体。
///
/// inject 不做 turn 断言，也不唤醒空闲 run；来源由服务端固定为 `web`，浏览器不能把输入
/// 伪装成其它入口的上下文。
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct InjectBody {
    session_id: String,
    run_id: String,
    #[serde(default)]
    turn_id: Option<String>,
    #[serde(default)]
    target: Option<String>,
    text: String,
}

/// 把一条绑定当前 turn 的引导排进指定 run；授权与生命周期仍由 ControlPlane 判定。
async fn steer_turn(
    State(app): State<Arc<WebApp>>,
    headers: HeaderMap,
    Json(body): Json<SteerBody>,
) -> Result<Json<Value>, ApiError> {
    authorize_mutation(&app, &headers)?;
    let tab_id = require_web_tab(&headers)?;
    if body.text.trim().is_empty() {
        return Err(ApiError::bad("web_input_text_required"));
    }
    let run_id = kiana_protocol::RunId::parse_str(&body.run_id)
        .ok_or_else(|| ApiError::bad("web_run_id_invalid"))?;
    let turn_id = kiana_protocol::TurnId::parse_str(&body.turn_id)
        .ok_or_else(|| ApiError::bad("web_turn_id_invalid"))?;
    let session_id = resolve_human_session(&app, Some(body.session_id.as_str())).await?;
    require_session_owner(&app, &session_id, &tab_id)?;
    // 先校验形状再占用幂等槽：畸形请求不能消费重放键。
    let action_id = claim_action_submission(&app, &headers, &session_id, &tab_id)?;
    if let WebActionClaim::Replay(response) = &action_id {
        return Ok(Json(response.clone()));
    }
    let response = harness_run::steer_envelope_on_host(
        app.host.clone(),
        session_id.clone(),
        run_id,
        turn_id,
        body.text,
        &app.options()?,
    )
    .await
    .map_err(|error| ApiError::fail(error.to_string()))?;
    let mut snapshot = app.snapshot(&session_id).await?;
    attach_optional_hydrate_tab(&mut snapshot, &headers)?;
    snapshot["response"] = serde_json::to_value(&response).unwrap_or(Value::Null);
    complete_action_submission(&app, &action_id, snapshot.clone())?;
    Ok(Json(snapshot))
}

/// 把一条带来源标签的上下文输入排给指定 run；空闲 run 不会被这条输入唤醒。
async fn inject_turn(
    State(app): State<Arc<WebApp>>,
    headers: HeaderMap,
    Json(body): Json<InjectBody>,
) -> Result<Json<Value>, ApiError> {
    authorize_mutation(&app, &headers)?;
    let tab_id = require_web_tab(&headers)?;
    if body.text.trim().is_empty() {
        return Err(ApiError::bad("web_input_text_required"));
    }
    let run_id = kiana_protocol::RunId::parse_str(&body.run_id)
        .ok_or_else(|| ApiError::bad("web_run_id_invalid"))?;
    let target_turn_id = match body.turn_id.as_deref() {
        Some(turn) => Some(
            kiana_protocol::TurnId::parse_str(turn)
                .ok_or_else(|| ApiError::bad("web_turn_id_invalid"))?,
        ),
        None => None,
    };
    let target = body.target.unwrap_or_else(|| "next-step".to_owned());
    let session_id = resolve_human_session(&app, Some(body.session_id.as_str())).await?;
    require_session_owner(&app, &session_id, &tab_id)?;
    let action_id = claim_action_submission(&app, &headers, &session_id, &tab_id)?;
    if let WebActionClaim::Replay(response) = &action_id {
        return Ok(Json(response.clone()));
    }
    // 来源是服务端固定的入口标签，不接受浏览器自称其它来源。
    let response = harness_run::inject_envelope_on_host(
        app.host.clone(),
        session_id.clone(),
        run_id,
        target,
        "web",
        body.text,
        target_turn_id,
        &app.options()?,
    )
    .await
    .map_err(|error| ApiError::fail(error.to_string()))?;
    let mut snapshot = app.snapshot(&session_id).await?;
    attach_optional_hydrate_tab(&mut snapshot, &headers)?;
    snapshot["response"] = serde_json::to_value(&response).unwrap_or(Value::Null);
    complete_action_submission(&app, &action_id, snapshot.clone())?;
    Ok(Json(snapshot))
}

async fn resume_turn(
    State(app): State<Arc<WebApp>>,
    headers: HeaderMap,
    Json(body): Json<SessionBody>,
) -> Result<Json<Value>, ApiError> {
    authorize_mutation(&app, &headers)?;
    let session_id = resolve_human_session(&app, body.session_id.as_deref()).await?;
    let tab_id = require_web_tab(&headers)?;
    require_session_owner(&app, &session_id, &tab_id)?;
    let action_id = claim_action_submission(&app, &headers, &session_id, &tab_id)?;
    if let WebActionClaim::Replay(response) = &action_id {
        return Ok(Json(response.clone()));
    }
    let response = harness_run::resume_envelope_on_host(
        app.host.clone(),
        session_id.clone(),
        None,
        &app.options()?,
    )
    .await
    .map_err(|error| ApiError::fail(error.to_string()))?;
    if response.output["restored"] == true
        && matches!(
            response.status,
            kiana_protocol::ExecutionStatus::AwaitingApproval
                | kiana_protocol::ExecutionStatus::Running
                | kiana_protocol::ExecutionStatus::Accepted
                | kiana_protocol::ExecutionStatus::Completed
        )
    {
        let (events, history) = app.read_session_ledger().await?;
        let previous = ledger_thread_for_session(&events, &history, &session_id);
        let mut sessions = app
            .sessions
            .lock()
            .map_err(|_| ApiError::fail("web_state_poisoned"))?;
        if !sessions.contains_key(&session_id) {
            ensure_session_capacity(&sessions)?;
            let mut restored = WebSession::new(app.host.authenticated_principal().principal_id);
            if let Some(thread) = previous {
                restored.name = thread.session.name;
                restored.run_id = RunId::parse_str(&thread.session.run_id);
                restored.turns = thread.turns;
                restored.last = thread.last;
            }
            sessions.insert(session_id.clone(), restored);
        }
        drop(sessions);
        store_turn(&app, &session_id, "(resume)", &response)?;
    }
    let mut snapshot = app.snapshot(&session_id).await?;
    attach_optional_hydrate_tab(&mut snapshot, &headers)?;
    snapshot["response"] = serde_json::to_value(response).unwrap_or(Value::Null);
    complete_action_submission(&app, &action_id, snapshot.clone())?;
    Ok(Json(snapshot))
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
        "sandbox": session.sandbox,
        "human_actions_allowed": true,
        "resume_required": true,
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
        "sandbox": thread.session.sandbox,
        "human_actions_allowed": true,
        "resume_required": true,
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
                    sandbox: event.data["sandbox"].as_str().map(str::to_owned),
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
            session.sandbox = event.data["sandbox"].as_str().map(str::to_owned);
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
            if session.terminal_status != "unknown" && session.terminal_status != status {
                session.terminal_status = "result_unknown".to_owned();
            } else {
                session.terminal_status = status.to_owned();
            }
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
                    if turn.status != "unknown" && turn.status != "completed" {
                        turn.status = "result_unknown".to_owned();
                        turn.error = Some("run_terminal_conflict".to_owned());
                    } else {
                        turn.status = "completed".to_owned();
                    }
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
                        if turn.status != "unknown" && turn.status != status {
                            turn.status = "result_unknown".to_owned();
                            turn.error = Some("run_terminal_conflict".to_owned());
                            continue;
                        }
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
        let item_prefix = self.id.clone();
        let text = if self.final_text.is_empty() {
            redact_history_text(&self.delta_text)
        } else {
            redact_history_text(&self.final_text)
        };
        let mut items = vec![web_thread::ItemView {
            id: format!("{item_prefix}:user"),
            kind: "userMessage".to_owned(),
            status: "completed".to_owned(),
            title: "You".to_owned(),
            body: redact_history_text(&self.prompt),
        }];
        for (index, capability) in self.capabilities.into_iter().enumerate() {
            items.push(web_thread::ItemView {
                id: format!("{item_prefix}:tool:{index}"),
                kind: "commandExecution".to_owned(),
                status: self.status.clone(),
                title: format!("tool · {capability}"),
                body: capability,
            });
        }
        if !self.files.is_empty() {
            items.push(web_thread::ItemView {
                id: format!("{item_prefix}:files"),
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
                id: format!("{item_prefix}:assistant"),
                kind: "agentMessage".to_owned(),
                status: self.status.clone(),
                title: "Builder".to_owned(),
                body: text,
            });
        }
        if let Some(error) = self.error.filter(|error| !error.trim().is_empty()) {
            items.push(web_thread::ItemView {
                id: format!("{item_prefix}:error"),
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
        "run.denied" => Some("denied"),
        "run.blocked" => Some("blocked"),
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

fn path_contains_traversal(path: &str) -> bool {
    let lowered = path.to_ascii_lowercase();
    path.split('/')
        .any(|segment| segment == ".." || segment == ".")
        || lowered.contains("%2e")
        || lowered.contains("%2f")
        || lowered.contains("%5c")
        || path.contains('\\')
}

fn validate_web_session_id(value: &str) -> Result<(), ApiError> {
    let value = value.trim();
    if value.is_empty()
        || value.len() > 128
        || value.contains('/')
        || value.contains('\\')
        || value.contains("..")
        || value.chars().any(char::is_control)
    {
        return Err(ApiError::bad("session_invalid"));
    }
    Ok(())
}

fn validate_web_opaque_id(value: &str, field: &str) -> Result<(), ApiError> {
    if value.trim().is_empty()
        || value.len() > 256
        || value.contains('/')
        || value.contains('\\')
        || value.chars().any(char::is_control)
    {
        return Err(ApiError::bad(format!("{field}_invalid")));
    }
    Ok(())
}

fn validate_web_tab_id(value: &str) -> Result<(), ApiError> {
    if value.trim().is_empty()
        || value.len() > MAX_WEB_TAB_BYTES
        || value.contains('/')
        || value.contains('\\')
        || value.chars().any(char::is_control)
    {
        return Err(ApiError::bad("web_tab_invalid"));
    }
    Ok(())
}

fn optional_web_tab(headers: &HeaderMap) -> Result<Option<String>, ApiError> {
    let value = single_header(headers, "x-kiana-ui-tab")?;
    let Some(value) = value.map(str::trim).filter(|value| !value.is_empty()) else {
        return Ok(None);
    };
    validate_web_tab_id(value)?;
    Ok(Some(value.to_owned()))
}

fn require_web_tab(headers: &HeaderMap) -> Result<String, ApiError> {
    optional_web_tab(headers)?.ok_or_else(|| ApiError::bad("web_tab_required"))
}

fn attach_hydrate_tab(snapshot: &mut Value, tab_id: &str) -> Result<(), ApiError> {
    validate_web_tab_id(tab_id)?;
    let hydrate = snapshot
        .get_mut("hydrate")
        .and_then(Value::as_object_mut)
        .ok_or_else(|| ApiError::fail("web_hydrate_missing"))?;
    hydrate.insert("tab_id".to_owned(), Value::String(tab_id.to_owned()));
    if let Some(lease) = snapshot.get_mut("lease").and_then(Value::as_object_mut) {
        let owner = lease
            .get("owner_tab_id")
            .and_then(Value::as_str)
            .is_some_and(|owner| owner == tab_id);
        lease.insert(
            "disposition".to_owned(),
            Value::String(if owner {
                "owner".to_owned()
            } else {
                "observer".to_owned()
            }),
        );
        lease.insert("feed_only".to_owned(), Value::Bool(!owner));
        snapshot["human_actions_allowed"] = Value::Bool(
            owner
                && snapshot
                    .get("read_only")
                    .and_then(Value::as_bool)
                    .is_none_or(|read_only| !read_only),
        );
    }
    Ok(())
}

fn attach_optional_hydrate_tab(snapshot: &mut Value, headers: &HeaderMap) -> Result<(), ApiError> {
    if let Some(tab_id) = optional_web_tab(headers)? {
        attach_hydrate_tab(snapshot, &tab_id)?;
    }
    Ok(())
}

/// Mark a hydrate envelope that projects a closed EventLog session.
///
/// `attach_hydrate_tab` relabels a lease, but a historical session has no live lease, no mutable
/// draft and no observer relationship. Restore the closed disposition, keep the response
/// feed-only and expose the projection source so a browser cannot mistake history for an active
/// session it could resume from this snapshot.
fn mark_historical_hydrate(snapshot: &mut Value) {
    if let Some(lease) = snapshot.get_mut("lease").and_then(Value::as_object_mut) {
        lease.insert("disposition".to_owned(), Value::String("closed".to_owned()));
        lease.insert("feed_only".to_owned(), Value::Bool(true));
    }
    snapshot["human_actions_allowed"] = Value::Bool(false);
    if let Some(hydrate) = snapshot.get_mut("hydrate").and_then(Value::as_object_mut) {
        hydrate.insert("historical".to_owned(), Value::Bool(true));
        hydrate.insert(
            "source".to_owned(),
            Value::String("event_log_projection".to_owned()),
        );
    }
}

fn validate_web_page_limit(limit: usize) -> Result<(), ApiError> {
    if limit == 0 || limit > MAX_WEB_HISTORY_PAGE {
        return Err(ApiError::bad("web_page_limit_invalid"));
    }
    Ok(())
}

fn web_now_unix_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis().min(u128::from(u64::MAX)) as u64)
        .unwrap_or(0)
}

fn history_thread_is_limited(events: &[LedgerEvent], run_id: &str) -> bool {
    events
        .iter()
        .filter(|event| {
            event.kind == "run.prompt"
                && event.data.get("run_id").and_then(Value::as_str) == Some(run_id)
        })
        .count()
        > MAX_WEB_TURNS_PER_SESSION
}

fn artifact_entry(event: &LedgerEvent, requested: Option<&str>) -> Option<Value> {
    let object = event
        .data
        .get("artifact_ref")
        .filter(|value| value.is_object());
    let id = event
        .data
        .get("artifact_id")
        .and_then(Value::as_str)
        .or_else(|| {
            object
                .and_then(|value| value.get("artifact_id"))
                .and_then(Value::as_str)
        })?;
    if requested.is_some_and(|requested| requested != id) {
        return None;
    }
    Some(json!({
        "artifact_id": id,
        "source_event_id": event.event_id,
        "source_sequence": event.sequence,
        "schema": event.data.get("artifact_schema").cloned().unwrap_or(Value::Null),
        "digest": event.data.get("content_hash")
            .or_else(|| event.data.get("artifact_digest"))
            .cloned()
            .or_else(|| object.and_then(|value| value.get("content_hash")).cloned())
            .unwrap_or(Value::Null),
        "content": Value::Null
    }))
}

fn authorize_mutation(app: &WebApp, headers: &HeaderMap) -> Result<(), ApiError> {
    if app.shutting_down.load(Ordering::Acquire) {
        return Err(ApiError {
            status: StatusCode::SERVICE_UNAVAILABLE,
            error: "web_shutting_down".to_owned(),
        });
    }
    let supplied = web_token_from_header(headers)?;
    authorize_web_request(app, headers, supplied)
}

fn authorize_sse(
    app: &WebApp,
    headers: &HeaderMap,
    query_token: Option<&str>,
) -> Result<(), ApiError> {
    let header_token = web_token_from_header(headers)?;
    let query_token = query_token.map(str::trim).filter(|value| !value.is_empty());
    if header_token.is_some() && query_token.is_some() && header_token != query_token {
        return Err(ApiError::unauthorized());
    }
    let supplied = header_token.or(query_token);
    authorize_web_request(app, headers, supplied)
}

fn web_token_from_header(headers: &HeaderMap) -> Result<Option<&str>, ApiError> {
    let value = single_header(headers, WEB_TOKEN_HEADER)?;
    Ok(value.map(str::trim).filter(|value| !value.is_empty()))
}

/// Rotate the in-memory Web token.
///
/// Rotation is a local exposure-control action: it replaces the process credential, bumps the
/// monotonic `token_generation` the snapshot/lease projections report, and invalidates every
/// previously issued credential.  It never changes the authenticated principal, a session lease,
/// or any ControlPlane authority — a rotated page still has to pass the normal owner/CAS checks.
/// The response deliberately does not echo a credential: the page re-hydrates from the host-only
/// root document, which serves the current token.
async fn rotate_web_token_route(
    State(app): State<Arc<WebApp>>,
    headers: HeaderMap,
) -> Result<Json<Value>, ApiError> {
    authorize_mutation(&app, &headers)?;
    rotate_web_token(&app)?;
    let token_generation = app
        .token_generation
        .load(std::sync::atomic::Ordering::Acquire);
    Ok(Json(json!({
        "schema": WEB_TOKEN_ROTATION_SCHEMA,
        "rotated": true,
        "token_generation": token_generation,
        "rehydrate_route": "/",
        "principal_changed": false,
        "lease_changed": false,
    })))
}

/// 每个 API 请求都要过这一关：**先 token，再 Host。**
///
/// 【顺序有讲究】
/// token 在前，Host 在后。反过来的话，一个不合法 Host 的探测请求会先触发
/// Host 检查并同样得到「未授权」，而攻击者无法据此区分「Host 不对」与「token 不对」——
/// 少一条可用于探测的信息。
///
/// 【⚠ 它证明的是什么】
/// 证明「这个请求来自本 Web UI，且不是从别的站点发来的」。
/// **不证明**「这个操作应该被允许」——那是 `harness_run` 到 `ControlPlane` 的事。
/// 把两者混为一谈，就会出现「token 对了就放行」的界面级授权。
fn authorize_web_request(
    app: &WebApp,
    headers: &HeaderMap,
    supplied: Option<&str>,
) -> Result<(), ApiError> {
    let current = app
        .web_token
        .lock()
        .map_err(|_| ApiError::fail("web_token_state_unavailable"))?;
    if supplied != Some(current.as_str()) {
        return Err(ApiError::unauthorized());
    }
    authorize_host(app, headers)?;
    Ok(())
}

/// Host 必须匹配绑定地址；Origin 若存在也必须匹配。
///
/// 【这一条防的是 DNS rebinding，不是「伪造 Host」】
/// 设想这样一个页面：攻击者的网页把一个域名解析到 127.0.0.1，然后让浏览器去请求
/// `http://恶意域名:3080/api/…`。请求**确实**打到了本机，
/// 而浏览器也**确实**认为它连的是恶意域名——恶意域名于是就获得了读取响应的机会。
///
/// 阻断它的办法只有一个：服务端检查 Host 头。如果 Host 是「恶意域名」而不是
/// 「127.0.0.1:3080」，直接拒绝。**请求能到本机，但拿不到响应**。
///
/// 【Origin 为什么是「若存在才检查」】
/// 因为同源请求（从本 UI 的页面发出去的）通常不带 Origin，或者带的是同源值。
/// 带了就必须匹配；不带不代表失败，但也代表这不是一个跨站请求。
///
/// 【⚠ 它防不住「本机进程伪造 Host」】
/// 本机进程可以随手写上正确的 Host。Host 检查防的是**浏览器**，
/// 不是本机的另一个进程。文件头那句「不能替代系统级边界」就是这个意思。
fn authorize_host(app: &WebApp, headers: &HeaderMap) -> Result<(), ApiError> {
    let host = single_header(headers, "host")?
        .filter(|value| authority_matches_bound_addr(value, app.bound_addr));
    if host.is_none() {
        return Err(ApiError::unauthorized());
    }
    if let Some(origin) = single_header(headers, "origin")? {
        if !origin_matches_bound_addr(origin, app.bound_addr) {
            return Err(ApiError::unauthorized());
        }
    }
    Ok(())
}

impl WebApp {
    fn enforce_rate_limit(&self) -> Result<(), ApiError> {
        let mut window = self
            .rate_window
            .lock()
            .map_err(|_| ApiError::fail("web_rate_state_unavailable"))?;
        if window.started.elapsed() >= WEB_RATE_WINDOW {
            window.started = Instant::now();
            window.requests = 0;
        }
        if window.requests >= MAX_WEB_REQUESTS_PER_WINDOW {
            return Err(ApiError {
                status: StatusCode::TOO_MANY_REQUESTS,
                error: "web_rate_limit_exceeded".to_owned(),
            });
        }
        window.requests += 1;
        Ok(())
    }
}

fn single_header<'a>(headers: &'a HeaderMap, name: &str) -> Result<Option<&'a str>, ApiError> {
    let mut values = headers.get_all(name).iter();
    let Some(value) = values.next() else {
        return Ok(None);
    };
    if values.next().is_some() {
        return Err(ApiError::unauthorized());
    }
    value
        .to_str()
        .map(Some)
        .map_err(|_| ApiError::unauthorized())
}

/// Host 头的值是否**完全等于**本服务绑定的那套地址。
///
/// 【⚠ 它比较的是「完全相等」，不是「同一个主机」】
/// `localhost:3080` 与 `127.0.0.1:3080` **不匹配**，尽管它们指向同一台机器。
/// 这是刻意的：一个更宽松的比较（比如只比主机部分、或者把 localhost 视作等价）
/// 就等于重新打开 rebinding——攻击者控制的正是「你把它解析成什么」。
/// 让页面上显示的地址与请求里的 Host 严格一致，是最省事也最不容易错的做法。
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

/// Resolve an owned UI session without creating or restoring a Runner.
/// History is supplied by DaemonHost::ui_events, filtered to the authenticated principal.
async fn resolve_human_session(app: &WebApp, requested: Option<&str>) -> Result<String, ApiError> {
    let id = match requested.map(str::trim).filter(|value| !value.is_empty()) {
        Some(id) => id.to_owned(),
        None => lock_string(&app.active)?,
    };
    validate_web_session_id(&id)?;
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
            return Ok(id);
        }
        return Err(ApiError::bad("session_unknown"));
    }
    Ok(id)
}

/// Bind the first bootstrap/new-session tab to the server principal. A different tab may still
/// observe the session through the feed, but it cannot acquire or steal the mutation lease.
fn claim_session_tab(app: &WebApp, session_id: &str, tab_id: &str) -> Result<(), ApiError> {
    validate_web_session_id(session_id)?;
    validate_web_tab_id(tab_id)?;
    let principal_id = app.host.authenticated_principal().principal_id;
    let mut sessions = app
        .sessions
        .lock()
        .map_err(|_| ApiError::fail("web_state_poisoned"))?;
    let session = sessions
        .get_mut(session_id)
        .ok_or_else(|| ApiError::bad("session_unknown"))?;
    if session.owner_principal_id != principal_id {
        return Err(ApiError::conflict("session_principal_mismatch"));
    }
    match session.owner_tab_id.as_deref() {
        None => {
            session.owner_tab_id = Some(tab_id.to_owned());
            session.owner_active = true;
            session.lease_epoch = session.lease_epoch.saturating_add(1).max(1);
            Ok(())
        }
        Some(owner) if owner == tab_id && session.owner_active => Ok(()),
        Some(_) => Ok(()),
    }
}

fn require_session_owner(app: &WebApp, session_id: &str, tab_id: &str) -> Result<(), ApiError> {
    let principal_id = app.host.authenticated_principal().principal_id;
    let sessions = app
        .sessions
        .lock()
        .map_err(|_| ApiError::fail("web_state_poisoned"))?;
    let session = sessions
        .get(session_id)
        .ok_or_else(|| ApiError::bad("session_unknown"))?;
    if session.owner_principal_id != principal_id {
        return Err(ApiError::conflict("session_principal_mismatch"));
    }
    if !session.owner_active || session.owner_tab_id.as_deref() != Some(tab_id) {
        return Err(ApiError::conflict("session_owner_required"));
    }
    Ok(())
}

fn authorize_feed_tab(app: &WebApp, session_id: &str, tab_id: &str) -> Result<(), ApiError> {
    validate_web_tab_id(tab_id)?;
    let principal_id = app.host.authenticated_principal().principal_id;
    let sessions = app
        .sessions
        .lock()
        .map_err(|_| ApiError::fail("web_state_poisoned"))?;
    let Some(session) = sessions.get(session_id) else {
        // Historical sessions have already been filtered through the authenticated EventLog
        // projection in resolve_human_session; they have no mutable in-memory tab lease.
        return Ok(());
    };
    if session.owner_principal_id != principal_id {
        return Err(ApiError::conflict("session_principal_mismatch"));
    }
    // Owner and observer tabs both receive the immutable feed. Only owner tabs may submit an
    // action; observer state never gets copied into a mutable draft or lease.
    Ok(())
}

async fn resolve_feed_session(app: &WebApp, requested: Option<&str>) -> Result<String, ApiError> {
    resolve_human_session(app, requested).await
}

async fn resolve_mutable_session(
    app: &WebApp,
    requested: Option<&str>,
) -> Result<String, ApiError> {
    let id = resolve_human_session(app, requested).await?;
    if !app
        .sessions
        .lock()
        .map_err(|_| ApiError::fail("web_state_poisoned"))?
        .contains_key(&id)
    {
        return Err(ApiError::conflict("session_read_only"));
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
            .map(|item| {
                item.id.len()
                    + item.kind.len()
                    + item.status.len()
                    + item.title.len()
                    + item.body.len()
            })
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
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&path, fs::Permissions::from_mode(0o600)).unwrap();
        }
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
                        id: format!("item-{index}"),
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
                        id: "item".to_owned(),
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
        let runtime_events = host.ui_events().await.unwrap().unwrap();
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
        assert!(PAGE.contains("st.textContent = '历史 · '"));
        assert!(PAGE.contains("human_actions_allowed"));
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
            .contains("requestStreamHydrate(gap.reason || 'stream_gap')"));
        assert!(PAGE[error_handler..event_stream]
            .contains("requestStreamHydrate(payload.error || 'stream_error')"));
        let hydrate = PAGE
            .find("async function requestStreamHydrate")
            .expect("snapshot hydration helper");
        assert!(PAGE[hydrate..gap_handler].contains("markStreamIncomplete(reason || 'stream_gap')"));
        assert!(PAGE[hydrate..gap_handler].contains("closeEventStream(true)"));
        assert!(PAGE[hydrate..gap_handler].contains("await refresh()"));
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
            host.clone(),
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
        assert_eq!(health["ok"], false);
        assert_eq!(health["status"], "unavailable");
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
        assert!(page.contains("receipt authoritative"));
        assert!(page.contains("信任此文件夹"));
        assert!(page.contains("第一次用") || page.contains("Ask Kiana"));
        assert!(page.contains("thread") || page.contains("Threads"));
        let token = page
            .split("window.__KIANA_WEB_TOKEN__ = '")
            .nth(1)
            .and_then(|value| value.split('\'').next())
            .expect("web token")
            .to_owned();
        let auth = |request: reqwest::RequestBuilder| {
            let cursor = host.ui_cursor();
            request
                .header("x-kiana-web-token", &token)
                .header("x-kiana-ui-tab", "unit-tab")
                .header("x-kiana-ui-epoch", &cursor.epoch)
                .header("x-kiana-ui-cursor", cursor.sequence.to_string())
                .header("x-kiana-action-id", uuid::Uuid::new_v4().to_string())
                .header("x-kiana-action-target", "workspace")
        };
        let bootstrap = auth(client.get(format!("http://{addr}/api/bootstrap")))
            .send()
            .await
            .unwrap();
        assert!(bootstrap.status().is_success());

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

    /// A live loopback Web server plus the env scope that keeps its temp `KIANA_HOME` valid for
    /// the whole test.  These fixtures exercise the real router/middleware boundary instead of
    /// asserting source markers, because UI-16 is about what the HTTP surface actually rejects.
    struct Ui16Server {
        addr: SocketAddr,
        host: Arc<DaemonHost>,
        root: PathBuf,
        home: PathBuf,
        _env: crate::test_support::ScopedEnv<'static>,
    }

    async fn ui16_server() -> Ui16Server {
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
        let home = unique_dir("ui16-home");
        let root = unique_dir("ui16-fixture");
        fs::create_dir_all(root.join(".git")).unwrap();
        let script = unique_dir("ui16-script").join("script.json");
        fs::write(&script, r#"[{"text":"ui16 noop"}]"#).unwrap();
        env.set_var("KIANA_HOME", &home);
        env.set_var("HOME", &home);
        env.set_var("KIANA_HARNESS_SCRIPT", &script);
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
            Arc::clone(&host),
            root.clone(),
            "workspace-write".to_owned(),
            ROLE_BUILDER.to_owned(),
            addr,
        );
        tokio::spawn(async move {
            axum::serve(listener, router(app)).await.unwrap();
        });
        Ui16Server {
            addr,
            host,
            root,
            home,
            _env: env,
        }
    }

    async fn ui16_page_token(client: &reqwest::Client, addr: SocketAddr) -> String {
        let page = client
            .get(format!("http://{addr}/"))
            .send()
            .await
            .unwrap()
            .text()
            .await
            .unwrap();
        assert!(
            !page.contains("__KIANA_CSP_NONCE__"),
            "the page must carry its per-process nonce"
        );
        page.split("window.__KIANA_WEB_TOKEN__ = '")
            .nth(1)
            .and_then(|rest| rest.split('\'').next())
            .expect("the host-only page issues the live web token")
            .to_owned()
    }

    async fn ui16_raw_http(addr: SocketAddr, request: &str) -> String {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        let mut stream = tokio::net::TcpStream::connect(addr).await.unwrap();
        stream.write_all(request.as_bytes()).await.unwrap();
        let mut buffer = Vec::new();
        let mut chunk = [0u8; 1024];
        loop {
            match tokio::time::timeout(Duration::from_secs(5), stream.read(&mut chunk)).await {
                Ok(Ok(0)) | Ok(Err(_)) | Err(_) => break,
                Ok(Ok(read)) => buffer.extend_from_slice(&chunk[..read]),
            }
        }
        String::from_utf8_lossy(&buffer).to_string()
    }

    fn ui16_head_line(response: &str) -> &str {
        response.lines().next().unwrap_or_default()
    }

    #[tokio::test]
    async fn ui16_http_boundary_is_deny_first_for_state_events_and_actions() {
        let server = ui16_server().await;
        let client = reqwest::Client::new();
        let base = format!("http://{}", server.addr);
        let token = ui16_page_token(&client, server.addr).await;

        // The host-only page itself needs no token; a foreign Origin cannot read it.
        let page = client.get(&base).send().await.unwrap();
        assert_eq!(page.status(), StatusCode::OK);
        let foreign_page = client
            .get(&base)
            .header("origin", "http://evil.example")
            .send()
            .await
            .unwrap();
        assert_eq!(foreign_page.status(), StatusCode::UNAUTHORIZED);

        for route in ["/api/state", "/api/events", "/api/sessions"] {
            let denied = client.get(format!("{base}{route}")).send().await.unwrap();
            assert_eq!(
                denied.status(),
                StatusCode::UNAUTHORIZED,
                "{route} must reject a missing token"
            );
            let body: Value = denied.json().await.unwrap();
            assert_eq!(body["error"], "web_auth_required");
        }
        let denied_action = client
            .post(format!("{base}/api/run"))
            .json(&json!({"prompt": "x"}))
            .send()
            .await
            .unwrap();
        assert_eq!(denied_action.status(), StatusCode::UNAUTHORIZED);

        // A token without the bound Host/Origin stays unauthorized, and the rejection is
        // identical to a missing token so a probe cannot separate the two checks.
        let foreign_origin = client
            .get(format!("{base}/api/state"))
            .header("x-kiana-web-token", &token)
            .header("origin", "http://evil.example")
            .send()
            .await
            .unwrap();
        assert_eq!(foreign_origin.status(), StatusCode::UNAUTHORIZED);
        let foreign_origin_body: Value = foreign_origin.json().await.unwrap();
        assert_eq!(foreign_origin_body["error"], "web_auth_required");

        let forged_host = ui16_raw_http(
            server.addr,
            &format!(
                "GET /api/state HTTP/1.1\r\nhost: evil.example:1234\r\nx-kiana-web-token: {token}\r\nconnection: close\r\n\r\n"
            ),
        )
        .await;
        assert!(
            ui16_head_line(&forged_host).contains("401"),
            "foreign Host must be denied: {forged_host}"
        );
        let duplicate_credential = ui16_raw_http(
            server.addr,
            &format!(
                "GET /api/state HTTP/1.1\r\nhost: {}\r\nx-kiana-web-token: {token}\r\nx-kiana-web-token: {token}\r\nconnection: close\r\n\r\n",
                server.addr
            ),
        )
        .await;
        assert!(
            ui16_head_line(&duplicate_credential).contains("401"),
            "duplicate credential headers must be denied: {duplicate_credential}"
        );

        // Encoded traversal is rejected by the request-bounds middleware before extraction.
        let traversal = ui16_raw_http(
            server.addr,
            "GET /api/%2e%2e/state HTTP/1.1\r\nhost: 127.0.0.1\r\nconnection: close\r\n\r\n",
        )
        .await;
        assert!(
            ui16_head_line(&traversal).contains("400")
                && traversal.contains("web_path_traversal_denied"),
            "encoded traversal must be denied: {traversal}"
        );

        // Cross-workspace session probes are refused: unknown ids and path-like ids alike.
        let unknown = client
            .get(format!("{base}/api/state"))
            .query(&[("session_id", "11111111-2222-3333-4444-555555555555")])
            .header("x-kiana-web-token", &token)
            .send()
            .await
            .unwrap();
        assert_eq!(unknown.status(), StatusCode::BAD_REQUEST);
        let unknown_body: Value = unknown.json().await.unwrap();
        assert_eq!(unknown_body["error"], "session_unknown");
        let path_like = client
            .get(format!("{base}/api/state"))
            .query(&[("session_id", "../../etc/passwd")])
            .header("x-kiana-web-token", &token)
            .send()
            .await
            .unwrap();
        assert_eq!(path_like.status(), StatusCode::BAD_REQUEST);
        let path_like_body: Value = path_like.json().await.unwrap();
        assert_eq!(path_like_body["error"], "session_invalid");

        // Wrong methods stay Axum method rejections: no handler, no authorization bypass.
        let post_on_get = client
            .post(format!("{base}/api/state"))
            .header("x-kiana-web-token", &token)
            .send()
            .await
            .unwrap();
        assert_eq!(post_on_get.status(), StatusCode::METHOD_NOT_ALLOWED);
        let get_on_post = client
            .get(format!("{base}/api/run"))
            .header("x-kiana-web-token", &token)
            .send()
            .await
            .unwrap();
        assert_eq!(get_on_post.status(), StatusCode::METHOD_NOT_ALLOWED);
        let options_on_action = client
            .request(reqwest::Method::OPTIONS, format!("{base}/api/run"))
            .send()
            .await
            .unwrap();
        assert_eq!(options_on_action.status(), StatusCode::METHOD_NOT_ALLOWED);
        let post_on_health = client
            .post(format!("{base}/api/health"))
            .send()
            .await
            .unwrap();
        assert_eq!(post_on_health.status(), StatusCode::METHOD_NOT_ALLOWED);

        // A valid token + bound Host gives the read model, hardened with the header draft.
        let allowed = client
            .get(format!("{base}/api/state"))
            .header("x-kiana-web-token", &token)
            .send()
            .await
            .unwrap();
        assert_eq!(allowed.status(), StatusCode::OK);
        assert_eq!(allowed.headers()["x-content-type-options"], "nosniff");
        assert_eq!(allowed.headers()["cache-control"], "no-store");
        assert_eq!(allowed.headers()["x-frame-options"], "DENY");
        assert_eq!(allowed.headers()["referrer-policy"], "no-referrer");
        let csp = allowed.headers()["content-security-policy"]
            .to_str()
            .unwrap()
            .to_owned();
        assert!(csp.contains("nonce-"), "CSP must carry a nonce: {csp}");
        assert!(!csp.contains("unsafe-inline"));
        let snapshot: Value = allowed.json().await.unwrap();
        assert_eq!(snapshot["harness"], harness_run::HARNESS_ID);
        assert!(snapshot["session_id"].as_str().is_some());
        assert!(snapshot["cursor"]["epoch"].as_str().is_some());

        // Health is host-only and low detail: no folder, storage path or home is echoed.
        let health = client
            .get(format!("{base}/api/health"))
            .send()
            .await
            .unwrap();
        assert_eq!(health.status(), StatusCode::OK);
        let health_body = health.text().await.unwrap();
        for secret in [
            server.home.display().to_string(),
            server.root.display().to_string(),
        ] {
            assert!(
                !health_body.contains(&secret),
                "health must not leak {secret}: {health_body}"
            );
        }
        assert!(!health_body.contains("events.jsonl"));
        assert!(!health_body.contains("web_state_poisoned"));
    }

    #[tokio::test]
    async fn ui16_request_limits_reject_oversized_body_and_rate_floods() {
        let server = ui16_server().await;
        let client = reqwest::Client::new();
        let base = format!("http://{}", server.addr);
        let token = ui16_page_token(&client, server.addr).await;

        // Body limit: one byte over the transport bound is rejected before the handler runs,
        // and the rejected payload is never echoed back.
        let oversized = "a".repeat(MAX_WEB_BODY_BYTES + 1);
        let response = client
            .post(format!("{base}/api/run"))
            .header("x-kiana-web-token", &token)
            .json(&json!({"prompt": oversized}))
            .send()
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::PAYLOAD_TOO_LARGE);
        let body = response.text().await.unwrap();
        assert!(
            !body.contains("aaaaaaaa"),
            "oversized body was echoed: {body}"
        );

        // URI bound: an oversized request target is refused before query extraction.
        let long_uri = format!(
            "{base}/api/state?session_id={}",
            "x".repeat(MAX_WEB_URI_BYTES)
        );
        let long_response = client
            .get(long_uri)
            .header("x-kiana-web-token", &token)
            .send()
            .await
            .unwrap();
        assert_eq!(long_response.status(), StatusCode::BAD_REQUEST);
        let long_body: Value = long_response.json().await.unwrap();
        assert_eq!(long_body["error"], "web_request_target_too_large");

        // Deterministic per-instance budget: 120 requests pass, the next one is a 429.
        let probe = WebApp::new(
            Arc::clone(&server.host),
            server.root.clone(),
            "workspace-write".to_owned(),
            ROLE_BUILDER.to_owned(),
            server.addr,
        );
        for _ in 0..MAX_WEB_REQUESTS_PER_WINDOW {
            assert!(probe.enforce_rate_limit().is_ok());
        }
        let limited = probe.enforce_rate_limit().unwrap_err();
        assert_eq!(limited.status, StatusCode::TOO_MANY_REQUESTS);
        assert_eq!(limited.error, "web_rate_limit_exceeded");
        {
            let mut window = probe.rate_window.lock().unwrap();
            window.started = Instant::now() - WEB_RATE_WINDOW - Duration::from_millis(1);
        }
        assert!(probe.enforce_rate_limit().is_ok(), "window must recover");

        // The same fence is wired into the live server and keeps the security headers.
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let app = WebApp::new(
            Arc::clone(&server.host),
            server.root.clone(),
            "workspace-write".to_owned(),
            ROLE_BUILDER.to_owned(),
            addr,
        );
        let rate_window = Arc::clone(&app.rate_window);
        tokio::spawn(async move {
            axum::serve(listener, router(app)).await.unwrap();
        });
        let mut observed = false;
        for _ in 0..5 {
            {
                let mut window = rate_window.lock().unwrap();
                window.started = Instant::now();
                window.requests = MAX_WEB_REQUESTS_PER_WINDOW;
            }
            let response = client
                .get(format!("http://{addr}/api/health"))
                .send()
                .await
                .unwrap();
            if response.status() == StatusCode::TOO_MANY_REQUESTS {
                assert_eq!(response.headers()["x-content-type-options"], "nosniff");
                let body: Value = response.json().await.unwrap();
                assert_eq!(body["error"], "web_rate_limit_exceeded");
                observed = true;
                break;
            }
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
        assert!(observed, "rate-limit fence never triggered over HTTP");
    }

    #[tokio::test]
    async fn ui16_token_rotation_invalidates_the_previous_credential() {
        let server = ui16_server().await;
        let client = reqwest::Client::new();
        let base = format!("http://{}", server.addr);
        let token = ui16_page_token(&client, server.addr).await;

        let rotated = client
            .post(format!("{base}/api/token/rotate"))
            .header("x-kiana-web-token", &token)
            .send()
            .await
            .unwrap();
        assert_eq!(rotated.status(), StatusCode::OK);
        let rotation_body: Value = rotated.json().await.unwrap();
        assert_eq!(rotation_body["schema"], WEB_TOKEN_ROTATION_SCHEMA);
        assert_eq!(rotation_body["rotated"], true);
        assert_eq!(rotation_body["token_generation"], 2);
        assert_eq!(rotation_body["principal_changed"], false);
        assert_eq!(rotation_body["lease_changed"], false);
        let printed = rotation_body.to_string();
        assert!(
            !printed.contains(&token),
            "rotation response must not echo the credential"
        );

        // Rotation without a credential is denied like any other mutation route.
        let denied = client
            .post(format!("{base}/api/token/rotate"))
            .send()
            .await
            .unwrap();
        assert_eq!(denied.status(), StatusCode::UNAUTHORIZED);

        let stale = client
            .get(format!("{base}/api/state"))
            .header("x-kiana-web-token", &token)
            .send()
            .await
            .unwrap();
        assert_eq!(
            stale.status(),
            StatusCode::UNAUTHORIZED,
            "the previous credential must be invalidated"
        );

        // The host-only page is the rehydration path for the new credential.
        let fresh = ui16_page_token(&client, server.addr).await;
        assert_ne!(fresh, token);
        assert!(!printed.contains(&fresh));
        let allowed = client
            .get(format!("{base}/api/state"))
            .header("x-kiana-web-token", &fresh)
            .send()
            .await
            .unwrap();
        assert_eq!(allowed.status(), StatusCode::OK);
        let snapshot: Value = allowed.json().await.unwrap();
        assert_eq!(snapshot["lease"]["token_generation"], 2);

        // Generations are monotonic and each rotation invalidates the credential before it.
        let second = client
            .post(format!("{base}/api/token/rotate"))
            .header("x-kiana-web-token", &fresh)
            .send()
            .await
            .unwrap();
        assert_eq!(second.status(), StatusCode::OK);
        let second_body: Value = second.json().await.unwrap();
        assert_eq!(second_body["token_generation"], 3);
        let stale_again = client
            .get(format!("{base}/api/state"))
            .header("x-kiana-web-token", &fresh)
            .send()
            .await
            .unwrap();
        assert_eq!(stale_again.status(), StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn ui16_concurrent_tabs_observe_one_server_snapshot() {
        let server = ui16_server().await;
        let client = reqwest::Client::new();
        let base = format!("http://{}", server.addr);
        let token = ui16_page_token(&client, server.addr).await;

        let request = |tab: &'static str| {
            client
                .get(format!("{base}/api/state"))
                .header("x-kiana-web-token", &token)
                .header("x-kiana-ui-tab", tab)
                .send()
        };
        let (first, second) = tokio::join!(request("tab-1"), request("tab-2"));
        let first = first.unwrap();
        let second = second.unwrap();
        assert_eq!(first.status(), StatusCode::OK);
        assert_eq!(second.status(), StatusCode::OK);
        assert_eq!(first.headers()["cache-control"], "no-store");
        assert_eq!(second.headers()["cache-control"], "no-store");
        let first: Value = first.json().await.unwrap();
        let second: Value = second.json().await.unwrap();
        assert_eq!(first["session_id"], second["session_id"]);
        assert_eq!(first["cursor"]["epoch"], second["cursor"]["epoch"]);
        assert_eq!(first["lease"]["feed_only"], true);
        assert_eq!(second["lease"]["feed_only"], true);
        assert!(
            !second.to_string().contains("tab-1"),
            "a tab must not observe another tab's local draft identity"
        );
        assert!(!first.to_string().contains("tab-2"));
    }

    // ------------------------------------------------------------------
    // UI-17: Web snapshot hydrate, history and pagination (completion slice).
    //
    // These fixtures drive the real loopback router on a host whose EventLog was seeded *before*
    // the host opened it: the seeded sessions exist only as committed history, and the active
    // in-memory session is empty.  Hydrate order, server-cursor pagination and the replay /
    // foreign-tab / stale-cursor fences are HTTP boundary behaviour, so they are exercised here
    // rather than asserted as source markers.
    // ------------------------------------------------------------------

    const UI17_HISTORY_TURNS: usize = 3;
    const UI17_ARTIFACT_DIGEST: &str =
        "sha256:1111111111111111111111111111111111111111111111111111111111111111";
    const UI17_SECOND_SESSION: &str = "ui17-second-session";

    struct Ui17SeedSession {
        session_id: &'static str,
        run_id: String,
        turns: usize,
        artifacts: Vec<&'static str>,
    }

    fn ui17_push_event(events: &mut Vec<Value>, sequence: &mut u64, kind: &str, data: Value) {
        events.push(ledger_event(*sequence, kind, data));
        *sequence += 1;
    }

    /// Seed committed history for one or more sessions.  Every session gets its own authorized
    /// run, bounded turns and artifact references so the history/artifact pages can prove that a
    /// page is both cursor-scoped and run-scoped.
    fn ui17_seeded_events(root: &Path, sessions: &[Ui17SeedSession]) -> Vec<Value> {
        let mut events = Vec::new();
        let mut sequence = 1u64;
        for session in sessions {
            ui17_push_event(
                &mut events,
                &mut sequence,
                "run.authorized",
                json!({
                    "run_id": session.run_id.as_str(),
                    "session_id": session.session_id,
                    "actor_id": "local-user",
                    "project_root": root.to_string_lossy(),
                    "role_id": ROLE_BUILDER,
                    "department_id": kiana_protocol::DEPARTMENT_EXECUTING,
                    "sandbox": "read-only",
                }),
            );
            for index in 0..session.turns {
                ui17_push_event(
                    &mut events,
                    &mut sequence,
                    "run.prompt",
                    json!({
                        "run_id": session.run_id.as_str(),
                        "session_id": session.session_id,
                        "text": format!("ui17 turn {index}"),
                    }),
                );
                ui17_push_event(
                    &mut events,
                    &mut sequence,
                    "run.delta",
                    json!({
                        "run_id": session.run_id.as_str(),
                        "text": format!("ui17 delta {index} "),
                    }),
                );
                ui17_push_event(
                    &mut events,
                    &mut sequence,
                    "run.completed",
                    json!({
                        "run_id": session.run_id.as_str(),
                        "text": format!("ui17 answer {index}"),
                    }),
                );
            }
            for artifact_id in &session.artifacts {
                ui17_push_event(
                    &mut events,
                    &mut sequence,
                    "run.artifact",
                    json!({
                        "run_id": session.run_id.as_str(),
                        "session_id": session.session_id,
                        "artifact_id": artifact_id,
                        "artifact_schema": "kiana.ui-artifact-ref.v1",
                        "content_hash": UI17_ARTIFACT_DIGEST,
                    }),
                );
            }
        }
        events
    }

    struct Ui17Server {
        addr: SocketAddr,
        host: Arc<DaemonHost>,
        root: PathBuf,
        home: PathBuf,
        active: String,
        session: String,
        run_id: String,
        options: HashMap<String, Value>,
        _env: crate::test_support::ScopedEnv<'static>,
    }

    async fn ui17_server() -> Ui17Server {
        let env = crate::test_support::scoped_env(&[
            "KIANA_HOME",
            "HOME",
            "KIANA_HARNESS_SCRIPT",
            "KIANA_PROVIDER",
            "KIANA_STREAMING",
            "KIANA_FAKE_PROVIDER_SCRIPT",
            "ANTHROPIC_API_KEY",
            "KIANA_OPENAI_API_KEY",
            "OPENAI_API_KEY",
        ]);
        let home = unique_dir("ui17-home");
        let root = unique_dir("ui17-fixture");
        fs::create_dir_all(root.join(".git")).unwrap();
        let script = unique_dir("ui17-script").join("script.json");
        fs::write(&script, r#"[{"text":"ui17 hydrated answer"}]"#).unwrap();
        env.set_var("KIANA_HOME", &home);
        env.set_var("HOME", &home);
        env.set_var("KIANA_HARNESS_SCRIPT", &script);
        env.set_var("KIANA_PROVIDER", "");
        env.set_var("KIANA_STREAMING", "off");
        std::env::remove_var("KIANA_FAKE_PROVIDER_SCRIPT");
        std::env::remove_var("ANTHROPIC_API_KEY");
        std::env::remove_var("KIANA_OPENAI_API_KEY");
        std::env::remove_var("OPENAI_API_KEY");
        write_project_trust(&root, ProjectTrust::Trusted).unwrap();

        let session = "ui17-history-session".to_owned();
        let run_id = RunId::new().to_string();
        let second_run = RunId::new().to_string();
        let events = ui17_seeded_events(
            &root,
            &[
                Ui17SeedSession {
                    session_id: "ui17-history-session",
                    run_id: run_id.clone(),
                    turns: UI17_HISTORY_TURNS,
                    artifacts: vec!["ui17-artifact-a", "ui17-artifact-b"],
                },
                Ui17SeedSession {
                    session_id: UI17_SECOND_SESSION,
                    run_id: second_run,
                    turns: 1,
                    artifacts: vec!["ui17-artifact-c"],
                },
            ],
        );
        write_ledger(&home, &events);

        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let host = Arc::new(DaemonHost::local().unwrap());
        let app = WebApp::new(
            Arc::clone(&host),
            root.clone(),
            "read-only".to_owned(),
            ROLE_BUILDER.to_owned(),
            addr,
        );
        let active = lock_string(&app.active).unwrap();
        let options = app.options().unwrap();
        tokio::spawn(async move {
            axum::serve(listener, router(app)).await.unwrap();
        });
        Ui17Server {
            addr,
            host,
            root,
            home,
            active,
            session,
            run_id,
            options,
            _env: env,
        }
    }

    struct Ui17Client {
        client: reqwest::Client,
        base: String,
        token: String,
    }

    async fn ui17_client(server: &Ui17Server) -> Ui17Client {
        let client = reqwest::Client::new();
        let token = ui16_page_token(&client, server.addr).await;
        Ui17Client {
            client,
            base: format!("http://{}", server.addr),
            token,
        }
    }

    impl Ui17Client {
        fn get(&self, path: &str, tab: &str) -> reqwest::RequestBuilder {
            self.client
                .get(format!("{}{path}", self.base))
                .header("x-kiana-web-token", &self.token)
                .header("x-kiana-ui-tab", tab)
        }
    }

    async fn ui17_json(response: reqwest::Response) -> (StatusCode, Value) {
        let status = response.status();
        let body = response.json().await.unwrap_or(Value::Null);
        (status, body)
    }

    /// Drive one real harness turn through the daemon host (not through the Web action route) so a
    /// fixture can advance the committed source cursor deterministically.
    async fn ui17_advance_cursor(server: &Ui17Server, prompt: &str) -> u64 {
        let before = server.host.ui_cursor();
        let response = harness_run::run_envelope_on_host(
            Arc::clone(&server.host),
            server.active.clone(),
            prompt,
            &server.options,
        )
        .await
        .expect("fixture harness turn");
        assert_eq!(
            response.status,
            kiana_protocol::ExecutionStatus::Completed,
            "{response:?}"
        );
        let after = server.host.ui_cursor();
        assert!(
            after.sequence > before.sequence,
            "a committed turn must advance the display cursor"
        );
        after.sequence
    }

    #[tokio::test]
    async fn ui17_bootstrap_hydrates_before_feed_and_matches_the_cli_parity_snapshot() {
        let server = ui17_server().await;
        let client = ui17_client(&server).await;
        let cursor = server.host.ui_cursor();

        // A fresh page hydrates a known historical session without creating a lease, and the
        // hydrate envelope declares the canonical order: snapshot, then history, then feed.
        let (status, body) = ui17_json(
            client
                .get(
                    &format!("/api/bootstrap?session_id={}", server.session),
                    "ui17-tab-a",
                )
                .send()
                .await
                .unwrap(),
        )
        .await;
        assert_eq!(status, StatusCode::OK, "{body}");
        assert_eq!(body["bootstrap"]["schema"], json!(WEB_HYDRATE_SCHEMA));
        assert_eq!(body["bootstrap"]["snapshot_first"], json!(true));
        assert_eq!(body["bootstrap"]["feed_after_hydrate"], json!(true));
        assert_eq!(body["bootstrap"]["tab_id"], json!("ui17-tab-a"));
        let hydrate = &body["hydrate"];
        assert_eq!(hydrate["schema"], json!(WEB_HYDRATE_SCHEMA));
        assert_eq!(hydrate["tab_id"], json!("ui17-tab-a"));
        assert_eq!(hydrate["cache"], json!("memory_only"));
        assert_eq!(hydrate["source"], json!("event_log_projection"));
        assert_eq!(hydrate["historical"], json!(true));
        assert_eq!(hydrate["feed_ready"], json!(false));
        assert_eq!(hydrate["session_id"], json!(server.session.as_str()));
        assert_eq!(
            hydrate["instance_id"],
            json!(server.host.feed_instance_id())
        );
        assert_eq!(hydrate["epoch"], json!(cursor.epoch.as_str()));
        assert_eq!(hydrate["snapshot_cursor"], json!(cursor.sequence));
        assert_eq!(hydrate["trace"]["schema"], json!(WEB_HYDRATE_TRACE_SCHEMA));
        assert_eq!(
            hydrate["trace"]["max_steps"],
            json!(MAX_WEB_HYDRATE_TRACE_STEPS)
        );
        let steps = hydrate["trace"]["steps"].as_array().expect("trace steps");
        let names: Vec<&str> = steps
            .iter()
            .map(|step| step["step"].as_str().expect("step name"))
            .collect();
        assert_eq!(names, ["snapshot", "history", "feed"]);
        assert_eq!(steps[0]["requires"], json!("none"));
        assert_eq!(steps[1]["requires"], json!("snapshot"));
        assert_eq!(steps[1]["schema"], json!(WEB_HISTORY_SCHEMA));
        assert_eq!(steps[2]["requires"], json!("snapshot"));
        assert_eq!(steps[2]["schema"], json!(WEB_SSE_SCHEMA));

        // A closed EventLog projection stays feed-only: no lease, no action authority.
        assert_eq!(body["read_only"], json!(true));
        assert_eq!(body["lease"]["disposition"], json!("closed"));
        assert_eq!(body["lease"]["feed_only"], json!(true));
        assert_eq!(body["human_actions_allowed"], json!(false));
        assert_eq!(body["history"]["status"], json!("ready"));
        assert_eq!(body["history"]["cache"], json!("memory_only"));
        assert_eq!(
            body["thread"]["turns"].as_array().unwrap().len(),
            UI17_HISTORY_TURNS
        );

        // Re-hydrating reports the same server-owned status projection the CLI reads for the same
        // session/run: only the entrypoint label differs.
        let cli = harness_run::parity_envelope_on_host(
            Arc::clone(&server.host),
            server.session.clone(),
            Some(RunId::parse_str(&server.run_id).expect("seeded run id")),
            EntryPointKind::Cli,
            &server.options,
        )
        .await
        .unwrap();
        assert_eq!(cli.status, kiana_protocol::ExecutionStatus::Completed);
        assert_eq!(body["parity_response_status"], json!("completed"));
        let web_parity = &body["parity"];
        let cli_parity = &cli.output;
        assert_eq!(web_parity["entrypoint"], json!("web"));
        assert_eq!(cli_parity["entrypoint"], json!("cli"));
        for field in [
            "status",
            "source_cursor",
            "source_event_ids",
            "projection_version",
            "receipt_digest",
            "audit_digest",
            "health_digest",
            "limitations",
        ] {
            assert_eq!(
                web_parity[field], cli_parity[field],
                "the hydrated Web status must match the CLI projection: {field}"
            );
        }

        // An empty live session is a known session with `empty` history, never a missing one.
        let (status, empty) = ui17_json(client.get("/api/bootstrap").send().await.unwrap()).await;
        assert_eq!(status, StatusCode::OK, "{empty}");
        assert_eq!(empty["session_id"], json!(server.active.as_str()));
        assert_eq!(empty["read_only"], json!(false));
        assert_eq!(empty["history"]["status"], json!("empty"));
        assert_eq!(empty["thread"]["turns"].as_array().unwrap().len(), 0);
        assert_eq!(empty["hydrate"]["historical"], Value::Null);
        assert_eq!(
            empty["hydrate"]["trace"]["steps"].as_array().unwrap().len(),
            3
        );

        // An id that is in neither memory nor the ledger fails closed.
        let (status, unknown) = ui17_json(
            client
                .get("/api/bootstrap?session_id=ui17-unknown-session")
                .send()
                .await
                .unwrap(),
        )
        .await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert_eq!(unknown["error"], json!("session_unknown"));

        let _ = fs::remove_dir_all(&server.root);
        let _ = fs::remove_dir_all(&server.home);
    }

    #[tokio::test]
    async fn ui17_history_pages_follow_one_server_cursor_and_refuse_replay_and_foreign_tabs() {
        let server = ui17_server().await;
        let client = ui17_client(&server).await;
        let cursor = server.host.ui_cursor();
        let page = |query: &str| format!("/api/history?session_id={}{query}", server.session);

        // The first page is bounded by the requested limit and reports `partial` plus a
        // single-use server cursor with the instance/epoch/source identity of the snapshot.
        let (status, first) = ui17_json(
            client
                .get(&page("&limit=2"), "ui17-tab-a")
                .send()
                .await
                .unwrap(),
        )
        .await;
        assert_eq!(status, StatusCode::OK, "{first}");
        assert_eq!(first["schema"], json!(WEB_HISTORY_SCHEMA));
        assert_eq!(first["cache"], json!("memory_only"));
        assert_eq!(first["status"], json!("partial"));
        assert_eq!(first["entries"].as_array().unwrap().len(), 2);
        assert_eq!(first["instance_id"], json!(server.host.feed_instance_id()));
        assert_eq!(first["epoch"], json!(cursor.epoch.as_str()));
        assert_eq!(first["source_cursor"], json!(cursor.sequence));
        let next_page = first["next_page"]
            .as_str()
            .expect("a partial page issues a next page cursor")
            .to_owned();

        // A cursor is bound to its tab: a foreign tab is refused and the owner's cursor survives.
        let (status, foreign) = ui17_json(
            client
                .get(&page(&format!("&limit=2&after={next_page}")), "ui17-tab-b")
                .send()
                .await
                .unwrap(),
        )
        .await;
        assert_eq!(status, StatusCode::CONFLICT);
        assert_eq!(foreign["error"], json!("web_page_cursor_scope_mismatch"));

        // A follow-up with the wrong limit is refused before the cursor is consumed.
        let (status, wrong_limit) = ui17_json(
            client
                .get(&page(&format!("&limit=1&after={next_page}")), "ui17-tab-a")
                .send()
                .await
                .unwrap(),
        )
        .await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert_eq!(
            wrong_limit["error"],
            json!("web_page_cursor_limit_mismatch")
        );

        // The owner consumes it exactly once, and the final page reports `ready` with no cursor.
        let (status, second) = ui17_json(
            client
                .get(&page(&format!("&limit=2&after={next_page}")), "ui17-tab-a")
                .send()
                .await
                .unwrap(),
        )
        .await;
        assert_eq!(status, StatusCode::OK, "{second}");
        assert_eq!(second["status"], json!("ready"));
        assert_eq!(second["entries"].as_array().unwrap().len(), 1);
        assert_eq!(second["next_page"], Value::Null);
        let (status, replayed) = ui17_json(
            client
                .get(&page(&format!("&limit=2&after={next_page}")), "ui17-tab-a")
                .send()
                .await
                .unwrap(),
        )
        .await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert_eq!(replayed["error"], json!("web_page_cursor_replayed"));

        // Empty pages are explicit: unknown sessions fail closed, a known live session without
        // turns is `empty`, and neither is confused with the other.
        let (status, unknown) = ui17_json(
            client
                .get("/api/history?session_id=ui17-unknown-session&limit=8")
                .send()
                .await
                .unwrap(),
        )
        .await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert_eq!(unknown["error"], json!("session_unknown"));
        let (status, empty) = ui17_json(
            client
                .get(&format!(
                    "/api/history?session_id={}&limit=8",
                    server.active
                ))
                .send()
                .await
                .unwrap(),
        )
        .await;
        assert_eq!(status, StatusCode::OK, "{empty}");
        assert_eq!(empty["status"], json!("empty"));
        assert_eq!(empty["entries"].as_array().unwrap().len(), 0);
        assert_eq!(empty["next_page"], Value::Null);

        // A page cursor issued before a new committed action must not replay that old action view.
        let (status, stale_page) = ui17_json(
            client
                .get(&page("&limit=1"), "ui17-tab-a")
                .send()
                .await
                .unwrap(),
        )
        .await;
        assert_eq!(status, StatusCode::OK, "{stale_page}");
        let stale = stale_page["next_page"]
            .as_str()
            .expect("next page")
            .to_owned();
        let advanced = ui17_advance_cursor(&server, "ui17 stale cursor turn").await;
        assert!(advanced > cursor.sequence);
        let (status, stale) = ui17_json(
            client
                .get(&page(&format!("&limit=1&after={stale}")), "ui17-tab-a")
                .send()
                .await
                .unwrap(),
        )
        .await;
        assert_eq!(status, StatusCode::CONFLICT);
        assert_eq!(stale["error"], json!("web_page_cursor_scope_mismatch"));

        let _ = fs::remove_dir_all(&server.root);
        let _ = fs::remove_dir_all(&server.home);
    }

    #[tokio::test]
    async fn ui17_artifact_page_is_run_scoped_and_never_returns_bytes() {
        let server = ui17_server().await;
        let client = ui17_client(&server).await;

        let (status, page) = ui17_json(
            client
                .get(&format!(
                    "/api/artifact?session_id={}&limit=8",
                    server.session
                ))
                .send()
                .await
                .unwrap(),
        )
        .await;
        assert_eq!(status, StatusCode::OK, "{page}");
        assert_eq!(page["schema"], json!(WEB_ARTIFACT_SCHEMA));
        assert_eq!(page["cache"], json!("memory_only"));
        assert_eq!(page["status"], json!("ready"));
        assert_eq!(
            page["limitations"][0],
            json!("artifact_content_requires_server_ref")
        );
        let entries = page["entries"].as_array().expect("artifact entries");
        let ids: Vec<&str> = entries
            .iter()
            .map(|entry| entry["artifact_id"].as_str().expect("artifact id"))
            .collect();
        assert_eq!(ids, ["ui17-artifact-a", "ui17-artifact-b"]);
        assert!(
            entries.iter().all(|entry| entry["content"].is_null()),
            "the reference page must not inline artifact bytes"
        );
        assert!(entries.iter().all(|entry| entry["digest"]
            .as_str()
            .is_some_and(|digest| digest.starts_with("sha256:"))));
        assert!(
            !page.to_string().contains("ui17-artifact-c"),
            "a run-scoped page must not enumerate another session's references"
        );

        // A reference that is not in this run is an explicit empty page, never a private listing.
        let (status, filtered) = ui17_json(
            client
                .get(&format!(
                    "/api/artifact?session_id={}&artifact_id=ui17-artifact-missing&limit=8",
                    server.session
                ))
                .send()
                .await
                .unwrap(),
        )
        .await;
        assert_eq!(status, StatusCode::OK, "{filtered}");
        assert_eq!(filtered["status"], json!("empty"));
        assert_eq!(filtered["entries"].as_array().unwrap().len(), 0);

        // Multi-session browsing: the second historical session hydrates read-only with its own
        // single turn and its own artifact reference.
        let (status, other) = ui17_json(
            client
                .get(&format!("/api/bootstrap?session_id={UI17_SECOND_SESSION}"))
                .send()
                .await
                .unwrap(),
        )
        .await;
        assert_eq!(status, StatusCode::OK, "{other}");
        assert_eq!(other["read_only"], json!(true));
        assert_eq!(other["thread"]["turns"].as_array().unwrap().len(), 1);
        assert_eq!(other["lease"]["disposition"], json!("closed"));
        let session_ids: Vec<&str> = other["sessions"]
            .as_array()
            .unwrap()
            .iter()
            .map(|session| session["id"].as_str().unwrap())
            .collect();
        assert!(session_ids.contains(&server.session.as_str()));
        assert!(session_ids.contains(&UI17_SECOND_SESSION));

        let (status, own) = ui17_json(
            client
                .get(&format!(
                    "/api/artifact?session_id={UI17_SECOND_SESSION}&limit=8"
                ))
                .send()
                .await
                .unwrap(),
        )
        .await;
        assert_eq!(status, StatusCode::OK, "{own}");
        assert_eq!(own["entries"].as_array().unwrap().len(), 1);
        assert_eq!(own["entries"][0]["artifact_id"], json!("ui17-artifact-c"));
        assert_eq!(own["entries"][0]["content"], Value::Null);

        let _ = fs::remove_dir_all(&server.root);
        let _ = fs::remove_dir_all(&server.home);
    }

    #[tokio::test]
    async fn ui17_source_change_fence_rejects_a_cursor_that_moved_mid_read() {
        let server = ui17_server().await;
        let app = WebApp::new(
            Arc::clone(&server.host),
            server.root.clone(),
            "read-only".to_owned(),
            ROLE_BUILDER.to_owned(),
            "127.0.0.1:0".parse().unwrap(),
        );
        let cursor = app.host.ui_cursor();
        assert!(cursor.sequence >= 1, "the seeded ledger commits events");
        assert!(app
            .ensure_page_source_unchanged(&cursor.epoch, cursor.sequence)
            .is_ok());
        assert_eq!(
            app.ensure_page_source_unchanged("ui17-stale-epoch", cursor.sequence)
                .unwrap_err()
                .error,
            "web_page_source_changed"
        );
        assert_eq!(
            app.ensure_page_source_unchanged(&cursor.epoch, cursor.sequence.saturating_sub(1))
                .unwrap_err()
                .error,
            "web_page_source_changed"
        );

        // The fence is not vacuous: a committed turn really moves the source cursor, and the
        // pre-commit identity is then refused.
        let advanced = ui17_advance_cursor(&server, "ui17 source change turn").await;
        assert!(advanced > cursor.sequence);
        assert_eq!(
            app.ensure_page_source_unchanged(&cursor.epoch, cursor.sequence)
                .unwrap_err()
                .error,
            "web_page_source_changed"
        );

        let _ = fs::remove_dir_all(&server.root);
        let _ = fs::remove_dir_all(&server.home);
    }

    // ------------------------------------------------------------------
    // UI-18: Web SSE reconnect, Last-Event-ID and gap (completion slice).
    //
    // These fixtures drive the real loopback router and a real `DaemonHost` run: frames are read
    // off the wire (`id`/event/schema/epoch/sequence), a reconnect is a second GET that carries a
    // Last-Event-ID cursor, and "a reconnect never resubmits an effect" is checked against the
    // committed facts instead of the browser source alone.
    // ------------------------------------------------------------------

    const UI18_TAB: &str = "ui18-tab";
    const UI18_FIRST_CHUNK: &str = "ui18 alpha";
    const UI18_SECOND_CHUNK: &str = " ui18 omega";
    const UI18_STALE_EPOCH: &str = "ui18-stale-epoch";

    struct Ui18Server {
        addr: SocketAddr,
        host: Arc<DaemonHost>,
        root: PathBuf,
        home: PathBuf,
        session: String,
        options: HashMap<String, Value>,
        shutdown: tokio::sync::watch::Receiver<bool>,
        _env: crate::test_support::ScopedEnv<'static>,
    }

    /// The default UI-18 script streams one delta, blocks inside a tool call (so a fixture can drop
    /// the connection in the middle of the turn) and then streams the second delta.
    fn ui18_stream_script() -> String {
        json!([
            {
                "text": UI18_FIRST_CHUNK,
                "tool_calls": [{
                    "id": "ui18-c1",
                    "name": "shell",
                    "arguments": {"command": "sleep 2"},
                }],
            },
            {"text": UI18_SECOND_CHUNK},
        ])
        .to_string()
    }

    async fn ui18_server(script: &str) -> Ui18Server {
        let env = crate::test_support::scoped_env(&[
            "KIANA_HOME",
            "HOME",
            "KIANA_HARNESS_SCRIPT",
            "KIANA_PROVIDER",
            "KIANA_STREAMING",
            "KIANA_FAKE_PROVIDER_SCRIPT",
            "ANTHROPIC_API_KEY",
            "KIANA_OPENAI_API_KEY",
            "OPENAI_API_KEY",
        ]);
        let home = unique_dir("ui18-home");
        let root = unique_dir("ui18-fixture");
        fs::create_dir_all(root.join(".git")).unwrap();
        let script_path = unique_dir("ui18-script").join("script.json");
        fs::write(&script_path, script).unwrap();
        env.set_var("KIANA_HOME", &home);
        env.set_var("HOME", &home);
        env.set_var("KIANA_HARNESS_SCRIPT", &script_path);
        std::env::remove_var("KIANA_PROVIDER");
        std::env::remove_var("KIANA_STREAMING");
        std::env::remove_var("KIANA_FAKE_PROVIDER_SCRIPT");
        std::env::remove_var("ANTHROPIC_API_KEY");
        std::env::remove_var("KIANA_OPENAI_API_KEY");
        std::env::remove_var("OPENAI_API_KEY");
        write_project_trust(&root, ProjectTrust::Trusted).unwrap();

        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let host = Arc::new(DaemonHost::with_env_harness().unwrap());
        let app = WebApp::new(
            Arc::clone(&host),
            root.clone(),
            "workspace-write".to_owned(),
            ROLE_BUILDER.to_owned(),
            addr,
        );
        let session = lock_string(&app.active).unwrap();
        let options = app.options().unwrap();
        let shutdown = app.shutdown.subscribe();
        tokio::spawn(async move {
            axum::serve(listener, router(app)).await.unwrap();
        });
        Ui18Server {
            addr,
            host,
            root,
            home,
            session,
            options,
            shutdown,
            _env: env,
        }
    }

    /// Claim the live session for this tab exactly as the browser bootstrap does; the action route
    /// refuses an unclaimed session, so the SSE fixtures run the same authorization path.
    async fn ui18_claim(client: &reqwest::Client, base: &str, token: &str, session: &str) {
        let response = client
            .get(format!("{base}/api/bootstrap"))
            .query(&[("session_id", session)])
            .header("x-kiana-web-token", token)
            .header("x-kiana-ui-tab", UI18_TAB)
            .send()
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
    }

    async fn ui18_run_turn(
        client: &reqwest::Client,
        base: &str,
        token: &str,
        session: &str,
        prompt: &str,
    ) -> (StatusCode, Value) {
        let response = client
            .post(format!("{base}/api/run"))
            .header("x-kiana-web-token", token)
            .header("x-kiana-ui-tab", UI18_TAB)
            .json(&json!({"prompt": prompt, "session_id": session}))
            .send()
            .await
            .unwrap();
        let status = response.status();
        let body = response.json().await.unwrap_or(Value::Null);
        (status, body)
    }

    /// Open the display stream.  A native reconnect carries `Last-Event-ID`; the loopback wrapper
    /// mirrors it in the query, and the server must agree with whichever form arrives.
    async fn ui18_open_stream(
        client: &reqwest::Client,
        base: &str,
        token: &str,
        session: &str,
        header_cursor: Option<&str>,
        query_cursor: Option<&str>,
    ) -> reqwest::Response {
        let mut request = client
            .get(format!("{base}/api/events"))
            .query(&[("session_id", session), ("tab_id", UI18_TAB)])
            .header("x-kiana-web-token", token);
        if let Some(cursor) = header_cursor {
            request = request.header("last-event-id", cursor);
        }
        if let Some(cursor) = query_cursor {
            request = request.query(&[("last_event_id", cursor)]);
        }
        request.send().await.unwrap()
    }

    #[derive(Debug)]
    struct Ui18Frame {
        id: String,
        event: String,
        data: Value,
    }

    fn ui18_take_frame(buffer: &mut Vec<u8>) -> Option<Ui18Frame> {
        loop {
            let boundary = buffer.windows(2).position(|window| window == b"\n\n")?;
            let block = String::from_utf8_lossy(&buffer[..boundary]).to_string();
            buffer.drain(..boundary + 2);
            let mut id = String::new();
            let mut event = String::new();
            let mut data = String::new();
            for line in block.lines() {
                let line = line.trim_end_matches('\r');
                if let Some(value) = line.strip_prefix("id:") {
                    id = value.trim().to_owned();
                } else if let Some(value) = line.strip_prefix("event:") {
                    event = value.trim().to_owned();
                } else if let Some(value) = line.strip_prefix("data:") {
                    data.push_str(value.trim_start());
                }
            }
            if id.is_empty() && event.is_empty() && data.is_empty() {
                // `keep_alive` comments carry no frame.
                continue;
            }
            return Some(Ui18Frame {
                id,
                event,
                data: serde_json::from_str(&data).unwrap_or(Value::Null),
            });
        }
    }

    async fn ui18_next_frame<S, B, E>(stream: &mut S, buffer: &mut Vec<u8>) -> Ui18Frame
    where
        S: futures_util::Stream<Item = Result<B, E>> + Unpin,
        B: AsRef<[u8]>,
        E: std::fmt::Debug,
    {
        use futures_util::StreamExt as _;
        loop {
            if let Some(frame) = ui18_take_frame(buffer) {
                return frame;
            }
            let chunk = tokio::time::timeout(Duration::from_secs(20), stream.next())
                .await
                .expect("SSE frame timeout")
                .expect("the display stream ended before the expected frame")
                .expect("SSE chunk");
            buffer.extend_from_slice(chunk.as_ref());
        }
    }

    /// A reconnect from an already-consumed cursor must not re-deliver that frame.
    async fn ui18_expect_no_frame<S, B, E>(stream: &mut S, buffer: &mut Vec<u8>, wait: Duration)
    where
        S: futures_util::Stream<Item = Result<B, E>> + Unpin,
        B: AsRef<[u8]>,
        E: std::fmt::Debug,
    {
        use futures_util::StreamExt as _;
        let deadline = tokio::time::Instant::now() + wait;
        loop {
            assert!(
                ui18_take_frame(buffer).is_none(),
                "an already-consumed cursor must not replay a frame"
            );
            let remaining = deadline.saturating_duration_since(tokio::time::Instant::now());
            if remaining.is_zero() {
                return;
            }
            match tokio::time::timeout(remaining, stream.next()).await {
                Err(_) => return,
                Ok(None) => return,
                Ok(Some(Ok(chunk))) => buffer.extend_from_slice(chunk.as_ref()),
                Ok(Some(Err(error))) => panic!("display stream chunk error: {error:?}"),
            }
        }
    }

    #[tokio::test]
    async fn ui18_events_frames_carry_server_owned_cursors_and_fence_last_event_id_conflicts() {
        let server = ui18_server(&ui18_stream_script()).await;
        let client = reqwest::Client::new();
        let base = format!("http://{}", server.addr);
        let token = ui16_page_token(&client, server.addr).await;
        ui18_claim(&client, &base, &token, &server.session).await;

        // Deny-first: an ambiguous, malformed or oversized resume cursor never opens a stream, and
        // `POST /api/events` stays a method rejection instead of becoming a second dispatch path.
        let conflict = client
            .get(format!("{base}/api/events"))
            .query(&[
                ("session_id", server.session.as_str()),
                ("tab_id", UI18_TAB),
                ("last_event_id", "ui18-epoch-a:1"),
            ])
            .header("x-kiana-web-token", &token)
            .header("last-event-id", "ui18-epoch-b:2")
            .send()
            .await
            .unwrap();
        assert_eq!(conflict.status(), StatusCode::BAD_REQUEST);
        assert_eq!(
            conflict.json::<Value>().await.unwrap()["error"],
            "stream_cursor_conflict"
        );
        for cursor in [
            "not-a-cursor".to_owned(),
            "ui18-epoch-a:1:2".to_owned(),
            format!("{}:1", "e".repeat(MAX_WEB_SSE_CURSOR_BYTES)),
        ] {
            let response = ui18_open_stream(
                &client,
                &base,
                &token,
                &server.session,
                Some(cursor.as_str()),
                None,
            )
            .await;
            assert_eq!(response.status(), StatusCode::BAD_REQUEST, "{cursor}");
            assert_eq!(
                response.json::<Value>().await.unwrap()["error"],
                "stream_cursor_invalid"
            );
        }
        let posted = client
            .post(format!("{base}/api/events"))
            .header("x-kiana-web-token", &token)
            .header("x-kiana-ui-tab", UI18_TAB)
            .send()
            .await
            .unwrap();
        assert_eq!(posted.status(), StatusCode::METHOD_NOT_ALLOWED);
        assert_eq!(
            server
                .host
                .ui_events()
                .await
                .unwrap()
                .unwrap()
                .iter()
                .filter(|event| event.kind == "run.authorized")
                .count(),
            0,
            "a rejected stream request must not authorize anything"
        );

        // The listener is installed first; the explicit turn submission follows (browser order).
        let response = ui18_open_stream(&client, &base, &token, &server.session, None, None).await;
        assert_eq!(response.status(), StatusCode::OK);
        assert!(response.headers()["content-type"]
            .to_str()
            .unwrap()
            .starts_with("text/event-stream"));
        let mut stream = response.bytes_stream();
        let mut buffer = Vec::new();
        let run_task = tokio::spawn({
            let client = client.clone();
            let base = base.clone();
            let token = token.clone();
            let session = server.session.clone();
            async move { ui18_run_turn(&client, &base, &token, &session, "ui18 stream").await }
        });

        let first = ui18_next_frame(&mut stream, &mut buffer).await;
        assert_eq!(first.event, "delta", "{}", first.data);
        assert_eq!(first.data["schema"], PROTOCOL_SCHEMA);
        assert_eq!(first.data["event"]["text"], UI18_FIRST_CHUNK);
        let epoch = first.data["epoch"]
            .as_str()
            .expect("server epoch")
            .to_owned();
        let first_sequence = first.data["sequence"].as_u64().expect("server sequence");
        assert!(first_sequence >= 1, "{first:?}");
        assert_eq!(first.id, format!("{epoch}:{first_sequence}"));
        assert!(
            !first.data.to_string().contains(&token) && !first.id.contains(&token),
            "the Web token must never enter an SSE frame: {first:?}"
        );

        // The live display stream is dense and server-owned: every frame advances the cursor by
        // exactly one, the id mirrors `epoch:sequence`, and the terminal response ends the stream.
        let mut names = vec![first.event.clone()];
        let mut last_sequence = first_sequence;
        let mut terminal = None;
        while terminal.is_none() {
            let frame = ui18_next_frame(&mut stream, &mut buffer).await;
            assert_eq!(frame.data["schema"], PROTOCOL_SCHEMA, "{}", frame.data);
            assert_eq!(frame.data["epoch"].as_str(), Some(epoch.as_str()));
            let sequence = frame.data["sequence"].as_u64().expect("server sequence");
            assert_eq!(
                sequence,
                last_sequence + 1,
                "a subscriber must not be served a skipped display frame: {frame:?}"
            );
            assert_eq!(frame.id, format!("{epoch}:{sequence}"));
            assert!(!frame.data.to_string().contains(&token));
            last_sequence = sequence;
            names.push(frame.event.clone());
            if frame.event == "terminal" {
                terminal = Some(frame);
            }
        }
        let terminal = terminal.expect("terminal frame");
        assert_eq!(terminal.data["event"]["response"]["status"], "completed");
        for name in &names {
            assert!(
                matches!(
                    name.as_str(),
                    "delta"
                        | "usage"
                        | "tool_call"
                        | "approval_requested"
                        | "runtime_error"
                        | "terminal"
                ),
                "unexpected display event name: {name}"
            );
        }
        drop(stream);
        let (status, run) = run_task.await.unwrap();
        assert_eq!(status, StatusCode::OK, "{run}");
        assert_eq!(run["response"]["status"], "completed", "{run}");

        // A native reconnect sends the last id it saw. Neither the header form nor the wrapper's
        // query mirror may re-deliver frames the tab already rendered.
        let terminal_cursor = format!("{epoch}:{last_sequence}");
        for (header, query) in [
            (Some(terminal_cursor.as_str()), None),
            (None, Some(terminal_cursor.as_str())),
        ] {
            let duplicate =
                ui18_open_stream(&client, &base, &token, &server.session, header, query).await;
            assert_eq!(duplicate.status(), StatusCode::OK);
            let mut duplicate_stream = duplicate.bytes_stream();
            let mut duplicate_buffer = Vec::new();
            ui18_expect_no_frame(
                &mut duplicate_stream,
                &mut duplicate_buffer,
                Duration::from_millis(1000),
            )
            .await;
            drop(duplicate_stream);
        }

        // A previous daemon incarnation can never keep rendering: the first frame is an explicit
        // snapshot-required gap that carries the *current* server epoch, never the stale one.
        let stale_cursor = format!("{UI18_STALE_EPOCH}:1");
        let stale = ui18_open_stream(
            &client,
            &base,
            &token,
            &server.session,
            Some(stale_cursor.as_str()),
            None,
        )
        .await;
        assert_eq!(stale.status(), StatusCode::OK);
        let mut stale_stream = stale.bytes_stream();
        let mut stale_buffer = Vec::new();
        let gap = ui18_next_frame(&mut stale_stream, &mut stale_buffer).await;
        assert_eq!(gap.event, "stream_gap", "{}", gap.data);
        assert_eq!(gap.data["schema"], WEB_SSE_SCHEMA);
        assert_eq!(gap.data["snapshot_required"], true);
        assert_eq!(gap.data["hydrate"], true);
        assert_eq!(gap.data["epoch"].as_str(), Some(epoch.as_str()));
        assert_eq!(
            gap.id,
            format!("{epoch}:{}", gap.data["sequence"].as_u64().unwrap())
        );
        drop(stale_stream);

        let _ = fs::remove_dir_all(&server.root);
        let _ = fs::remove_dir_all(&server.home);
    }

    #[tokio::test]
    async fn ui18_reconnect_and_gap_paths_never_resubmit_effectful_commands() {
        let server = ui18_server(&ui18_stream_script()).await;
        let client = reqwest::Client::new();
        let base = format!("http://{}", server.addr);
        let token = ui16_page_token(&client, server.addr).await;
        ui18_claim(&client, &base, &token, &server.session).await;

        // Browser order: the listener is installed first, the command follows.
        let response = ui18_open_stream(&client, &base, &token, &server.session, None, None).await;
        assert_eq!(response.status(), StatusCode::OK);
        let mut stream = response.bytes_stream();
        let mut buffer = Vec::new();
        let run_task = tokio::spawn({
            let client = client.clone();
            let base = base.clone();
            let token = token.clone();
            let session = server.session.clone();
            async move { ui18_run_turn(&client, &base, &token, &session, "ui18 reconnect").await }
        });
        let first = ui18_next_frame(&mut stream, &mut buffer).await;
        assert_eq!(first.event, "delta", "{}", first.data);
        assert_eq!(first.data["event"]["text"], UI18_FIRST_CHUNK);
        let epoch = first.data["epoch"]
            .as_str()
            .expect("server epoch")
            .to_owned();
        drop(stream);

        // Resuming from an older cursor cannot silently continue: the display window the tab
        // missed is reported as one explicit snapshot-required gap.
        let reconnect_cursor = format!("{epoch}:0");
        let reconnect = ui18_open_stream(
            &client,
            &base,
            &token,
            &server.session,
            Some(reconnect_cursor.as_str()),
            None,
        )
        .await;
        assert_eq!(reconnect.status(), StatusCode::OK);
        let mut reconnect_stream = reconnect.bytes_stream();
        let mut reconnect_buffer = Vec::new();
        let gap = ui18_next_frame(&mut reconnect_stream, &mut reconnect_buffer).await;
        assert_eq!(gap.event, "stream_gap", "{}", gap.data);
        assert_eq!(gap.data["schema"], WEB_SSE_SCHEMA);
        assert_eq!(gap.data["reason"], "snapshot_required_after_stream_gap");
        assert_eq!(gap.data["snapshot_required"], true);
        assert_eq!(gap.data["hydrate"], true);
        let mut replayed_first_chunk = 0usize;
        let mut terminal_status = None;
        loop {
            let frame = ui18_next_frame(&mut reconnect_stream, &mut reconnect_buffer).await;
            if frame.event == "delta" && frame.data["event"]["text"] == UI18_FIRST_CHUNK {
                replayed_first_chunk += 1;
            }
            if frame.event == "terminal" {
                terminal_status = frame.data["event"]["response"]["status"]
                    .as_str()
                    .map(str::to_owned);
                break;
            }
        }
        assert_eq!(terminal_status.as_deref(), Some("completed"));
        assert_eq!(
            replayed_first_chunk, 0,
            "a reconnect must never re-render a delta the tab already consumed"
        );
        drop(reconnect_stream);

        let (status, run) = run_task.await.unwrap();
        assert_eq!(status, StatusCode::OK, "{run}");
        let events = server.host.ui_events().await.unwrap().unwrap();
        let authorized = events
            .iter()
            .filter(|event| event.kind == "run.authorized")
            .count();
        let prompts = events
            .iter()
            .filter(|event| event.kind == "run.prompt")
            .count();
        assert_eq!(
            authorized, 1,
            "a reconnect must never authorize a second run"
        );
        assert_eq!(prompts, 1, "a reconnect must never resubmit the command");

        let _ = fs::remove_dir_all(&server.root);
        let _ = fs::remove_dir_all(&server.home);
    }

    #[tokio::test]
    async fn ui18_terminal_retention_replays_the_latest_terminal_without_deltas() {
        let script = json!([
            {"text": "ui18 retained one"},
            {"text": "ui18 retained two"},
        ])
        .to_string();
        let server = ui18_server(&script).await;
        let client = reqwest::Client::new();
        let base = format!("http://{}", server.addr);
        let token = ui16_page_token(&client, server.addr).await;
        ui18_claim(&client, &base, &token, &server.session).await;

        // Turn 1 finishes with no display subscriber attached: only the terminal is retained.
        let (status, first_turn) = ui18_run_turn(
            &client,
            &base,
            &token,
            &server.session,
            "ui18 retention one",
        )
        .await;
        assert_eq!(status, StatusCode::OK, "{first_turn}");
        assert_eq!(
            first_turn["response"]["status"], "completed",
            "{first_turn}"
        );

        let reconnect = ui18_open_stream(&client, &base, &token, &server.session, None, None).await;
        assert_eq!(reconnect.status(), StatusCode::OK);
        let mut reconnect_stream = reconnect.bytes_stream();
        let mut reconnect_buffer = Vec::new();
        let gap = ui18_next_frame(&mut reconnect_stream, &mut reconnect_buffer).await;
        assert_eq!(gap.event, "stream_gap", "{}", gap.data);
        assert_eq!(gap.data["reason"], "snapshot_required_after_stream_gap");
        assert_eq!(gap.data["snapshot_required"], true);
        let mut retained = Vec::new();
        let mut delta_frames = 0usize;
        loop {
            let frame = ui18_next_frame(&mut reconnect_stream, &mut reconnect_buffer).await;
            if frame.event == "delta" {
                delta_frames += 1;
            }
            let terminal = frame.event == "terminal";
            retained.push(frame);
            if terminal {
                break;
            }
        }
        assert_eq!(delta_frames, 0, "terminal retention must not replay deltas");
        assert_eq!(retained.len(), 1, "the retained terminal is replayed once");
        assert_eq!(
            retained[0].data["event"]["response"]["output"]["output"]["text"],
            "ui18 retained one"
        );
        let retained_cursor = retained[0].id.clone();
        drop(reconnect_stream);

        let duplicate = ui18_open_stream(
            &client,
            &base,
            &token,
            &server.session,
            Some(retained_cursor.as_str()),
            None,
        )
        .await;
        assert_eq!(duplicate.status(), StatusCode::OK);
        let mut duplicate_stream = duplicate.bytes_stream();
        let mut duplicate_buffer = Vec::new();
        ui18_expect_no_frame(
            &mut duplicate_stream,
            &mut duplicate_buffer,
            Duration::from_millis(1000),
        )
        .await;
        drop(duplicate_stream);

        // A tab that resumes exactly from the retained cursor sees the next turn live: the
        // replaced terminal is never re-delivered and no snapshot is required.
        let live = ui18_open_stream(
            &client,
            &base,
            &token,
            &server.session,
            Some(retained_cursor.as_str()),
            None,
        )
        .await;
        assert_eq!(live.status(), StatusCode::OK);
        let mut live_stream = live.bytes_stream();
        let mut live_buffer = Vec::new();
        let run_task = tokio::spawn({
            let client = client.clone();
            let base = base.clone();
            let token = token.clone();
            let session = server.session.clone();
            async move { ui18_run_turn(&client, &base, &token, &session, "ui18 retention two").await }
        });
        let mut live_terminal = None;
        while live_terminal.is_none() {
            let frame = ui18_next_frame(&mut live_stream, &mut live_buffer).await;
            assert_ne!(
                frame.event, "stream_gap",
                "resuming from the retained cursor must not require a snapshot: {}",
                frame.data
            );
            assert_ne!(
                frame.data["event"]["response"]["output"]["output"]["text"], "ui18 retained one",
                "the replaced terminal must never be replayed: {}",
                frame.data
            );
            if frame.event == "terminal" {
                live_terminal = Some(frame);
            }
        }
        assert_eq!(
            live_terminal.unwrap().data["event"]["response"]["output"]["output"]["text"],
            "ui18 retained two"
        );
        drop(live_stream);
        let (status, second_turn) = run_task.await.unwrap();
        assert_eq!(status, StatusCode::OK, "{second_turn}");

        let stale = ui18_open_stream(
            &client,
            &base,
            &token,
            &server.session,
            Some(retained_cursor.as_str()),
            None,
        )
        .await;
        assert_eq!(stale.status(), StatusCode::OK);
        let mut stale_stream = stale.bytes_stream();
        let mut stale_buffer = Vec::new();
        let stale_gap = ui18_next_frame(&mut stale_stream, &mut stale_buffer).await;
        assert_eq!(stale_gap.event, "stream_gap");
        let mut newest = Vec::new();
        let mut stale_deltas = 0usize;
        loop {
            let frame = ui18_next_frame(&mut stale_stream, &mut stale_buffer).await;
            if frame.event == "delta" {
                stale_deltas += 1;
            }
            let terminal = frame.event == "terminal";
            newest.push(frame);
            if terminal {
                break;
            }
        }
        assert_eq!(stale_deltas, 0, "a terminal replay never re-renders deltas");
        assert_eq!(newest.len(), 1, "only the newest terminal is retained");
        assert_eq!(
            newest[0].data["event"]["response"]["output"]["output"]["text"],
            "ui18 retained two"
        );
        drop(stale_stream);

        let _ = fs::remove_dir_all(&server.root);
        let _ = fs::remove_dir_all(&server.home);
    }

    #[tokio::test]
    async fn ui18_slow_tab_becomes_an_explicit_lagged_error_without_blocking_facts() {
        let server = ui18_server(&json!([{"text": "ui18 slow tab"}]).to_string()).await;

        // The first turn only provides the run identity every display subscription is bound to.
        let first = harness_run::run_envelope_on_host(
            Arc::clone(&server.host),
            server.session.clone(),
            "ui18 slow tab",
            &server.options,
        )
        .await
        .expect("fixture harness turn");
        assert_eq!(
            first.status,
            kiana_protocol::ExecutionStatus::Completed,
            "{first:?}"
        );
        let run_id = RunId::parse_str(first.output["run_id"].as_str().expect("run id"))
            .expect("fixture run id");

        // A tab that attaches and then never drains is exactly a slow/background tab.  Facts keep
        // committing; the display subscription falls behind the bounded queue.
        let subscription = server.host.subscribe_run_after(run_id, None);
        let attached = subscription.cursor().clone();
        let attached_sequence = attached.sequence;
        let mut state = EventStreamState {
            subscription,
            gap: None,
            done: false,
            shutdown: server.shutdown.clone(),
            last_cursor: attached,
        };
        let mut turns = 0usize;
        let budget = tokio::time::Instant::now() + Duration::from_secs(120);
        while server.host.run_stream_cursor(run_id).sequence
            <= kiana_daemon::UI_FEED_QUEUE_CAPACITY as u64
        {
            assert!(
                turns < 1024,
                "the slow-tab fixture must reach the bounded queue capacity"
            );
            assert!(
                tokio::time::Instant::now() < budget,
                "the slow-tab fixture exceeded its time budget"
            );
            let response = harness_run::run_envelope_on_host(
                Arc::clone(&server.host),
                server.session.clone(),
                "ui18 slow tab",
                &server.options,
            )
            .await
            .expect("slow-tab harness turn");
            assert_eq!(
                response.status,
                kiana_protocol::ExecutionStatus::Completed,
                "{response:?}"
            );
            turns += 1;
        }

        // A tab that fell behind receives one explicit bounded error carrying the server cursor it
        // observed last — not silence, not an unbounded buffer, and not a replayed delta.
        let frame = next_run_stream_frame(&mut state)
            .await
            .expect("a lagged display subscription still yields one explicit frame");
        let body = sse_event_body(frame).await;
        assert!(body.contains("event: stream_error"), "{body}");
        assert!(body.contains("stream_subscription_lagged:"), "{body}");
        assert!(body.contains("\"hydrate\":true"), "{body}");
        assert!(
            body.contains(&format!(
                "\"epoch\":\"{}\"",
                server.host.run_stream_cursor(run_id).epoch
            )),
            "{body}"
        );
        assert!(
            body.contains(&format!("\"sequence\":{attached_sequence}")),
            "the lagged error must carry the cursor the tab attached at: {body}"
        );
        assert!(
            state.done,
            "a lagged stream closes after its explicit error"
        );

        // Committed facts were never blocked: the same run kept completing turns and a fresh
        // subscriber resumes from the retained terminal.
        let events = server.host.ui_events().await.unwrap().unwrap();
        assert_eq!(
            events
                .iter()
                .filter(|event| event.kind == "run.authorized")
                .count(),
            1,
            "the slow tab never turned into a second run"
        );
        let mut fresh = server.host.subscribe_run_after(run_id, None);
        assert!(fresh.has_gap());
        let terminal = tokio::time::timeout(Duration::from_secs(10), fresh.recv())
            .await
            .expect("fresh subscriber timeout")
            .expect("fresh subscriber terminal");
        assert!(
            matches!(terminal.event, RunStreamEvent::Terminal { .. }),
            "{terminal:?}"
        );

        let _ = fs::remove_dir_all(&server.root);
        let _ = fs::remove_dir_all(&server.home);
    }

    #[tokio::test]
    async fn ui18_heartbeat_and_error_frames_re_state_the_cursor_without_payloads() {
        let cursor = UiCursor {
            epoch: "ui18-heartbeat-epoch".to_owned(),
            sequence: 7,
        };
        let heartbeat = sse_event_body(stream_heartbeat_sse_event(&cursor)).await;
        assert!(heartbeat.contains("event: heartbeat"), "{heartbeat}");
        assert!(
            heartbeat.contains("id: ui18-heartbeat-epoch:7"),
            "{heartbeat}"
        );
        assert!(
            heartbeat.contains("\"schema\":\"kiana.web-sse.v1\""),
            "{heartbeat}"
        );
        assert!(heartbeat.contains("\"sequence\":7"), "{heartbeat}");
        let error = sse_event_body(stream_error_sse_event_with_cursor(
            "stream_subscription_lagged:9",
            &cursor,
        ))
        .await;
        assert!(error.contains("event: stream_error"), "{error}");
        assert!(error.contains("id: ui18-heartbeat-epoch:7"), "{error}");
        assert!(error.contains("\"hydrate\":true"), "{error}");
        // The display frames never carry a credential or a payload beyond the bounded reason.
        for body in [heartbeat, error] {
            assert!(!body.contains("x-kiana-web-token"), "{body}");
            assert!(!body.contains("Authorization"), "{body}");
        }
    }
}

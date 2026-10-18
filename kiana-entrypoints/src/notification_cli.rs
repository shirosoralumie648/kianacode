//! CLI/TTY notification inbox and run-status presenter.
//!
//! This module is a bounded view adapter. It accepts server-produced page/frame DTOs, strips
//! payload/detail authority, and returns display rows. It never submits an action, changes read or
//! HumanTask state, retries a run, or starts another execution loop.
//!
//! The inbox route below is the single parse/plan surface behind the CLI and TTY surfaces:
//! `/inbox`, `/read`, `/ack`, `/approve`, `/deny`, `/review`, `/reconcile` and a dropped connection
//! all pass through [`plan_cli_inbox_intent`], which may only return a plan bound to a
//! server-issued page row or approval challenge. A forged scope, a foreign session, a read state
//! used as a decision, and a disconnect turned into cancel/resume are refused before anything
//! reaches the daemon. A planned notification action is still only a *candidate*: the
//! ControlPlane re-admits it through the shared `kiana.notification-action-command.v1` protocol.

use kiana_protocol::{NotificationActionKind, UiFeedFrameKind, UiFeedFrameV1};
use serde::{Deserialize, Serialize};
use serde_json::Value;

pub const CLI_NOTIFICATION_VIEW_SCHEMA: &str = "kiana.cli-notification-view.v1";
pub const CLI_RUN_STATUS_VIEW_SCHEMA: &str = "kiana.cli-run-status-view.v1";
pub const CLI_INBOX_ROUTE_SCHEMA: &str = "kiana.cli-inbox-route.v1";
pub const CLI_INBOX_PLAN_SCHEMA: &str = "kiana.cli-inbox-plan.v1";
pub const CLI_SESSION_BADGE_SCHEMA: &str = "kiana.cli-session-badges.v1";
pub const CLI_NOTIFICATION_MAX_ITEMS: usize = 30;
pub const CLI_NOTIFICATION_MAX_ACTIONS: usize = 16;
pub const CLI_INBOX_MAX_SESSIONS: usize = 16;
pub const CLI_INBOX_MAX_APPROVALS: usize = 30;
pub const CLI_INBOX_MAX_TARGET_BYTES: usize = 192;

/// Stable refusal codes owned by this entrypoint surface.
pub const CLI_NOTIFICATION_CLIENT_HINT: &str = "cli_notification_client_hint_forbidden";
pub const CLI_NOTIFICATION_PAGE_DIGEST_INVALID: &str = "cli_notification_page_digest_invalid";
pub const CLI_INBOX_LINE_INVALID: &str = "cli_inbox_line_invalid";
pub const CLI_INBOX_ACTION_UNKNOWN: &str = "cli_inbox_action_unknown";
pub const CLI_INBOX_SCOPE_FORBIDDEN: &str = "cli_inbox_scope_forbidden";
pub const CLI_INBOX_TARGET_INVALID: &str = "cli_inbox_target_invalid";
pub const CLI_INBOX_SESSION_INVALID: &str = "cli_inbox_session_invalid";
pub const CLI_INBOX_SESSION_MISMATCH: &str = "cli_inbox_session_mismatch";
pub const CLI_INBOX_SESSION_DUPLICATE: &str = "cli_inbox_session_duplicate";
pub const CLI_INBOX_SESSIONS_LIMIT: &str = "cli_inbox_sessions_limit";
pub const CLI_INBOX_CURSOR_INVALID: &str = "cli_inbox_cursor_invalid";
pub const CLI_INBOX_PAGE_DIGEST_REQUIRED: &str = "cli_inbox_page_digest_required";
pub const CLI_INBOX_SERVER_TIME_REQUIRED: &str = "cli_inbox_server_time_required";
pub const CLI_INBOX_ACTION_NOT_IN_PROJECTION: &str = "cli_inbox_action_not_in_projection";
pub const CLI_INBOX_DECISION_NOT_OFFERED: &str = "cli_inbox_decision_not_offered";
pub const CLI_INBOX_READ_IS_NOT_CONSUME: &str = "cli_inbox_read_is_not_consume";
pub const CLI_INBOX_DISCONNECT_IS_NOT_CANCEL: &str = "cli_inbox_disconnect_is_not_cancel";
pub const CLI_INBOX_PLAN_INVALID: &str = "cli_inbox_plan_invalid";

/// Client keys a server page may never carry. A page is a server projection: a client clock, a
/// client counter or a preselected answer would turn the display into a claim, so it fails closed
/// before any row is rendered.
const CLI_NOTIFICATION_FORBIDDEN_HINTS: [&str; 7] = [
    "client_time_unix_ms",
    "counted_by",
    "local_counts",
    "unread_hint",
    "preselected_actions",
    "client_scope",
    "recipient_scope",
];

/// Scope prefixes a CLI target may never carry: scope comes from the server page, never typed.
const CLI_INBOX_SCOPE_PREFIXES: [&str; 6] = [
    "session:",
    "recipient:",
    "scope:",
    "project:",
    "role:",
    "sandbox:",
];

fn required(value: &str, field: &'static str, max: usize) -> Result<(), String> {
    if value.trim().is_empty() || value.len() > max || value.contains(['\0', '\n', '\r']) {
        return Err(format!("{field}_invalid"));
    }
    Ok(())
}

/// A bounded opaque target. Whitespace is refused so one line can never smuggle scope tokens.
fn required_opaque(value: &str, field: &'static str, max: usize) -> Result<(), String> {
    required(value, field, max)?;
    if value.contains(' ') || value.contains('\t') {
        return Err(format!("{field}_invalid"));
    }
    Ok(())
}

fn required_digest(value: &str, field: &'static str) -> Result<(), String> {
    let Some(hex) = value.strip_prefix("sha256:") else {
        return Err(format!("{field}_invalid"));
    };
    if hex.len() != 64 || !hex.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err(format!("{field}_invalid"));
    }
    Ok(())
}

/// True when a server page carries a client-supplied hint key.
fn client_hint_in(value: &Value) -> Option<&'static str> {
    let object = value.as_object()?;
    object.keys().find_map(|key| {
        CLI_NOTIFICATION_FORBIDDEN_HINTS
            .iter()
            .copied()
            .find(|hint| key.as_str() == *hint)
    })
}

/// Scan the display-relevant levels of a page for client hints. `detail` is not scanned: it is
/// server-redacted display text, and a word inside it is not a page-level claim.
fn client_hint_in_page(page: &Value) -> Option<&'static str> {
    if let Some(hint) = client_hint_in(page) {
        return Some(hint);
    }
    let Some(items) = page.get("items").and_then(Value::as_array) else {
        return None;
    };
    for value in items {
        if let Some(hint) = client_hint_in(value) {
            return Some(hint);
        }
        let Some(item) = value.get("item") else {
            continue;
        };
        if let Some(hint) = client_hint_in(item) {
            return Some(hint);
        }
        if let Some(actions) = item.get("actions").and_then(Value::as_array) {
            for action in actions {
                if let Some(hint) = client_hint_in(action) {
                    return Some(hint);
                }
            }
        }
    }
    None
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CliNotificationAction {
    pub action_id: String,
    pub command: String,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CliNotificationRow {
    pub item_id: String,
    pub kind: String,
    pub title: String,
    pub urgency: String,
    pub due_at_unix_ms: u64,
    pub read: bool,
    pub acknowledged: bool,
    pub snoozed: bool,
    /// Whether the server marked the source outcome unresolved (`unknown`).
    #[serde(default)]
    pub unknown: bool,
    pub actions: Vec<CliNotificationAction>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CliNotificationView {
    pub schema: String,
    pub source_cursor: u64,
    /// Server clock reading carried by the page, when the server bound one.
    #[serde(default)]
    pub server_time_unix_ms: Option<u64>,
    /// Server-issued page digest; the inbox route refuses a page without it.
    #[serde(default)]
    pub page_digest: String,
    pub rows: Vec<CliNotificationRow>,
    pub next_after_item_id: Option<String>,
    pub unknown_visible: bool,
    pub limitations: Vec<String>,
}

/// 把服务端的通知页 DTO 投影成 CLI 能显示的行。
///
/// 【和 `web_notifications.rs` 是同一件事的两种输出】
/// 那边给浏览器、这边给终端，但**边界完全一样**：都只保留显示需要的字段，
/// 都不提交动作、都不改状态、都不重试、都不把断流当成完成。
///
/// 写成两份而不是抽一个公共函数，是因为两边的显示形态差别很大
/// （终端是纯文本行，浏览器是结构化 DTO）。共享的是**规则**，不是**代码**——
/// 而规则要靠两边各自的注释写清楚，靠一个共用函数反而没人读。
///
/// ⚠ 这意味着改这条边界时**两处都要改**。这是有意的成本：
///   少写一处的诱惑，正是一条 payload 悄悄越过边界的机会。
pub fn present_notification_page(page: &Value) -> Result<CliNotificationView, String> {
    if page.get("schema").and_then(Value::as_str) != Some("kiana.notification-page.v1") {
        return Err("cli_notification_page_schema_invalid".to_owned());
    }
    if let Some(hint) = client_hint_in_page(page) {
        return Err(format!("{CLI_NOTIFICATION_CLIENT_HINT}:{hint}"));
    }
    let source_cursor = page
        .get("source_cursor")
        .and_then(Value::as_u64)
        .filter(|value| *value > 0)
        .ok_or_else(|| "cli_notification_page_cursor_invalid".to_owned())?;
    let page_digest = page
        .get("page_digest")
        .and_then(Value::as_str)
        .ok_or_else(|| CLI_NOTIFICATION_PAGE_DIGEST_INVALID.to_owned())?;
    required_digest(page_digest, "cli_notification_page_digest")?;
    let server_time_unix_ms = page
        .get("server_time_unix_ms")
        .and_then(Value::as_u64)
        .filter(|value| *value > 0);
    let values = page
        .get("items")
        .and_then(Value::as_array)
        .ok_or_else(|| "cli_notification_page_items_invalid".to_owned())?;
    if values.len() > CLI_NOTIFICATION_MAX_ITEMS {
        return Err("cli_notification_page_limit".to_owned());
    }
    let mut rows = Vec::with_capacity(values.len());
    for value in values {
        let item = value
            .get("item")
            .ok_or_else(|| "cli_notification_item_missing".to_owned())?;
        let item_id = item
            .get("item_id")
            .and_then(Value::as_str)
            .ok_or_else(|| "cli_notification_item_id_missing".to_owned())?;
        let kind = item
            .get("kind")
            .and_then(Value::as_str)
            .ok_or_else(|| "cli_notification_kind_missing".to_owned())?;
        let title = item
            .get("title")
            .and_then(Value::as_str)
            .ok_or_else(|| "cli_notification_title_missing".to_owned())?;
        required(item_id, "cli_notification_item_id", 512)?;
        required(kind, "cli_notification_kind", 64)?;
        required(title, "cli_notification_title", 512)?;
        let urgency = value
            .get("urgency")
            .and_then(Value::as_str)
            .ok_or_else(|| "cli_notification_urgency_missing".to_owned())?;
        required(urgency, "cli_notification_urgency", 32)?;
        let due_at_unix_ms = value
            .get("due_at_unix_ms")
            .and_then(Value::as_u64)
            .ok_or_else(|| "cli_notification_due_missing".to_owned())?;
        let actions = item
            .get("actions")
            .and_then(Value::as_array)
            .ok_or_else(|| "cli_notification_actions_missing".to_owned())?;
        if actions.len() > CLI_NOTIFICATION_MAX_ACTIONS {
            return Err("cli_notification_actions_limit".to_owned());
        }
        let mut safe_actions = Vec::with_capacity(actions.len());
        for action in actions {
            let action_id = action
                .get("id")
                .and_then(Value::as_str)
                .ok_or_else(|| "cli_notification_action_id_missing".to_owned())?;
            let command = action
                .get("command")
                .and_then(Value::as_str)
                .ok_or_else(|| "cli_notification_action_command_missing".to_owned())?;
            required(action_id, "cli_notification_action_id", 256)?;
            required(command, "cli_notification_action_command", 128)?;
            safe_actions.push(CliNotificationAction {
                action_id: action_id.to_owned(),
                command: command.to_owned(),
            });
        }
        rows.push(CliNotificationRow {
            item_id: item_id.to_owned(),
            kind: kind.to_owned(),
            title: title.to_owned(),
            urgency: urgency.to_owned(),
            due_at_unix_ms,
            read: value.get("read").and_then(Value::as_bool).unwrap_or(false),
            acknowledged: value
                .get("acknowledged")
                .and_then(Value::as_bool)
                .unwrap_or(false),
            snoozed: value
                .get("snoozed_until_unix_ms")
                .and_then(Value::as_u64)
                .is_some(),
            unknown: value
                .get("unknown")
                .and_then(Value::as_bool)
                .unwrap_or(false),
            actions: safe_actions,
        });
    }
    Ok(CliNotificationView {
        schema: CLI_NOTIFICATION_VIEW_SCHEMA.to_owned(),
        source_cursor,
        server_time_unix_ms,
        page_digest: page_digest.to_owned(),
        rows,
        next_after_item_id: page
            .get("next_after_item_id")
            .and_then(Value::as_str)
            .map(str::to_owned),
        unknown_visible: true,
        limitations: vec![
            "actions_are_display_only_until_re_admitted_by_control_plane".to_owned(),
            "presenter_does_not_prove_durable_or_live_delivery".to_owned(),
        ],
    })
}

/// One CLI/TTY inbox verb. The set mirrors the shared notification action registry plus the two
/// entrypoint-only meanings (`read` is presentation state, `approve`/`deny` decide an approval
/// challenge); it is deliberately closed, so a word this surface does not own is an unknown
/// action rather than a new command.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CliInboxVerb {
    Read,
    Acknowledge,
    Approve,
    Deny,
    Review,
    Reconcile,
    Snooze,
    Escalate,
    Delegate,
    Withdraw,
}

impl CliInboxVerb {
    pub fn parse(word: &str) -> Option<Self> {
        match word {
            "read" | "mark-read" | "mark_read" => Some(Self::Read),
            "ack" | "acknowledge" => Some(Self::Acknowledge),
            "approve" => Some(Self::Approve),
            "deny" | "reject" => Some(Self::Deny),
            "review" => Some(Self::Review),
            "reconcile" => Some(Self::Reconcile),
            "snooze" => Some(Self::Snooze),
            "escalate" => Some(Self::Escalate),
            "delegate" => Some(Self::Delegate),
            "withdraw" => Some(Self::Withdraw),
            _ => None,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Read => "read",
            Self::Acknowledge => "acknowledge",
            Self::Approve => "approve",
            Self::Deny => "deny",
            Self::Review => "review",
            Self::Reconcile => "reconcile",
            Self::Snooze => "snooze",
            Self::Escalate => "escalate",
            Self::Delegate => "delegate",
            Self::Withdraw => "withdraw",
        }
    }

    /// The shared registry kind this verb must find offered by the server row. `read` is not a
    /// notification action, and `approve`/`deny` belong to the approval challenge protocol.
    pub fn action_kind(self) -> Option<NotificationActionKind> {
        match self {
            Self::Read | Self::Approve | Self::Deny => None,
            Self::Acknowledge => Some(NotificationActionKind::Acknowledge),
            Self::Review => Some(NotificationActionKind::Review),
            Self::Reconcile => Some(NotificationActionKind::Reconcile),
            Self::Snooze => Some(NotificationActionKind::Snooze),
            Self::Escalate => Some(NotificationActionKind::Escalate),
            Self::Delegate => Some(NotificationActionKind::Delegate),
            Self::Withdraw => Some(NotificationActionKind::Withdraw),
        }
    }

    /// The registered server command this verb must find in the server projection, if any.
    pub fn registered_command(self) -> Option<&'static str> {
        self.action_kind().map(|kind| kind.command())
    }

    pub fn is_approval_decision(self) -> bool {
        matches!(self, Self::Approve | Self::Deny)
    }

    /// Presentation verbs may change read/ACK/snooze projection state only; they never decide the
    /// original task and never consume its server-issued action.
    pub fn is_presentation_only(self) -> bool {
        self.registered_command()
            .is_some_and(|command| action_is_presentation(command))
            || matches!(self, Self::Read)
    }

    /// The authority surface this verb may reach. `read` is a presentation read; an approval
    /// decision decides one HumanTask; the registry verbs keep their own effect.
    pub fn authority_effect(self) -> &'static str {
        match self {
            Self::Read => "presentation_read",
            Self::Acknowledge | Self::Snooze => "presentation",
            Self::Approve | Self::Deny => "approval_decision",
            Self::Review | Self::Reconcile => "decision",
            Self::Escalate | Self::Delegate | Self::Withdraw => "governance",
        }
    }

    /// Whether the verb consumes the server-issued action it is bound to. A read never does.
    pub fn consumes_action(self) -> bool {
        !self.is_presentation_only()
    }
}

fn action_is_presentation(command: &str) -> bool {
    NotificationActionKind::from_command(command).is_some_and(|kind| kind.is_presentation_only())
}

/// What one CLI/TTY inbox line asked for. Parsing is pure: it creates no plan, holds no scope and
/// reaches no daemon.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum CliInboxIntent {
    /// Display the server page; never a submission.
    List,
    /// One verb against one server-issued target.
    Act { verb: CliInboxVerb, target: String },
    /// The connection ended (EOF/disconnect/TTY loss). Never an implicit cancel or resume.
    Detach,
}

fn carries_scope_prefix(value: &str) -> bool {
    CLI_INBOX_SCOPE_PREFIXES
        .iter()
        .any(|prefix| value.starts_with(prefix))
}

/// Parse one CLI/TTY inbox line.
///
/// A verb owns exactly one target argument. Every extra token, `key=value` pair, `--flag` or
/// `scope:`-style prefix is a forged scope and is refused here, before any verb is bound to the
/// server projection.
pub fn parse_cli_inbox_line(raw: &str) -> Result<CliInboxIntent, String> {
    let trimmed = raw.trim();
    let rest = trimmed
        .strip_prefix('/')
        .ok_or_else(|| CLI_INBOX_LINE_INVALID.to_owned())?;
    let mut parts = rest.split_whitespace();
    let Some(word) = parts.next() else {
        return Err(CLI_INBOX_LINE_INVALID.to_owned());
    };
    let arguments: Vec<&str> = parts.collect();
    if matches!(word, "inbox" | "list") {
        if arguments.is_empty() {
            return Ok(CliInboxIntent::List);
        }
        return Err(format!("{CLI_INBOX_SCOPE_FORBIDDEN}:{word}"));
    }
    if matches!(word, "detach" | "eof" | "disconnect") {
        if arguments.is_empty() {
            return Ok(CliInboxIntent::Detach);
        }
        return Err(format!("{CLI_INBOX_SCOPE_FORBIDDEN}:{word}"));
    }
    let Some(verb) = CliInboxVerb::parse(word) else {
        return Err(format!("{CLI_INBOX_ACTION_UNKNOWN}:{word}"));
    };
    let [target] = arguments.as_slice() else {
        return Err(CLI_INBOX_SCOPE_FORBIDDEN.to_owned());
    };
    if target.contains('=') || target.starts_with("--") || carries_scope_prefix(target) {
        return Err(CLI_INBOX_SCOPE_FORBIDDEN.to_owned());
    }
    required_opaque(target, "cli_inbox_target", CLI_INBOX_MAX_TARGET_BYTES)?;
    Ok(CliInboxIntent::Act {
        verb,
        target: (*target).to_owned(),
    })
}

/// One server-issued pending approval challenge, as projected for this CLI session.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CliApprovalBinding {
    pub approval_id: String,
    /// Request digest the server bound to the challenge; the decision must present it back.
    pub request_hash: String,
    /// Server-issued one-time nonce; empty when the server issued no nonce.
    pub nonce: String,
    /// The decisions the server offers for this challenge.
    pub decisions: Vec<String>,
}

impl CliApprovalBinding {
    /// Read the pending-approval listing this CLI session received.
    ///
    /// Id, proof hash, nonce and the offered decisions all come from the server; a listing that
    /// does not carry them is refused instead of being filled in locally.
    pub fn from_listing(listing: &Value) -> Result<Vec<Self>, String> {
        let approvals = listing
            .get("approvals")
            .and_then(Value::as_array)
            .ok_or_else(|| "cli_inbox_approvals_invalid".to_owned())?;
        if approvals.len() > CLI_INBOX_MAX_APPROVALS {
            return Err("cli_inbox_approvals_limit".to_owned());
        }
        let mut bindings = Vec::with_capacity(approvals.len());
        for entry in approvals {
            let challenge = entry
                .get("challenge")
                .ok_or_else(|| "cli_inbox_approvals_invalid".to_owned())?;
            let approval_id = challenge
                .get("approval_id")
                .and_then(Value::as_str)
                .ok_or_else(|| "cli_inbox_approvals_invalid".to_owned())?;
            let request_hash = challenge
                .get("request_hash")
                .and_then(Value::as_str)
                .ok_or_else(|| "cli_inbox_approvals_invalid".to_owned())?;
            let nonce = challenge
                .get("nonce")
                .and_then(Value::as_str)
                .unwrap_or_default();
            required_opaque(approval_id, "cli_inbox_approval_id", 128)?;
            required_opaque(request_hash, "cli_inbox_approval_request_hash", 256)?;
            // The server may issue no nonce; a present nonce still has to be one opaque token.
            if !nonce.is_empty() {
                required_opaque(nonce, "cli_inbox_approval_nonce", 256)?;
            }
            let decisions = entry
                .get("available_decisions")
                .and_then(Value::as_array)
                .ok_or_else(|| "cli_inbox_approvals_invalid".to_owned())?
                .iter()
                .map(|decision| {
                    decision
                        .as_str()
                        .map(str::to_owned)
                        .ok_or_else(|| "cli_inbox_approvals_invalid".to_owned())
                })
                .collect::<Result<Vec<String>, String>>()?;
            if decisions.is_empty() || decisions.len() > 8 {
                return Err("cli_inbox_approvals_invalid".to_owned());
            }
            for decision in &decisions {
                required_opaque(decision, "cli_inbox_approval_decision", 32)?;
            }
            bindings.push(Self {
                approval_id: approval_id.to_owned(),
                request_hash: request_hash.to_owned(),
                nonce: nonce.to_owned(),
                decisions,
            });
        }
        Ok(bindings)
    }

    pub fn offers(&self, decision: &str) -> bool {
        self.decisions.iter().any(|offered| offered == decision)
    }
}

/// One server page bound to the CLI session that fetched it, plus the server-issued approval
/// challenges that session may decide. It carries no authority: only the server's own cursor and
/// page digest, which is what a plan is bound to.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CliInboxRoute {
    pub schema: String,
    pub session_id: String,
    pub source_cursor: u64,
    pub page_digest: String,
    /// Server clock reading carried by the bound page, when the server bound one.
    pub server_time_unix_ms: Option<u64>,
    pub rows: Vec<CliNotificationRow>,
    pub approvals: Vec<CliApprovalBinding>,
    pub unknown_visible: bool,
}

impl CliInboxRoute {
    /// Bind one presented page to the CLI session that fetched it. A page without a server cursor
    /// or a server digest cannot be acted on: every plan must be traceable to the page it came
    /// from.
    pub fn from_view(session_id: &str, view: &CliNotificationView) -> Result<Self, String> {
        required(session_id, "cli_inbox_session", 128)?;
        if view.source_cursor == 0 {
            return Err(CLI_INBOX_CURSOR_INVALID.to_owned());
        }
        required_digest(&view.page_digest, "cli_inbox_page_digest")?;
        Ok(Self {
            schema: CLI_INBOX_ROUTE_SCHEMA.to_owned(),
            session_id: session_id.to_owned(),
            source_cursor: view.source_cursor,
            page_digest: view.page_digest.clone(),
            server_time_unix_ms: view.server_time_unix_ms,
            rows: view.rows.clone(),
            approvals: Vec::new(),
            unknown_visible: view.unknown_visible,
        })
    }

    /// A route for a session that has not fetched a page yet. Only approval challenges may be
    /// planned against it: they carry their own server binding and need no inbox row.
    pub fn for_session(session_id: &str) -> Result<Self, String> {
        required(session_id, "cli_inbox_session", 128)?;
        Ok(Self {
            schema: CLI_INBOX_ROUTE_SCHEMA.to_owned(),
            session_id: session_id.to_owned(),
            source_cursor: 0,
            page_digest: String::new(),
            server_time_unix_ms: None,
            rows: Vec::new(),
            approvals: Vec::new(),
            unknown_visible: true,
        })
    }

    /// Attach the approval challenges from the listing this session just received.
    pub fn with_approvals(mut self, listing: &Value) -> Result<Self, String> {
        self.approvals = CliApprovalBinding::from_listing(listing)?;
        Ok(self)
    }

    pub fn has_page(&self) -> bool {
        self.source_cursor > 0 && !self.page_digest.is_empty()
    }

    pub fn row(&self, item_id: &str) -> Option<&CliNotificationRow> {
        self.rows.iter().find(|row| row.item_id == item_id)
    }

    pub fn approval(&self, approval_id: &str) -> Option<&CliApprovalBinding> {
        self.approvals
            .iter()
            .find(|binding| binding.approval_id == approval_id)
    }
}

/// The outcome of planning one inbox intent. Every variant is effect-free: the caller still has to
/// hand an `Action` candidate to the ControlPlane re-admission path, and an `Approval` decision to
/// the existing approval protocol.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum CliInboxPlan {
    /// Display only: no submission, no state change.
    Display {
        session_id: String,
        source_cursor: u64,
        rows: usize,
    },
    /// Presentation-only read state for one row. Reading never consumes the action.
    Read {
        session_id: String,
        target: String,
        already_read: bool,
        acknowledged: bool,
        /// Always `false`: a read can never decide or consume the server-issued action.
        decides: bool,
        submission_route: String,
    },
    /// A registered notification action candidate that the ControlPlane must re-admit.
    Action {
        session_id: String,
        target: String,
        command: String,
        authority_effect: String,
        source_cursor: u64,
        re_admitted_by: String,
    },
    /// An approval decision bound to the server-issued challenge.
    Approval {
        session_id: String,
        approval_id: String,
        request_hash: String,
        approve: bool,
    },
    /// The connection ended. Detach only: never an implicit cancel or resume.
    Detach {
        session_id: String,
        follow_up: String,
        refusal: &'static str,
    },
}

impl CliInboxPlan {
    pub fn schema(&self) -> &'static str {
        CLI_INBOX_PLAN_SCHEMA
    }

    /// Whether this plan cancels or resumes a run. No CLI/TTY inbox plan ever does: a dropped
    /// connection is a detach, and the run keeps its own lifecycle.
    pub fn cancels_or_resumes(&self) -> bool {
        false
    }

    /// Bounded one-line description for the terminal. Display only.
    pub fn describe(&self) -> String {
        match self {
            Self::Display {
                source_cursor, rows, ..
            } => format!("inbox display: {rows} row(s) at server cursor {source_cursor}"),
            Self::Read {
                target,
                already_read,
                acknowledged,
                ..
            } => format!(
                "read {target}: presentation only (read {already_read}, acknowledged {acknowledged}); the action is not consumed"
            ),
            Self::Action {
                target,
                command,
                authority_effect,
                re_admitted_by,
                ..
            } => format!(
                "action candidate {target} -> {command} ({authority_effect}); re-admitted by {re_admitted_by}"
            ),
            Self::Approval {
                approval_id,
                approve,
                ..
            } => format!(
                "approval {approval_id}: {}",
                if *approve { "approve" } else { "deny" }
            ),
            Self::Detach { follow_up, .. } => {
                format!("detached: {follow_up}; no implicit cancel or resume")
            }
        }
    }
}

fn plan_read(route: &CliInboxRoute, target: &str) -> Result<CliInboxPlan, String> {
    let row = route
        .row(target)
        .ok_or_else(|| format!("{CLI_INBOX_ACTION_NOT_IN_PROJECTION}:{target}"))?;
    Ok(CliInboxPlan::Read {
        session_id: route.session_id.clone(),
        target: row.item_id.clone(),
        already_read: row.read,
        acknowledged: row.acknowledged,
        // A read is presentation state. It is not a decision, and it never consumes the action.
        decides: false,
        submission_route: "notification_read_state".to_owned(),
    })
}

fn plan_action(
    route: &CliInboxRoute,
    verb: CliInboxVerb,
    target: &str,
) -> Result<CliInboxPlan, String> {
    let Some(kind) = verb.action_kind() else {
        return Err(format!("{CLI_INBOX_ACTION_UNKNOWN}:{}", verb.as_str()));
    };
    let row = route
        .row(target)
        .ok_or_else(|| format!("{CLI_INBOX_ACTION_NOT_IN_PROJECTION}:{target}"))?;
    let command = kind.command();
    if !row.actions.iter().any(|action| action.command == command) {
        // Reading or acknowledging a row is not consuming it: when the projection only offers
        // presentation actions, a decision verb is refused instead of borrowing them.
        if !row.actions.is_empty()
            && row
                .actions
                .iter()
                .all(|action| action_is_presentation(&action.command))
        {
            return Err(format!("{CLI_INBOX_READ_IS_NOT_CONSUME}:{target}"));
        }
        return Err(format!("{CLI_INBOX_ACTION_NOT_IN_PROJECTION}:{target}"));
    }
    Ok(CliInboxPlan::Action {
        session_id: route.session_id.clone(),
        target: row.item_id.clone(),
        command: command.to_owned(),
        authority_effect: verb.authority_effect().to_owned(),
        source_cursor: route.source_cursor,
        re_admitted_by: "control_plane".to_owned(),
    })
}

fn plan_approval(
    route: &CliInboxRoute,
    verb: CliInboxVerb,
    target: &str,
) -> Result<CliInboxPlan, String> {
    let binding = route
        .approval(target)
        .ok_or_else(|| format!("{CLI_INBOX_ACTION_NOT_IN_PROJECTION}:{target}"))?;
    let decision = if verb == CliInboxVerb::Approve {
        "approve"
    } else {
        "deny"
    };
    if !binding.offers(decision) {
        return Err(format!("{CLI_INBOX_DECISION_NOT_OFFERED}:{target}"));
    }
    Ok(CliInboxPlan::Approval {
        session_id: route.session_id.clone(),
        approval_id: binding.approval_id.clone(),
        request_hash: binding.request_hash.clone(),
        approve: verb == CliInboxVerb::Approve,
    })
}

/// Plan one parsed intent against the server projection this CLI session fetched.
///
/// Deny-first, in this order: a line parsed for another session is refused; a target that is not in
/// the bound projection is refused; a decision verb that the projection does not offer is refused
/// rather than borrowed from read/ACK state; and a disconnect only ever yields a detach. Nothing
/// here submits, mutates or retries.
pub fn plan_cli_inbox_intent(
    route: &CliInboxRoute,
    session_id: &str,
    intent: &CliInboxIntent,
) -> Result<CliInboxPlan, String> {
    if session_id != route.session_id {
        return Err(format!("{CLI_INBOX_SESSION_MISMATCH}:{session_id}"));
    }
    match intent {
        CliInboxIntent::List => Ok(CliInboxPlan::Display {
            session_id: route.session_id.clone(),
            source_cursor: route.source_cursor,
            rows: route.rows.len(),
        }),
        CliInboxIntent::Detach => Ok(CliInboxPlan::Detach {
            session_id: route.session_id.clone(),
            follow_up: "requery_original".to_owned(),
            refusal: CLI_INBOX_DISCONNECT_IS_NOT_CANCEL,
        }),
        CliInboxIntent::Act { verb, target } => {
            required_opaque(target, "cli_inbox_target", CLI_INBOX_MAX_TARGET_BYTES)
                .map_err(|_| CLI_INBOX_TARGET_INVALID.to_owned())?;
            if verb.is_approval_decision() {
                plan_approval(route, *verb, target)
            } else if *verb == CliInboxVerb::Read {
                plan_read(route, target)
            } else {
                plan_action(route, *verb, target)
            }
        }
    }
}

/// Plan the disposition of a dropped CLI/TTY connection. A disconnect is a detach: it never
/// cancels the run, resumes it, or submits anything. The caller re-queries the original facts.
pub fn plan_cli_detach(route: &CliInboxRoute, session_id: &str) -> Result<CliInboxPlan, String> {
    plan_cli_inbox_intent(route, session_id, &CliInboxIntent::Detach)
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CliSessionBadge {
    pub session_id: String,
    pub source_cursor: u64,
    pub page_digest: String,
    pub unread: u64,
    pub read: u64,
    pub acknowledged: u64,
    pub unknown: u64,
    pub critical: u64,
    pub pending_approvals: u64,
    /// Always `server_page`: the counts are a projection of the server page, never client counts.
    pub derived_from: String,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CliSessionBadgeView {
    pub schema: String,
    pub active_session_id: String,
    pub server_time_unix_ms: u64,
    pub badges: Vec<CliSessionBadge>,
    pub unknown_visible: bool,
    pub limitations: Vec<String>,
}

/// Render the multi-session badges from the server pages the CLI holds.
///
/// The counts are a projection of each bound server page (`unread`/`read`/`acknowledged`/
/// `unknown`/`critical` come from server rows); the client never counts its own notification
/// state. The server clock is mandatory: a badge with no server time would be a client claim.
pub fn present_session_badges(
    active_session_id: &str,
    server_time_unix_ms: u64,
    routes: &[CliInboxRoute],
) -> Result<CliSessionBadgeView, String> {
    required(active_session_id, "cli_inbox_session", 128)?;
    if server_time_unix_ms == 0 {
        return Err(CLI_INBOX_SERVER_TIME_REQUIRED.to_owned());
    }
    if routes.len() > CLI_INBOX_MAX_SESSIONS {
        return Err(CLI_INBOX_SESSIONS_LIMIT.to_owned());
    }
    let mut badges: Vec<CliSessionBadge> = Vec::with_capacity(routes.len());
    for route in routes {
        required(&route.session_id, "cli_inbox_session", 128)
            .map_err(|_| CLI_INBOX_SESSION_INVALID.to_owned())?;
        if badges
            .iter()
            .any(|badge| badge.session_id == route.session_id)
        {
            return Err(CLI_INBOX_SESSION_DUPLICATE.to_owned());
        }
        if !route.has_page() {
            return Err(CLI_INBOX_PAGE_DIGEST_REQUIRED.to_owned());
        }
        required_digest(&route.page_digest, "cli_inbox_page_digest")?;
        let mut badge = CliSessionBadge {
            session_id: route.session_id.clone(),
            source_cursor: route.source_cursor,
            page_digest: route.page_digest.clone(),
            unread: 0,
            read: 0,
            acknowledged: 0,
            unknown: 0,
            critical: 0,
            pending_approvals: route.approvals.len() as u64,
            derived_from: "server_page".to_owned(),
        };
        for row in &route.rows {
            if row.read {
                badge.read += 1;
            } else {
                badge.unread += 1;
            }
            if row.acknowledged {
                badge.acknowledged += 1;
            }
            if row.unknown {
                badge.unknown += 1;
            }
            if row.urgency == "critical" {
                badge.critical += 1;
            }
        }
        badges.push(badge);
    }
    Ok(CliSessionBadgeView {
        schema: CLI_SESSION_BADGE_SCHEMA.to_owned(),
        active_session_id: active_session_id.to_owned(),
        server_time_unix_ms,
        badges,
        unknown_visible: true,
        limitations: vec![
            "badges_count_the_bound_server_page_not_a_client_counter".to_owned(),
            "badges_do_not_prove_durable_or_live_delivery".to_owned(),
        ],
    })
}

/// Bounded display lines for one inbox page.
///
/// Display only: a line shows what the server offered, and an action label is never presented as a
/// decision that has already happened.
pub fn render_inbox_lines(view: &CliNotificationView) -> Vec<String> {
    let mut lines = Vec::with_capacity(view.rows.len() + 1);
    lines.push(format!(
        "inbox: {} row(s) at server cursor {}",
        view.rows.len(),
        view.source_cursor
    ));
    for row in &view.rows {
        let mut marks: Vec<&str> = Vec::new();
        marks.push(if row.read { "read" } else { "unread" });
        if row.acknowledged {
            marks.push("ack");
        }
        if row.snoozed {
            marks.push("snoozed");
        }
        if row.unknown {
            marks.push("unknown");
        }
        let actions = if row.actions.is_empty() {
            "none".to_owned()
        } else {
            row.actions
                .iter()
                .map(|action| format!("{}({})", action.action_id, action.command))
                .collect::<Vec<String>>()
                .join(", ")
        };
        lines.push(format!(
            "[{}] {} · {} · actions: {}",
            row.urgency,
            row.title,
            marks.join("/"),
            actions
        ));
    }
    lines
}

/// Bounded display lines for the multi-session badge view.
pub fn render_badge_lines(view: &CliSessionBadgeView) -> Vec<String> {
    let mut lines = Vec::with_capacity(view.badges.len() + 1);
    lines.push(format!(
        "sessions: {} at server time {}",
        view.badges.len(),
        view.server_time_unix_ms
    ));
    for badge in &view.badges {
        let marker = if badge.session_id == view.active_session_id {
            "*"
        } else {
            " "
        };
        lines.push(format!(
            "{marker}{} unread:{} pending:{} unknown:{} critical:{} (server cursor {})",
            badge.session_id,
            badge.unread,
            badge.pending_approvals,
            badge.unknown,
            badge.critical,
            badge.source_cursor
        ));
    }
    lines
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CliRunStatus {
    Running,
    Heartbeat,
    Unknown,
    Terminal,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CliRunStatusView {
    pub schema: String,
    pub status: CliRunStatus,
    pub instance_id: String,
    pub authority_epoch: String,
    pub feed_sequence: u64,
    pub source_cursor: u64,
    pub snapshot_required: bool,
    pub retry: String,
}

pub fn present_run_status(frame: &UiFeedFrameV1) -> Result<CliRunStatusView, String> {
    frame.validate()?;
    let (status, snapshot_required, retry) = match frame.kind.clone() {
        UiFeedFrameKind::Gap => (CliRunStatus::Unknown, true, "query_original"),
        UiFeedFrameKind::Heartbeat => (CliRunStatus::Heartbeat, false, "do_not_retry"),
        UiFeedFrameKind::Terminal => (CliRunStatus::Terminal, false, "do_not_retry"),
        UiFeedFrameKind::Unknown => (CliRunStatus::Unknown, true, "query_original"),
        UiFeedFrameKind::SnapshotBoundary | UiFeedFrameKind::Delta => {
            (CliRunStatus::Running, false, "do_not_retry")
        }
    };
    Ok(CliRunStatusView {
        schema: CLI_RUN_STATUS_VIEW_SCHEMA.to_owned(),
        status,
        instance_id: frame.cursor.instance_id.clone(),
        authority_epoch: frame.cursor.authority_epoch.clone(),
        feed_sequence: frame.cursor.feed_sequence,
        source_cursor: frame.cursor.snapshot_cursor.sequence,
        snapshot_required,
        retry: retry.to_owned(),
    })
}

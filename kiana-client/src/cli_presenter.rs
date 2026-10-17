//! Pure CLI output presentation and exit-code contracts.
//!
//! The presenter is deliberately downstream of [`super::CliOutput`].  It only decides which
//! already-authorized projection is written to stdout or stderr; it never calls a daemon,
//! broker, runner, filesystem or process.  JSON output is always the versioned `CliOutput` DTO,
//! while TTY output is a disposable human view of that same DTO.
//!
//! The completion slice keeps four presentation-only contracts next to the same boundary:
//!
//! * [`CLI_EXIT_CODE_TABLE`] — one stable row per shell exit class with its wire name and
//!   meaning, shared by the documentation, the fixtures and [`CliExitCode`];
//! * [`CliStreamFault`] + [`write_cli_presentation`] — an adapter-owned writer contract that
//!   classifies `EPIPE`/interrupt without retrying, paging or spawning anything;
//! * [`CliPagerPlan`] — a bounded, line-aligned page split of one already-bounded TTY body, so
//!   pagination cannot drop or duplicate output and never spawns a pager;
//! * [`CliLocale`] — presentation-only labels; a locale never changes the DTO, the payload or
//!   the exit class.

use crate::{CliOutput, CliOutputMode, CLI_MAX_OUTPUT_BYTES};
use kiana_protocol::{CapabilityErrorCode, ExecutionStatus};
use serde::{Deserialize, Serialize};
use std::io::Write;

/// Stable shell exit values shared by the CLI presenter and capability error policy.
///
/// Values 0 through 8 are the protocol's stable classes.  130 and 141 retain the conventional
/// signal values for an interrupt and a broken pipe.  The presenter never turns `ResultUnknown`
/// into success: reconciliation remains visible to shell callers.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd, Serialize, Deserialize)]
#[repr(i32)]
pub enum CliExitCode {
    Success = 0,
    ExecutionFailed = 1,
    InvalidRequest = 2,
    PermissionDenied = 3,
    ApprovalRequired = 4,
    Conflict = 5,
    Capacity = 6,
    Unavailable = 7,
    Unknown = 8,
    Cancelled = 130,
    BrokenPipe = 141,
}

impl CliExitCode {
    /// Every stable exit class, ordered by the documented table.
    pub const ALL: [Self; 11] = [
        Self::Success,
        Self::ExecutionFailed,
        Self::InvalidRequest,
        Self::PermissionDenied,
        Self::ApprovalRequired,
        Self::Conflict,
        Self::Capacity,
        Self::Unavailable,
        Self::Unknown,
        Self::Cancelled,
        Self::BrokenPipe,
    ];

    pub const fn as_i32(self) -> i32 {
        self as i32
    }

    pub const fn is_success(self) -> bool {
        matches!(self, Self::Success)
    }

    /// Stable machine name used by the exit-code table, docs and shell wrappers.
    pub const fn wire_name(self) -> &'static str {
        match self {
            Self::Success => "success",
            Self::ExecutionFailed => "execution_failed",
            Self::InvalidRequest => "invalid_request",
            Self::PermissionDenied => "permission_denied",
            Self::ApprovalRequired => "approval_required",
            Self::Conflict => "conflict",
            Self::Capacity => "capacity",
            Self::Unavailable => "unavailable",
            Self::Unknown => "result_unknown",
            Self::Cancelled => "cancelled",
            Self::BrokenPipe => "broken_pipe",
        }
    }

    /// One-line meaning of the class for docs, help text and diagnostics.
    pub const fn meaning(self) -> &'static str {
        match self {
            Self::Success => {
                "accepted, in-flight or completed presentation without a structured error"
            }
            Self::ExecutionFailed => "execution failed",
            Self::InvalidRequest => "invalid request or unsupported schema",
            Self::PermissionDenied => "policy or permission denial",
            Self::ApprovalRequired => "approval is required",
            Self::Conflict => "conflict or not-found class",
            Self::Capacity => "capacity or budget limit",
            Self::Unavailable => "unavailable or timed out",
            Self::Unknown => "result is unknown and must be reconciled before any retry",
            Self::Cancelled => "cancelled or interrupted (SIGINT)",
            Self::BrokenPipe => "broken pipe on stdout or stderr (SIGPIPE)",
        }
    }

    /// Resolve a stable wire name back to its class (adapter/shell wrapper input only).
    pub fn parse_name(value: &str) -> Option<Self> {
        let normalized = value.trim();
        Self::ALL
            .iter()
            .copied()
            .find(|code| code.wire_name() == normalized)
    }

    /// Map one protocol error class to its exit code without running anything.
    ///
    /// New domain error classes must fail closed until this table is extended: an unknown
    /// `cli_exit` value falls back to `ExecutionFailed`, never to success.
    pub fn from_capability_error(code: CapabilityErrorCode) -> Self {
        match code.policy().cli_exit as i32 {
            0 => Self::Success,
            1 => Self::ExecutionFailed,
            2 => Self::InvalidRequest,
            3 => Self::PermissionDenied,
            4 => Self::ApprovalRequired,
            5 => Self::Conflict,
            6 => Self::Capacity,
            7 => Self::Unavailable,
            8 => Self::Unknown,
            130 => Self::Cancelled,
            _ => Self::ExecutionFailed,
        }
    }

    /// Derive a shell result from server-owned lifecycle status and its optional stable reason.
    ///
    /// A known reason takes precedence for statuses such as `Blocked` (which can represent
    /// permission, capacity or another policy decision).  Terminal unknown/cancelled statuses
    /// always retain their dedicated codes even if a diagnostic string is present.
    pub fn for_output(status: ExecutionStatus, error: Option<&str>) -> Self {
        match status {
            ExecutionStatus::ResultUnknown => Self::Unknown,
            ExecutionStatus::Cancelled => Self::Cancelled,
            _ => {
                if let Some(reason) = error {
                    return Self::from_capability_error(CapabilityErrorCode::from_reason(reason));
                }
                match status {
                    ExecutionStatus::Denied | ExecutionStatus::Blocked => Self::PermissionDenied,
                    ExecutionStatus::AwaitingApproval => Self::ApprovalRequired,
                    ExecutionStatus::Failed => Self::ExecutionFailed,
                    ExecutionStatus::Accepted
                    | ExecutionStatus::Queued
                    | ExecutionStatus::Cancelling
                    | ExecutionStatus::Running
                    | ExecutionStatus::Completed => Self::Success,
                    ExecutionStatus::Cancelled | ExecutionStatus::ResultUnknown => {
                        unreachable!("handled above")
                    }
                }
            }
        }
    }
}

/// The documented exit-code table: class, wire name and meaning in table order.
///
/// The roadmap card, `USER.md` and the UI-11 fixtures bind to this array; the client fixture
/// asserts row-by-row that [`CliExitCode::ALL`], [`CliExitCode::wire_name`] and
/// [`CliExitCode::meaning`] still agree with it, so the documented table cannot drift from the
/// code that produces the shell result.
pub const CLI_EXIT_CODE_TABLE: [(CliExitCode, &'static str, &'static str); 11] = [
    (
        CliExitCode::Success,
        "success",
        "accepted, in-flight or completed presentation without a structured error",
    ),
    (
        CliExitCode::ExecutionFailed,
        "execution_failed",
        "execution failed",
    ),
    (
        CliExitCode::InvalidRequest,
        "invalid_request",
        "invalid request or unsupported schema",
    ),
    (
        CliExitCode::PermissionDenied,
        "permission_denied",
        "policy or permission denial",
    ),
    (
        CliExitCode::ApprovalRequired,
        "approval_required",
        "approval is required",
    ),
    (
        CliExitCode::Conflict,
        "conflict",
        "conflict or not-found class",
    ),
    (
        CliExitCode::Capacity,
        "capacity",
        "capacity or budget limit",
    ),
    (
        CliExitCode::Unavailable,
        "unavailable",
        "unavailable or timed out",
    ),
    (
        CliExitCode::Unknown,
        "result_unknown",
        "result is unknown and must be reconciled before any retry",
    ),
    (
        CliExitCode::Cancelled,
        "cancelled",
        "cancelled or interrupted (SIGINT)",
    ),
    (
        CliExitCode::BrokenPipe,
        "broken_pipe",
        "broken pipe on stdout or stderr (SIGPIPE)",
    ),
];

/// Signals that can terminate a presentation before a response is available.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CliSignal {
    Sigint,
    BrokenPipe,
}

impl CliSignal {
    pub const ALL: [Self; 2] = [Self::Sigint, Self::BrokenPipe];

    pub const fn exit_code(self) -> CliExitCode {
        match self {
            Self::Sigint => CliExitCode::Cancelled,
            Self::BrokenPipe => CliExitCode::BrokenPipe,
        }
    }

    pub const fn wire_name(self) -> &'static str {
        match self {
            Self::Sigint => "sigint",
            Self::BrokenPipe => "broken_pipe",
        }
    }

    /// Classify an observed signal name.  Unknown names return `None` so the adapter keeps
    /// its own default instead of inventing an exit class.  No handler is installed here.
    pub fn parse(value: &str) -> Option<Self> {
        match value.trim().to_ascii_uppercase().as_str() {
            "SIGINT" | "INT" | "SIGINT_PENDING" | "INTERRUPT" => Some(Self::Sigint),
            "SIGPIPE" | "PIPE" | "EPIPE" | "BROKEN_PIPE" => Some(Self::BrokenPipe),
            _ => None,
        }
    }
}

/// An operating-system fault observed while writing an already-rendered presentation.
///
/// The presenter never retries, never closes a caller-owned stream and never turns a pipe
/// fault into a successful exit: the adapter decides whether to stop producing output.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CliStreamFault {
    BrokenPipe,
    Interrupted,
    Io,
}

impl CliStreamFault {
    /// Classify one `io::Error` without inspecting its message text.
    pub fn classify(error: &std::io::Error) -> Self {
        match error.kind() {
            std::io::ErrorKind::BrokenPipe => Self::BrokenPipe,
            std::io::ErrorKind::Interrupted => Self::Interrupted,
            _ => Self::Io,
        }
    }

    /// Signal class this fault maps to, when it is a signal-class fault.
    pub const fn signal(self) -> Option<CliSignal> {
        match self {
            Self::BrokenPipe => Some(CliSignal::BrokenPipe),
            Self::Interrupted => Some(CliSignal::Sigint),
            Self::Io => None,
        }
    }

    pub const fn exit_code(self) -> CliExitCode {
        match self {
            Self::BrokenPipe => CliExitCode::BrokenPipe,
            Self::Interrupted => CliExitCode::Cancelled,
            Self::Io => CliExitCode::Unavailable,
        }
    }

    pub const fn wire_name(self) -> &'static str {
        match self {
            Self::BrokenPipe => "broken_pipe",
            Self::Interrupted => "interrupted",
            Self::Io => "io",
        }
    }
}

/// TTY labels are presentation-only.  They never alter the JSON DTO or command identity.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CliLocale {
    English,
    Chinese,
}

impl Default for CliLocale {
    fn default() -> Self {
        Self::English
    }
}

impl CliLocale {
    pub const ALL: [Self; 2] = [Self::English, Self::Chinese];

    pub const fn wire_name(self) -> &'static str {
        match self {
            Self::English => "en",
            Self::Chinese => "zh",
        }
    }

    /// Classify one `LC_ALL`/`LC_MESSAGES`/`LANG` style value.
    ///
    /// Only the language part matters; encoding (`zh_CN.UTF-8`) and modifiers (`sr_RS@latin`)
    /// are ignored.  Unknown or garbled values fall back to English instead of failing the
    /// command, because labels are presentation-only and must never become a gate.
    pub fn from_env_value(value: &str) -> Self {
        let trimmed = value.trim();
        let without_encoding = trimmed.split('.').next().unwrap_or_default();
        let without_modifier = without_encoding.split('@').next().unwrap_or_default();
        let normalized = without_modifier.replace('-', "_").to_ascii_lowercase();
        match normalized.split('_').next().unwrap_or_default() {
            "zh" => Self::Chinese,
            _ => Self::English,
        }
    }
}

/// Bounds and environment facts supplied by the actual CLI adapter.
///
/// No pager or process is spawned here.  `paginate` is a contract check: only a real TTY may
/// request pagination, and JSON/quiet output always remains pipe-safe.  `page_bytes` is the
/// line-aligned page budget used only when `paginate` is set.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct CliPresenterOptions {
    pub tty: bool,
    pub locale: CliLocale,
    pub paginate: bool,
    pub max_tty_bytes: usize,
    pub page_bytes: usize,
}

impl Default for CliPresenterOptions {
    fn default() -> Self {
        Self {
            tty: false,
            locale: CliLocale::English,
            paginate: false,
            max_tty_bytes: CLI_DEFAULT_TTY_BYTES,
            page_bytes: CLI_DEFAULT_PAGE_BYTES,
        }
    }
}

pub const CLI_DEFAULT_TTY_BYTES: usize = 64 * 1024;
pub const CLI_MAX_DIAGNOSTIC_BYTES: usize = 512;
/// Default page budget for `--paginate` when the adapter does not choose one.
pub const CLI_DEFAULT_PAGE_BYTES: usize = 4 * 1024;
/// Smallest page budget the presenter accepts; anything smaller cannot hold a readable page.
pub const CLI_MIN_PAGE_BYTES: usize = 256;
/// Hard bound on pages derived from one bounded TTY body; more pages fail closed.
pub const CLI_MAX_PAGE_COUNT: usize = 512;
/// Upper bound for the pagination footer so a page plus footer stays provably bounded.
pub const CLI_MAX_PAGER_FOOTER_BYTES: usize = 64;
/// Deterministic, presentation-only footer printed after a paginated page.
pub const CLI_PAGER_FOOTER_PREFIX: &str = "-- more: page ";

/// A bounded, line-aligned page split of one already-rendered TTY body.
///
/// Pagination is a presentation contract, not a terminal control protocol: the presenter never
/// opens a pager, never reads the user's answer and never re-runs a command to fetch "the rest".
/// Pages are slices of the same bounded body, so nothing is dropped, duplicated or reordered;
/// a single line larger than one page budget stays whole on its own page instead of being
/// truncated mid-line.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CliPagerPlan {
    /// 1-based page carried by the presentation that produced this plan.
    pub page: usize,
    pub page_count: usize,
    pub line_count: usize,
    pub page_bytes: usize,
    starts: Vec<usize>,
    ends: Vec<usize>,
}

impl CliPagerPlan {
    /// Split one bounded TTY body into line-aligned pages of at most `page_bytes` bytes.
    pub fn plan(body: &str, page_bytes: usize) -> Result<Self, String> {
        if page_bytes < CLI_MIN_PAGE_BYTES || page_bytes > CLI_MAX_OUTPUT_BYTES {
            return Err("cli_pager_page_bytes_invalid".to_owned());
        }
        let mut starts: Vec<usize> = Vec::new();
        let mut ends: Vec<usize> = Vec::new();
        let mut lines = 0usize;
        let mut cursor = 0usize;
        let mut page_start = 0usize;
        let mut page_open = false;
        for line in body.split_inclusive('\n') {
            let line_start = cursor;
            let line_len = line.len();
            cursor += line_len;
            lines += 1;
            if !page_open {
                page_start = line_start;
                page_open = true;
            }
            if cursor - page_start > page_bytes && line_start > page_start {
                // This line did not fit and the page already holds earlier lines: close the
                // page before it instead of splitting the line across two pages.
                starts.push(page_start);
                ends.push(line_start);
                page_start = line_start;
            }
            if cursor - page_start > page_bytes {
                // One line larger than the whole page budget keeps its own page; the body is
                // already bounded, so this cannot grow without limit.
                starts.push(page_start);
                ends.push(cursor);
                page_open = false;
            }
            if starts.len() > CLI_MAX_PAGE_COUNT {
                return Err("cli_pager_too_many_pages".to_owned());
            }
        }
        if page_open {
            starts.push(page_start);
            ends.push(cursor);
        }
        if starts.is_empty() {
            // An empty body still has exactly one (empty) page, so an adapter never renders
            // "page 1/0" or invents a page from an index.
            starts.push(0);
            ends.push(0);
        }
        Ok(Self {
            page: 1,
            page_count: starts.len(),
            line_count: lines,
            page_bytes,
            starts,
            ends,
        })
    }

    pub const fn next_page(&self) -> Option<usize> {
        if self.page < self.page_count {
            Some(self.page + 1)
        } else {
            None
        }
    }

    pub const fn remaining_pages(&self) -> usize {
        self.page_count - self.page
    }

    /// Slice of the same bounded body that belongs to `page`, when `page` exists.
    pub fn page_text<'a>(&self, body: &'a str, page: usize) -> Option<&'a str> {
        if page == 0 || page > self.page_count {
            return None;
        }
        let start = *self.starts.get(page - 1)?;
        let end = *self.ends.get(page - 1)?;
        body.get(start..end)
    }

    /// Deterministic footer for the carried page; it is presentation-only and never a fact.
    pub fn footer(&self) -> String {
        format!(
            "{}{}/{} ({} remaining)",
            CLI_PAGER_FOOTER_PREFIX,
            self.page,
            self.page_count,
            self.remaining_pages()
        )
    }
}

/// The result that an adapter may write.  Keeping the two streams separate prevents logs and
/// diagnostics from corrupting a JSON pipeline.  The adapter owns the actual `Write` calls and
/// can turn an OS broken-pipe error into [`CliSignal::BrokenPipe`].
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CliPresentation {
    pub exit_code: CliExitCode,
    pub stdout: String,
    pub stderr: String,
    /// True when a byte bound cut the rendered body.  For a paginated TTY body this means a
    /// single line exceeded even the page budget after the footer reserve: the page offsets in
    /// `pager` still describe the untruncated body, so nothing is silently reported as complete.
    pub truncated: bool,
    /// Present exactly when the adapter asked for TTY pagination; `stdout` then carries
    /// `pager.page` and the remaining pages stay renderable from the same bounded body.
    pub pager: Option<CliPagerPlan>,
}

impl CliPresentation {
    pub const fn exit_status(&self) -> i32 {
        self.exit_code.as_i32()
    }

    /// True when `stdout` is safe for a shell pipeline: either empty or exactly one JSON
    /// document with no ANSI escapes and no extra lines (so a log line cannot hide inside it).
    pub fn stdout_is_pipe_safe(&self) -> bool {
        if self.stdout.is_empty() {
            return true;
        }
        if self.stdout.contains('\u{1b}') {
            return false;
        }
        let document = self.stdout.trim_end_matches('\n');
        if document.is_empty() || document.contains('\n') {
            return false;
        }
        if !(document.starts_with('{') && document.ends_with('}')) {
            return false;
        }
        serde_json::from_str::<serde_json::Value>(document).is_ok()
    }
}

/// Render a validated [`CliOutput`] without performing any side effect.
pub fn present_cli_output(
    output: &CliOutput,
    options: CliPresenterOptions,
) -> Result<CliPresentation, String> {
    output.validate()?;
    if options.max_tty_bytes == 0 || options.max_tty_bytes > CLI_MAX_OUTPUT_BYTES {
        return Err("cli_presenter_output_limit_invalid".to_owned());
    }
    if output.mode == CliOutputMode::Tty && !options.tty {
        return Err("cli_tty_output_requires_tty".to_owned());
    }
    if options.paginate && (!options.tty || output.mode != CliOutputMode::Tty) {
        return Err("cli_pager_requires_tty".to_owned());
    }
    if options.paginate
        && (options.page_bytes < CLI_MIN_PAGE_BYTES
            || options.page_bytes > CLI_MAX_OUTPUT_BYTES
            || options.page_bytes + CLI_MAX_PAGER_FOOTER_BYTES > options.max_tty_bytes)
    {
        return Err("cli_pager_page_bytes_invalid".to_owned());
    }

    let exit_code = CliExitCode::for_output(output.status, output.error.as_deref());
    match output.mode {
        CliOutputMode::Json => present_json(output, exit_code),
        CliOutputMode::Tty => present_tty(output, options, exit_code),
        CliOutputMode::Quiet => present_quiet(output, exit_code),
    }
}

fn present_json(output: &CliOutput, exit_code: CliExitCode) -> Result<CliPresentation, String> {
    // `validate` above bounds this value and rejects ANSI/secret fields.  Serializing the DTO as
    // one line keeps it safe for shell pipelines and preserves structured errors verbatim.
    let mut encoded =
        serde_json::to_string(output).map_err(|_| "cli_json_encode_failed".to_owned())?;
    encoded.push('\n');
    let warnings = render_warnings(output, CliLocale::English);
    let (stdout, stderr) = if exit_code.is_success() {
        (encoded, warnings)
    } else {
        // Keep the failing DTO as the first and only structured stderr record.  Its warnings
        // field remains machine-readable; duplicating warning text would corrupt JSON consumers.
        (String::new(), encoded)
    };
    Ok(CliPresentation {
        exit_code,
        stdout,
        stderr,
        truncated: false,
        pager: None,
    })
}

fn present_tty(
    output: &CliOutput,
    options: CliPresenterOptions,
    exit_code: CliExitCode,
) -> Result<CliPresentation, String> {
    let label = match options.locale {
        CliLocale::English => "status",
        CliLocale::Chinese => "状态",
    };
    let status = status_name(output.status);
    let payload = serde_json::to_string_pretty(&output.payload)
        .map_err(|_| "cli_tty_payload_encode_failed".to_owned())?;
    let mut body = format!("{label}: {status}\n{payload}\n");
    if let Some(error) = &output.error {
        let error_label = match options.locale {
            CliLocale::English => "error",
            CliLocale::Chinese => "错误",
        };
        body.push_str(&format!("{error_label}: {error}\n"));
    }
    let (body, mut truncated) = bound_text(body, options.max_tty_bytes);
    let mut pager = None;
    let body = if options.paginate {
        let plan = CliPagerPlan::plan(&body, options.page_bytes)?;
        // Reserve the footer budget before bounding the page text, so the footer is always
        // present and the rendered page plus footer stays inside `max_tty_bytes`.
        let page_limit = options.max_tty_bytes - CLI_MAX_PAGER_FOOTER_BYTES;
        let (page, page_truncated) = bound_text(
            plan.page_text(&body, plan.page)
                .unwrap_or_default()
                .to_owned(),
            page_limit,
        );
        truncated |= page_truncated;
        let mut rendered = page;
        rendered.push_str(&plan.footer());
        rendered.push('\n');
        pager = Some(plan);
        rendered
    } else {
        body
    };
    let mut stderr = render_warnings(output, options.locale);
    if !exit_code.is_success() && output.error.is_none() {
        let error_label = match options.locale {
            CliLocale::English => "error",
            CliLocale::Chinese => "错误",
        };
        stderr.push_str(&format!("{error_label}: {}\n", status_name(output.status)));
    }
    if exit_code.is_success() {
        Ok(CliPresentation {
            exit_code,
            stdout: body,
            stderr,
            truncated,
            pager,
        })
    } else {
        // Error output, including the bounded diagnostic body, is kept off stdout.
        stderr.insert_str(0, &body);
        Ok(CliPresentation {
            exit_code,
            stdout: String::new(),
            stderr,
            truncated,
            pager,
        })
    }
}

fn present_quiet(output: &CliOutput, exit_code: CliExitCode) -> Result<CliPresentation, String> {
    let mut stderr = render_warnings(output, CliLocale::English);
    if let Some(error) = &output.error {
        let (error, _) = bound_text(error.to_owned(), CLI_MAX_DIAGNOSTIC_BYTES);
        stderr.push_str(&format!("error: {error}\n"));
    } else if !exit_code.is_success() {
        stderr.push_str(&format!("error: {}\n", status_name(output.status)));
    }
    Ok(CliPresentation {
        exit_code,
        stdout: String::new(),
        stderr,
        truncated: false,
        pager: None,
    })
}

/// Write one already-rendered presentation to caller-owned streams.
///
/// The presenter owns no file descriptor, spawns no pager and never retries a failed write:
/// the first operating-system fault is returned as a typed [`CliStreamFault`] so the adapter
/// can map `EPIPE`/interrupt to the signal exit class.  The returned count only includes bytes
/// handed to the writers; flushing remains the adapter's job.
pub fn write_cli_presentation<O: Write, E: Write>(
    presentation: &CliPresentation,
    stdout: &mut O,
    stderr: &mut E,
) -> Result<usize, CliStreamFault> {
    let mut written = 0usize;
    if !presentation.stdout.is_empty() {
        stdout
            .write_all(presentation.stdout.as_bytes())
            .map_err(|error| CliStreamFault::classify(&error))?;
        written += presentation.stdout.len();
    }
    if !presentation.stderr.is_empty() {
        stderr
            .write_all(presentation.stderr.as_bytes())
            .map_err(|error| CliStreamFault::classify(&error))?;
        written += presentation.stderr.len();
    }
    Ok(written)
}

/// The exit code an adapter uses after a write attempt.
///
/// A signal-class fault (`EPIPE`, interrupt) replaces the response class, because the pipeline
/// or the user ended the presentation.  An ordinary I/O fault never hides the response class:
/// a denied command still reports its stable denial code.
pub fn exit_code_after_write(
    presentation: &CliPresentation,
    fault: Option<CliStreamFault>,
) -> CliExitCode {
    match fault.and_then(CliStreamFault::signal) {
        Some(signal) => signal.exit_code(),
        None => presentation.exit_code,
    }
}

fn render_warnings(output: &CliOutput, locale: CliLocale) -> String {
    let label = match locale {
        CliLocale::English => "warning",
        CliLocale::Chinese => "警告",
    };
    output
        .warnings
        .iter()
        .map(|warning| format!("{label}: {warning}\n"))
        .collect()
}

fn status_name(status: ExecutionStatus) -> &'static str {
    match status {
        ExecutionStatus::Accepted => "accepted",
        ExecutionStatus::Queued => "queued",
        ExecutionStatus::Cancelling => "cancelling",
        ExecutionStatus::Denied => "denied",
        ExecutionStatus::AwaitingApproval => "awaiting_approval",
        ExecutionStatus::Running => "running",
        ExecutionStatus::Completed => "completed",
        ExecutionStatus::Failed => "failed",
        ExecutionStatus::Cancelled => "cancelled",
        ExecutionStatus::ResultUnknown => "result_unknown",
        ExecutionStatus::Blocked => "blocked",
    }
}

fn bound_text(value: String, limit: usize) -> (String, bool) {
    if value.len() <= limit {
        return (value, false);
    }
    if limit <= 3 {
        let mut bounded = String::new();
        for character in value.chars() {
            if bounded.len() + character.len_utf8() > limit {
                break;
            }
            bounded.push(character);
        }
        return (bounded, true);
    }
    let mut bounded = String::new();
    for character in value.chars() {
        if bounded.len() + character.len_utf8() + 3 > limit {
            break;
        }
        bounded.push(character);
    }
    bounded.push_str("...");
    (bounded, true)
}

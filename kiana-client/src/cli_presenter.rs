//! Pure CLI output presentation and exit-code contracts.
//!
//! The presenter is deliberately downstream of [`super::CliOutput`].  It only decides which
//! already-authorized projection is written to stdout or stderr; it never calls a daemon,
//! broker, runner, filesystem or process.  JSON output is always the versioned `CliOutput` DTO,
//! while TTY output is a disposable human view of that same DTO.

use crate::{CliOutput, CliOutputMode, CLI_MAX_OUTPUT_BYTES};
use kiana_protocol::{CapabilityErrorCode, ExecutionStatus};
use serde::{Deserialize, Serialize};

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
    pub const fn as_i32(self) -> i32 {
        self as i32
    }

    pub const fn is_success(self) -> bool {
        matches!(self, Self::Success)
    }

    fn from_capability_error(code: CapabilityErrorCode) -> Self {
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
            // New domain error classes must fail closed until this table is extended.
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

/// Signals that can terminate a presentation before a response is available.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CliSignal {
    Sigint,
    BrokenPipe,
}

impl CliSignal {
    pub const fn exit_code(self) -> CliExitCode {
        match self {
            Self::Sigint => CliExitCode::Cancelled,
            Self::BrokenPipe => CliExitCode::BrokenPipe,
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

/// Bounds and environment facts supplied by the actual CLI adapter.
///
/// No pager or process is spawned here.  `paginate` is a contract check: only a real TTY may
/// request pagination, and JSON/quiet output always remains pipe-safe.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct CliPresenterOptions {
    pub tty: bool,
    pub locale: CliLocale,
    pub paginate: bool,
    pub max_tty_bytes: usize,
}

impl Default for CliPresenterOptions {
    fn default() -> Self {
        Self {
            tty: false,
            locale: CliLocale::English,
            paginate: false,
            max_tty_bytes: CLI_DEFAULT_TTY_BYTES,
        }
    }
}

pub const CLI_DEFAULT_TTY_BYTES: usize = 64 * 1024;
pub const CLI_MAX_DIAGNOSTIC_BYTES: usize = 512;

/// The result that an adapter may write.  Keeping the two streams separate prevents logs and
/// diagnostics from corrupting a JSON pipeline.  The adapter owns the actual `Write` calls and
/// can turn an OS broken-pipe error into [`CliSignal::BrokenPipe`].
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CliPresentation {
    pub exit_code: CliExitCode,
    pub stdout: String,
    pub stderr: String,
    pub truncated: bool,
}

impl CliPresentation {
    pub const fn exit_status(&self) -> i32 {
        self.exit_code.as_i32()
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
    let mut encoded = serde_json::to_string(output)
        .map_err(|_| "cli_json_encode_failed".to_owned())?;
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
    let (body, truncated) = bound_text(body, options.max_tty_bytes);
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
        })
    } else {
        // Error output, including the bounded diagnostic body, is kept off stdout.
        stderr.insert_str(0, &body);
        Ok(CliPresentation {
            exit_code,
            stdout: String::new(),
            stderr,
            truncated,
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
    })
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

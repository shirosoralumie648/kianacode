//! Versioned CLI command and output contracts.
//!
//! Parsing and normalization live next to the typed client boundary so every entrypoint can use
//! the same command IDs, workspace/session fences and redaction rules.  This module does not own a
//! transport, daemon, broker or model loop; callers still delegate an accepted invocation to
//! [`KianaClient::command`](crate::KianaClient::command).
//!
//! The completion slice keeps four pure tables at the same boundary:
//!
//! * [`CLI_COMMAND_SPECS`] — one canonical row per command with its typed client facade, protocol
//!   operation, session/retry policy, argument list and supported output modes;
//! * [`CliArgumentError`] — the stable argument error catalogue with one remediation per code;
//! * [`CliSessionRef`] — server-owned session resolution (`latest` sentinel plus explicit ids);
//! * [`CLI_LEGACY_FLAGS`] — the one-way legacy flag table used by [`normalize_legacy_args`].
//!
//! [`cli_help_text`] renders the same tables as deterministic help text.

use crate::typed::{
    UI_ACTION_CANCEL_OPERATION, UI_ACTION_CONTINUE_OPERATION, UI_ACTION_SUBMIT_OPERATION,
    UI_ARTIFACT_PAGE_OPERATION, UI_FEED_RESUME_OPERATION, UI_FEED_SUBSCRIBE_OPERATION,
    UI_QUERY_COMMAND_STATUS_OPERATION, UI_QUERY_HISTORY_OPERATION, UI_QUERY_SNAPSHOT_OPERATION,
};
use kiana_protocol::{ExecutionStatus, RequestId};
use serde::{Deserialize, Serialize};
use serde_json::Value;

pub const CLI_COMMAND_SCHEMA: &str = "kiana.cli-command.v1";
pub const CLI_OUTPUT_SCHEMA: &str = "kiana.cli-output.v1";
pub const CLI_COMMAND_CONTRACT_SCHEMA: &str = "kiana.cli-command-contract.v1";
/// Reserved `--session` value: the server resolves the most recent session it owns.
pub const CLI_SESSION_LATEST: &str = "latest";
pub const CLI_MAX_WORKSPACE_BYTES: usize = 4_096;
pub const CLI_MAX_SESSION_BYTES: usize = 256;
pub const CLI_MAX_ARGUMENT_BYTES: usize = 64 * 1024;
pub const CLI_MAX_OUTPUT_BYTES: usize = 1024 * 1024;
/// Byte bound for one normalized argument value inside the bounded JSON arguments object.
pub const CLI_MAX_ARGUMENT_VALUE_BYTES: usize = 64 * 1024;
/// Bounds for [`normalize_legacy_args`]: one command line and its migration notices stay bounded.
pub const CLI_MAX_ARGV_TOKENS: usize = 4_096;
pub const CLI_MAX_MIGRATION_NOTICES: usize = 32;

/// The stable command surface for CLI adapters.  Aliases are accepted by [`parse`], but the
/// canonical wire names below never change silently.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CliCommand {
    Run,
    Status,
    Events,
    Approve,
    Deny,
    Cancel,
    Resume,
    Receipt,
    Export,
    Session,
}

impl CliCommand {
    pub const ALL: [Self; 10] = [
        Self::Run,
        Self::Status,
        Self::Events,
        Self::Approve,
        Self::Deny,
        Self::Cancel,
        Self::Resume,
        Self::Receipt,
        Self::Export,
        Self::Session,
    ];

    pub fn parse(value: &str) -> Result<Self, String> {
        Self::parse_typed(value).map_err(|error| error.code().to_owned())
    }

    /// Canonical-command parsing with the stable error catalogue instead of a bare string.
    pub fn parse_typed(value: &str) -> Result<Self, CliArgumentError> {
        match value.trim().to_ascii_lowercase().as_str() {
            "run" => Ok(Self::Run),
            "status" => Ok(Self::Status),
            "events" | "event" => Ok(Self::Events),
            "approve" | "approval" => Ok(Self::Approve),
            "deny" => Ok(Self::Deny),
            "cancel" => Ok(Self::Cancel),
            "resume" | "continue" => Ok(Self::Resume),
            "receipt" => Ok(Self::Receipt),
            "export" => Ok(Self::Export),
            "session" | "sessions" => Ok(Self::Session),
            _ => Err(CliArgumentError::CommandUnknown),
        }
    }

    /// The canonical token a user types on the command line.
    pub const fn name(self) -> &'static str {
        match self {
            Self::Run => "run",
            Self::Status => "status",
            Self::Events => "events",
            Self::Approve => "approve",
            Self::Deny => "deny",
            Self::Cancel => "cancel",
            Self::Resume => "resume",
            Self::Receipt => "receipt",
            Self::Export => "export",
            Self::Session => "session",
        }
    }

    pub const fn wire_name(self) -> &'static str {
        match self {
            Self::Run => "run",
            Self::Status => "run.status",
            Self::Events => "run.events",
            Self::Approve => "approval.approve",
            Self::Deny => "approval.deny",
            Self::Cancel => "run.cancel",
            Self::Resume => "run.resume",
            Self::Receipt => "run.receipt",
            Self::Export => "audit.export",
            Self::Session => "session.query",
        }
    }

    pub const fn requires_session(self) -> bool {
        !matches!(self, Self::Run | Self::Session)
    }

    pub const fn mutates_state(self) -> bool {
        matches!(
            self,
            Self::Run | Self::Approve | Self::Deny | Self::Cancel | Self::Resume
        )
    }

    /// The declared contract row for this command.  The table is aligned with [`Self::ALL`] by the
    /// client fixtures, so a missing row is an internal invariant violation, not user input.
    pub fn spec(self) -> &'static CliCommandSpec {
        match CLI_COMMAND_SPECS.iter().find(|spec| spec.command == self) {
            Some(spec) => spec,
            None => panic!("cli_command_spec_missing"),
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CliOutputMode {
    Json,
    Tty,
    Quiet,
}

impl CliOutputMode {
    pub const fn wire_name(self) -> &'static str {
        match self {
            Self::Json => "json",
            Self::Tty => "tty",
            Self::Quiet => "quiet",
        }
    }

    pub fn parse(value: &str) -> Result<Self, String> {
        Self::parse_typed(value).map_err(|error| error.code().to_owned())
    }

    pub fn parse_typed(value: &str) -> Result<Self, CliArgumentError> {
        match value.trim().to_ascii_lowercase().as_str() {
            "json" => Ok(Self::Json),
            "tty" | "text" => Ok(Self::Tty),
            "quiet" => Ok(Self::Quiet),
            _ => Err(CliArgumentError::OutputModeInvalid),
        }
    }
}

/// Typed client facade a canonical command is allowed to call.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CliFacade {
    Query,
    Feed,
    Action,
    Artifact,
}

impl CliFacade {
    pub const fn wire_name(self) -> &'static str {
        match self {
            Self::Query => "query",
            Self::Feed => "feed",
            Self::Action => "action",
            Self::Artifact => "artifact",
        }
    }

    pub const fn client(self) -> &'static str {
        match self {
            Self::Query => "QueryClient",
            Self::Feed => "FeedClient",
            Self::Action => "ActionClient",
            Self::Artifact => "ArtifactClient",
        }
    }
}

/// Value shape of one bounded command argument.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CliArgumentKind {
    Text,
    Integer,
    Boolean,
    Enum(&'static [&'static str]),
}

impl CliArgumentKind {
    pub const fn wire_name(self) -> &'static str {
        match self {
            Self::Text => "text",
            Self::Integer => "integer",
            Self::Boolean => "boolean",
            Self::Enum(_) => "enum",
        }
    }

    pub const fn allowed_values(self) -> Option<&'static [&'static str]> {
        match self {
            Self::Enum(values) => Some(values),
            _ => None,
        }
    }

    fn accepts(self, value: &Value) -> bool {
        match self {
            Self::Text => value.as_str().is_some_and(|text| !text.trim().is_empty()),
            Self::Integer => value.as_i64().is_some_and(|number| number >= 0),
            Self::Boolean => value.is_boolean(),
            Self::Enum(values) => value.as_str().is_some_and(|text| values.contains(&text)),
        }
    }
}

/// One bounded command argument.  `max_bytes` is the encoded JSON size of the value.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct CliArgumentSpec {
    pub name: &'static str,
    pub required: bool,
    pub kind: CliArgumentKind,
    pub max_bytes: usize,
}

/// Declared contract for one canonical command.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct CliCommandSpec {
    pub command: CliCommand,
    pub wire_name: &'static str,
    pub summary: &'static str,
    pub usage: &'static str,
    pub operation: &'static str,
    /// Read-only follow-up operation a presenter uses while a primary command settles; `None`
    /// when the primary operation is already terminal.
    pub follow_up_operation: Option<&'static str>,
    pub facade: CliFacade,
    pub requires_session: bool,
    pub mutates_state: bool,
    pub idempotency_required: bool,
    pub output_modes: &'static [CliOutputMode],
    pub arguments: &'static [CliArgumentSpec],
}

impl CliCommandSpec {
    pub fn supports(&self, mode: CliOutputMode) -> bool {
        self.output_modes.contains(&mode)
    }

    pub fn argument(&self, name: &str) -> Option<&'static CliArgumentSpec> {
        self.arguments.iter().find(|argument| argument.name == name)
    }

    pub fn mode_names(&self) -> Vec<&'static str> {
        self.output_modes
            .iter()
            .map(|mode| mode.wire_name())
            .collect()
    }
}

const RUN_ARGUMENTS: [CliArgumentSpec; 2] = [
    CliArgumentSpec {
        name: "prompt",
        required: true,
        kind: CliArgumentKind::Text,
        max_bytes: CLI_MAX_ARGUMENT_VALUE_BYTES,
    },
    CliArgumentSpec {
        name: "stream",
        required: false,
        kind: CliArgumentKind::Boolean,
        max_bytes: 16,
    },
];

const STATUS_ARGUMENTS: [CliArgumentSpec; 1] = [CliArgumentSpec {
    name: "detail",
    required: false,
    kind: CliArgumentKind::Enum(&["summary", "full"]),
    max_bytes: 32,
}];

const EVENTS_ARGUMENTS: [CliArgumentSpec; 2] = [
    CliArgumentSpec {
        name: "after_sequence",
        required: false,
        kind: CliArgumentKind::Integer,
        max_bytes: 32,
    },
    CliArgumentSpec {
        name: "follow",
        required: false,
        kind: CliArgumentKind::Boolean,
        max_bytes: 16,
    },
];

const APPROVE_ARGUMENTS: [CliArgumentSpec; 2] = [
    CliArgumentSpec {
        name: "approval_id",
        required: true,
        kind: CliArgumentKind::Text,
        max_bytes: CLI_MAX_SESSION_BYTES,
    },
    CliArgumentSpec {
        name: "note",
        required: false,
        kind: CliArgumentKind::Text,
        max_bytes: 4_096,
    },
];

const DENY_ARGUMENTS: [CliArgumentSpec; 2] = [
    CliArgumentSpec {
        name: "approval_id",
        required: true,
        kind: CliArgumentKind::Text,
        max_bytes: CLI_MAX_SESSION_BYTES,
    },
    CliArgumentSpec {
        name: "reason",
        required: true,
        kind: CliArgumentKind::Text,
        max_bytes: 4_096,
    },
];

const CANCEL_ARGUMENTS: [CliArgumentSpec; 1] = [CliArgumentSpec {
    name: "reason",
    required: false,
    kind: CliArgumentKind::Text,
    max_bytes: 4_096,
}];

const RESUME_ARGUMENTS: [CliArgumentSpec; 1] = [CliArgumentSpec {
    name: "prompt",
    required: false,
    kind: CliArgumentKind::Text,
    max_bytes: CLI_MAX_ARGUMENT_VALUE_BYTES,
}];

const RECEIPT_ARGUMENTS: [CliArgumentSpec; 1] = [CliArgumentSpec {
    name: "detail",
    required: false,
    kind: CliArgumentKind::Enum(&["summary", "full"]),
    max_bytes: 32,
}];

const EXPORT_ARGUMENTS: [CliArgumentSpec; 2] = [
    CliArgumentSpec {
        name: "format",
        required: true,
        kind: CliArgumentKind::Enum(&["jsonl", "json"]),
        max_bytes: 32,
    },
    CliArgumentSpec {
        name: "since_sequence",
        required: false,
        kind: CliArgumentKind::Integer,
        max_bytes: 32,
    },
];

const SESSION_ARGUMENTS: [CliArgumentSpec; 1] = [CliArgumentSpec {
    name: "action",
    required: false,
    kind: CliArgumentKind::Enum(&["list", "attach", "new"]),
    max_bytes: 32,
}];

const ALL_MODES: [CliOutputMode; 3] = [
    CliOutputMode::Json,
    CliOutputMode::Tty,
    CliOutputMode::Quiet,
];

const ATTACHED_MODES: [CliOutputMode; 2] = [CliOutputMode::Json, CliOutputMode::Tty];

/// The ten canonical commands with their typed client facade, protocol operation, session/retry
/// policy, bounded arguments and supported output modes.
///
/// `quiet` is supported exactly by the commands whose outcome is described by the shell exit code
/// and server state (`run`, `status`, `cancel`, `resume`).  Commands whose server-owned payload is
/// the result (`events`, `approve`, `deny`, `receipt`, `export`, `session`) refuse it with
/// `cli_output_mode_unsupported` because suppressing the payload would hide the decision, the
/// receipt or the stream.
pub static CLI_COMMAND_SPECS: [CliCommandSpec; 10] = [
    CliCommandSpec {
        command: CliCommand::Run,
        wire_name: "run",
        summary: "Submit a prompt and wait for the terminal result.",
        usage: "kiana run --workspace <path> [--session <id>|--session latest] --prompt <text> [--stream] [--json|--quiet]",
        operation: UI_ACTION_SUBMIT_OPERATION,
        follow_up_operation: Some(UI_QUERY_COMMAND_STATUS_OPERATION),
        facade: CliFacade::Action,
        requires_session: false,
        mutates_state: true,
        idempotency_required: true,
        output_modes: &ALL_MODES,
        arguments: &RUN_ARGUMENTS,
    },
    CliCommandSpec {
        command: CliCommand::Status,
        wire_name: "run.status",
        summary: "Show the server-owned status snapshot for one session.",
        usage: "kiana status --workspace <path> --session <id> [--detail summary|full] [--json|--quiet]",
        operation: UI_QUERY_SNAPSHOT_OPERATION,
        follow_up_operation: None,
        facade: CliFacade::Query,
        requires_session: true,
        mutates_state: false,
        idempotency_required: false,
        output_modes: &ALL_MODES,
        arguments: &STATUS_ARGUMENTS,
    },
    CliCommandSpec {
        command: CliCommand::Events,
        wire_name: "run.events",
        summary: "Stream the versioned event feed for one session.",
        usage: "kiana events --workspace <path> --session <id> [--after <sequence>] [--follow] [--json]",
        operation: UI_FEED_SUBSCRIBE_OPERATION,
        follow_up_operation: Some(UI_FEED_RESUME_OPERATION),
        facade: CliFacade::Feed,
        requires_session: true,
        mutates_state: false,
        idempotency_required: false,
        output_modes: &ATTACHED_MODES,
        arguments: &EVENTS_ARGUMENTS,
    },
    CliCommandSpec {
        command: CliCommand::Approve,
        wire_name: "approval.approve",
        summary: "Approve one human action card by its server-owned id.",
        usage: "kiana approve --workspace <path> --session <id> --approval <id> [--note <text>] [--json]",
        operation: UI_ACTION_SUBMIT_OPERATION,
        follow_up_operation: Some(UI_QUERY_COMMAND_STATUS_OPERATION),
        facade: CliFacade::Action,
        requires_session: true,
        mutates_state: true,
        idempotency_required: true,
        output_modes: &ATTACHED_MODES,
        arguments: &APPROVE_ARGUMENTS,
    },
    CliCommandSpec {
        command: CliCommand::Deny,
        wire_name: "approval.deny",
        summary: "Deny one human action card and record the reason.",
        usage: "kiana deny --workspace <path> --session <id> --approval <id> --reason <text> [--json]",
        operation: UI_ACTION_SUBMIT_OPERATION,
        follow_up_operation: Some(UI_QUERY_COMMAND_STATUS_OPERATION),
        facade: CliFacade::Action,
        requires_session: true,
        mutates_state: true,
        idempotency_required: true,
        output_modes: &ATTACHED_MODES,
        arguments: &DENY_ARGUMENTS,
    },
    CliCommandSpec {
        command: CliCommand::Cancel,
        wire_name: "run.cancel",
        summary: "Request cancellation; the server owns the terminal state.",
        usage: "kiana cancel --workspace <path> --session <id> [--reason <text>] [--json|--quiet]",
        operation: UI_ACTION_CANCEL_OPERATION,
        follow_up_operation: Some(UI_QUERY_COMMAND_STATUS_OPERATION),
        facade: CliFacade::Action,
        requires_session: true,
        mutates_state: true,
        idempotency_required: true,
        output_modes: &ALL_MODES,
        arguments: &CANCEL_ARGUMENTS,
    },
    CliCommandSpec {
        command: CliCommand::Resume,
        wire_name: "run.resume",
        summary: "Continue a paused session through a new authority check.",
        usage: "kiana resume --workspace <path> --session <id> [--prompt <text>] [--json|--quiet]",
        operation: UI_ACTION_CONTINUE_OPERATION,
        follow_up_operation: Some(UI_QUERY_COMMAND_STATUS_OPERATION),
        facade: CliFacade::Action,
        requires_session: true,
        mutates_state: true,
        idempotency_required: true,
        output_modes: &ALL_MODES,
        arguments: &RESUME_ARGUMENTS,
    },
    CliCommandSpec {
        command: CliCommand::Receipt,
        wire_name: "run.receipt",
        summary: "Show the receipt projection for one session.",
        usage: "kiana receipt --workspace <path> --session <id> [--detail summary|full] [--json]",
        operation: UI_QUERY_SNAPSHOT_OPERATION,
        follow_up_operation: Some(UI_QUERY_HISTORY_OPERATION),
        facade: CliFacade::Query,
        requires_session: true,
        mutates_state: false,
        idempotency_required: false,
        output_modes: &ATTACHED_MODES,
        arguments: &RECEIPT_ARGUMENTS,
    },
    CliCommandSpec {
        command: CliCommand::Export,
        wire_name: "audit.export",
        summary: "Export the audit projection as bounded artifact pages.",
        usage: "kiana export --workspace <path> --session <id> --format jsonl|json [--since <sequence>] [--json]",
        operation: UI_ARTIFACT_PAGE_OPERATION,
        follow_up_operation: Some(UI_QUERY_HISTORY_OPERATION),
        facade: CliFacade::Artifact,
        requires_session: true,
        mutates_state: false,
        idempotency_required: false,
        output_modes: &ATTACHED_MODES,
        arguments: &EXPORT_ARGUMENTS,
    },
    CliCommandSpec {
        command: CliCommand::Session,
        wire_name: "session.query",
        summary: "List server-owned sessions; unknown ids are refused.",
        usage: "kiana session --workspace <path> [--action list|attach|new] [--json]",
        operation: UI_QUERY_SNAPSHOT_OPERATION,
        follow_up_operation: None,
        facade: CliFacade::Query,
        requires_session: false,
        mutates_state: false,
        idempotency_required: false,
        output_modes: &ATTACHED_MODES,
        arguments: &SESSION_ARGUMENTS,
    },
];
/// Stable argument/command error catalogue.  Every code is machine-readable and carries one
/// remediation; `Unknown` never maps to success and no code implies an implicit retry.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd)]
pub enum CliArgumentError {
    CommandUnknown,
    CommandSchemaInvalid,
    CommandIdInvalid,
    CommandIdMismatch,
    WorkspaceRequired,
    SessionRequired,
    SessionInvalid,
    SessionUnknown,
    InteractiveRequiresTty,
    TtyOutputRequiresTty,
    OutputModeInvalid,
    OutputModeUnsupported,
    ImplicitRetryForbidden,
    ArgumentsInvalid,
    SensitiveOrOversizedArguments,
    TooManyArguments,
    UnknownArgument,
    MissingArgument,
    InvalidArgument,
    OversizedArgument,
    LegacyFlagRetired,
    OutputSchemaInvalid,
    OutputInvalid,
    OutputSensitiveOrOversized,
    OutputErrorInvalid,
    OutputWarningInvalid,
    JsonAnsiForbidden,
}

impl CliArgumentError {
    pub const ALL: [Self; 27] = [
        Self::CommandUnknown,
        Self::CommandSchemaInvalid,
        Self::CommandIdInvalid,
        Self::CommandIdMismatch,
        Self::WorkspaceRequired,
        Self::SessionRequired,
        Self::SessionInvalid,
        Self::SessionUnknown,
        Self::InteractiveRequiresTty,
        Self::TtyOutputRequiresTty,
        Self::OutputModeInvalid,
        Self::OutputModeUnsupported,
        Self::ImplicitRetryForbidden,
        Self::ArgumentsInvalid,
        Self::SensitiveOrOversizedArguments,
        Self::TooManyArguments,
        Self::UnknownArgument,
        Self::MissingArgument,
        Self::InvalidArgument,
        Self::OversizedArgument,
        Self::LegacyFlagRetired,
        Self::OutputSchemaInvalid,
        Self::OutputInvalid,
        Self::OutputSensitiveOrOversized,
        Self::OutputErrorInvalid,
        Self::OutputWarningInvalid,
        Self::JsonAnsiForbidden,
    ];

    pub const fn code(self) -> &'static str {
        match self {
            Self::CommandUnknown => "cli_command_unknown",
            Self::CommandSchemaInvalid => "cli_command_schema_invalid",
            Self::CommandIdInvalid => "cli_command_id_invalid",
            Self::CommandIdMismatch => "cli_command_id_mismatch",
            Self::WorkspaceRequired => "cli_workspace_required",
            Self::SessionRequired => "cli_session_required",
            Self::SessionInvalid => "cli_session_invalid",
            Self::SessionUnknown => "cli_session_unknown",
            Self::InteractiveRequiresTty => "cli_interactive_requires_tty",
            Self::TtyOutputRequiresTty => "cli_tty_output_requires_tty",
            Self::OutputModeInvalid => "cli_output_mode_invalid",
            Self::OutputModeUnsupported => "cli_output_mode_unsupported",
            Self::ImplicitRetryForbidden => "cli_implicit_retry_forbidden",
            Self::ArgumentsInvalid => "cli_arguments_invalid",
            Self::SensitiveOrOversizedArguments => "cli_arguments_sensitive_or_oversized",
            Self::TooManyArguments => "cli_arguments_too_many",
            Self::UnknownArgument => "cli_argument_unknown",
            Self::MissingArgument => "cli_argument_missing",
            Self::InvalidArgument => "cli_argument_invalid",
            Self::OversizedArgument => "cli_argument_oversized",
            Self::LegacyFlagRetired => "cli_legacy_flag_retired",
            Self::OutputSchemaInvalid => "cli_output_schema_invalid",
            Self::OutputInvalid => "cli_output_invalid",
            Self::OutputSensitiveOrOversized => "cli_output_sensitive_or_oversized",
            Self::OutputErrorInvalid => "cli_output_error_invalid",
            Self::OutputWarningInvalid => "cli_output_warning_invalid",
            Self::JsonAnsiForbidden => "cli_json_ansi_forbidden",
        }
    }

    pub const fn remediation(self) -> &'static str {
        match self {
            Self::CommandUnknown => "run `kiana help` and use one of the ten canonical commands",
            Self::CommandSchemaInvalid => "recreate the invocation with the current command schema",
            Self::CommandIdInvalid => "generate a fresh non-nil request id for the command",
            Self::CommandIdMismatch => "keep the command id bound to the same command",
            Self::WorkspaceRequired => "pass --workspace <path> pointing at the workspace root",
            Self::SessionRequired => "pass --session <id> (or --session latest) for this command",
            Self::SessionInvalid => "session ids are 1..=256 bytes without control characters",
            Self::SessionUnknown => {
                "list sessions with `kiana session` and retry with an existing id"
            }
            Self::InteractiveRequiresTty => {
                "interactive prompts need a TTY; run without --interactive or use --json"
            }
            Self::TtyOutputRequiresTty => "use --json or --quiet when stdout is not a TTY",
            Self::OutputModeInvalid => "choose --json, --tty or --quiet",
            Self::OutputModeUnsupported => {
                "this command refuses the requested mode; the payload is the result"
            }
            Self::ImplicitRetryForbidden => {
                "query the original command id instead of retrying a state mutation"
            }
            Self::ArgumentsInvalid => "command arguments must be a JSON object",
            Self::SensitiveOrOversizedArguments => {
                "remove secret-shaped keys/values and keep arguments under the byte bound"
            }
            Self::TooManyArguments => "shorten argv and keep migration notices under the bound",
            Self::UnknownArgument => "remove the argument or check `kiana help`",
            Self::MissingArgument => "provide the required argument named in the command help",
            Self::InvalidArgument => "argument value has the wrong shape; check `kiana help`",
            Self::OversizedArgument => "argument exceeds its byte bound; shorten the value",
            Self::LegacyFlagRetired => {
                "replace the retired flag with the canonical command or flag from the notice"
            }
            Self::OutputSchemaInvalid => "render the current output schema only",
            Self::OutputInvalid => "re-encode the bounded output DTO",
            Self::OutputSensitiveOrOversized => {
                "strip secret-shaped fields and keep the output under the byte bound"
            }
            Self::OutputErrorInvalid => "keep one bounded, redacted error string",
            Self::OutputWarningInvalid => "keep at most 32 bounded, redacted warning strings",
            Self::JsonAnsiForbidden => "strip ANSI escapes before writing JSON",
        }
    }
}

/// Typed legacy-token disposition.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CliLegacyDisposition {
    /// Accepted: the token is rewritten to the canonical token.
    Rewrite(&'static str),
    /// Accepted: a boolean print-mode token is dropped.
    Drop,
    /// Refused with [`CliArgumentError::LegacyFlagRetired`].
    Retired,
}

/// One legacy CLI token and its canonical replacement.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct CliLegacyFlagSpec {
    pub legacy: &'static str,
    pub canonical: Option<&'static str>,
    pub disposition: CliLegacyDisposition,
    pub guidance: &'static str,
}

/// Legacy flags that predate the ten-command surface.  Rewriting and dropping never widens
/// authority: each canonical token is one the current contract already accepts, and transport or
/// recording flags that would bypass it are retired and refused.
pub static CLI_LEGACY_FLAGS: [CliLegacyFlagSpec; 14] = [
    CliLegacyFlagSpec {
        legacy: "--session-id",
        canonical: Some("--session"),
        disposition: CliLegacyDisposition::Rewrite("--session"),
        guidance: "use --session <id>",
    },
    CliLegacyFlagSpec {
        legacy: "-c",
        canonical: Some("--resume"),
        disposition: CliLegacyDisposition::Rewrite("--resume"),
        guidance: "use --resume <id> or the `resume` command",
    },
    CliLegacyFlagSpec {
        legacy: "--continue",
        canonical: Some("--resume"),
        disposition: CliLegacyDisposition::Rewrite("--resume"),
        guidance: "use --resume <id> or the `resume` command",
    },
    CliLegacyFlagSpec {
        legacy: "-r",
        canonical: Some("--resume"),
        disposition: CliLegacyDisposition::Rewrite("--resume"),
        guidance: "use --resume <id> or the `resume` command",
    },
    CliLegacyFlagSpec {
        legacy: "-p",
        canonical: Some("--prompt"),
        disposition: CliLegacyDisposition::Rewrite("--prompt"),
        guidance: "use the `run` command with --prompt <text>",
    },
    CliLegacyFlagSpec {
        legacy: "--print",
        canonical: None,
        disposition: CliLegacyDisposition::Drop,
        guidance: "use the `run` command",
    },
    CliLegacyFlagSpec {
        legacy: "--sdk-url",
        canonical: None,
        disposition: CliLegacyDisposition::Retired,
        guidance: "SDK transport is retired; use the local daemon transport",
    },
    CliLegacyFlagSpec {
        legacy: "--input-format",
        canonical: None,
        disposition: CliLegacyDisposition::Retired,
        guidance: "input is prompt text on the canonical `run` command",
    },
    CliLegacyFlagSpec {
        legacy: "--output-format",
        canonical: Some("--mode json|tty|quiet"),
        disposition: CliLegacyDisposition::Retired,
        guidance: "choose --json, --tty or --quiet",
    },
    CliLegacyFlagSpec {
        legacy: "--debug-file",
        canonical: None,
        disposition: CliLegacyDisposition::Retired,
        guidance: "diagnostics are server-owned events, not client-side files",
    },
    CliLegacyFlagSpec {
        legacy: "--record-only",
        canonical: None,
        disposition: CliLegacyDisposition::Retired,
        guidance: "recording mode is retired; submit a real command",
    },
    CliLegacyFlagSpec {
        legacy: "--replay-user-messages",
        canonical: None,
        disposition: CliLegacyDisposition::Retired,
        guidance: "use `kiana events` for the server-owned feed",
    },
    CliLegacyFlagSpec {
        legacy: "--include-partial-messages",
        canonical: None,
        disposition: CliLegacyDisposition::Retired,
        guidance: "use `kiana events --follow` for streaming deltas",
    },
    CliLegacyFlagSpec {
        legacy: "--fork-session",
        canonical: None,
        disposition: CliLegacyDisposition::Retired,
        guidance: "session forking is not a canonical command; start a new session",
    },
];

/// One migration notice a caller must display before running the rewritten command.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CliMigrationNotice {
    pub legacy: String,
    pub canonical: Option<String>,
    pub guidance: String,
}

/// Result of [`normalize_legacy_args`]: the canonical tokens plus the bounded notices.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CliLegacyNormalization {
    pub args: Vec<String>,
    pub notices: Vec<CliMigrationNotice>,
}

/// Look up one legacy token name (without its `=value` suffix).
pub fn legacy_flag_spec(name: &str) -> Option<&'static CliLegacyFlagSpec> {
    CLI_LEGACY_FLAGS.iter().find(|spec| spec.legacy == name)
}

/// Classify one token as a legacy flag, a retired flag or an ordinary token.
pub fn classify_legacy_flag(
    name: &str,
) -> Option<Result<&'static CliLegacyFlagSpec, CliArgumentError>> {
    legacy_flag_spec(name).map(|spec| match spec.disposition {
        CliLegacyDisposition::Retired => Err(CliArgumentError::LegacyFlagRetired),
        _ => Ok(spec),
    })
}

/// Rewrite legacy argv into canonical tokens.  Ordinary tokens and everything after `--` pass
/// through unchanged; retired flags fail closed with [`CliArgumentError::LegacyFlagRetired`].
pub fn normalize_legacy_args(args: &[String]) -> Result<CliLegacyNormalization, CliArgumentError> {
    if args.len() > CLI_MAX_ARGV_TOKENS {
        return Err(CliArgumentError::TooManyArguments);
    }
    let mut normalized = Vec::with_capacity(args.len());
    let mut notices = Vec::new();
    let mut passthrough = false;
    for token in args {
        if passthrough {
            normalized.push(token.clone());
            continue;
        }
        if token == "--" {
            passthrough = true;
            normalized.push(token.clone());
            continue;
        }
        let name = legacy_token_name(token);
        let Some(spec) = legacy_flag_spec(name) else {
            normalized.push(token.clone());
            continue;
        };
        let suffix = if token.len() > name.len() {
            &token[name.len()..]
        } else {
            ""
        };
        match spec.disposition {
            CliLegacyDisposition::Rewrite(canonical) => {
                normalized.push(format!("{canonical}{suffix}"));
                push_migration_notice(&mut notices, spec)?;
            }
            CliLegacyDisposition::Drop => {
                if !suffix.is_empty() {
                    // A value cannot be dropped with the token; fail closed instead of guessing.
                    return Err(CliArgumentError::LegacyFlagRetired);
                }
                push_migration_notice(&mut notices, spec)?;
            }
            CliLegacyDisposition::Retired => {
                return Err(CliArgumentError::LegacyFlagRetired);
            }
        }
    }
    Ok(CliLegacyNormalization {
        args: normalized,
        notices,
    })
}

fn legacy_token_name(token: &str) -> &str {
    token.split('=').next().unwrap_or(token)
}

fn push_migration_notice(
    notices: &mut Vec<CliMigrationNotice>,
    spec: &'static CliLegacyFlagSpec,
) -> Result<(), CliArgumentError> {
    if notices.len() >= CLI_MAX_MIGRATION_NOTICES {
        return Err(CliArgumentError::TooManyArguments);
    }
    notices.push(CliMigrationNotice {
        legacy: spec.legacy.to_owned(),
        canonical: spec.canonical.map(str::to_owned),
        guidance: spec.guidance.to_owned(),
    });
    Ok(())
}

/// Deterministic help text rendered from [`CLI_COMMAND_SPECS`].  It is a presentation of the same
/// tables (never a second contract) and stays byte-stable until the tables change.
pub fn cli_help_text() -> String {
    let mut lines = Vec::new();
    lines.push(format!(
        "kiana <command> [options] — {}",
        CLI_COMMAND_CONTRACT_SCHEMA
    ));
    lines.push("Commands:".to_owned());
    for spec in CLI_COMMAND_SPECS.iter() {
        lines.push(format!("  {:<16}  {}", spec.wire_name, spec.summary));
    }
    lines.push(String::new());
    for spec in CLI_COMMAND_SPECS.iter() {
        lines.push(format!("{}:", spec.wire_name));
        lines.push(format!("  usage: {}", spec.usage));
        lines.push(format!("  args: {}", describe_arguments(spec)));
        lines.push(format!("  modes: {}", spec.mode_names().join(", ")));
        lines.push(format!(
            "  scope: workspace required; session {}",
            if spec.requires_session {
                "required"
            } else {
                "optional"
            }
        ));
    }
    lines.join("\n")
}

fn describe_arguments(spec: &CliCommandSpec) -> String {
    if spec.arguments.is_empty() {
        return "none".to_owned();
    }
    spec.arguments
        .iter()
        .map(|argument| {
            let requirement = if argument.required {
                "required"
            } else {
                "optional"
            };
            match argument.kind.allowed_values() {
                Some(values) => format!(
                    "{} ({}, {}, {})",
                    argument.name,
                    argument.kind.wire_name(),
                    requirement,
                    values.join("|")
                ),
                None => format!(
                    "{} ({}, {}, <= {} bytes)",
                    argument.name,
                    argument.kind.wire_name(),
                    requirement,
                    argument.max_bytes
                ),
            }
        })
        .collect::<Vec<_>>()
        .join("; ")
}

/// Normalize a workspace value: surrounding whitespace is dropped, everything else must pass
/// [`validate_workspace`].  The daemon still owns workspace trust; this only bounds the local value.
pub fn normalize_workspace(value: &str) -> Result<String, CliArgumentError> {
    let value = value.trim();
    validate_workspace(value).map_err(|_| CliArgumentError::WorkspaceRequired)?;
    Ok(value.to_owned())
}

/// Validate the bounded JSON arguments object of one command.  Unknown, missing, wrongly shaped
/// and oversized arguments fail closed before an invocation reaches the client facade.
pub fn validate_command_arguments(
    command: CliCommand,
    arguments: &Value,
) -> Result<(), CliArgumentError> {
    let spec = command.spec();
    let fields = match arguments {
        Value::Null => None,
        Value::Object(fields) => Some(fields),
        _ => return Err(CliArgumentError::InvalidArgument),
    };
    for argument in spec.arguments.iter().filter(|argument| argument.required) {
        if fields
            .and_then(|fields| fields.get(argument.name))
            .is_none()
        {
            return Err(CliArgumentError::MissingArgument);
        }
    }
    let Some(fields) = fields else {
        return Ok(());
    };
    for (name, value) in fields {
        let Some(argument) = spec.argument(name) else {
            return Err(CliArgumentError::UnknownArgument);
        };
        validate_argument_value(argument, value)?;
    }
    Ok(())
}

fn validate_argument_value(
    argument: &CliArgumentSpec,
    value: &Value,
) -> Result<(), CliArgumentError> {
    let encoded = serde_json::to_vec(value).map_err(|_| CliArgumentError::InvalidArgument)?;
    if encoded.len() > argument.max_bytes {
        return Err(CliArgumentError::OversizedArgument);
    }
    if argument.kind.accepts(value) {
        Ok(())
    } else {
        Err(CliArgumentError::InvalidArgument)
    }
}

/// Server-owned session reference.  The CLI never invents a session id: `latest` is resolved by
/// the server and an explicit id must appear in the server-derived session snapshot.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum CliSessionRef {
    Latest,
    Explicit(String),
}

impl CliSessionRef {
    pub fn parse(value: &str) -> Result<Self, CliArgumentError> {
        let value = value.trim();
        if value == CLI_SESSION_LATEST {
            return Ok(Self::Latest);
        }
        validate_session(value).map_err(|_| CliArgumentError::SessionInvalid)?;
        Ok(Self::Explicit(value.to_owned()))
    }

    /// Resolve against the server-derived session list.  Unknown ids fail closed; `latest` is
    /// reported as `None` because only the server may pick the session.
    pub fn resolve(&self, known_sessions: &[String]) -> Result<Option<String>, CliArgumentError> {
        match self {
            Self::Latest => Ok(None),
            Self::Explicit(session_id) => {
                if known_sessions.iter().any(|known| known == session_id) {
                    Ok(Some(session_id.clone()))
                } else {
                    Err(CliArgumentError::SessionUnknown)
                }
            }
        }
    }

    pub fn session_id(&self) -> Option<&str> {
        match self {
            Self::Latest => None,
            Self::Explicit(session_id) => Some(session_id),
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CliCommandId {
    pub command: CliCommand,
    pub request_id: RequestId,
}

impl CliCommandId {
    pub fn new(command: CliCommand, request_id: RequestId) -> Self {
        Self {
            command,
            request_id,
        }
    }

    pub fn stable_key(&self) -> String {
        format!("{}:{}", self.command.wire_name(), self.request_id.as_uuid())
    }

    pub fn validate_typed(&self) -> Result<(), CliArgumentError> {
        if self.request_id.as_uuid().is_nil() {
            return Err(CliArgumentError::CommandIdInvalid);
        }
        Ok(())
    }

    fn validate(&self) -> Result<(), String> {
        self.validate_typed()
            .map_err(|error| error.code().to_owned())
    }
}

/// Input normalized by the CLI adapter before it calls the typed client facade.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CliInvocation {
    pub schema: String,
    pub command: CliCommand,
    pub command_id: CliCommandId,
    pub workspace: String,
    #[serde(default)]
    pub session_id: Option<String>,
    pub output_mode: CliOutputMode,
    pub tty: bool,
    pub interactive: bool,
    pub implicit_retry: bool,
    #[serde(default)]
    pub arguments: Value,
}

impl CliInvocation {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        command: CliCommand,
        request_id: RequestId,
        workspace: impl Into<String>,
        session_id: Option<String>,
        output_mode: CliOutputMode,
        tty: bool,
        interactive: bool,
        implicit_retry: bool,
        arguments: Value,
    ) -> Self {
        Self {
            schema: CLI_COMMAND_SCHEMA.to_owned(),
            command,
            command_id: CliCommandId::new(command, request_id),
            workspace: workspace.into(),
            session_id,
            output_mode,
            tty,
            interactive,
            implicit_retry,
            arguments,
        }
    }

    pub fn validate(&self) -> Result<(), String> {
        self.validate_typed()
            .map_err(|error| error.code().to_owned())
    }

    /// Structured validation for adapters, presenters and tests.  Same fences as
    /// [`CliInvocation::validate`], with the stable [`CliArgumentError`] catalogue.
    pub fn validate_typed(&self) -> Result<(), CliArgumentError> {
        if self.schema != CLI_COMMAND_SCHEMA {
            return Err(CliArgumentError::CommandSchemaInvalid);
        }
        self.command_id.validate_typed()?;
        if self.command_id.command != self.command {
            return Err(CliArgumentError::CommandIdMismatch);
        }
        normalize_workspace(&self.workspace)?;
        if let Some(session_id) = &self.session_id {
            validate_session(session_id).map_err(|_| CliArgumentError::SessionInvalid)?;
        }
        if self.command.requires_session() && self.session_id.is_none() {
            return Err(CliArgumentError::SessionRequired);
        }
        if self.interactive && !self.tty {
            return Err(CliArgumentError::InteractiveRequiresTty);
        }
        if self.output_mode == CliOutputMode::Tty && !self.tty {
            return Err(CliArgumentError::TtyOutputRequiresTty);
        }
        if !self.command.spec().supports(self.output_mode) {
            return Err(CliArgumentError::OutputModeUnsupported);
        }
        if self.implicit_retry && self.command.mutates_state() {
            return Err(CliArgumentError::ImplicitRetryForbidden);
        }
        let encoded =
            serde_json::to_vec(&self.arguments).map_err(|_| CliArgumentError::ArgumentsInvalid)?;
        if encoded.len() > CLI_MAX_ARGUMENT_BYTES
            || contains_secret_key(&self.arguments)
            || contains_secret_text_value(&self.arguments)
        {
            return Err(CliArgumentError::SensitiveOrOversizedArguments);
        }
        validate_command_arguments(self.command, &self.arguments)
    }

    /// The declared contract row this invocation claims; a caller may only route an invocation it
    /// has validated.
    pub fn route(&self) -> Result<&'static CliCommandSpec, CliArgumentError> {
        self.validate_typed()?;
        Ok(self.command.spec())
    }

    pub fn session_ref(&self) -> Result<Option<CliSessionRef>, CliArgumentError> {
        match self.session_id.as_deref() {
            None => Ok(None),
            Some(value) => CliSessionRef::parse(value).map(Some),
        }
    }

    /// Resolve the session against the server-derived session list.  `latest` returns `None`
    /// because the server picks the session; an unknown explicit id fails closed.
    pub fn resolve_session(
        &self,
        known_sessions: &[String],
    ) -> Result<Option<String>, CliArgumentError> {
        match self.session_ref()? {
            None => Ok(None),
            Some(reference) => reference.resolve(known_sessions),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CliOutput {
    pub schema: String,
    pub command_id: CliCommandId,
    pub mode: CliOutputMode,
    pub status: ExecutionStatus,
    pub payload: Value,
    #[serde(default)]
    pub error: Option<String>,
    /// Non-fatal diagnostics are part of the DTO but are routed to stderr by the presenter.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub warnings: Vec<String>,
}

impl CliOutput {
    pub fn new(
        command_id: CliCommandId,
        mode: CliOutputMode,
        status: ExecutionStatus,
        payload: Value,
        error: Option<String>,
    ) -> Self {
        Self {
            schema: CLI_OUTPUT_SCHEMA.to_owned(),
            command_id,
            mode,
            status,
            payload,
            error,
            warnings: Vec::new(),
        }
    }

    /// Attach bounded, non-fatal diagnostics without changing command identity or payload.
    pub fn with_warnings(mut self, warnings: impl IntoIterator<Item = String>) -> Self {
        self.warnings = warnings.into_iter().collect();
        self
    }

    pub fn validate(&self) -> Result<(), String> {
        self.validate_typed()
            .map_err(|error| error.code().to_owned())
    }

    /// Structured output validation with the same catalogue as the invocation boundary.
    pub fn validate_typed(&self) -> Result<(), CliArgumentError> {
        if self.schema != CLI_OUTPUT_SCHEMA {
            return Err(CliArgumentError::OutputSchemaInvalid);
        }
        self.command_id.validate_typed()?;
        let encoded = serde_json::to_vec(self).map_err(|_| CliArgumentError::OutputInvalid)?;
        if encoded.len() > CLI_MAX_OUTPUT_BYTES
            || contains_secret_key(&self.payload)
            || contains_secret_text_value(&self.payload)
        {
            return Err(CliArgumentError::OutputSensitiveOrOversized);
        }
        if self.mode == CliOutputMode::Json && contains_ansi(&self.payload) {
            return Err(CliArgumentError::JsonAnsiForbidden);
        }
        if self.error.as_deref().is_some_and(|value| {
            value.trim().is_empty()
                || value.len() > 512
                || contains_secret_text(value)
                || (self.mode == CliOutputMode::Json && value.contains('\u{1b}'))
        }) {
            return Err(CliArgumentError::OutputErrorInvalid);
        }
        if self.warnings.len() > 32
            || self.warnings.iter().any(|warning| {
                warning.trim().is_empty()
                    || warning.len() > 512
                    || contains_secret_text(warning)
                    || (self.mode == CliOutputMode::Json && warning.contains('\u{1b}'))
            })
        {
            return Err(CliArgumentError::OutputWarningInvalid);
        }
        Ok(())
    }
}

pub fn validate_workspace(value: &str) -> Result<(), String> {
    if value.trim().is_empty()
        || value.len() > CLI_MAX_WORKSPACE_BYTES
        || value.contains(['\0', '\r', '\n'])
    {
        return Err("cli_workspace_required".to_owned());
    }
    Ok(())
}

pub fn validate_session(value: &str) -> Result<(), String> {
    if value.trim().is_empty()
        || value.len() > CLI_MAX_SESSION_BYTES
        || value.contains(['\0', '\r', '\n'])
    {
        return Err("cli_session_invalid".to_owned());
    }
    Ok(())
}

fn contains_secret_key(value: &Value) -> bool {
    match value {
        Value::Object(fields) => fields.iter().any(|(key, value)| {
            let key = key.to_ascii_lowercase();
            is_secret_key(&key) || contains_secret_key(value)
        }),
        Value::Array(values) => values.iter().any(contains_secret_key),
        _ => false,
    }
}

fn contains_secret_text_value(value: &Value) -> bool {
    match value {
        Value::String(value) => contains_secret_text(value),
        Value::Object(fields) => fields.values().any(contains_secret_text_value),
        Value::Array(values) => values.iter().any(contains_secret_text_value),
        _ => false,
    }
}

fn is_secret_key(key: &str) -> bool {
    key == "token"
        || key.ends_with("_token")
        || key == "secret"
        || key.ends_with("_secret")
        || key == "authorization"
        || key == "password"
        || key.ends_with("_password")
        || key.contains("api_key")
        || key == "credential_value"
}

fn contains_secret_text(value: &str) -> bool {
    let lower = value.to_ascii_lowercase();
    lower.contains("bearer ")
        || lower.contains("api_key=")
        || lower.contains("authorization:")
        || lower.contains("secret=")
        || lower.contains("token=")
}

fn contains_ansi(value: &Value) -> bool {
    match value {
        Value::String(value) => value.contains('\u{1b}'),
        Value::Object(fields) => fields.values().any(contains_ansi),
        Value::Array(values) => values.iter().any(contains_ansi),
        _ => false,
    }
}

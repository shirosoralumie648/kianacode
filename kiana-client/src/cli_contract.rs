//! Versioned CLI command and output contracts.
//!
//! Parsing and normalization live next to the typed client boundary so every entrypoint can use
//! the same command IDs, workspace/session fences and redaction rules.  This module does not own a
//! transport, daemon, broker or model loop; callers still delegate an accepted invocation to
//! [`KianaClient::command`](crate::KianaClient::command).

use kiana_protocol::{ExecutionStatus, RequestId};
use serde::{Deserialize, Serialize};
use serde_json::Value;

pub const CLI_COMMAND_SCHEMA: &str = "kiana.cli-command.v1";
pub const CLI_OUTPUT_SCHEMA: &str = "kiana.cli-output.v1";
pub const CLI_MAX_WORKSPACE_BYTES: usize = 4_096;
pub const CLI_MAX_SESSION_BYTES: usize = 256;
pub const CLI_MAX_ARGUMENT_BYTES: usize = 64 * 1024;
pub const CLI_MAX_OUTPUT_BYTES: usize = 1024 * 1024;

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
            _ => Err("cli_command_unknown".to_owned()),
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
        matches!(self, Self::Run | Self::Approve | Self::Deny | Self::Cancel | Self::Resume)
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
    pub fn parse(value: &str) -> Result<Self, String> {
        match value.trim().to_ascii_lowercase().as_str() {
            "json" => Ok(Self::Json),
            "tty" | "text" => Ok(Self::Tty),
            "quiet" => Ok(Self::Quiet),
            _ => Err("cli_output_mode_invalid".to_owned()),
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
        Self { command, request_id }
    }

    pub fn stable_key(&self) -> String {
        format!("{}:{}", self.command.wire_name(), self.request_id.as_uuid())
    }

    fn validate(&self) -> Result<(), String> {
        if self.request_id.as_uuid().is_nil() {
            return Err("cli_command_id_invalid".to_owned());
        }
        Ok(())
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
        if self.schema != CLI_COMMAND_SCHEMA {
            return Err("cli_command_schema_invalid".to_owned());
        }
        self.command_id.validate()?;
        if self.command_id.command != self.command {
            return Err("cli_command_id_mismatch".to_owned());
        }
        validate_workspace(&self.workspace)?;
        if let Some(session_id) = &self.session_id {
            validate_session(session_id)?;
        }
        if self.command.requires_session() && self.session_id.is_none() {
            return Err("cli_session_required".to_owned());
        }
        if self.interactive && !self.tty {
            return Err("cli_interactive_requires_tty".to_owned());
        }
        if self.output_mode == CliOutputMode::Tty && !self.tty {
            return Err("cli_tty_output_requires_tty".to_owned());
        }
        if self.implicit_retry && self.command.mutates_state() {
            return Err("cli_implicit_retry_forbidden".to_owned());
        }
        let encoded = serde_json::to_vec(&self.arguments)
            .map_err(|_| "cli_arguments_invalid".to_owned())?;
        if encoded.len() > CLI_MAX_ARGUMENT_BYTES
            || contains_secret_key(&self.arguments)
            || contains_secret_text_value(&self.arguments)
        {
            return Err("cli_arguments_sensitive_or_oversized".to_owned());
        }
        Ok(())
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
        if self.schema != CLI_OUTPUT_SCHEMA {
            return Err("cli_output_schema_invalid".to_owned());
        }
        self.command_id.validate()?;
        let encoded = serde_json::to_vec(self).map_err(|_| "cli_output_invalid".to_owned())?;
        if encoded.len() > CLI_MAX_OUTPUT_BYTES
            || contains_secret_key(&self.payload)
            || contains_secret_text_value(&self.payload)
        {
            return Err("cli_output_sensitive_or_oversized".to_owned());
        }
        if self.mode == CliOutputMode::Json && contains_ansi(&self.payload) {
            return Err("cli_json_ansi_forbidden".to_owned());
        }
        if self.error.as_deref().is_some_and(|value| {
            value.trim().is_empty()
                || value.len() > 512
                || contains_secret_text(value)
                || (self.mode == CliOutputMode::Json && value.contains('\u{1b}'))
        }) {
            return Err("cli_output_error_invalid".to_owned());
        }
        if self.warnings.len() > 32
            || self.warnings.iter().any(|warning| {
                warning.trim().is_empty()
                    || warning.len() > 512
                    || contains_secret_text(warning)
                    || (self.mode == CliOutputMode::Json && warning.contains('\u{1b}'))
            })
        {
            return Err("cli_output_warning_invalid".to_owned());
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

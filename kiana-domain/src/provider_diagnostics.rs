//! Provider selection, configuration and diagnostics projections.
//!
//! These values are read-only views.  They describe a secret-free catalog/configuration snapshot
//! and the progress observed for an already admitted model attempt; they never authorize a model
//! request, open a connection or mutate an active run.  A UI may discard a projection whenever
//! either authority or configuration epoch changes and hydrate a fresh snapshot from the daemon.

use crate::{
    json_digest, ModelCapabilities, ModelCatalog, ModelProtocol, ModelUsage,
    ProviderConfigSnapshot, SchemaVersion,
};
use serde::{Deserialize, Serialize};
use serde_json::json;

pub const PROVIDER_DIAGNOSTICS_SCHEMA: &str = "kiana.provider-diagnostics.v1";
pub const PROVIDER_DIAGNOSTICS_SNAPSHOT_SCHEMA: &str = "kiana.provider-diagnostics-snapshot.v1";
pub const PROVIDER_DIAGNOSTICS_CURSOR_SCHEMA: &str = "kiana.provider-diagnostics-cursor.v1";
pub const PROVIDER_CONFIG_CHECK_SCHEMA: &str = "kiana.provider-config-check.v1";
pub const PROVIDER_CONNECTION_TEST_SCHEMA: &str = "kiana.provider-connection-test.v1";
pub const PROVIDER_TERMINAL_REPLAY_SCHEMA: &str = "kiana.provider-terminal-replay.v1";
pub const PROVIDER_DIAGNOSTICS_VERSION: SchemaVersion = SchemaVersion::new(1, 0);
pub const PROVIDER_DIAGNOSTICS_PROJECTION_STALE: &str = "provider_diagnostics_projection_stale";
pub const PROVIDER_DIAGNOSTICS_CONNECTION_TEST_EXPLICIT: &str =
    "provider_connection_test_requires_explicit_admission";
pub const PROVIDER_DIAGNOSTICS_SETTINGS_NO_INFERENCE: &str =
    "provider_settings_view_does_not_trigger_inference";
const MAX_ENTRIES: usize = 4_096;
const MAX_ERROR_TEXT: usize = 512;
const MAX_ACTION_TEXT: usize = 256;

fn required(value: &str, field: &str, max: usize) -> Result<(), String> {
    if value.trim().is_empty() || value.len() > max || value.contains('\0') {
        return Err(format!("{field}_invalid"));
    }
    Ok(())
}

fn digest(value: &str, field: &str) -> Result<(), String> {
    let Some(hex) = value.strip_prefix("sha256:") else {
        return Err(format!("{field}_invalid"));
    };
    if hex.len() != 64 || !hex.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err(format!("{field}_invalid"));
    }
    Ok(())
}

fn safe_text(value: &str, field: &str, max: usize) -> Result<(), String> {
    required(value, field, max)?;
    let lower = value.to_ascii_lowercase();
    for marker in [
        "authorization:",
        "bearer ",
        "api_key",
        "api-key",
        "access_token",
        "refresh_token",
        "client_secret",
        "password=",
    ] {
        if lower.contains(marker) {
            return Err(format!("{field}_contains_secret"));
        }
    }
    Ok(())
}

/// How a surface renders the provider output.  This is display metadata and is never a claim
/// that an upstream connection was contacted.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProviderDisplayMode {
    Native,
    Synthetic,
    Buffered,
}

/// The three explicit configuration/evidence states shown to users.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProviderConfigCheckState {
    Saved,
    StaticValidated,
    LiveVerified,
}

impl ProviderConfigCheckState {
    pub const fn is_live(self) -> bool {
        matches!(self, Self::LiveVerified)
    }
}

/// A bounded, actionable error.  Protocol bodies and credentials stay behind this boundary.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProviderDiagnosticError {
    pub code: String,
    pub action: String,
}

impl ProviderDiagnosticError {
    pub fn new(code: impl Into<String>, action: impl Into<String>) -> Result<Self, String> {
        let error = Self {
            code: code.into(),
            action: action.into(),
        };
        error.validate()?;
        Ok(error)
    }

    pub fn validate(&self) -> Result<(), String> {
        safe_text(&self.code, "provider_diagnostic_error_code", MAX_ERROR_TEXT)?;
        safe_text(
            &self.action,
            "provider_diagnostic_error_action",
            MAX_ACTION_TEXT,
        )
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub enum ProviderUsageDiagnostic {
    Known { usage: ModelUsage },
    Unknown { reason: String },
}

impl ProviderUsageDiagnostic {
    pub fn unknown(reason: impl Into<String>) -> Result<Self, String> {
        let value = Self::Unknown {
            reason: reason.into(),
        };
        value.validate()?;
        Ok(value)
    }

    pub fn validate(&self) -> Result<(), String> {
        match self {
            Self::Known { usage } => {
                if usage.input_tokens > 1_000_000_000 || usage.output_tokens > 1_000_000_000 {
                    return Err("provider_diagnostic_usage_limit".to_owned());
                }
            }
            Self::Unknown { reason } => safe_text(reason, "provider_diagnostic_usage_reason", 256)?,
        }
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProviderDiagnosticStatus {
    Ready,
    Queued,
    Retrying,
    Cancelling,
    Failed,
    UsageUnknown,
    Terminal,
}

impl ProviderDiagnosticStatus {
    pub const fn is_terminal(self) -> bool {
        matches!(self, Self::Failed | Self::UsageUnknown | Self::Terminal)
    }
}

/// A single connection/model row shared by CLI, Workbench, Web and Desktop projections.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProviderDiagnosticEntry {
    pub schema: String,
    pub version: SchemaVersion,
    pub provider_id: String,
    pub connection_id: String,
    pub profile: String,
    pub requested_model: String,
    pub reported_model: Option<String>,
    pub protocol: ModelProtocol,
    pub capabilities: ModelCapabilities,
    pub display_mode: ProviderDisplayMode,
    pub config_state: ProviderConfigCheckState,
    pub status: ProviderDiagnosticStatus,
    pub queue_position: Option<u32>,
    pub retry_at_unix_ms: Option<u64>,
    pub cancellation_requested: bool,
    pub error: Option<ProviderDiagnosticError>,
    pub usage: ProviderUsageDiagnostic,
    pub last_sequence: u64,
    pub terminal_event_digest: Option<String>,
}

impl ProviderDiagnosticEntry {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        provider_id: impl Into<String>,
        connection_id: impl Into<String>,
        profile: impl Into<String>,
        requested_model: impl Into<String>,
        protocol: ModelProtocol,
        capabilities: ModelCapabilities,
        display_mode: ProviderDisplayMode,
        config_state: ProviderConfigCheckState,
        status: ProviderDiagnosticStatus,
        usage: ProviderUsageDiagnostic,
    ) -> Result<Self, String> {
        let entry = Self {
            schema: PROVIDER_DIAGNOSTICS_SCHEMA.to_owned(),
            version: PROVIDER_DIAGNOSTICS_VERSION,
            provider_id: provider_id.into(),
            connection_id: connection_id.into(),
            profile: profile.into(),
            requested_model: requested_model.into(),
            reported_model: None,
            protocol,
            capabilities,
            display_mode,
            config_state,
            status,
            queue_position: None,
            retry_at_unix_ms: None,
            cancellation_requested: false,
            error: None,
            usage,
            last_sequence: 1,
            terminal_event_digest: None,
        };
        entry.validate()?;
        Ok(entry)
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.schema != PROVIDER_DIAGNOSTICS_SCHEMA
            || self.version != PROVIDER_DIAGNOSTICS_VERSION
            || self.last_sequence == 0
        {
            return Err("provider_diagnostic_entry_header_invalid".to_owned());
        }
        for (value, field, max) in [
            (&self.provider_id, "provider_diagnostic_provider_id", 128),
            (
                &self.connection_id,
                "provider_diagnostic_connection_id",
                128,
            ),
            (&self.profile, "provider_diagnostic_profile", 128),
            (
                &self.requested_model,
                "provider_diagnostic_requested_model",
                256,
            ),
        ] {
            required(value, field, max)?;
        }
        if let Some(model) = &self.reported_model {
            required(model, "provider_diagnostic_reported_model", 256)?;
        }
        self.usage.validate()?;
        if self.queue_position.is_some() != matches!(self.status, ProviderDiagnosticStatus::Queued)
        {
            return Err("provider_diagnostic_queue_status_mismatch".to_owned());
        }
        if self.retry_at_unix_ms.is_some()
            != matches!(self.status, ProviderDiagnosticStatus::Retrying)
        {
            return Err("provider_diagnostic_retry_status_mismatch".to_owned());
        }
        if self.queue_position.is_some_and(|position| position == 0) {
            return Err("provider_diagnostic_queue_position_invalid".to_owned());
        }
        if let Some(error) = &self.error {
            error.validate()?;
            if !self.status.is_terminal() {
                return Err("provider_diagnostic_error_requires_terminal".to_owned());
            }
        }
        if self.status == ProviderDiagnosticStatus::Failed && self.error.is_none() {
            return Err("provider_diagnostic_failure_error_required".to_owned());
        }
        if self.status == ProviderDiagnosticStatus::UsageUnknown
            && !matches!(self.usage, ProviderUsageDiagnostic::Unknown { .. })
        {
            return Err("provider_diagnostic_unknown_usage_required".to_owned());
        }
        if let Some(digest) = &self.terminal_event_digest {
            digest(digest, "provider_diagnostic_terminal_event_digest")?;
            if !self.status.is_terminal() {
                return Err("provider_diagnostic_terminal_digest_requires_terminal".to_owned());
            }
        }
        Ok(())
    }
}

/// A configuration validation result. `LiveVerified` is only an explicit evidence state; saving
/// or statically validating a configuration never contacts a provider.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProviderConfigCheck {
    pub schema: String,
    pub version: SchemaVersion,
    pub state: ProviderConfigCheckState,
    pub snapshot_digest: String,
    pub checked_at_unix_ms: u64,
    pub evidence_digest: Option<String>,
}

impl ProviderConfigCheck {
    pub fn new(
        state: ProviderConfigCheckState,
        snapshot_digest: impl Into<String>,
        checked_at_unix_ms: u64,
        evidence_digest: Option<String>,
    ) -> Result<Self, String> {
        let value = Self {
            schema: PROVIDER_CONFIG_CHECK_SCHEMA.to_owned(),
            version: PROVIDER_DIAGNOSTICS_VERSION,
            state,
            snapshot_digest: snapshot_digest.into(),
            checked_at_unix_ms,
            evidence_digest,
        };
        value.validate()?;
        Ok(value)
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.schema != PROVIDER_CONFIG_CHECK_SCHEMA
            || self.version != PROVIDER_DIAGNOSTICS_VERSION
            || self.checked_at_unix_ms == 0
        {
            return Err("provider_config_check_header_invalid".to_owned());
        }
        digest(&self.snapshot_digest, "provider_config_snapshot_digest")?;
        if self.state.is_live() != self.evidence_digest.is_some() {
            return Err("provider_config_check_evidence_state_mismatch".to_owned());
        }
        if let Some(value) = &self.evidence_digest {
            digest(value, "provider_config_evidence_digest")?;
        }
        Ok(())
    }
}

/// Explicit connection-test admission. Merely opening a settings/catalog view cannot create this
/// value; a caller must obtain the same gateway admission reference used by model execution.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProviderConnectionTestRequest {
    pub schema: String,
    pub version: SchemaVersion,
    pub profile: String,
    pub model_id: String,
    pub catalog_digest: String,
    pub config_digest: String,
    pub gateway_admission_digest: String,
    pub explicit: bool,
    pub allow_inference: bool,
}

impl ProviderConnectionTestRequest {
    pub fn validate(&self) -> Result<(), String> {
        if self.schema != PROVIDER_CONNECTION_TEST_SCHEMA
            || self.version != PROVIDER_DIAGNOSTICS_VERSION
            || !self.explicit
            || !self.allow_inference
        {
            return Err(PROVIDER_DIAGNOSTICS_CONNECTION_TEST_EXPLICIT.to_owned());
        }
        required(&self.profile, "provider_connection_test_profile", 128)?;
        required(&self.model_id, "provider_connection_test_model", 256)?;
        digest(
            &self.catalog_digest,
            "provider_connection_test_catalog_digest",
        )?;
        digest(
            &self.config_digest,
            "provider_connection_test_config_digest",
        )?;
        digest(
            &self.gateway_admission_digest,
            "provider_connection_test_gateway_admission_digest",
        )
    }
}

/// Cursor for the provider diagnostic projection. It is scoped by both authority and config
/// epochs; an old cursor can request a new snapshot but cannot replay or mutate an active run.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProviderDiagnosticsCursor {
    pub schema: String,
    pub version: SchemaVersion,
    pub authority_epoch: u64,
    pub config_epoch: u64,
    pub sequence: u64,
    pub cursor_digest: String,
}

impl ProviderDiagnosticsCursor {
    pub fn new(authority_epoch: u64, config_epoch: u64, sequence: u64) -> Result<Self, String> {
        let mut cursor = Self {
            schema: PROVIDER_DIAGNOSTICS_CURSOR_SCHEMA.to_owned(),
            version: PROVIDER_DIAGNOSTICS_VERSION,
            authority_epoch,
            config_epoch,
            sequence,
            cursor_digest: String::new(),
        };
        cursor.cursor_digest = cursor.digest();
        cursor.validate()?;
        Ok(cursor)
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.schema != PROVIDER_DIAGNOSTICS_CURSOR_SCHEMA
            || self.version != PROVIDER_DIAGNOSTICS_VERSION
            || self.authority_epoch == 0
            || self.config_epoch == 0
        {
            return Err("provider_diagnostics_cursor_header_invalid".to_owned());
        }
        digest(&self.cursor_digest, "provider_diagnostics_cursor_digest")?;
        if self.cursor_digest != self.digest() {
            return Err("provider_diagnostics_cursor_digest_mismatch".to_owned());
        }
        Ok(())
    }

    pub fn digest(&self) -> String {
        json_digest(&json!({
            "schema": self.schema,
            "version": self.version,
            "authority_epoch": self.authority_epoch,
            "config_epoch": self.config_epoch,
            "sequence": self.sequence,
        }))
    }

    pub fn is_current(&self, authority_epoch: u64, config_epoch: u64) -> bool {
        self.authority_epoch == authority_epoch && self.config_epoch == config_epoch
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProviderTerminalReplay {
    pub schema: String,
    pub version: SchemaVersion,
    pub sequence: u64,
    pub event_digest: String,
    pub receipt_digest: String,
    pub outcome: String,
}

impl ProviderTerminalReplay {
    pub fn validate(&self) -> Result<(), String> {
        if self.schema != PROVIDER_TERMINAL_REPLAY_SCHEMA
            || self.version != PROVIDER_DIAGNOSTICS_VERSION
            || self.sequence == 0
        {
            return Err("provider_terminal_replay_header_invalid".to_owned());
        }
        digest(&self.event_digest, "provider_terminal_replay_event_digest")?;
        digest(
            &self.receipt_digest,
            "provider_terminal_replay_receipt_digest",
        )?;
        safe_text(&self.outcome, "provider_terminal_replay_outcome", 128)
    }
}

/// Secret-free, replayable provider diagnostics shared by all four surfaces.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProviderDiagnosticsSnapshot {
    pub schema: String,
    pub version: SchemaVersion,
    pub authority_epoch: u64,
    pub config_epoch: u64,
    pub configuration: ProviderConfigSnapshot,
    pub catalog: ModelCatalog,
    pub config_check: ProviderConfigCheck,
    pub entries: Vec<ProviderDiagnosticEntry>,
    pub cursor: ProviderDiagnosticsCursor,
    pub terminal: Option<ProviderTerminalReplay>,
    pub snapshot_digest: String,
}

impl ProviderDiagnosticsSnapshot {
    pub fn new(
        authority_epoch: u64,
        config_epoch: u64,
        configuration: ProviderConfigSnapshot,
        catalog: ModelCatalog,
        config_check: ProviderConfigCheck,
        entries: Vec<ProviderDiagnosticEntry>,
        terminal: Option<ProviderTerminalReplay>,
    ) -> Result<Self, String> {
        let cursor = ProviderDiagnosticsCursor::new(authority_epoch, config_epoch, 1)?;
        let mut snapshot = Self {
            schema: PROVIDER_DIAGNOSTICS_SNAPSHOT_SCHEMA.to_owned(),
            version: PROVIDER_DIAGNOSTICS_VERSION,
            authority_epoch,
            config_epoch,
            configuration,
            catalog,
            config_check,
            entries,
            cursor,
            terminal,
            snapshot_digest: String::new(),
        };
        snapshot.snapshot_digest = snapshot.digest();
        snapshot.validate()?;
        Ok(snapshot)
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.schema != PROVIDER_DIAGNOSTICS_SNAPSHOT_SCHEMA
            || self.version != PROVIDER_DIAGNOSTICS_VERSION
            || self.authority_epoch == 0
            || self.config_epoch == 0
            || self.entries.len() > MAX_ENTRIES
            || !self
                .cursor
                .is_current(self.authority_epoch, self.config_epoch)
        {
            return Err("provider_diagnostics_snapshot_header_invalid".to_owned());
        }
        self.configuration.validate()?;
        self.catalog.validate(None)?;
        self.config_check.validate()?;
        if self.config_check.snapshot_digest != self.configuration.snapshot_digest {
            return Err("provider_diagnostics_config_snapshot_mismatch".to_owned());
        }
        let mut identities = std::collections::BTreeSet::new();
        for entry in &self.entries {
            entry.validate()?;
            if !identities.insert((
                entry.provider_id.clone(),
                entry.connection_id.clone(),
                entry.requested_model.clone(),
            )) {
                return Err("provider_diagnostics_duplicate_entry".to_owned());
            }
        }
        if let Some(terminal) = &self.terminal {
            terminal.validate()?;
            if terminal.sequence > self.cursor.sequence {
                return Err("provider_diagnostics_terminal_sequence_invalid".to_owned());
            }
        }
        self.cursor.validate()?;
        digest(
            &self.snapshot_digest,
            "provider_diagnostics_snapshot_digest",
        )?;
        if self.snapshot_digest != self.digest() {
            return Err("provider_diagnostics_snapshot_digest_mismatch".to_owned());
        }
        Ok(())
    }

    pub fn digest(&self) -> String {
        json_digest(&json!({
            "schema": self.schema,
            "version": self.version,
            "authority_epoch": self.authority_epoch,
            "config_epoch": self.config_epoch,
            "configuration": self.configuration,
            "catalog": self.catalog,
            "config_check": self.config_check,
            "entries": self.entries,
            "cursor": self.cursor,
            "terminal": self.terminal,
        }))
    }

    /// Reject old authority/config projections. Callers must hydrate a fresh snapshot; they may
    /// never use a stale view to issue an action or retry a model request.
    pub fn validate_for_epoch(
        &self,
        authority_epoch: u64,
        config_epoch: u64,
    ) -> Result<(), String> {
        self.validate()?;
        if self.authority_epoch != authority_epoch || self.config_epoch != config_epoch {
            return Err(PROVIDER_DIAGNOSTICS_PROJECTION_STALE.to_owned());
        }
        Ok(())
    }

    /// Return the terminal receipt only when it is newer than a subscriber's cursor. No model
    /// request is made here; the terminal fact is replayed from the existing durable projection.
    pub fn terminal_after(&self, sequence: u64) -> Result<Option<&ProviderTerminalReplay>, String> {
        self.validate()?;
        Ok(self
            .terminal
            .as_ref()
            .filter(|terminal| terminal.sequence > sequence))
    }

    pub fn settings_read_only_marker() -> &'static str {
        PROVIDER_DIAGNOSTICS_SETTINGS_NO_INFERENCE
    }
}

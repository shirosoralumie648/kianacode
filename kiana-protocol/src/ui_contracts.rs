//! Versioned UI snapshot/feed/action contracts.
//!
//! These DTOs are projections and intents at the protocol boundary.  They carry no broker
//! authority; every action must be re-authorized by ControlPlane with the supplied CAS/digest.

use kiana_domain::{
    json_digest, validate_json_limits, ArtifactId, AuthenticatedPrincipalRef, ExecutionStatus,
    ProjectId, ReceiptId, RunId, SessionId,
};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::BTreeSet;

pub const UI_SNAPSHOT_SCHEMA: &str = "kiana.ui-snapshot.v1";
pub const UI_FEED_SCHEMA: &str = "kiana.ui-feed.v1";
pub const UI_ACTION_SCHEMA: &str = "kiana.ui-action.v1";
pub const UI_ACTION_RESULT_SCHEMA: &str = "kiana.ui-action-result.v1";
pub const UI_CAPABILITY_SCHEMA: &str = "kiana.ui-capability.v1";
pub const UI_ERROR_SCHEMA: &str = "kiana.ui-error.v1";

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

fn cursor_valid(epoch: &str, sequence: u64) -> Result<(), String> {
    if epoch.trim().is_empty() || epoch.len() > 256 || sequence == 0 {
        return Err("ui_cursor_invalid".to_owned());
    }
    Ok(())
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum UiSeverity {
    Info,
    Warning,
    Error,
    Critical,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum UiActionDisposition {
    Accepted,
    Applied,
    Rejected,
    Unknown,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum UiRetryDisposition {
    QueryOriginal,
    SafeRetry,
    DoNotRetry,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum UiErrorCode {
    InvalidRequest,
    SchemaUnsupported,
    PermissionDenied,
    ApprovalRequired,
    Conflict,
    Capacity,
    Unavailable,
    Cancelled,
    Failed,
    Unknown,
    Persistence,
}

impl UiErrorCode {
    pub const ALL: [Self; 11] = [
        Self::InvalidRequest,
        Self::SchemaUnsupported,
        Self::PermissionDenied,
        Self::ApprovalRequired,
        Self::Conflict,
        Self::Capacity,
        Self::Unavailable,
        Self::Cancelled,
        Self::Failed,
        Self::Unknown,
        Self::Persistence,
    ];
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct UiError {
    pub schema: String,
    pub code: UiErrorCode,
    pub message: String,
    pub retry: UiRetryDisposition,
}

impl UiError {
    pub fn new(code: UiErrorCode, message: impl Into<String>, retry: UiRetryDisposition) -> Self {
        Self {
            schema: UI_ERROR_SCHEMA.to_owned(),
            code,
            message: message.into(),
            retry,
        }
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.schema != UI_ERROR_SCHEMA {
            return Err("ui_error_schema_invalid".to_owned());
        }
        required(&self.message, "ui_error_message", 2_048)
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct UiCapability {
    pub schema: String,
    pub capability_id: String,
    pub enabled: bool,
    #[serde(default)]
    pub actions: Vec<String>,
    #[serde(default)]
    pub reason: Option<String>,
    pub scope_digest: String,
}

impl UiCapability {
    pub fn validate(&self) -> Result<(), String> {
        if self.schema != UI_CAPABILITY_SCHEMA {
            return Err("ui_capability_schema_invalid".to_owned());
        }
        required(&self.capability_id, "ui_capability_id", 128)?;
        digest(&self.scope_digest, "ui_capability_scope_digest")?;
        if self.actions.len() > 64
            || self.actions.iter().any(|action| {
                action.trim().is_empty() || action.len() > 128 || action.contains('\0')
            })
        {
            return Err("ui_capability_actions_invalid".to_owned());
        }
        if self
            .reason
            .as_deref()
            .is_some_and(|reason| reason.trim().is_empty() || reason.len() > 1_024)
        {
            return Err("ui_capability_reason_invalid".to_owned());
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SessionSummary {
    pub session_id: SessionId,
    pub title: String,
    pub status: String,
    pub revision: u64,
}

impl SessionSummary {
    fn validate(&self) -> Result<(), String> {
        required(self.session_id.as_str(), "ui_session_id", 256)?;
        required(&self.title, "ui_session_title", 512)?;
        required(&self.status, "ui_session_status", 64)?;
        if self.revision == 0 {
            return Err("ui_session_revision_invalid".to_owned());
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ReceiptRef {
    pub receipt_id: ReceiptId,
    pub receipt_digest: String,
}

impl ReceiptRef {
    fn validate(&self) -> Result<(), String> {
        digest(&self.receipt_digest, "ui_receipt_digest")
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ArtifactSummary {
    pub artifact_id: ArtifactId,
    pub digest: String,
    pub mime: String,
    pub size_bytes: u64,
}

impl ArtifactSummary {
    fn validate(&self) -> Result<(), String> {
        digest(&self.digest, "ui_artifact_digest")?;
        required(&self.mime, "ui_artifact_mime", 128)
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RunSummary {
    pub run_id: RunId,
    pub status: ExecutionStatus,
    pub revision: u64,
    #[serde(default)]
    pub final_assistant_text: Option<String>,
    #[serde(default)]
    pub receipt: Option<ReceiptRef>,
}

impl RunSummary {
    fn validate(&self) -> Result<(), String> {
        if self.revision == 0 {
            return Err("ui_run_revision_invalid".to_owned());
        }
        if self
            .final_assistant_text
            .as_deref()
            .is_some_and(|text| text.len() > 64 * 1024 || text.contains('\0'))
        {
            return Err("ui_run_text_invalid".to_owned());
        }
        if let Some(receipt) = &self.receipt {
            receipt.validate()?;
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HumanActionCard {
    pub action_id: String,
    pub command: String,
    pub target_id: String,
    pub expected_revision: Option<u64>,
    pub expires_at_unix_ms: Option<u64>,
    pub allowed_decisions: Vec<String>,
    pub payload_digest: String,
}

impl HumanActionCard {
    fn validate(&self) -> Result<(), String> {
        required(&self.action_id, "ui_action_id", 256)?;
        required(&self.command, "ui_action_command", 128)?;
        required(&self.target_id, "ui_action_target", 256)?;
        digest(&self.payload_digest, "ui_action_payload_digest")?;
        if self.expected_revision == Some(0) || self.expires_at_unix_ms == Some(0) {
            return Err("ui_action_card_revision_invalid".to_owned());
        }
        if self.allowed_decisions.len() > 16
            || self
                .allowed_decisions
                .iter()
                .any(|decision| decision.trim().is_empty() || decision.len() > 64)
        {
            return Err("ui_action_decisions_invalid".to_owned());
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct UiNotice {
    pub notice_id: String,
    pub severity: UiSeverity,
    pub title: String,
    pub summary: String,
    #[serde(default)]
    pub action_refs: Vec<String>,
}

impl UiNotice {
    fn validate(&self) -> Result<(), String> {
        required(&self.notice_id, "ui_notice_id", 256)?;
        required(&self.title, "ui_notice_title", 512)?;
        required(&self.summary, "ui_notice_summary", 4_096)?;
        if self.action_refs.len() > 32
            || self
                .action_refs
                .iter()
                .any(|reference| reference.trim().is_empty() || reference.len() > 256)
        {
            return Err("ui_notice_actions_invalid".to_owned());
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EvidenceLimitation {
    pub code: String,
    pub detail: String,
}

impl EvidenceLimitation {
    fn validate(&self) -> Result<(), String> {
        required(&self.code, "ui_limitation_code", 128)?;
        required(&self.detail, "ui_limitation_detail", 1_024)
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct UiSnapshotV1 {
    pub schema: String,
    pub instance_id: String,
    pub authority_epoch: u64,
    pub snapshot_cursor: UiCursorV1,
    pub generated_at_unix_ms: u64,
    pub principal: AuthenticatedPrincipalRef,
    pub workspace_id: ProjectId,
    #[serde(default)]
    pub capabilities: Vec<UiCapability>,
    pub sessions: Vec<SessionSummary>,
    #[serde(default)]
    pub active_session_id: Option<SessionId>,
    #[serde(default)]
    pub runs: Vec<RunSummary>,
    #[serde(default)]
    pub pending_actions: Vec<HumanActionCard>,
    #[serde(default)]
    pub artifacts: Vec<ArtifactSummary>,
    #[serde(default)]
    pub notices: Vec<UiNotice>,
    #[serde(default)]
    pub next_page: Option<String>,
    #[serde(default)]
    pub limitations: Vec<EvidenceLimitation>,
}

impl UiSnapshotV1 {
    pub fn validate(&self) -> Result<(), String> {
        if self.schema != UI_SNAPSHOT_SCHEMA
            || self.instance_id.trim().is_empty()
            || self.instance_id.len() > 256
            || self.authority_epoch == 0
            || self.generated_at_unix_ms == 0
        {
            return Err("ui_snapshot_header_invalid".to_owned());
        }
        self.snapshot_cursor.validate()?;
        self.principal.validate()?;
        if self.capabilities.len() > 128
            || self.sessions.len() > 256
            || self.runs.len() > 512
            || self.pending_actions.len() > 256
            || self.artifacts.len() > 512
            || self.notices.len() > 512
            || self.limitations.len() > 128
        {
            return Err("ui_snapshot_limit".to_owned());
        }
        let mut sessions = BTreeSet::new();
        for session in &self.sessions {
            session.validate()?;
            if !sessions.insert(session.session_id.to_string()) {
                return Err("ui_snapshot_session_duplicate".to_owned());
            }
        }
        if let Some(active) = &self.active_session_id {
            if !sessions.contains(active.as_str()) {
                return Err("ui_snapshot_active_session_missing".to_owned());
            }
        }
        let mut runs = BTreeSet::new();
        for run in &self.runs {
            run.validate()?;
            if !runs.insert(run.run_id.to_string()) {
                return Err("ui_snapshot_run_duplicate".to_owned());
            }
        }
        let mut actions = BTreeSet::new();
        for action in &self.pending_actions {
            action.validate()?;
            if !actions.insert(action.action_id.clone()) {
                return Err("ui_snapshot_action_duplicate".to_owned());
            }
        }
        for capability in &self.capabilities {
            capability.validate()?;
        }
        for artifact in &self.artifacts {
            artifact.validate()?;
        }
        for notice in &self.notices {
            notice.validate()?;
        }
        for limitation in &self.limitations {
            limitation.validate()?;
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct UiCursorV1 {
    pub epoch: String,
    pub sequence: u64,
}

impl UiCursorV1 {
    pub fn validate(&self) -> Result<(), String> {
        cursor_valid(&self.epoch, self.sequence)
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct UiFeedEnvelope {
    pub schema: String,
    pub instance_id: String,
    pub authority_epoch: u64,
    pub feed_sequence: u64,
    pub snapshot_cursor: UiCursorV1,
    pub event_id: String,
    #[serde(default)]
    pub aggregate_type: Option<String>,
    #[serde(default)]
    pub aggregate_id: Option<String>,
    pub object_revision: u64,
    pub event: Value,
}

impl UiFeedEnvelope {
    pub fn validate(&self) -> Result<(), String> {
        if self.schema != UI_FEED_SCHEMA
            || self.instance_id.trim().is_empty()
            || self.instance_id.len() > 256
            || self.authority_epoch == 0
            || self.feed_sequence == 0
            || self.object_revision == 0
            || self.event_id.trim().is_empty()
            || self.event_id.len() > 256
        {
            return Err("ui_feed_header_invalid".to_owned());
        }
        self.snapshot_cursor.validate()?;
        if self
            .aggregate_type
            .as_deref()
            .is_some_and(|value| value.trim().is_empty() || value.len() > 128)
            || self
                .aggregate_id
                .as_deref()
                .is_some_and(|value| value.trim().is_empty() || value.len() > 256)
        {
            return Err("ui_feed_aggregate_invalid".to_owned());
        }
        validate_json_limits(&self.event).map_err(|_| "ui_feed_event_invalid".to_owned())
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct UiActionV1 {
    pub schema: String,
    pub command_id: kiana_domain::RequestId,
    pub idempotency_key: String,
    pub target_id: String,
    pub expected_epoch: String,
    pub expected_cursor: u64,
    #[serde(default)]
    pub expected_revision: Option<u64>,
    pub payload: Value,
    pub payload_digest: String,
    pub submitted_by: String,
    #[serde(default)]
    pub deadline_unix_ms: Option<u64>,
}

impl UiActionV1 {
    pub fn validate(&self) -> Result<(), String> {
        if self.schema != UI_ACTION_SCHEMA {
            return Err("ui_action_schema_invalid".to_owned());
        }
        required(&self.idempotency_key, "ui_action_idempotency_key", 256)?;
        required(&self.target_id, "ui_action_target", 256)?;
        required(&self.submitted_by, "ui_action_submitted_by", 256)?;
        cursor_valid(&self.expected_epoch, self.expected_cursor)?;
        if self.expected_revision == Some(0) || self.deadline_unix_ms == Some(0) {
            return Err("ui_action_revision_invalid".to_owned());
        }
        validate_json_limits(&self.payload).map_err(|_| "ui_action_payload_invalid".to_owned())?;
        digest(&self.payload_digest, "ui_action_payload_digest")?;
        if self.payload_digest != json_digest(&self.payload) {
            return Err("ui_action_payload_digest_mismatch".to_owned());
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct UiActionResult {
    pub schema: String,
    pub command_id: kiana_domain::RequestId,
    pub disposition: UiActionDisposition,
    #[serde(default)]
    pub resulting_cursor: Option<UiCursorV1>,
    #[serde(default)]
    pub resulting_revision: Option<u64>,
    #[serde(default)]
    pub receipt: Option<ReceiptRef>,
    #[serde(default)]
    pub error: Option<UiError>,
    pub retry: UiRetryDisposition,
}

impl UiActionResult {
    pub fn validate(&self) -> Result<(), String> {
        if self.schema != UI_ACTION_RESULT_SCHEMA {
            return Err("ui_action_result_schema_invalid".to_owned());
        }
        if self.resulting_revision == Some(0) {
            return Err("ui_action_result_revision_invalid".to_owned());
        }
        if let Some(cursor) = &self.resulting_cursor {
            cursor.validate()?;
        }
        if let Some(receipt) = &self.receipt {
            receipt.validate()?;
        }
        if let Some(error) = &self.error {
            error.validate()?;
        }
        match self.disposition {
            UiActionDisposition::Applied
                if self.resulting_cursor.is_none() && self.receipt.is_none() =>
            {
                Err("ui_action_applied_evidence_missing".to_owned())
            }
            UiActionDisposition::Rejected if self.error.is_none() => {
                Err("ui_action_rejected_error_missing".to_owned())
            }
            UiActionDisposition::Unknown if self.retry != UiRetryDisposition::QueryOriginal => {
                Err("ui_action_unknown_retry_invalid".to_owned())
            }
            _ => Ok(()),
        }
    }
}

// Names used by surface adapters while the legacy UiSnapshot/UiAction remain available.
pub type VersionedUiSnapshot = UiSnapshotV1;
pub type VersionedUiAction = UiActionV1;

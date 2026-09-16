//! Versioned UI snapshot/feed/action contracts.
//!
//! These DTOs are projections and intents at the protocol boundary.  They carry no broker
//! authority; every action must be re-authorized by ControlPlane with the supplied CAS/digest.

use kiana_domain::{
    json_digest, validate_json_limits, ArtifactId, AuthenticatedPrincipalRef, ExecutionStatus,
    ProjectId, ReceiptId, RunId, SessionId,
};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::BTreeSet;

pub const UI_SNAPSHOT_SCHEMA: &str = "kiana.ui-snapshot.v1";
pub const UI_FEED_SCHEMA: &str = "kiana.ui-feed.v1";
pub const UI_ACTION_SCHEMA: &str = "kiana.ui-action.v1";
pub const UI_ACTION_RESULT_SCHEMA: &str = "kiana.ui-action-result.v1";
pub const UI_CAPABILITY_SCHEMA: &str = "kiana.ui-capability.v1";
pub const UI_ERROR_SCHEMA: &str = "kiana.ui-error.v1";
pub const UI_HANDSHAKE_REQUEST_SCHEMA: &str = "kiana.ui-handshake-request.v1";
pub const UI_HANDSHAKE_RESPONSE_SCHEMA: &str = "kiana.ui-handshake-response.v1";
pub const UI_HEALTH_SCHEMA: &str = "kiana.ui-health.v1";
pub const UI_INSTANCE_RECORD_SCHEMA: &str = "kiana.ui-instance-record.v1";

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

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum UiTransportKind {
    InProcess,
    UnixSocket,
    NamedPipe,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct UiInstanceRecord {
    pub schema: String,
    pub instance_id: String,
    pub authority_epoch: u64,
    pub protocol_schema: String,
    pub workspace_digest: String,
    pub transport: UiTransportKind,
    pub endpoint_digest: String,
    pub pid: u32,
    pub ready: bool,
    pub record_digest: String,
}

impl UiInstanceRecord {
    pub fn new(
        instance_id: impl Into<String>,
        authority_epoch: u64,
        workspace: &str,
        transport: UiTransportKind,
        endpoint: &str,
        pid: u32,
        ready: bool,
    ) -> Result<Self, String> {
        if workspace.trim().is_empty() || endpoint.trim().is_empty() {
            return Err("ui_instance_endpoint_required".to_owned());
        }
        let mut record = Self {
            schema: UI_INSTANCE_RECORD_SCHEMA.to_owned(),
            instance_id: instance_id.into(),
            authority_epoch,
            protocol_schema: super::PROTOCOL_SCHEMA.to_owned(),
            workspace_digest: json_digest(&json!({"workspace": workspace})),
            transport,
            endpoint_digest: json_digest(&json!({"endpoint": endpoint})),
            pid,
            ready,
            record_digest: String::new(),
        };
        record.record_digest = record.digest();
        record.validate()?;
        Ok(record)
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.schema != UI_INSTANCE_RECORD_SCHEMA
            || self.protocol_schema != super::PROTOCOL_SCHEMA
            || self.authority_epoch == 0
            || self.pid == 0
            || !self.ready
            || self.instance_id.trim().is_empty()
            || self.instance_id.len() > 256
            || self.instance_id.contains('/')
            || self.instance_id.contains('\\')
            || self.instance_id.contains('\0')
        {
            return Err("ui_instance_record_header_invalid".to_owned());
        }
        digest(&self.workspace_digest, "ui_instance_workspace_digest")?;
        digest(&self.endpoint_digest, "ui_instance_endpoint_digest")?;
        digest(&self.record_digest, "ui_instance_record_digest")?;
        if self.record_digest != self.digest() {
            return Err("ui_instance_record_digest_mismatch".to_owned());
        }
        Ok(())
    }

    pub fn validate_peer(
        &self,
        workspace: &str,
        protocol_schema: &str,
        expected_epoch: Option<u64>,
    ) -> Result<(), String> {
        self.validate()?;
        if self.protocol_schema != protocol_schema
            || self.workspace_digest != json_digest(&json!({"workspace": workspace}))
            || expected_epoch.is_some_and(|epoch| epoch != self.authority_epoch)
        {
            return Err("ui_instance_peer_mismatch".to_owned());
        }
        Ok(())
    }

    pub fn digest(&self) -> String {
        let mut value = serde_json::to_value(self).unwrap_or_default();
        if let Some(object) = value.as_object_mut() {
            object.insert("record_digest".to_owned(), Value::String(String::new()));
        }
        json_digest(&value)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum UiSurface {
    Cli,
    Workbench,
    Web,
    Desktop,
    Ide,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct UiCapabilityRequest {
    pub capability_id: String,
    pub feature_version: String,
}

impl UiCapabilityRequest {
    fn validate(&self) -> Result<(), String> {
        required(&self.capability_id, "ui_requested_capability", 128)?;
        required(&self.feature_version, "ui_feature_version", 64)
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct UiHandshakeRequest {
    pub schema: String,
    pub client_version: String,
    pub surface: UiSurface,
    #[serde(default)]
    pub requested_capabilities: Vec<UiCapabilityRequest>,
    #[serde(default)]
    pub known_instance_id: Option<String>,
    #[serde(default)]
    pub known_authority_epoch: Option<u64>,
}

impl UiHandshakeRequest {
    pub fn validate(&self) -> Result<(), String> {
        if self.schema != UI_HANDSHAKE_REQUEST_SCHEMA {
            return Err("ui_handshake_request_schema_invalid".to_owned());
        }
        required(&self.client_version, "ui_client_version", 64)?;
        if self.requested_capabilities.len() > 128 {
            return Err("ui_handshake_capability_limit".to_owned());
        }
        let mut capabilities = BTreeSet::new();
        for capability in &self.requested_capabilities {
            capability.validate()?;
            if !capabilities.insert(capability.capability_id.clone()) {
                return Err("ui_handshake_capability_duplicate".to_owned());
            }
        }
        if self.known_authority_epoch == Some(0)
            || self
                .known_instance_id
                .as_deref()
                .is_some_and(|value| value.trim().is_empty() || value.len() > 256)
        {
            return Err("ui_handshake_known_instance_invalid".to_owned());
        }
        Ok(())
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct UiHandshakeResponse {
    pub schema: String,
    pub server_version: String,
    pub instance_id: String,
    pub authority_epoch: u64,
    pub surface: UiSurface,
    pub capabilities: Vec<UiCapability>,
    #[serde(default)]
    pub limitations: Vec<EvidenceLimitation>,
}

impl UiHandshakeResponse {
    pub fn validate(&self) -> Result<(), String> {
        if self.schema != UI_HANDSHAKE_RESPONSE_SCHEMA || self.authority_epoch == 0 {
            return Err("ui_handshake_response_header_invalid".to_owned());
        }
        required(&self.server_version, "ui_server_version", 64)?;
        required(&self.instance_id, "ui_instance_id", 256)?;
        if self.capabilities.len() > 128 || self.limitations.len() > 128 {
            return Err("ui_handshake_response_limit".to_owned());
        }
        let mut ids = BTreeSet::new();
        for capability in &self.capabilities {
            capability.validate()?;
            if !ids.insert(capability.capability_id.clone()) {
                return Err("ui_handshake_response_capability_duplicate".to_owned());
            }
        }
        for limitation in &self.limitations {
            limitation.validate()?;
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct UiHealth {
    pub schema: String,
    pub instance_id: String,
    pub authority_epoch: u64,
    pub status: String,
    pub capabilities: Vec<String>,
    #[serde(default)]
    pub limitations: Vec<EvidenceLimitation>,
}

impl UiHealth {
    pub fn validate(&self) -> Result<(), String> {
        if self.schema != UI_HEALTH_SCHEMA || self.authority_epoch == 0 {
            return Err("ui_health_header_invalid".to_owned());
        }
        required(&self.instance_id, "ui_health_instance", 256)?;
        required(&self.status, "ui_health_status", 64)?;
        if self.capabilities.len() > 128 || self.limitations.len() > 128 {
            return Err("ui_health_limit".to_owned());
        }
        if self
            .capabilities
            .iter()
            .any(|capability| capability.trim().is_empty() || capability.len() > 128)
        {
            return Err("ui_health_capability_invalid".to_owned());
        }
        for limitation in &self.limitations {
            limitation.validate()?;
        }
        Ok(())
    }
}

/// Intersect server-derived principal and surface capability sets. Client requests can only
/// narrow the result; a scope digest mismatch disables the capability rather than widening it.
pub fn intersect_ui_capabilities(
    principal: &[UiCapability],
    surface: &[UiCapability],
    requested: &[UiCapabilityRequest],
) -> Result<Vec<UiCapability>, String> {
    let mut requested_ids = BTreeSet::new();
    for request in requested {
        request.validate()?;
        if !requested_ids.insert(request.capability_id.clone()) {
            return Err("ui_handshake_capability_duplicate".to_owned());
        }
    }
    let principal = principal
        .iter()
        .map(|capability| (capability.capability_id.clone(), capability))
        .collect::<std::collections::BTreeMap<_, _>>();
    let surface = surface
        .iter()
        .map(|capability| (capability.capability_id.clone(), capability))
        .collect::<std::collections::BTreeMap<_, _>>();
    let mut result = Vec::new();
    for request in requested {
        let Some(left) = principal.get(&request.capability_id) else {
            result.push(UiCapability {
                schema: UI_CAPABILITY_SCHEMA.to_owned(),
                capability_id: request.capability_id.clone(),
                enabled: false,
                actions: Vec::new(),
                reason: Some("principal_capability_missing".to_owned()),
                scope_digest: kiana_domain::json_digest(
                    &json!({"capability": request.capability_id}),
                ),
            });
            continue;
        };
        let Some(right) = surface.get(&request.capability_id) else {
            result.push(UiCapability {
                schema: UI_CAPABILITY_SCHEMA.to_owned(),
                capability_id: request.capability_id.clone(),
                enabled: false,
                actions: Vec::new(),
                reason: Some("surface_capability_missing".to_owned()),
                scope_digest: left.scope_digest.clone(),
            });
            continue;
        };
        let enabled = left.enabled && right.enabled && left.scope_digest == right.scope_digest;
        let mut actions = left
            .actions
            .iter()
            .filter(|action| right.actions.contains(action))
            .cloned()
            .collect::<Vec<_>>();
        actions.sort();
        result.push(UiCapability {
            schema: UI_CAPABILITY_SCHEMA.to_owned(),
            capability_id: request.capability_id.clone(),
            enabled,
            actions,
            reason: (!enabled).then(|| {
                if left.scope_digest != right.scope_digest {
                    "ui_scope_intersection_empty".to_owned()
                } else {
                    "capability_disabled_by_intersection".to_owned()
                }
            }),
            scope_digest: left.scope_digest.clone(),
        });
    }
    Ok(result)
}

pub type StableError = UiError;

pub fn stable_error_from_response(response: &super::ResponseEnvelope) -> Option<UiError> {
    if response.status == kiana_domain::ExecutionStatus::Completed && response.error.is_none() {
        return None;
    }
    let code = response
        .failure_code()
        .map(|code| match code {
            kiana_domain::CapabilityErrorCode::InvalidArguments => UiErrorCode::InvalidRequest,
            kiana_domain::CapabilityErrorCode::SchemaUnsupported => UiErrorCode::SchemaUnsupported,
            kiana_domain::CapabilityErrorCode::PermissionDenied
            | kiana_domain::CapabilityErrorCode::ProjectUntrusted => UiErrorCode::PermissionDenied,
            kiana_domain::CapabilityErrorCode::ApprovalRequired => UiErrorCode::ApprovalRequired,
            kiana_domain::CapabilityErrorCode::Conflict => UiErrorCode::Conflict,
            kiana_domain::CapabilityErrorCode::BudgetExceeded => UiErrorCode::Capacity,
            kiana_domain::CapabilityErrorCode::Unavailable => UiErrorCode::Unavailable,
            kiana_domain::CapabilityErrorCode::Cancelled => UiErrorCode::Cancelled,
            kiana_domain::CapabilityErrorCode::ResultUnknown => UiErrorCode::Unknown,
            kiana_domain::CapabilityErrorCode::ExecutionFailed
            | kiana_domain::CapabilityErrorCode::CompensationRequired
            | kiana_domain::CapabilityErrorCode::TimedOut => UiErrorCode::Failed,
            _ => UiErrorCode::Failed,
        })
        .unwrap_or(UiErrorCode::Failed);
    let retry = match code {
        UiErrorCode::Unknown => UiRetryDisposition::QueryOriginal,
        UiErrorCode::Unavailable | UiErrorCode::Capacity => UiRetryDisposition::DoNotRetry,
        _ => UiRetryDisposition::DoNotRetry,
    };
    Some(UiError::new(code, ui_error_message(code), retry))
}

fn ui_error_message(code: UiErrorCode) -> &'static str {
    match code {
        UiErrorCode::InvalidRequest => "Request is invalid",
        UiErrorCode::SchemaUnsupported => "Protocol version is unsupported",
        UiErrorCode::PermissionDenied => "Permission denied",
        UiErrorCode::ApprovalRequired => "Approval is required",
        UiErrorCode::Conflict => "State conflict; refresh and try again",
        UiErrorCode::Capacity => "Capacity limit reached",
        UiErrorCode::Unavailable => "Service is unavailable",
        UiErrorCode::Cancelled => "Request cancelled",
        UiErrorCode::Failed => "Request failed",
        UiErrorCode::Unknown => "Request outcome is unknown; query the original command",
        UiErrorCode::Persistence => "Persistence failed",
    }
}

//! Versioned UI snapshot/feed/action contracts.
//!
//! These DTOs are projections and intents at the protocol boundary.  They carry no broker
//! authority; every action must be re-authorized by ControlPlane with the supplied CAS/digest.

use crate::UiCursor;
use kiana_domain::{
    json_digest, validate_json_limits, ArtifactId, AuthenticatedPrincipalRef, ConnectorHealthFact,
    ExecutionStatus, ProjectId, ReceiptId, RequestId, RunId, SessionId,
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
pub const UI_HOST_CAPABILITY_SCHEMA: &str = "kiana.ui-host-capability.v1";
pub const UI_LIVE_HOST_EVIDENCE_SCHEMA: &str = "kiana.ui-live-host-evidence.v1";
pub const UI_EVIDENCE_CASE_SCHEMA: &str = "kiana.ui-evidence-case.v1";
pub const UI_EVIDENCE_BUNDLE_SCHEMA: &str = "kiana.ui-evidence-bundle.v1";
pub const UI_CONNECTOR_HEALTH_SCHEMA: &str = "kiana.ui-connector-health.v1";
/// Versioned server-owned Web Human Inbox projection.  The projection is read-only; an action
/// intent carries only the item/action identity and CAS material back to ControlPlane.
pub const UI_HUMAN_INBOX_SCHEMA: &str = "kiana.ui-human-inbox.v1";
pub const UI_HUMAN_ACTION_CARD_SCHEMA: &str = "kiana.ui-human-action-card.v1";
pub const UI_HUMAN_ACTION_INTENT_SCHEMA: &str = "kiana.ui-human-action-intent.v1";
/// Versioned server-owned relationship between one browser tab and a session.  The tab value is
/// an observation scope, never a principal credential or a new authorization authority.
pub const UI_TAB_SESSION_SCHEMA: &str = "kiana.ui-tab-session.v1";
/// Versioned client submission envelope.  The server still rechecks principal, lease, CAS and
/// idempotency through the existing ControlPlane/UI action path.
pub const UI_ACTION_SUBMISSION_SCHEMA: &str = "kiana.ui-action-submission.v1";
/// Read-only Web artifact/detail projections.  These schemas carry server-owned references and
/// bounded pages only; they never grant a browser a filesystem path, URL fetch or capability.
pub const UI_ARTIFACT_DETAIL_SCHEMA: &str = "kiana.ui-artifact-detail.v1";
pub const UI_ARTIFACT_REF_SCHEMA: &str = "kiana.ui-artifact-ref.v1";
pub const UI_ARTIFACT_PAGE_SCHEMA: &str = "kiana.ui-artifact-page.v2";
pub const UI_DIFF_DETAIL_SCHEMA: &str = "kiana.ui-diff-detail.v1";
pub const UI_RECEIPT_DETAIL_SCHEMA: &str = "kiana.ui-receipt-detail.v1";
pub const UI_DETAIL_SCOPE_SCHEMA: &str = "kiana.ui-detail-scope.v1";
pub const UI_DETAIL_LINK_SCHEMA: &str = "kiana.ui-detail-link.v1";

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

fn command_argument_contains_secret_marker(value: &str) -> bool {
    let lower = value.to_ascii_lowercase();
    [
        "api_key",
        "api-key",
        "client_secret",
        "client-secret",
        "access_token",
        "access-token",
        "refresh_token",
        "refresh-token",
        "authorization:",
        "authorization=",
        "--authorization",
        "bearer ",
        "password ",
        "password=",
        "--password",
        "private_key",
        "private-key",
        "secret ",
        "secret=",
        "--secret",
        "token ",
        "token:",
        "token=",
        "--token",
    ]
    .iter()
    .any(|marker| lower.contains(marker))
}

// Explicit name for new UI contracts so field names such as `scope.digest` do not shadow the
// validation helper in presenter code.
fn digest_value(value: &str, field: &str) -> Result<(), String> {
    digest(value, field)
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
pub enum UiTabSessionDisposition {
    Owner,
    Observer,
    Revoked,
    Closed,
}

/// A server projection used by Web and other clients to keep tab-local state separate while
/// allowing observers to consume the same read-only feed.  `principal` is always server-owned;
/// a wire supplied owner/principal value must never grant a mutation.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct UiTabSessionV1 {
    pub schema: String,
    pub principal: AuthenticatedPrincipalRef,
    pub session_id: SessionId,
    pub tab_id: String,
    pub owner_tab_id: String,
    pub lease_epoch: u64,
    pub token_generation: u64,
    pub disposition: UiTabSessionDisposition,
    pub feed_only: bool,
}

impl UiTabSessionV1 {
    pub fn validate(&self) -> Result<(), String> {
        if self.schema != UI_TAB_SESSION_SCHEMA
            || self.lease_epoch == 0
            || self.token_generation == 0
            || self.feed_only != (self.disposition != UiTabSessionDisposition::Owner)
        {
            return Err("ui_tab_session_header_invalid".to_owned());
        }
        self.principal.validate()?;
        required(self.session_id.as_str(), "ui_tab_session_id", 256)?;
        required(&self.tab_id, "ui_tab_id", 256)?;
        required(&self.owner_tab_id, "ui_owner_tab_id", 256)
    }
}

/// Client generated submission metadata.  It deliberately carries no owner or grant fields;
/// those are resolved from the authenticated server principal and the tab/session lease.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct UiActionSubmissionV1 {
    pub schema: String,
    pub command_id: RequestId,
    pub idempotency_key: String,
    pub session_id: SessionId,
    pub tab_id: String,
    pub expected_epoch: String,
    pub expected_cursor: u64,
    #[serde(default)]
    pub expected_revision: Option<u64>,
    pub payload_digest: String,
    pub submitted_at_unix_ms: u64,
}

impl UiActionSubmissionV1 {
    pub fn validate(&self) -> Result<(), String> {
        if self.schema != UI_ACTION_SUBMISSION_SCHEMA
            || self.command_id.as_uuid().is_nil()
            || self.expected_cursor == 0
            || self.expected_revision == Some(0)
            || self.submitted_at_unix_ms == 0
        {
            return Err("ui_action_submission_header_invalid".to_owned());
        }
        required(
            &self.idempotency_key,
            "ui_action_submission_idempotency",
            256,
        )?;
        required(
            self.session_id.as_str(),
            "ui_action_submission_session",
            256,
        )?;
        required(&self.tab_id, "ui_action_submission_tab", 256)?;
        required(&self.expected_epoch, "ui_action_submission_epoch", 256)?;
        digest(&self.payload_digest, "ui_action_submission_payload_digest")
    }
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

/// Secret-free connector health row consumed by CLI/Web/Workbench projections.
/// `stale=true` is retained when the binding revision changed after the last probe; clients must
/// not turn a stale or connectivity-only row into a verified claim.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct UiConnectorHealth {
    pub health: ConnectorHealthFact,
    pub source_cursor: u64,
    pub projection_version: String,
    pub binding_revision: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub credential_generation: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub data_epoch: Option<u64>,
    pub stale: bool,
    pub proof_level: String,
    #[serde(default)]
    pub limitations: Vec<String>,
}

impl UiConnectorHealth {
    pub fn validate(&self) -> Result<(), String> {
        if self.health.validate().is_err()
            || self.source_cursor == 0
            || required(&self.projection_version, "ui_connector_projection", 128).is_err()
            || self.binding_revision == 0
            || self.credential_generation == Some(0)
            || self.proof_level != "source"
            || self.limitations.len() > 16
            || self
                .limitations
                .iter()
                .any(|value| value.trim().is_empty() || value.len() > 256 || value.contains('\0'))
            || self.binding_revision != self.health.binding_revision
            || self.credential_generation != self.health.credential_generation
        {
            return Err("ui_connector_health_invalid".to_owned());
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct UiConnectorHealthProjection {
    pub schema: String,
    pub source_cursor: u64,
    pub projection_version: String,
    #[serde(default)]
    pub entries: Vec<UiConnectorHealth>,
    #[serde(default)]
    pub limitations: Vec<String>,
}

impl UiConnectorHealthProjection {
    pub fn validate(&self) -> Result<(), String> {
        if self.schema != "kiana.connector-health-projection.v1"
            || self.source_cursor == 0
            || self.projection_version.trim().is_empty()
            || self.entries.len() > 256
            || self.limitations.len() > 16
        {
            return Err("ui_connector_health_projection_invalid".to_owned());
        }
        for entry in &self.entries {
            entry.validate()?;
        }
        if self
            .limitations
            .iter()
            .any(|value| value.trim().is_empty() || value.len() > 256 || value.contains('\0'))
        {
            return Err("ui_connector_health_projection_limitation_invalid".to_owned());
        }
        Ok(())
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

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum UiHostCapabilityDisposition {
    Advertised,
    Disabled,
    NotSupported,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct UiHostCapability {
    pub schema: String,
    pub capability_id: String,
    pub actions: Vec<String>,
    pub scope_digest: String,
    pub disposition: UiHostCapabilityDisposition,
    /// Host/editor/terminal APIs never become a Kiana side-effect authority.
    pub direct_effect: bool,
    /// Advertised host capabilities must route actions back through the Kiana protocol.
    pub delegated_to_kiana: bool,
    #[serde(default)]
    pub reason: Option<String>,
}

impl UiHostCapability {
    pub fn validate(&self) -> Result<(), String> {
        if self.schema != UI_HOST_CAPABILITY_SCHEMA {
            return Err("ui_host_capability_schema_invalid".to_owned());
        }
        required(&self.capability_id, "ui_host_capability_id", 128)?;
        digest(&self.scope_digest, "ui_host_capability_scope_digest")?;
        if self.actions.len() > 64
            || self.actions.iter().any(|action| {
                action.trim().is_empty() || action.len() > 128 || action.contains('\0')
            })
        {
            return Err("ui_host_capability_actions_invalid".to_owned());
        }
        if self.direct_effect {
            return Err("ui_host_capability_direct_effect_forbidden".to_owned());
        }
        if self.disposition == UiHostCapabilityDisposition::Advertised && !self.delegated_to_kiana {
            return Err("ui_host_capability_delegation_required".to_owned());
        }
        if self.disposition != UiHostCapabilityDisposition::Advertised
            && self
                .reason
                .as_deref()
                .is_none_or(|value| value.trim().is_empty())
        {
            return Err("ui_host_capability_reason_required".to_owned());
        }
        if self
            .reason
            .as_deref()
            .is_some_and(|reason| reason.len() > 1_024 || reason.contains('\0'))
        {
            return Err("ui_host_capability_reason_invalid".to_owned());
        }
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum UiLiveHostStatus {
    NotSupported,
    OptedIn,
    Verified,
    Unknown,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct UiLiveHostEvidence {
    pub schema: String,
    pub protocol_version: String,
    pub surface: UiSurface,
    pub host_id: String,
    pub host_version: String,
    pub environment_digest: String,
    pub workspace_digest: String,
    #[serde(default)]
    pub session_ref: Option<String>,
    pub status: UiLiveHostStatus,
    #[serde(default)]
    pub operator_approval_ref: Option<String>,
    pub host_capabilities: Vec<UiHostCapability>,
    pub handshake_verified: bool,
    pub session_verified: bool,
    pub prompt_routed: bool,
    pub update_observed: bool,
    pub permission_routed: bool,
    pub cancel_fence_verified: bool,
    pub reconnect_verified: bool,
    #[serde(default)]
    pub receipt_digest: Option<String>,
    #[serde(default)]
    pub limitations: Vec<EvidenceLimitation>,
    pub evidence_digest: String,
}

impl UiLiveHostEvidence {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        protocol_version: impl Into<String>,
        surface: UiSurface,
        host_id: impl Into<String>,
        host_version: impl Into<String>,
        environment_digest: impl Into<String>,
        workspace_digest: impl Into<String>,
        session_ref: Option<String>,
        status: UiLiveHostStatus,
        operator_approval_ref: Option<String>,
        host_capabilities: Vec<UiHostCapability>,
        handshake_verified: bool,
        session_verified: bool,
        prompt_routed: bool,
        update_observed: bool,
        permission_routed: bool,
        cancel_fence_verified: bool,
        reconnect_verified: bool,
        receipt_digest: Option<String>,
        limitations: Vec<EvidenceLimitation>,
    ) -> Result<Self, String> {
        let mut evidence = Self {
            schema: UI_LIVE_HOST_EVIDENCE_SCHEMA.to_owned(),
            protocol_version: protocol_version.into(),
            surface,
            host_id: host_id.into(),
            host_version: host_version.into(),
            environment_digest: environment_digest.into(),
            workspace_digest: workspace_digest.into(),
            session_ref,
            status,
            operator_approval_ref,
            host_capabilities,
            handshake_verified,
            session_verified,
            prompt_routed,
            update_observed,
            permission_routed,
            cancel_fence_verified,
            reconnect_verified,
            receipt_digest,
            limitations,
            evidence_digest: String::new(),
        };
        evidence.evidence_digest = evidence.digest();
        evidence.validate()?;
        Ok(evidence)
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.schema != UI_LIVE_HOST_EVIDENCE_SCHEMA {
            return Err("ui_live_host_evidence_schema_invalid".to_owned());
        }
        required(&self.protocol_version, "ui_live_protocol_version", 64)?;
        required(&self.host_id, "ui_live_host_id", 256)?;
        required(&self.host_version, "ui_live_host_version", 128)?;
        digest(&self.environment_digest, "ui_live_environment_digest")?;
        digest(&self.workspace_digest, "ui_live_workspace_digest")?;
        if self
            .session_ref
            .as_deref()
            .is_some_and(|value| value.trim().is_empty() || value.len() > 256)
        {
            return Err("ui_live_session_ref_invalid".to_owned());
        }
        if let Some(value) = &self.operator_approval_ref {
            required(value, "ui_live_operator_approval_ref", 256)?;
        }
        if self.host_capabilities.len() > 128 {
            return Err("ui_live_host_capability_limit".to_owned());
        }
        let mut capability_ids = BTreeSet::new();
        for capability in &self.host_capabilities {
            capability.validate()?;
            if !capability_ids.insert(capability.capability_id.clone()) {
                return Err("ui_live_host_capability_duplicate".to_owned());
            }
        }
        if let Some(receipt) = &self.receipt_digest {
            digest(receipt, "ui_live_host_receipt_digest")?;
        }
        if self.limitations.len() > 128 {
            return Err("ui_live_host_limitation_limit".to_owned());
        }
        for limitation in &self.limitations {
            limitation.validate()?;
        }
        match self.status {
            UiLiveHostStatus::NotSupported | UiLiveHostStatus::Unknown => {
                if self.limitations.is_empty() {
                    return Err("ui_live_host_limitation_required".to_owned());
                }
            }
            UiLiveHostStatus::OptedIn => {
                if self.operator_approval_ref.is_none() || self.limitations.is_empty() {
                    return Err("ui_live_host_opt_in_evidence_incomplete".to_owned());
                }
            }
            UiLiveHostStatus::Verified => {
                if self.operator_approval_ref.is_none()
                    || self.session_ref.is_none()
                    || self.receipt_digest.is_none()
                    || self.host_capabilities.is_empty()
                    || !self.handshake_verified
                    || !self.session_verified
                    || !self.prompt_routed
                    || !self.update_observed
                    || !self.permission_routed
                    || !self.cancel_fence_verified
                    || !self.reconnect_verified
                {
                    return Err("ui_live_host_verified_evidence_incomplete".to_owned());
                }
            }
        }
        if self.evidence_digest != self.digest() {
            return Err("ui_live_host_evidence_digest_mismatch".to_owned());
        }
        Ok(())
    }

    pub fn digest(&self) -> String {
        json_digest(&json!({
            "schema": self.schema,
            "protocol_version": self.protocol_version,
            "surface": self.surface,
            "host_id": self.host_id,
            "host_version": self.host_version,
            "environment_digest": self.environment_digest,
            "workspace_digest": self.workspace_digest,
            "session_ref": self.session_ref,
            "status": self.status,
            "operator_approval_ref": self.operator_approval_ref,
            "host_capabilities": self.host_capabilities,
            "handshake_verified": self.handshake_verified,
            "session_verified": self.session_verified,
            "prompt_routed": self.prompt_routed,
            "update_observed": self.update_observed,
            "permission_routed": self.permission_routed,
            "cancel_fence_verified": self.cancel_fence_verified,
            "reconnect_verified": self.reconnect_verified,
            "receipt_digest": self.receipt_digest,
            "limitations": self.limitations,
        }))
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum UiEvidenceClass {
    Deny,
    Recovery,
    Happy,
    Parity,
    Performance,
    Live,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum UiEvidenceOutcome {
    Passed,
    Failed,
    Skipped,
    Unknown,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum UiFeatureStatus {
    Implemented,
    Partial,
    Target,
    Deferred,
    NotSupported,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum UiProofLevel {
    Source,
    LocalBehavior,
    Durable,
    Live,
    Physical,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct UiEvidenceCase {
    pub schema: String,
    pub case_id: String,
    pub surface: UiSurface,
    pub class: UiEvidenceClass,
    pub command_argv: Vec<String>,
    pub source_snapshot: String,
    pub fixture_digest: String,
    pub environment_digest: String,
    #[serde(default)]
    pub exit_code: Option<i32>,
    pub feature_status: UiFeatureStatus,
    pub proof_level: UiProofLevel,
    pub outcome: UiEvidenceOutcome,
    #[serde(default)]
    pub receipt_digest: Option<String>,
    #[serde(default)]
    pub artifact_digests: Vec<String>,
    #[serde(default)]
    pub limitations: Vec<EvidenceLimitation>,
    pub reviewer: String,
    pub case_digest: String,
}

impl UiEvidenceCase {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        case_id: impl Into<String>,
        surface: UiSurface,
        class: UiEvidenceClass,
        command_argv: Vec<String>,
        source_snapshot: impl Into<String>,
        fixture_digest: impl Into<String>,
        environment_digest: impl Into<String>,
        exit_code: Option<i32>,
        feature_status: UiFeatureStatus,
        proof_level: UiProofLevel,
        outcome: UiEvidenceOutcome,
        receipt_digest: Option<String>,
        mut artifact_digests: Vec<String>,
        limitations: Vec<EvidenceLimitation>,
        reviewer: impl Into<String>,
    ) -> Result<Self, String> {
        artifact_digests.sort();
        artifact_digests.dedup();
        let mut case = Self {
            schema: UI_EVIDENCE_CASE_SCHEMA.to_owned(),
            case_id: case_id.into(),
            surface,
            class,
            command_argv,
            source_snapshot: source_snapshot.into(),
            fixture_digest: fixture_digest.into(),
            environment_digest: environment_digest.into(),
            exit_code,
            feature_status,
            proof_level,
            outcome,
            receipt_digest,
            artifact_digests,
            limitations,
            reviewer: reviewer.into(),
            case_digest: String::new(),
        };
        case.case_digest = case.digest();
        case.validate()?;
        Ok(case)
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.schema != UI_EVIDENCE_CASE_SCHEMA {
            return Err("ui_evidence_case_schema_invalid".to_owned());
        }
        required(&self.case_id, "ui_evidence_case_id", 128)?;
        required(&self.source_snapshot, "ui_evidence_source_snapshot", 256)?;
        required(&self.reviewer, "ui_evidence_reviewer", 256)?;
        if self.command_argv.is_empty() || self.command_argv.len() > 64 {
            return Err("ui_evidence_command_argv_invalid".to_owned());
        }
        for argument in &self.command_argv {
            required(argument, "ui_evidence_command_argument", 4_096)?;
            if command_argument_contains_secret_marker(argument) {
                return Err("ui_evidence_secret_in_command_argv".to_owned());
            }
        }
        digest(&self.fixture_digest, "ui_evidence_fixture_digest")?;
        digest(&self.environment_digest, "ui_evidence_environment_digest")?;
        if self.outcome != UiEvidenceOutcome::Skipped && self.exit_code.is_none() {
            return Err("ui_evidence_exit_code_missing".to_owned());
        }
        if self.outcome == UiEvidenceOutcome::Passed && self.exit_code != Some(0) {
            return Err("ui_evidence_pass_exit_code_invalid".to_owned());
        }
        if let Some(receipt) = &self.receipt_digest {
            digest(receipt, "ui_evidence_receipt_digest")?;
        }
        if self
            .artifact_digests
            .iter()
            .any(|value| digest(value, "ui_evidence_artifact_digest").is_err())
            || self
                .artifact_digests
                .windows(2)
                .any(|pair| pair[0] >= pair[1])
        {
            return Err("ui_evidence_artifact_digests_invalid".to_owned());
        }
        if self.limitations.len() > 128 {
            return Err("ui_evidence_limitation_limit".to_owned());
        }
        for limitation in &self.limitations {
            limitation.validate()?;
        }
        if self.feature_status == UiFeatureStatus::Implemented
            && self.proof_level == UiProofLevel::Source
        {
            return Err("ui_evidence_implemented_requires_behavior".to_owned());
        }
        if self.class == UiEvidenceClass::Live
            && self.outcome == UiEvidenceOutcome::Passed
            && !matches!(
                self.proof_level,
                UiProofLevel::Live | UiProofLevel::Physical
            )
        {
            return Err("ui_evidence_live_proof_required".to_owned());
        }
        if matches!(
            self.proof_level,
            UiProofLevel::Durable | UiProofLevel::Live | UiProofLevel::Physical
        ) && self.receipt_digest.is_none()
        {
            return Err("ui_evidence_receipt_required".to_owned());
        }
        if self.outcome != UiEvidenceOutcome::Passed && self.limitations.is_empty() {
            return Err("ui_evidence_outcome_limitation_required".to_owned());
        }
        if self.case_digest != self.digest() {
            return Err("ui_evidence_case_digest_mismatch".to_owned());
        }
        Ok(())
    }

    pub fn digest(&self) -> String {
        json_digest(&json!({
            "schema": self.schema,
            "case_id": self.case_id,
            "surface": self.surface,
            "class": self.class,
            "command_argv": self.command_argv,
            "source_snapshot": self.source_snapshot,
            "fixture_digest": self.fixture_digest,
            "environment_digest": self.environment_digest,
            "exit_code": self.exit_code,
            "feature_status": self.feature_status,
            "proof_level": self.proof_level,
            "outcome": self.outcome,
            "receipt_digest": self.receipt_digest,
            "artifact_digests": self.artifact_digests,
            "limitations": self.limitations,
            "reviewer": self.reviewer,
        }))
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct UiEvidenceBundle {
    pub schema: String,
    pub source_snapshot: String,
    pub cases: Vec<UiEvidenceCase>,
    pub bundle_digest: String,
}

impl UiEvidenceBundle {
    pub fn new(
        source_snapshot: impl Into<String>,
        cases: Vec<UiEvidenceCase>,
    ) -> Result<Self, String> {
        let mut bundle = Self {
            schema: UI_EVIDENCE_BUNDLE_SCHEMA.to_owned(),
            source_snapshot: source_snapshot.into(),
            cases,
            bundle_digest: String::new(),
        };
        bundle.bundle_digest = bundle.digest();
        bundle.validate()?;
        Ok(bundle)
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.schema != UI_EVIDENCE_BUNDLE_SCHEMA {
            return Err("ui_evidence_bundle_schema_invalid".to_owned());
        }
        required(
            &self.source_snapshot,
            "ui_evidence_bundle_source_snapshot",
            256,
        )?;
        if self.cases.is_empty() || self.cases.len() > 256 {
            return Err("ui_evidence_bundle_case_count_invalid".to_owned());
        }
        let mut ids = BTreeSet::new();
        for case in &self.cases {
            case.validate()?;
            if case.source_snapshot != self.source_snapshot {
                return Err("ui_evidence_bundle_source_snapshot_drift".to_owned());
            }
            if !ids.insert(case.case_id.clone()) {
                return Err("ui_evidence_bundle_case_duplicate".to_owned());
            }
        }
        if self.bundle_digest != self.digest() {
            return Err("ui_evidence_bundle_digest_mismatch".to_owned());
        }
        Ok(())
    }

    pub fn digest(&self) -> String {
        json_digest(&json!({
            "schema": self.schema,
            "source_snapshot": self.source_snapshot,
            "cases": self.cases,
        }))
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

const UI_HUMAN_MAX_ITEMS: usize = 256;
const UI_HUMAN_MAX_FIELDS: usize = 32;
const UI_HUMAN_MAX_DECISIONS: usize = 16;
const UI_HUMAN_MAX_PATHS: usize = 256;
const UI_HUMAN_MAX_FIELD_VALUE_BYTES: usize = 64 * 1024;

/// Redacted scope metadata displayed by a Web action card.  It is a projection and never grants
/// the browser permission to add paths or widen a write set.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct UiHumanActionScopeV1 {
    pub summary: String,
    #[serde(default)]
    pub digest: Option<String>,
    #[serde(default)]
    pub paths: Vec<String>,
}

impl UiHumanActionScopeV1 {
    pub fn validate(&self) -> Result<(), String> {
        required(&self.summary, "ui_human_scope_summary", 4_096)?;
        if let Some(digest) = &self.digest {
            digest_value(digest, "ui_human_scope_digest")?;
        }
        if self.paths.len() > UI_HUMAN_MAX_PATHS
            || self.paths.iter().any(|path| {
                path.trim().is_empty()
                    || path.len() > 1_024
                    || path.contains('\0')
                    || path.starts_with("http://")
                    || path.starts_with("https://")
            })
        {
            return Err("ui_human_scope_paths_invalid".to_owned());
        }
        Ok(())
    }
}

/// A server-supplied form field.  Allowed values and requiredness are display/validation hints;
/// ControlPlane remains the authority for the action and its effect scope.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct UiHumanActionFieldV1 {
    pub name: String,
    pub label: String,
    pub field_type: String,
    pub required: bool,
    #[serde(default)]
    pub allowed_values: Vec<String>,
}

impl UiHumanActionFieldV1 {
    pub fn validate(&self) -> Result<(), String> {
        required(&self.name, "ui_human_field_name", 128)?;
        required(&self.label, "ui_human_field_label", 512)?;
        required(&self.field_type, "ui_human_field_type", 64)?;
        if self.allowed_values.len() > UI_HUMAN_MAX_DECISIONS
            || self
                .allowed_values
                .iter()
                .any(|value| value.trim().is_empty() || value.len() > 256)
        {
            return Err("ui_human_field_values_invalid".to_owned());
        }
        Ok(())
    }
}

/// Complete Web Human Inbox action card.  `payload_digest` binds the opaque server payload
/// without exposing it as a browser-editable scope or actor field.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct UiHumanActionCardV1 {
    pub schema: String,
    pub item_id: String,
    pub action_id: String,
    pub command: String,
    pub target_id: String,
    pub reason: String,
    pub scope: UiHumanActionScopeV1,
    #[serde(default)]
    pub expires_at_unix_ms: Option<u64>,
    #[serde(default)]
    pub revoked: bool,
    #[serde(default)]
    pub expected_revision: Option<u64>,
    pub fields: Vec<UiHumanActionFieldV1>,
    pub allowed_decisions: Vec<String>,
    pub payload_digest: String,
}

impl UiHumanActionCardV1 {
    pub fn validate(&self) -> Result<(), String> {
        if self.schema != UI_HUMAN_ACTION_CARD_SCHEMA {
            return Err("ui_human_card_schema_invalid".to_owned());
        }
        required(&self.item_id, "ui_human_item_id", 256)?;
        required(&self.action_id, "ui_human_action_id", 256)?;
        required(&self.command, "ui_human_command", 128)?;
        required(&self.target_id, "ui_human_target_id", 256)?;
        required(&self.reason, "ui_human_reason", 4_096)?;
        self.scope.validate()?;
        if self.expires_at_unix_ms == Some(0) || self.expected_revision == Some(0) {
            return Err("ui_human_card_revision_invalid".to_owned());
        }
        if self.fields.len() > UI_HUMAN_MAX_FIELDS {
            return Err("ui_human_card_fields_limit".to_owned());
        }
        let mut names = BTreeSet::new();
        for field in &self.fields {
            field.validate()?;
            if !names.insert(&field.name) {
                return Err("ui_human_card_field_duplicate".to_owned());
            }
        }
        if self.allowed_decisions.is_empty()
            || self.allowed_decisions.len() > UI_HUMAN_MAX_DECISIONS
        {
            return Err("ui_human_card_decisions_invalid".to_owned());
        }
        let mut decisions = BTreeSet::new();
        for decision in &self.allowed_decisions {
            required(decision, "ui_human_decision", 64)?;
            if !decisions.insert(decision) {
                return Err("ui_human_card_decision_duplicate".to_owned());
            }
        }
        digest_value(&self.payload_digest, "ui_human_payload_digest")
    }

    pub fn is_expired(&self, now_unix_ms: u64) -> bool {
        self.expires_at_unix_ms
            .is_some_and(|expires_at| now_unix_ms >= expires_at)
    }

    pub fn allows(&self, decision: &str) -> bool {
        self.allowed_decisions.iter().any(|item| item == decision)
    }
}

/// Read-only Web inbox projection.  `revision` is the server digest used for action CAS.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct UiHumanInboxV1 {
    pub schema: String,
    pub revision: String,
    pub items: Vec<UiHumanActionCardV1>,
    #[serde(default)]
    pub limitations: Vec<String>,
}

impl UiHumanInboxV1 {
    pub fn validate(&self) -> Result<(), String> {
        if self.schema != UI_HUMAN_INBOX_SCHEMA {
            return Err("ui_human_inbox_schema_invalid".to_owned());
        }
        digest_value(&self.revision, "ui_human_inbox_revision")?;
        if self.items.len() > UI_HUMAN_MAX_ITEMS || self.limitations.len() > 32 {
            return Err("ui_human_inbox_limit".to_owned());
        }
        let mut ids = BTreeSet::new();
        for item in &self.items {
            item.validate()?;
            if !ids.insert((item.item_id.as_str(), item.action_id.as_str())) {
                return Err("ui_human_inbox_duplicate".to_owned());
            }
        }
        if self
            .limitations
            .iter()
            .any(|limitation| limitation.trim().is_empty() || limitation.len() > 512)
        {
            return Err("ui_human_inbox_limitations_invalid".to_owned());
        }
        Ok(())
    }
}

/// Versioned intent emitted by the Web form.  It contains no actor, approval grant or scope;
/// those are recovered and rechecked by the server from `item_id`/`action_id`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct UiHumanActionIntentV1 {
    pub schema: String,
    pub item_id: String,
    pub action_id: String,
    pub inbox_revision: String,
    #[serde(default)]
    pub expected_revision: Option<u64>,
    pub fields: Value,
    pub idempotency_key: String,
    pub payload_digest: String,
}

impl UiHumanActionIntentV1 {
    pub fn validate(&self) -> Result<(), String> {
        if self.schema != UI_HUMAN_ACTION_INTENT_SCHEMA {
            return Err("ui_human_intent_schema_invalid".to_owned());
        }
        required(&self.item_id, "ui_human_intent_item_id", 256)?;
        required(&self.action_id, "ui_human_intent_action_id", 256)?;
        digest_value(&self.inbox_revision, "ui_human_intent_revision")?;
        if self.expected_revision == Some(0) {
            return Err("ui_human_intent_expected_revision_invalid".to_owned());
        }
        required(&self.idempotency_key, "ui_human_intent_idempotency", 256)?;
        digest_value(&self.payload_digest, "ui_human_intent_payload_digest")?;
        let Some(fields) = self.fields.as_object() else {
            return Err("ui_human_intent_fields_object_required".to_owned());
        };
        if fields.len() > UI_HUMAN_MAX_FIELDS
            || serde_json::to_vec(&self.fields)
                .map(|bytes| bytes.len() > UI_HUMAN_MAX_FIELD_VALUE_BYTES)
                .unwrap_or(true)
        {
            return Err("ui_human_intent_fields_limit".to_owned());
        }
        if fields
            .keys()
            .any(|key| key.trim().is_empty() || key.len() > 128)
        {
            return Err("ui_human_intent_field_name_invalid".to_owned());
        }
        Ok(())
    }
}

const UI_DETAIL_MAX_ITEMS: usize = 512;
const UI_DETAIL_MAX_LINKS: usize = 256;
const UI_DETAIL_MAX_PAGE_BYTES: usize = 64 * 1024;
const UI_DETAIL_MAX_DIFF_BYTES: usize = 256 * 1024;
const UI_DETAIL_MAX_LIMITATIONS: usize = 32;

fn bounded_detail_text(value: &str, field: &str, max: usize) -> Result<(), String> {
    if value.len() > max || value.contains('\0') {
        return Err(format!("{field}_invalid"));
    }
    Ok(())
}

fn valid_mime(value: &str) -> bool {
    let value = value.trim();
    !value.is_empty()
        && value.len() <= 128
        && !value.contains('\0')
        && !matches!(
            value,
            "text/html" | "application/xhtml+xml" | "image/svg+xml"
        )
}

/// Scope and cursor supplied by the server for every detail page.  The browser may use it to
/// discard stale data, but cannot turn a different tab/session into an owner or a new authority.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct UiDetailScopeV1 {
    pub schema: String,
    pub session_id: SessionId,
    pub tab_id: String,
    pub owner_tab_id: String,
    pub instance_id: String,
    pub epoch: String,
    pub source_cursor: u64,
    pub revision: u64,
}

impl UiDetailScopeV1 {
    pub fn validate(&self) -> Result<(), String> {
        if self.schema != UI_DETAIL_SCOPE_SCHEMA || self.source_cursor == 0 || self.revision == 0 {
            return Err("ui_detail_scope_header_invalid".to_owned());
        }
        required(self.session_id.as_str(), "ui_detail_scope_session", 256)?;
        required(&self.tab_id, "ui_detail_scope_tab", 256)?;
        required(&self.owner_tab_id, "ui_detail_scope_owner_tab", 256)?;
        required(&self.instance_id, "ui_detail_scope_instance", 256)?;
        required(&self.epoch, "ui_detail_scope_epoch", 256)
    }

    pub fn matches_session_tab(&self, session_id: &SessionId, tab_id: &str) -> bool {
        &self.session_id == session_id && self.tab_id == tab_id
    }
}

/// A typed cross-location link.  All links are identifiers in the same server session; they are
/// never URLs or paths and do not cause the browser to fetch another origin.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct UiDetailLinkV1 {
    pub schema: String,
    pub kind: String,
    pub id: String,
    pub session_id: SessionId,
}

impl UiDetailLinkV1 {
    pub fn validate(&self) -> Result<(), String> {
        if self.schema != UI_DETAIL_LINK_SCHEMA {
            return Err("ui_detail_link_schema_invalid".to_owned());
        }
        required(&self.kind, "ui_detail_link_kind", 64)?;
        required(&self.id, "ui_detail_link_id", 512)?;
        required(self.session_id.as_str(), "ui_detail_link_session", 256)
    }
}

/// Server-owned artifact metadata.  `content_hash` is the immutable artifact digest; `revision`
/// fences a view against a replacement projection and is never chosen by the client.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct UiArtifactRefV1 {
    pub schema: String,
    pub artifact_id: ArtifactId,
    pub version: u64,
    pub artifact_schema: String,
    pub mime: String,
    pub size_bytes: u64,
    pub content_hash: String,
    pub scope_digest: String,
    pub revision: u64,
    pub session_id: SessionId,
    #[serde(default)]
    pub run_id: Option<RunId>,
    pub provenance: String,
    #[serde(default)]
    pub links: Vec<UiDetailLinkV1>,
}

impl UiArtifactRefV1 {
    pub fn validate(&self) -> Result<(), String> {
        if self.schema != UI_ARTIFACT_REF_SCHEMA || self.version == 0 || self.revision == 0 {
            return Err("ui_artifact_ref_header_invalid".to_owned());
        }
        required(self.artifact_id.as_str(), "ui_artifact_ref_id", 256)?;
        required(&self.artifact_schema, "ui_artifact_ref_schema", 128)?;
        if !valid_mime(&self.mime) {
            return Err("ui_artifact_ref_mime_invalid".to_owned());
        }
        digest_value(&self.content_hash, "ui_artifact_ref_content_hash")?;
        digest_value(&self.scope_digest, "ui_artifact_ref_scope_digest")?;
        required(self.session_id.as_str(), "ui_artifact_ref_session", 256)?;
        bounded_detail_text(&self.provenance, "ui_artifact_ref_provenance", 4_096)?;
        if self.links.len() > UI_DETAIL_MAX_LINKS {
            return Err("ui_artifact_ref_links_limit".to_owned());
        }
        for link in &self.links {
            link.validate()?;
            if link.session_id != self.session_id {
                return Err("ui_artifact_ref_cross_session_link".to_owned());
            }
        }
        Ok(())
    }
}

/// One server-provided artifact page.  Content is optional for binary, redacted or unavailable
/// artifacts; a missing page is a limitation/Unknown state, never a successful empty artifact.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct UiArtifactPageV1 {
    pub schema: String,
    pub scope: UiDetailScopeV1,
    pub artifact: UiArtifactRefV1,
    pub revision: u64,
    pub page_index: u32,
    pub page_count: u32,
    pub page_digest: String,
    pub content_encoding: String,
    #[serde(default)]
    pub content: Option<String>,
    #[serde(default)]
    pub truncated: bool,
    #[serde(default)]
    pub next_cursor: Option<String>,
    #[serde(default)]
    pub limitations: Vec<String>,
}

impl UiArtifactPageV1 {
    pub fn validate(&self) -> Result<(), String> {
        if self.schema != UI_ARTIFACT_PAGE_SCHEMA
            || self.revision == 0
            || self.page_count == 0
            || self.page_index >= self.page_count
            || self.limitations.len() > UI_DETAIL_MAX_LIMITATIONS
        {
            return Err("ui_artifact_page_header_invalid".to_owned());
        }
        self.scope.validate()?;
        self.artifact.validate()?;
        if self.scope.session_id != self.artifact.session_id
            || self.revision != self.artifact.revision
            || self.scope.revision != self.revision
        {
            return Err("ui_artifact_page_revision_scope_mismatch".to_owned());
        }
        digest_value(&self.page_digest, "ui_artifact_page_digest")?;
        required(&self.content_encoding, "ui_artifact_page_encoding", 32)?;
        if self.content_encoding != "text"
            && self.content_encoding != "base64"
            && self.content_encoding != "none"
        {
            return Err("ui_artifact_page_encoding_invalid".to_owned());
        }
        if self.content_encoding == "none" && self.content.is_some() {
            return Err("ui_artifact_page_content_encoding_mismatch".to_owned());
        }
        if self.content.as_deref().is_some_and(|content| {
            content.len() > UI_DETAIL_MAX_PAGE_BYTES || content.contains('\0')
        }) {
            return Err("ui_artifact_page_content_limit".to_owned());
        }
        if self.next_cursor.as_deref().is_some_and(|cursor| {
            cursor.trim().is_empty() || cursor.len() > 512 || cursor.contains('\0')
        }) {
            return Err("ui_artifact_page_cursor_invalid".to_owned());
        }
        if self
            .limitations
            .iter()
            .any(|value| value.trim().is_empty() || value.len() > 512 || value.contains('\0'))
        {
            return Err("ui_artifact_page_limitations_invalid".to_owned());
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct UiDiffFileV1 {
    pub path_display: String,
    pub status: String,
    pub additions: u64,
    pub deletions: u64,
    pub patch_digest: String,
    #[serde(default)]
    pub patch: Option<String>,
}

impl UiDiffFileV1 {
    pub fn validate(&self) -> Result<(), String> {
        required(&self.path_display, "ui_diff_file_path", 1_024)?;
        if self.path_display.contains('\0') || self.path_display.starts_with("http") {
            return Err("ui_diff_file_path_invalid".to_owned());
        }
        required(&self.status, "ui_diff_file_status", 64)?;
        digest_value(&self.patch_digest, "ui_diff_file_patch_digest")?;
        if self
            .patch
            .as_deref()
            .is_some_and(|patch| patch.len() > UI_DETAIL_MAX_DIFF_BYTES || patch.contains('\0'))
        {
            return Err("ui_diff_file_patch_limit".to_owned());
        }
        Ok(())
    }
}

/// Diff statistics and file status are calculated server-side.  The browser only renders these
/// values and never computes a patch, reads a path or treats a file list as authorization.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct UiDiffDetailV1 {
    pub schema: String,
    pub scope: UiDetailScopeV1,
    pub artifact: UiArtifactRefV1,
    pub base_revision: u64,
    pub target_revision: u64,
    pub status: String,
    pub total_additions: u64,
    pub total_deletions: u64,
    pub files: Vec<UiDiffFileV1>,
    #[serde(default)]
    pub links: Vec<UiDetailLinkV1>,
    #[serde(default)]
    pub limitations: Vec<String>,
}

impl UiDiffDetailV1 {
    pub fn validate(&self) -> Result<(), String> {
        if self.schema != UI_DIFF_DETAIL_SCHEMA
            || self.base_revision == 0
            || self.target_revision == 0
            || self.files.len() > UI_DETAIL_MAX_ITEMS
            || self.limitations.len() > UI_DETAIL_MAX_LIMITATIONS
        {
            return Err("ui_diff_detail_header_invalid".to_owned());
        }
        self.scope.validate()?;
        self.artifact.validate()?;
        if self.scope.session_id != self.artifact.session_id {
            return Err("ui_diff_detail_scope_mismatch".to_owned());
        }
        for file in &self.files {
            file.validate()?;
        }
        for link in &self.links {
            link.validate()?;
            if link.session_id != self.scope.session_id {
                return Err("ui_diff_detail_cross_session_link".to_owned());
            }
        }
        Ok(())
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct UiReceiptEntryV1 {
    pub entry_id: String,
    pub status: String,
    pub effect_known: bool,
    #[serde(default)]
    pub result_digest: Option<String>,
    pub source_cursor: u64,
    #[serde(default)]
    pub artifact_ids: Vec<ArtifactId>,
}

impl UiReceiptEntryV1 {
    pub fn validate(&self) -> Result<(), String> {
        required(&self.entry_id, "ui_receipt_entry_id", 256)?;
        required(&self.status, "ui_receipt_entry_status", 64)?;
        if self.source_cursor == 0 || self.artifact_ids.len() > UI_DETAIL_MAX_ITEMS {
            return Err("ui_receipt_entry_bounds_invalid".to_owned());
        }
        if let Some(digest) = &self.result_digest {
            digest_value(digest, "ui_receipt_entry_result_digest")?;
        }
        Ok(())
    }
}

/// Redacted receipt detail projection.  `unknown` is explicit and must stay visible in the page;
/// a missing effect/result is never rendered as a green success state.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct UiReceiptDetailV1 {
    pub schema: String,
    pub scope: UiDetailScopeV1,
    pub receipt_id: ReceiptId,
    pub run_id: RunId,
    pub session_id: SessionId,
    pub receipt_digest: String,
    pub status: String,
    pub unknown: bool,
    pub source_cursor: u64,
    pub entries: Vec<UiReceiptEntryV1>,
    #[serde(default)]
    pub artifact_ids: Vec<ArtifactId>,
    #[serde(default)]
    pub timeline_ids: Vec<String>,
    #[serde(default)]
    pub inbox_ids: Vec<String>,
    #[serde(default)]
    pub limitations: Vec<String>,
}

impl UiReceiptDetailV1 {
    pub fn validate(&self) -> Result<(), String> {
        if self.schema != UI_RECEIPT_DETAIL_SCHEMA
            || self.run_id.as_uuid().is_nil()
            || self.session_id.is_empty()
            || self.source_cursor == 0
            || self.entries.len() > UI_DETAIL_MAX_ITEMS
            || self.artifact_ids.len() > UI_DETAIL_MAX_ITEMS
            || self.timeline_ids.len() > UI_DETAIL_MAX_LINKS
            || self.inbox_ids.len() > UI_DETAIL_MAX_LINKS
            || self.limitations.len() > UI_DETAIL_MAX_LIMITATIONS
        {
            return Err("ui_receipt_detail_header_invalid".to_owned());
        }
        self.scope.validate()?;
        if self.scope.session_id != self.session_id {
            return Err("ui_receipt_detail_scope_mismatch".to_owned());
        }
        digest_value(&self.receipt_digest, "ui_receipt_detail_digest")?;
        for entry in &self.entries {
            entry.validate()?;
        }
        if self.unknown && self.status == "completed" {
            return Err("ui_receipt_unknown_completed_conflict".to_owned());
        }
        Ok(())
    }
}

/// Aggregate read-only detail used by deep links.  Each child projection repeats the scope so a
/// browser cannot combine an artifact, diff and receipt from different sessions or revisions.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct UiArtifactDetailV1 {
    pub schema: String,
    pub scope: UiDetailScopeV1,
    pub artifact_page: UiArtifactPageV1,
    #[serde(default)]
    pub diff: Option<UiDiffDetailV1>,
    #[serde(default)]
    pub receipt: Option<UiReceiptDetailV1>,
    pub status: String,
    #[serde(default)]
    pub limitations: Vec<String>,
}

impl UiArtifactDetailV1 {
    pub fn validate(&self) -> Result<(), String> {
        if self.schema != UI_ARTIFACT_DETAIL_SCHEMA
            || self.limitations.len() > UI_DETAIL_MAX_LIMITATIONS
        {
            return Err("ui_artifact_detail_header_invalid".to_owned());
        }
        self.scope.validate()?;
        self.artifact_page.validate()?;
        if self.artifact_page.scope != self.scope {
            return Err("ui_artifact_detail_scope_mismatch".to_owned());
        }
        if let Some(diff) = &self.diff {
            diff.validate()?;
            if diff.scope != self.scope {
                return Err("ui_artifact_detail_diff_scope_mismatch".to_owned());
            }
        }
        if let Some(receipt) = &self.receipt {
            receipt.validate()?;
            if receipt.scope.session_id != self.scope.session_id {
                return Err("ui_artifact_detail_receipt_scope_mismatch".to_owned());
            }
        }
        Ok(())
    }
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

/// Cursor for the daemon-owned UI feed.  It is intentionally separate from the EventLog cursor:
/// the feed is a bounded display projection and an old or foreign cursor can only request a
/// snapshot/gap, never authorize a replay or an effect.
pub const UI_FEED_CURSOR_SCHEMA: &str = "kiana.ui-feed-cursor.v1";
pub const UI_FEED_FRAME_SCHEMA: &str = "kiana.ui-feed-frame.v1";
pub const UI_FEED_GAP_SCHEMA: &str = "kiana.ui-feed-gap.v1";

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum UiFeedGapReason {
    SequenceGap,
    ReplayExpired,
    OldEpoch,
    InstanceChanged,
    SequenceAhead,
    Backpressure,
    TerminalRetention,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum UiFeedFrameKind {
    SnapshotBoundary,
    Delta,
    Heartbeat,
    Gap,
    Terminal,
    Unknown,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct UiFeedCursorV1 {
    pub schema: String,
    pub instance_id: String,
    pub authority_epoch: String,
    pub feed_sequence: u64,
    pub snapshot_cursor: UiCursor,
    pub cursor_digest: String,
}

impl UiFeedCursorV1 {
    pub fn new(
        instance_id: impl Into<String>,
        authority_epoch: impl Into<String>,
        feed_sequence: u64,
        snapshot_cursor: UiCursor,
    ) -> Result<Self, String> {
        let mut cursor = Self {
            schema: UI_FEED_CURSOR_SCHEMA.to_owned(),
            instance_id: instance_id.into(),
            authority_epoch: authority_epoch.into(),
            feed_sequence,
            snapshot_cursor,
            cursor_digest: String::new(),
        };
        cursor.cursor_digest = cursor.digest();
        cursor.validate()?;
        Ok(cursor)
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.schema != UI_FEED_CURSOR_SCHEMA {
            return Err("ui_feed_cursor_header_invalid".to_owned());
        }
        required(&self.instance_id, "ui_feed_cursor_instance", 256)?;
        required(&self.authority_epoch, "ui_feed_cursor_epoch", 256)?;
        if self.snapshot_cursor.sequence > 0 {
            required(
                &self.snapshot_cursor.epoch,
                "ui_feed_cursor_snapshot_epoch",
                256,
            )?;
            if self.snapshot_cursor.epoch != self.authority_epoch {
                return Err("ui_feed_cursor_snapshot_epoch_mismatch".to_owned());
            }
        } else if !self.snapshot_cursor.epoch.is_empty() {
            return Err("ui_feed_cursor_snapshot_invalid".to_owned());
        }
        digest(&self.cursor_digest, "ui_feed_cursor_digest")?;
        if self.cursor_digest != self.digest() {
            return Err("ui_feed_cursor_digest_mismatch".to_owned());
        }
        Ok(())
    }

    pub fn encode(&self) -> Result<String, String> {
        self.validate()?;
        serde_json::to_string(self).map_err(|_| "ui_feed_cursor_encode_failed".to_owned())
    }

    pub fn decode(value: &str) -> Result<Self, String> {
        if value.len() > 2_048 {
            return Err("ui_feed_cursor_too_large".to_owned());
        }
        let cursor: Self =
            serde_json::from_str(value).map_err(|_| "ui_feed_cursor_decode_failed".to_owned())?;
        cursor.validate()?;
        Ok(cursor)
    }

    fn digest(&self) -> String {
        json_digest(&json!({
            "schema": self.schema,
            "instance_id": self.instance_id,
            "authority_epoch": self.authority_epoch,
            "feed_sequence": self.feed_sequence,
            "snapshot_cursor": self.snapshot_cursor,
        }))
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct UiFeedGapV1 {
    pub schema: String,
    pub reason: UiFeedGapReason,
    pub from: Option<UiFeedCursorV1>,
    pub to: UiFeedCursorV1,
    pub snapshot_required: bool,
}

impl UiFeedGapV1 {
    pub fn validate(&self) -> Result<(), String> {
        if self.schema != UI_FEED_GAP_SCHEMA {
            return Err("ui_feed_gap_schema_invalid".to_owned());
        }
        self.to.validate()?;
        if let Some(from) = &self.from {
            from.validate()?;
            if from.instance_id == self.to.instance_id
                && from.authority_epoch == self.to.authority_epoch
                && from.feed_sequence > self.to.feed_sequence
            {
                return Err("ui_feed_gap_range_invalid".to_owned());
            }
        }
        if !self.snapshot_required {
            return Err("ui_feed_gap_snapshot_required".to_owned());
        }
        Ok(())
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct UiFeedFrameV1 {
    pub schema: String,
    pub kind: UiFeedFrameKind,
    pub cursor: UiFeedCursorV1,
    pub event_id: String,
    pub replay: bool,
    pub terminal: bool,
    #[serde(default)]
    pub event: Option<Value>,
    #[serde(default)]
    pub gap: Option<UiFeedGapV1>,
}

impl UiFeedFrameV1 {
    pub fn validate(&self) -> Result<(), String> {
        if self.schema != UI_FEED_FRAME_SCHEMA {
            return Err("ui_feed_frame_schema_invalid".to_owned());
        }
        self.cursor.validate()?;
        required(&self.event_id, "ui_feed_frame_event_id", 512)?;
        if self.terminal != matches!(self.kind, UiFeedFrameKind::Terminal) {
            return Err("ui_feed_frame_terminal_mismatch".to_owned());
        }
        if self.kind == UiFeedFrameKind::Gap {
            let gap = self
                .gap
                .as_ref()
                .ok_or_else(|| "ui_feed_frame_gap_missing".to_owned())?;
            gap.validate()?;
        } else if self.gap.is_some() {
            return Err("ui_feed_frame_gap_unexpected".to_owned());
        }
        if let Some(event) = &self.event {
            validate_json_limits(event).map_err(|_| "ui_feed_frame_event_invalid".to_owned())?;
        }
        Ok(())
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
